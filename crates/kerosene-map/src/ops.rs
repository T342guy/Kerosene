// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Cutting brushes up and turning them round: clip, carve, hollow, rotate,
//! flip.
//!
//! Every one of these is cheap to write *because* a solid is an intersection
//! of half-spaces. Clipping is adding a plane. Carving is clipping by each
//! of the carver's planes in turn. Hollowing is clipping by each face's
//! plane pushed inward. None of them touches a vertex, and the result is
//! convex and closed by construction or it is nothing at all -- which is
//! why they live here, next to the format, rather than in the editor.

use crate::solid::points_for_plane;
use crate::{Side, Solid};
use kerosene_math::{Aabb, Plane, Quat, Vec3};

/// Below this, two planes are the same plane and a cut along one of a
/// brush's own faces is not a cut.
const SAME_PLANE: f32 = 1e-4;

impl Solid {
    /// Keep the part of the brush behind `plane` (where its normal points
    /// away), textured on the new face like the face of this brush that
    /// points most nearly the same way -- so a clipped wall's cut end wears
    /// the wall's material, aligned as the wall is.
    ///
    /// `None` if nothing is left: the plane missed the brush, or cut off
    /// less than a sliver.
    pub fn behind(&self, plane: &Plane) -> Option<Solid> {
        // Cutting along one of the brush's own faces leaves it whole or
        // takes it all; neither wants a duplicate plane in the list.
        for own in self.planes() {
            if (own.normal - plane.normal).length() < SAME_PLANE
                && (own.dist - plane.dist).abs() < SAME_PLANE
            {
                return Some(self.clone());
            }
            if (own.normal + plane.normal).length() < SAME_PLANE
                && (own.dist + plane.dist).abs() < SAME_PLANE
            {
                return None;
            }
        }
        let mut cut = self.clone();
        cut.sides.push(self.side_on(plane));
        cut.drop_redundant_sides();
        if cut.validate().is_err() || cut.bounds().size().min_element() < 1e-3 {
            return None;
        }
        Some(cut)
    }

    /// Cut the brush in two along a plane: `(behind, in front)`. Either
    /// half may be `None` when the plane misses.
    pub fn clip(&self, plane: &Plane) -> (Option<Solid>, Option<Solid>) {
        (self.behind(plane), self.behind(&plane.flipped()))
    }

    /// What is left of this brush once `carver` has been taken out of it.
    ///
    /// Zero pieces if the carver swallows it, one untouched clone if they do
    /// not meet, and otherwise up to one piece per face of the carver: each
    /// is what lies outside that face and inside every face before it, which
    /// tiles the remainder without overlap.
    pub fn subtract(&self, carver: &Solid) -> Vec<Solid> {
        if !self.bounds().intersects(&carver.bounds()) {
            return vec![self.clone()];
        }
        let mut pieces = Vec::new();
        let mut remainder = Some(self.clone());
        for plane in carver.planes() {
            let Some(current) = remainder.as_ref() else {
                break;
            };
            // Outside this face of the carver: a piece.
            if let Some(outside) = current.behind(&plane.flipped()) {
                pieces.push(outside);
            }
            // Inside it: what the next face gets to cut.
            remainder = current.behind(&plane);
        }
        pieces
    }

    /// The brush as a shell: one wall per face, `thickness` thick, mitred
    /// where they meet so no two overlap. Negative thickness grows the walls
    /// outward instead, leaving the original volume as the cavity.
    pub fn hollow(&self, thickness: f32) -> Vec<Solid> {
        if thickness.abs() < 1e-3 {
            return vec![self.clone()];
        }
        let planes = self.planes();
        // The shell's outer surface and the planes of its inner surface.
        let outer = if thickness < 0.0 {
            self.expanded(-thickness)
        } else {
            self.clone()
        };
        // Inward: the inner surface is the brush shrunk by the thickness.
        // Outward: the inner surface is the brush itself.
        let inner: Vec<Plane> = planes
            .iter()
            .map(|p| Plane::new(p.normal, p.dist - thickness.max(0.0)))
            .collect();
        let mut walls = Vec::new();
        for (i, inner_plane) in inner.iter().enumerate() {
            // Between the outer face and its inner plane...
            let Some(mut wall) = outer.behind(&inner_plane.flipped()) else {
                continue;
            };
            // ...and inside the inner planes of the walls already made, so
            // the corners are shared out rather than doubled.
            for earlier in &inner[..i] {
                match wall.behind(earlier) {
                    Some(w) => wall = w,
                    None => break,
                }
            }
            if wall.validate().is_ok() {
                walls.push(wall);
            }
        }
        walls
    }

    /// The same brush with every face pushed outward by `amount`.
    pub fn expanded(&self, amount: f32) -> Solid {
        let mut out = self.clone();
        for side in &mut out.sides {
            if let Some(plane) = side.plane() {
                let shift = plane.normal * amount;
                for p in &mut side.plane_points {
                    *p += shift;
                }
            }
        }
        out
    }

    /// Turn the brush about a point. The texture turns with it, so a
    /// surface point keeps its texel, the way [`Solid::translate`] keeps it.
    pub fn rotate(&mut self, pivot: Vec3, rotation: Quat) {
        for side in &mut self.sides {
            for p in &mut side.plane_points {
                *p = pivot + rotation * (*p - pivot);
            }
            for axis in [&mut side.uaxis, &mut side.vaxis] {
                let turned = rotation * axis.axis;
                // The offset absorbs what the pivot's projection changed by.
                axis.offset += (pivot.dot(axis.axis) - pivot.dot(turned)) / axis.safe_scale();
                axis.axis = turned;
            }
        }
    }

    /// Mirror the brush across a plane through `pivot` perpendicular to
    /// `axis` (0, 1 or 2). The faces stay outward.
    pub fn flip(&mut self, axis: usize, pivot: Vec3) {
        let mut factor = Vec3::ONE;
        factor[axis] = -1.0;
        self.scale(pivot, factor);
    }

    /// Move the brush's lowest corner onto the grid.
    pub fn align_to_grid(&mut self, grid: f32) {
        if grid <= 0.0 {
            return;
        }
        let min = self.bounds().min;
        let snapped = (min / grid).round() * grid;
        self.translate(snapped - min);
    }

    /// A new face on `plane`, textured like the existing face that points
    /// most nearly the same way.
    fn side_on(&self, plane: &Plane) -> Side {
        let template = self
            .sides
            .iter()
            .filter_map(|s| s.plane().map(|p| (p.normal.dot(plane.normal), s)))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, s)| s);
        let mut side = Side::from_plane(0, *plane, "dev/grid");
        if let Some(template) = template {
            side.material = template.material.clone();
            side.lightmap_scale = template.lightmap_scale;
            side.smoothing_groups = template.smoothing_groups;
            side.walkmap = template.walkmap;
            // The template's axes, when they still lie in the new plane
            // well enough to project sensibly; otherwise the defaults.
            let (u, v) = (template.uaxis, template.vaxis);
            let in_plane =
                u.axis.dot(plane.normal).abs() < 0.9 && v.axis.dot(plane.normal).abs() < 0.9;
            if in_plane {
                side.uaxis = u;
                side.vaxis = v;
            } else {
                side.uaxis.scale = u.scale;
                side.vaxis.scale = v.scale;
            }
        }
        side.plane_points = points_for_plane(plane);
        side
    }

    /// Drop faces whose plane never reaches the hull. A cut leaves the faces
    /// on the far side of it bounding nothing, and while a redundant face is
    /// legal it is noise in the file and a trap for the next cut.
    fn drop_redundant_sides(&mut self) {
        let windings = self.windings();
        let mut i = 0;
        self.sides.retain(|_| {
            let keep = windings[i].is_some();
            i += 1;
            keep
        });
    }

    /// Volume, for tests and for the report: the sum over faces of the
    /// signed tetrahedra from the origin.
    pub fn volume(&self) -> f32 {
        let mut total = 0.0;
        for (_, winding) in self.face_windings() {
            let p = &winding.points;
            for i in 1..p.len().saturating_sub(1) {
                total += p[0].dot(p[i].cross(p[i + 1]));
            }
        }
        (total / 6.0).abs()
    }

    /// Whether this brush and another share any interior volume.
    pub fn overlaps(&self, other: &Solid) -> bool {
        if !self.bounds().intersects(&other.bounds()) {
            return false;
        }
        // Convex against convex: they overlap unless some face of either
        // separates them. A shared face is not an overlap.
        let separated = |a: &Solid, b: &Solid| {
            a.planes().iter().any(|plane| {
                b.face_windings()
                    .iter()
                    .flat_map(|(_, w)| w.points.iter())
                    .all(|&p| plane.distance_to(p) >= -1e-3)
            })
        };
        !(separated(self, other) || separated(other, self))
    }
}

/// The bounds of several solids together.
pub fn bounds_of(solids: &[Solid]) -> Aabb {
    solids.iter().fold(Aabb::EMPTY, |b, s| b.union(&s.bounds()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube64() -> Solid {
        Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/wall")
    }

    #[test]
    fn clipping_a_cube_makes_two_boxes_that_wear_its_material() {
        let cube = cube64();
        let plane = Plane::new(Vec3::X, 32.0);
        let (behind, front) = cube.clip(&plane);
        let (behind, front) = (behind.unwrap(), front.unwrap());
        assert_eq!(behind.bounds().max.x, 32.0);
        assert_eq!(front.bounds().min.x, 32.0);
        assert_eq!(
            behind.sides.len(),
            6,
            "the far face was dropped, the cut added"
        );
        assert!(behind.sides.iter().all(|s| s.material == "dev/wall"));
        assert!((behind.volume() + front.volume() - cube.volume()).abs() < 1e-2);
    }

    #[test]
    fn a_plane_that_misses_leaves_one_half_empty() {
        let cube = cube64();
        let (behind, front) = cube.clip(&Plane::new(Vec3::X, 500.0));
        assert!(behind.is_some());
        assert!(front.is_none());
    }

    #[test]
    fn cutting_along_an_existing_face_is_not_a_cut() {
        let cube = cube64();
        let (behind, front) = cube.clip(&Plane::new(Vec3::X, 64.0));
        assert_eq!(behind.as_ref().map(|s| s.sides.len()), Some(6));
        assert!(front.is_none());
    }

    #[test]
    fn carving_a_hole_tiles_the_remainder_without_overlap() {
        let big = cube64();
        let small = Solid::cube(Aabb::new(Vec3::splat(16.0), Vec3::splat(48.0)), "dev/grid");
        let pieces = big.subtract(&small);
        assert_eq!(pieces.len(), 6);
        let total: f32 = pieces.iter().map(Solid::volume).sum();
        assert!(
            (total - (big.volume() - small.volume())).abs() < 1e-1,
            "{total}"
        );
        for (i, a) in pieces.iter().enumerate() {
            assert!(a.validate().is_ok());
            for b in &pieces[i + 1..] {
                assert!(!a.overlaps(b), "pieces share volume");
            }
            assert!(!a.overlaps(&small), "a piece reaches into the hole");
        }
    }

    #[test]
    fn carving_with_something_that_does_not_touch_changes_nothing() {
        let big = cube64();
        let far = Solid::cube(
            Aabb::new(Vec3::splat(500.0), Vec3::splat(564.0)),
            "dev/grid",
        );
        let pieces = big.subtract(&far);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0], big);
        let huge = Solid::cube(
            Aabb::new(Vec3::splat(-10.0), Vec3::splat(100.0)),
            "dev/grid",
        );
        assert!(big.subtract(&huge).is_empty(), "swallowed whole");
    }

    #[test]
    fn hollowing_makes_six_mitred_walls_around_an_empty_cavity() {
        let cube = cube64();
        let walls = cube.hollow(8.0);
        assert_eq!(walls.len(), 6);
        let total: f32 = walls.iter().map(Solid::volume).sum();
        let cavity = 48.0f32.powi(3);
        assert!((total - (cube.volume() - cavity)).abs() < 1e-1, "{total}");
        for (i, a) in walls.iter().enumerate() {
            for b in &walls[i + 1..] {
                assert!(!a.overlaps(b));
            }
            assert!(!a.contains_point(Vec3::splat(32.0)), "the middle is empty");
        }
        // Outward: the original is the cavity.
        let outer = cube.hollow(-8.0);
        assert_eq!(outer.len(), 6);
        assert!(outer.iter().all(|w| !w.overlaps(&cube)));
        assert_eq!(
            bounds_of(&outer),
            Aabb::new(Vec3::splat(-8.0), Vec3::splat(72.0))
        );
    }

    #[test]
    fn rotating_keeps_the_volume_and_the_texture_in_place() {
        let mut cube = cube64();
        let before = cube.volume();
        let pivot = Vec3::splat(32.0);
        let top = cube.sides[0].clone();
        let corner = Vec3::new(64.0, 64.0, 64.0);
        let texel_before = top.texcoord(corner);
        cube.rotate(pivot, Quat::from_rotation_z(90f32.to_radians()));
        assert!((cube.volume() - before).abs() < 1e-1);
        assert!(cube.validate().is_ok());
        let turned = pivot + Quat::from_rotation_z(90f32.to_radians()) * (corner - pivot);
        let texel_after = cube.sides[0].texcoord(turned);
        assert!(
            (texel_before.0 - texel_after.0).abs() < 1e-2,
            "{texel_before:?} {texel_after:?}"
        );
        assert!((texel_before.1 - texel_after.1).abs() < 1e-2);
    }

    #[test]
    fn flipping_keeps_every_face_outward() {
        let mut wedge = cube64();
        wedge.sides.push(Side::from_plane(
            99,
            Plane::from_point_normal(
                Vec3::new(48.0, 48.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0).normalize(),
            ),
            "dev/grid",
        ));
        let before = wedge.volume();
        wedge.flip(0, Vec3::splat(32.0));
        assert!(wedge.validate().is_ok());
        assert!((wedge.volume() - before).abs() < 1e-1);
        assert!(
            wedge.contains_point(Vec3::new(56.0, 8.0, 32.0)),
            "the corner cut moved to the other side"
        );
        assert!(!wedge.contains_point(Vec3::new(8.0, 56.0, 32.0)));
    }

    #[test]
    fn aligning_moves_the_lowest_corner_onto_the_grid() {
        let mut cube = Solid::cube(Aabb::new(Vec3::splat(3.0), Vec3::splat(67.0)), "dev/grid");
        cube.align_to_grid(16.0);
        assert_eq!(cube.bounds().min, Vec3::ZERO);
        assert_eq!(cube.bounds().max, Vec3::splat(64.0));
    }
}
