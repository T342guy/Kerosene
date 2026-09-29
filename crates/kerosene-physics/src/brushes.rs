// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! A compiled map's brushes as bodies in the [`PhysicsWorld`].
//!
//! Props need something to land on. The world's solid brushes become static
//! convex hulls, one section at a time so a streamed map only pays for what
//! is resident, and each moving brush entity (a door, a shutter) becomes a
//! set of static bodies the caller re-places as the entity moves.

use crate::rigid::{Body, PhysicsWorld};
use kerosene_bsp::{Bsp, contents};
use kerosene_math::{Angles, ON_EPSILON, Quat, Vec3, Winding};

impl PhysicsWorld {
    /// Add the static hulls of one section of the map: every solid brush that
    /// does not move and is not a player or monster clip volume. Returns the
    /// bodies, for [`PhysicsWorld::destroy_body`] when the section unloads.
    pub fn add_world_section(&mut self, bsp: &Bsp, section: usize) -> Vec<Body> {
        let mut bodies = Vec::new();
        for (i, brush) in bsp.brushes.iter().enumerate() {
            if bsp.brush_section(i) as usize != section {
                continue;
            }
            if brush.contents & contents::SOLID == 0 {
                continue;
            }
            if brush.contents & contents::MOVEABLE != 0 {
                continue;
            }
            if brush.contents & (contents::PLAYER_CLIP | contents::MONSTER_CLIP) != 0 {
                continue;
            }

            let Some(points) = brush_vertices(bsp, brush) else {
                continue;
            };
            // Static hulls live in world coordinates already.
            match self.add_static_hull(&points, Vec3::ZERO, Quat::IDENTITY) {
                Some(body) => bodies.push(body),
                None => log::debug!("physics: skipped degenerate world brush {i}"),
            }
        }
        bodies
    }

    /// Add the bodies of one moving brush entity, placed at `origin` and
    /// `angles`. Returns them with the pivot the entity's angles turn about,
    /// in its own space, or `None` if the model has no solid moving brush.
    ///
    /// Brushes are compiled in world coordinates; each body's local space is
    /// centred on the pivot so the entity's angles turn the body about the
    /// same point the renderer does.
    pub fn add_mover(
        &mut self,
        bsp: &Bsp,
        model: usize,
        origin: Vec3,
        angles: Angles,
    ) -> Option<(Vec<Body>, Vec3)> {
        let pivot = bsp
            .models
            .get(model)
            .map(|m| m.bounds().center())
            .unwrap_or(Vec3::ZERO);
        let rotation = Quat::from_mat3(&angles.to_mat3());
        let mut bodies = Vec::new();
        for &brush_index in &model_brush_indices(bsp, model) {
            let Some(brush) = bsp.brushes.get(brush_index) else {
                continue;
            };
            if brush.contents & contents::MOVEABLE == 0 || brush.contents & contents::SOLID == 0 {
                continue;
            }
            let Some(points) = brush_vertices(bsp, brush) else {
                continue;
            };
            let local: Vec<Vec3> = points.iter().map(|&p| p - pivot).collect();
            if let Some(body) = self.add_static_hull(&local, origin + pivot, rotation) {
                bodies.push(body);
            }
        }
        (!bodies.is_empty()).then_some((bodies, pivot))
    }
}

/// The unique vertices of one BSP brush, computed by clipping each face's base
/// winding against every other face. Returns `None` for a degenerate brush.
pub fn brush_vertices(bsp: &Bsp, brush: &kerosene_bsp::Brush) -> Option<Vec<Vec3>> {
    let mut planes = Vec::with_capacity(brush.num_sides as usize);
    for i in 0..brush.num_sides as usize {
        let side = bsp.brushsides.get(brush.first_side as usize + i)?;
        let plane = bsp.planes.get(side.plane as usize)?.to_plane();
        planes.push(plane);
    }
    if planes.len() < 4 {
        return None;
    }

    let mut points = Vec::new();
    for (i, plane) in planes.iter().enumerate() {
        let mut w = Winding::base_for_plane(plane);
        for (j, other) in planes.iter().enumerate() {
            if i == j {
                continue;
            }
            // Keep the half of the brush we are inside: the other face's
            // plane, flipped to point inward.
            w = w.clipped(&other.flipped(), ON_EPSILON)?;
        }
        w.remove_collinear();
        if w.is_tiny() {
            continue;
        }
        points.extend(w.points);
    }

    let mut unique: Vec<Vec3> = Vec::new();
    for p in points {
        if !unique.iter().any(|&q| (q - p).length_squared() < 0.01) {
            unique.push(p);
        }
    }
    (unique.len() >= 4).then_some(unique)
}

/// The brush indices belonging to one BSP model (0 = world, 1.. = brush
/// entities). A brush model's head node is a single leaf whose leafbrushes
/// reference exactly its brushes.
pub fn model_brush_indices(bsp: &Bsp, model: usize) -> Vec<usize> {
    let Some(m) = bsp.models.get(model) else {
        return Vec::new();
    };
    let kerosene_bsp::Child::Leaf(leaf) = kerosene_bsp::decode_child(m.head_node) else {
        return Vec::new();
    };
    let Some(leaf) = bsp.leaves.get(leaf) else {
        return Vec::new();
    };
    let first = leaf.first_leafbrush as usize;
    let count = leaf.num_leafbrushes as usize;
    (first..first + count)
        .filter_map(|i| bsp.leafbrushes.get(i).map(|&bi| bi as usize))
        .collect()
}
