// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Editing a brush by its corners, edges and faces.
//!
//! A brush is stored as planes -- the compiler's view of it, and the reason
//! it is always convex. Hammer 5 edits geometry by its vertices instead,
//! which is how anyone thinks about shape: pull this corner up, push that
//! face in. This file is the translation between the two.
//!
//! Every edit works the same way: take the brush's corners, move some of
//! them, and wrap a convex hull around where they ended up. The hull's
//! faces become the brush's planes, each keeping the material and texture
//! alignment of the face it came from. If a moved corner ends up *inside*
//! the hull -- the edit would have dented the brush -- the edit is refused
//! with a reason, rather than silently producing a different shape: a brush
//! cannot be concave, and pretending otherwise is how a corner goes missing.

use kerosene_map::{Side, Solid};
use kerosene_math::{Plane, Vec3};

/// How close two corners are before they are one corner.
const WELD: f32 = 0.05;
/// How far off a plane a point may be and still be on it.
const ON_PLANE: f32 = 0.05;

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    /// A corner would end up inside the brush: it would be concave.
    Concave,
    /// Everything collapsed onto a plane or a line: no volume left.
    Flat,
    /// The union of the brushes to merge is not convex.
    NotConvex,
    /// Nothing to work on.
    Nothing,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EditError::Concave => {
                "that would dent the brush, and a brush has to be convex -- clip it in two first"
            }
            EditError::Flat => "that would flatten the brush to nothing",
            EditError::NotConvex => {
                "those brushes together are not convex, so they cannot be one brush"
            }
            EditError::Nothing => "nothing selected to edit",
        })
    }
}

/// A brush's corners, edges and faces, found from its planes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Topology {
    pub vertices: Vec<Vec3>,
    /// Pairs of vertex indices, each edge once.
    pub edges: Vec<(usize, usize)>,
    /// Each face: the side's id, and its corners in winding order.
    pub faces: Vec<(u32, Vec<usize>)>,
}

impl Topology {
    /// The corner nearest `point`, if one is within `reach`.
    pub fn vertex_near(&self, point: Vec3, reach: f32) -> Option<usize> {
        nearest(self.vertices.iter().copied(), point, reach)
    }

    /// The middle of an edge.
    pub fn edge_midpoint(&self, edge: usize) -> Vec3 {
        let (a, b) = self.edges[edge];
        (self.vertices[a] + self.vertices[b]) * 0.5
    }

    /// The middle of a face.
    pub fn face_centre(&self, face: usize) -> Vec3 {
        let corners = &self.faces[face].1;
        corners.iter().map(|&i| self.vertices[i]).sum::<Vec3>() / corners.len().max(1) as f32
    }
}

fn nearest(points: impl Iterator<Item = Vec3>, to: Vec3, reach: f32) -> Option<usize> {
    points
        .enumerate()
        .map(|(i, p)| (i, p.distance(to)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A corner as the planes put it, pulled onto the sixteenth-unit lattice
/// when it is within a hair of it.
///
/// Corners are found by intersecting planes, which is exact only on paper:
/// a sloped face puts its corners a few thousandths off where they were
/// dragged to, and dragging those corners again compounds it. A corner that
/// close to a sixteenth is that sixteenth; one that is not -- a cut made at
/// an odd angle -- is left where it is.
fn settle(p: Vec3) -> Vec3 {
    const LATTICE: f32 = 16.0;
    const CLOSE: f32 = 0.02;
    Vec3::from_array(p.to_array().map(|c| {
        let snapped = (c * LATTICE).round() / LATTICE;
        if (snapped - c).abs() <= CLOSE {
            snapped
        } else {
            c
        }
    }))
}

/// The corners, edges and faces of a brush.
pub fn topology(solid: &Solid) -> Topology {
    let mut topology = Topology::default();
    for (side, winding) in solid.face_windings() {
        let mut corners = Vec::with_capacity(winding.points.len());
        for p in winding.points.iter().map(|p| settle(*p)) {
            let p = &p;
            let index = match topology.vertices.iter().position(|v| v.distance(*p) < WELD) {
                Some(i) => i,
                None => {
                    topology.vertices.push(*p);
                    topology.vertices.len() - 1
                }
            };
            if corners.last() != Some(&index) && corners.first() != Some(&index) {
                corners.push(index);
            }
        }
        for i in 0..corners.len() {
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            let edge = (a.min(b), a.max(b));
            if a != b && !topology.edges.contains(&edge) {
                topology.edges.push(edge);
            }
        }
        topology.faces.push((side.id, corners));
    }
    topology
}

/// The planes of the convex hull around some points, outward facing, each
/// with the points that lie on it.
///
/// Brute force over every triple: a brush has tens of corners, not
/// thousands, and a hull that is obviously right beats a faster one that
/// is subtly wrong on a degenerate case.
fn hull(points: &[Vec3]) -> Result<Vec<(Plane, Vec<usize>)>, EditError> {
    let mut planes: Vec<(Plane, Vec<usize>)> = Vec::new();
    let n = points.len();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let normal = (points[j] - points[i]).cross(points[k] - points[i]);
                if normal.length() < 1e-3 {
                    continue;
                }
                let normal = normal.normalize();
                for normal in [normal, -normal] {
                    let dist = normal.dot(points[i]);
                    if points.iter().any(|p| normal.dot(*p) - dist > ON_PLANE) {
                        continue;
                    }
                    if planes.iter().any(|(p, _)| {
                        p.normal.dot(normal) > 1.0 - 1e-4 && (p.dist - dist).abs() < ON_PLANE
                    }) {
                        continue;
                    }
                    let on: Vec<usize> = (0..n)
                        .filter(|&m| (normal.dot(points[m]) - dist).abs() <= ON_PLANE)
                        .collect();
                    planes.push((Plane::new(normal, dist), on));
                }
            }
        }
    }
    if planes.len() < 4 {
        return Err(EditError::Flat);
    }
    Ok(planes)
}

/// A new brush from moved corners, keeping the old brush's faces where
/// they still are.
///
/// `old` is the brush before the edit and `topology` its corners, so a
/// hull face made of an old face's corners can be matched back to that
/// face and keep its material and texture alignment.
fn rebuild(old: &Solid, topology: &Topology, moved: &[Vec3]) -> Result<Solid, EditError> {
    // Welded again: two corners dragged together are one corner.
    let mut points: Vec<Vec3> = Vec::new();
    let mut remap = Vec::with_capacity(moved.len());
    for p in moved {
        match points.iter().position(|q| q.distance(*p) < WELD) {
            Some(i) => remap.push(i),
            None => {
                points.push(*p);
                remap.push(points.len() - 1);
            }
        }
    }
    let planes = hull(&points)?;

    // Every corner must be on the hull: one inside means a dent.
    for i in 0..points.len() {
        if !planes.iter().any(|(_, on)| on.contains(&i)) {
            return Err(EditError::Concave);
        }
    }

    let material = most_common_material(old);
    let mut used = std::collections::HashSet::new();
    let mut sides = Vec::with_capacity(planes.len());
    let mut next_id = old.sides.iter().map(|s| s.id).max().unwrap_or(0) + 1;
    for (plane, on) in &planes {
        // The old face with the most corners on this plane is the one this
        // plane is: it keeps that face's id, material and alignment.
        let best = topology
            .faces
            .iter()
            .filter(|(id, _)| !used.contains(id))
            .map(|(id, corners)| {
                let shared = corners.iter().filter(|&&c| on.contains(&remap[c])).count();
                (*id, shared)
            })
            .filter(|(_, shared)| *shared >= 3)
            .max_by_key(|(_, shared)| *shared);
        let mut fresh = Side::from_plane(0, *plane, &material);
        if let Some(points) = spell(&points, on, plane.normal) {
            fresh.plane_points = points;
        }
        let side = match best.and_then(|(id, _)| old.sides.iter().find(|s| s.id == id)) {
            Some(existing) => {
                used.insert(existing.id);
                let mut side = existing.clone();
                side.plane_points = fresh.plane_points;
                side
            }
            None => {
                let mut side = fresh;
                side.id = next_id;
                next_id += 1;
                side
            }
        };
        sides.push(side);
    }
    let mut solid = old.clone();
    solid.sides = sides;
    if solid.volume() < 1.0 {
        return Err(EditError::Flat);
    }
    Ok(solid)
}

/// Three corners on a hull plane to spell it with, in the order a brush
/// file wants them (clockwise seen from the front).
///
/// Taken from the corners themselves rather than rebuilt from the normal:
/// corners dragged on the grid are exact, and a plane spelt with them is
/// exact too. Spelt from the normal instead, every edit nudged the brush a
/// few thousandths off the grid, and edits piled up.
fn spell(points: &[Vec3], on: &[usize], normal: Vec3) -> Option<[Vec3; 3]> {
    let mut best: Option<([Vec3; 3], f32)> = None;
    for (x, &i) in on.iter().enumerate() {
        for (y, &j) in on.iter().enumerate().skip(x + 1) {
            for &k in on.iter().skip(y + 1) {
                let (a, b, c) = (points[i], points[j], points[k]);
                let n = (a - b).cross(c - b);
                let area = n.length();
                if area < 1e-3 || best.as_ref().is_some_and(|(_, best)| area <= *best) {
                    continue;
                }
                let triple = if n.dot(normal) > 0.0 {
                    [a, b, c]
                } else {
                    [c, b, a]
                };
                best = Some((triple, area));
            }
        }
    }
    best.map(|(triple, _)| triple)
}

fn most_common_material(solid: &Solid) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for side in &solid.sides {
        match counts.iter_mut().find(|(m, _)| *m == side.material) {
            Some((_, n)) => *n += 1,
            None => counts.push((&side.material, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(m, _)| m.to_string())
        .unwrap_or_default()
}

/// Move some corners of a brush.
pub fn move_vertices(solid: &Solid, vertices: &[usize], delta: Vec3) -> Result<Solid, EditError> {
    if vertices.is_empty() {
        return Err(EditError::Nothing);
    }
    let topology = topology(solid);
    let mut moved = topology.vertices.clone();
    for &v in vertices {
        if let Some(p) = moved.get_mut(v) {
            *p += delta;
        }
    }
    rebuild(solid, &topology, &moved)
}

/// Move the corners at these positions: the form the editor keeps a
/// selection in, since a corner's index changes whenever the brush does.
pub fn move_points(solid: &Solid, points: &[Vec3], delta: Vec3) -> Result<Solid, EditError> {
    let topology = topology(solid);
    let indices: Vec<usize> = points
        .iter()
        .filter_map(|p| topology.vertex_near(*p, WELD * 4.0))
        .collect();
    move_vertices(solid, &indices, delta)
}

/// Move one face of a brush, all its corners together.
pub fn move_face(solid: &Solid, side: u32, delta: Vec3) -> Result<Solid, EditError> {
    let topology = topology(solid);
    let corners = topology
        .faces
        .iter()
        .find(|(id, _)| *id == side)
        .map(|(_, c)| c.clone())
        .ok_or(EditError::Nothing)?;
    move_vertices(solid, &corners, delta)
}

/// A new brush grown out of one face: the face swept `distance` along its
/// normal. The new brush wears the face's material.
pub fn extrude_face(solid: &Solid, side: u32, distance: f32) -> Result<Solid, EditError> {
    let (face, winding) = solid
        .face_windings()
        .into_iter()
        .find(|(s, _)| s.id == side)
        .map(|(s, w)| (s.clone(), w))
        .ok_or(EditError::Nothing)?;
    let normal = face.plane().ok_or(EditError::Flat)?.normal;
    if distance.abs() < 1e-3 {
        return Err(EditError::Flat);
    }
    let mut points = winding.points.clone();
    points.extend(winding.points.iter().map(|p| *p + normal * distance));
    let planes = hull(&points)?;
    let sides = planes
        .iter()
        .enumerate()
        .map(|(i, (plane, on))| {
            let mut side = Side::from_plane(i as u32 + 1, *plane, &face.material);
            if let Some(exact) = spell(&points, on, plane.normal) {
                side.plane_points = exact;
            }
            // The cap facing the same way as the face wears its alignment.
            if plane.normal.dot(normal * distance.signum()) > 0.999 {
                side.uaxis = face.uaxis;
                side.vaxis = face.vaxis;
            }
            side
        })
        .collect();
    let brush = Solid::new(0, sides);
    if brush.volume() < 1.0 {
        return Err(EditError::Flat);
    }
    Ok(brush)
}

/// One brush from several, when together they are convex: two halves of a
/// clipped block back into the block. The first brush's faces are kept
/// wherever they still bound the result.
pub fn merge(solids: &[&Solid]) -> Result<Solid, EditError> {
    let (first, rest) = solids.split_first().ok_or(EditError::Nothing)?;
    if rest.is_empty() {
        return Err(EditError::Nothing);
    }
    let mut points: Vec<Vec3> = Vec::new();
    for solid in solids {
        for p in topology(solid).vertices {
            if !points.iter().any(|q| q.distance(p) < WELD) {
                points.push(p);
            }
        }
    }
    let planes = hull(&points)?;
    // Convex together exactly when the hull holds no more than the parts.
    let parts: f32 = solids.iter().map(|s| s.volume()).sum();
    let mut sides = Vec::new();
    for (i, (plane, on)) in planes.iter().enumerate() {
        let kept = solids
            .iter()
            .flat_map(|s| s.sides.iter())
            .find(|s| s.plane().is_some_and(|p| p.approx_eq(plane)));
        let side = match kept {
            Some(side) => {
                let mut side = side.clone();
                side.id = i as u32 + 1;
                side
            }
            None => {
                let mut side = Side::from_plane(i as u32 + 1, *plane, &most_common_material(first));
                if let Some(exact) = spell(&points, on, plane.normal) {
                    side.plane_points = exact;
                }
                side
            }
        };
        sides.push(side);
    }
    let mut merged = (*first).clone();
    merged.sides = sides;
    let whole = merged.volume();
    if (whole - parts).abs() > parts.max(1.0) * 1e-3 {
        return Err(EditError::NotConvex);
    }
    Ok(merged)
}

#[cfg(test)]
mod tests;
