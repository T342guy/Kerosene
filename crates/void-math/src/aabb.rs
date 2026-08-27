// SPDX-License-Identifier: LGPL-3.0-or-later
use crate::{MAX_MAP_COORD, Plane, PlaneSide};
use glam::Vec3;

/// An axis-aligned bounding box.
///
/// An *empty* box is one whose `min` exceeds its `max` on some axis; that is
/// the state [`Aabb::EMPTY`] starts in, so that [`Aabb::expand`] over zero
/// points yields empty rather than a box around the origin.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// Inverted box; grows correctly from the first point added.
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    /// A box covering the entire legal world.
    pub const WORLD: Aabb = Aabb {
        min: Vec3::splat(-MAX_MAP_COORD),
        max: Vec3::splat(MAX_MAP_COORD),
    };

    #[inline]
    pub const fn new(min: Vec3, max: Vec3) -> Self { Self { min, max } }

    /// Box centred on `center` with the given half-extents.
    #[inline]
    pub fn from_center_half(center: Vec3, half: Vec3) -> Self {
        Self { min: center - half, max: center + half }
    }

    pub fn from_points(points: &[Vec3]) -> Self {
        let mut b = Self::EMPTY;
        for &p in points { b.add_point(p); }
        b
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    #[inline]
    pub fn add_point(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    #[inline]
    pub fn union(&self, other: &Aabb) -> Aabb {
        if self.is_empty() { return *other; }
        if other.is_empty() { return *self; }
        Aabb { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    #[inline]
    pub fn center(&self) -> Vec3 { (self.min + self.max) * 0.5 }

    #[inline]
    pub fn size(&self) -> Vec3 { self.max - self.min }

    #[inline]
    pub fn half_extents(&self) -> Vec3 { self.size() * 0.5 }

    /// Grow the box by `amount` on every axis.
    #[inline]
    pub fn expanded(&self, amount: f32) -> Aabb {
        Aabb { min: self.min - Vec3::splat(amount), max: self.max + Vec3::splat(amount) }
    }

    /// Grow the box by another box's half-extents -- the Minkowski expansion
    /// that turns a swept-box trace into a swept-point trace.
    #[inline]
    pub fn expanded_by(&self, half: Vec3) -> Aabb {
        Aabb { min: self.min - half, max: self.max + half }
    }

    #[inline]
    pub fn contains_point(&self, p: Vec3) -> bool {
        p.x >= self.min.x && p.x <= self.max.x
            && p.y >= self.min.y && p.y <= self.max.y
            && p.z >= self.min.z && p.z <= self.max.z
    }

    #[inline]
    pub fn intersects(&self, other: &Aabb) -> bool {
        self.min.x <= other.max.x && self.max.x >= other.min.x
            && self.min.y <= other.max.y && self.max.y >= other.min.y
            && self.min.z <= other.max.z && self.max.z >= other.min.z
    }

    /// The eight corners, in a fixed order.
    pub fn corners(&self) -> [Vec3; 8] {
        [
            Vec3::new(self.min.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.max.y, self.max.z),
            Vec3::new(self.min.x, self.max.y, self.max.z),
        ]
    }

    /// Which side of a plane the whole box is on.
    pub fn classify(&self, plane: &Plane) -> PlaneSide {
        let (lo, hi) = plane.box_distances(self.center(), self.half_extents());
        if lo > 0.0 { PlaneSide::Front }
        else if hi < 0.0 { PlaneSide::Back }
        else { PlaneSide::Cross }
    }

    /// Longest axis (0/1/2) -- the one a spatial split should cut.
    pub fn longest_axis(&self) -> usize {
        let s = self.size();
        if s.x >= s.y && s.x >= s.z { 0 } else if s.y >= s.z { 1 } else { 2 }
    }

    /// Snap outward to whole units, which is what the BSP lumps store.
    pub fn rounded_out(&self) -> Aabb {
        Aabb { min: self.min.floor(), max: self.max.ceil() }
    }
}

impl Default for Aabb {
    fn default() -> Self { Self::EMPTY }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_box_grows_from_first_point() {
        let mut b = Aabb::EMPTY;
        assert!(b.is_empty());
        b.add_point(Vec3::new(5.0, 5.0, 5.0));
        assert!(!b.is_empty());
        assert_eq!(b.min, b.max);
    }

    #[test]
    fn union_with_empty_is_identity() {
        let b = Aabb::new(Vec3::ZERO, Vec3::splat(10.0));
        assert_eq!(b.union(&Aabb::EMPTY), b);
        assert_eq!(Aabb::EMPTY.union(&b), b);
    }

    #[test]
    fn classify_against_plane() {
        let b = Aabb::new(Vec3::ZERO, Vec3::splat(16.0));
        assert_eq!(b.classify(&Plane::new(Vec3::Z, -10.0)), PlaneSide::Front);
        assert_eq!(b.classify(&Plane::new(Vec3::Z, 100.0)), PlaneSide::Back);
        assert_eq!(b.classify(&Plane::new(Vec3::Z, 8.0)), PlaneSide::Cross);
    }

    #[test]
    fn intersects_is_symmetric_and_touching_counts() {
        let a = Aabb::new(Vec3::ZERO, Vec3::splat(10.0));
        let b = Aabb::new(Vec3::splat(10.0), Vec3::splat(20.0));
        assert!(a.intersects(&b) && b.intersects(&a));
        let c = Aabb::new(Vec3::splat(11.0), Vec3::splat(20.0));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn longest_axis_picks_the_widest() {
        assert_eq!(Aabb::new(Vec3::ZERO, Vec3::new(1.0, 5.0, 2.0)).longest_axis(), 1);
        assert_eq!(Aabb::new(Vec3::ZERO, Vec3::new(9.0, 5.0, 2.0)).longest_axis(), 0);
        assert_eq!(Aabb::new(Vec3::ZERO, Vec3::new(1.0, 5.0, 9.0)).longest_axis(), 2);
    }
}
