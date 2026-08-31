// SPDX-License-Identifier: MPL-2.0
use crate::{NORMAL_EPSILON, ON_EPSILON, PLANE_DIST_EPSILON, major_axis, snap_normal};
use glam::Vec3;
use std::collections::HashMap;

/// Which side of a plane something lies on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaneSide {
    /// Entirely in the plane's positive half-space (the direction the normal points).
    Front,
    /// Entirely in the negative half-space.
    Back,
    /// Coplanar within [`ON_EPSILON`].
    On,
    /// Straddles the plane -- only ever returned for extended shapes.
    Cross,
}

/// A plane's orientation, cached so traces can take axial fast paths.
///
/// The overwhelming majority of planes in a brush-built map are axis-aligned,
/// and knowing that up front turns a dot product into a single component read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum PlaneKind {
    X = 0,
    Y = 1,
    Z = 2,
    /// Non-axial. The stored axis is the one the normal leans on most, which
    /// the BSP builder uses to prefer axial splits.
    AnyX = 3,
    AnyY = 4,
    AnyZ = 5,
}

impl PlaneKind {
    #[inline]
    pub fn is_axial(self) -> bool { (self as u8) < 3 }
}

/// An infinite plane: the set of points `p` where `normal . p == dist`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Plane {
    pub normal: Vec3,
    pub dist: f32,
}

impl Plane {
    #[inline]
    pub const fn new(normal: Vec3, dist: f32) -> Self { Self { normal, dist } }

    /// Plane through `point` facing along `normal`.
    #[inline]
    pub fn from_point_normal(point: Vec3, normal: Vec3) -> Self {
        Self { normal, dist: normal.dot(point) }
    }

    /// Plane from three points in the `.keromap` / Quake `.map` brush convention.
    ///
    /// The three points are listed **clockwise when viewed from the front of
    /// the face**, so the normal comes out as `(p0 - p1) x (p2 - p1)`. This
    /// looks like the wrong cross product and is not: every brush face ever
    /// written to a `.map`-lineage file depends on exactly this ordering.
    ///
    /// Returns `None` if the points are collinear.
    pub fn from_map_points(p0: Vec3, p1: Vec3, p2: Vec3) -> Option<Self> {
        let n = (p0 - p1).cross(p2 - p1);
        if n.length_squared() < 1e-12 { return None; }
        let n = snap_normal(n.normalize());
        Some(Self { normal: n, dist: n.dot(p1) })
    }

    /// Plane from three points wound counter-clockwise about the normal --
    /// the usual convention everywhere outside brush files.
    pub fn from_points_ccw(p0: Vec3, p1: Vec3, p2: Vec3) -> Option<Self> {
        let n = (p1 - p0).cross(p2 - p0);
        if n.length_squared() < 1e-12 { return None; }
        let n = snap_normal(n.normalize());
        Some(Self { normal: n, dist: n.dot(p0) })
    }

    /// Signed distance from `p` to the plane; positive is in front.
    #[inline]
    pub fn distance_to(&self, p: Vec3) -> f32 { self.normal.dot(p) - self.dist }

    /// Classify a single point, with [`ON_EPSILON`] slack.
    #[inline]
    pub fn classify_point(&self, p: Vec3) -> PlaneSide {
        let d = self.distance_to(p);
        if d > ON_EPSILON { PlaneSide::Front }
        else if d < -ON_EPSILON { PlaneSide::Back }
        else { PlaneSide::On }
    }

    /// The same plane facing the other way.
    #[inline]
    pub fn flipped(&self) -> Self { Self { normal: -self.normal, dist: -self.dist } }

    /// Project `p` onto the plane.
    #[inline]
    pub fn project(&self, p: Vec3) -> Vec3 { p - self.normal * self.distance_to(p) }

    /// Where the segment `a -> b` crosses the plane, as a parameter in `[0,1]`.
    ///
    /// Returns `None` when the segment is parallel to the plane.
    pub fn intersect_segment_t(&self, a: Vec3, b: Vec3) -> Option<f32> {
        let da = self.distance_to(a);
        let db = self.distance_to(b);
        let denom = da - db;
        if denom.abs() < 1e-9 { return None; }
        Some(da / denom)
    }

    /// The point where the segment `a -> b` crosses the plane.
    pub fn intersect_segment(&self, a: Vec3, b: Vec3) -> Option<Vec3> {
        let t = self.intersect_segment_t(a, b)?;
        // Interpolate per-axis, and take the plane's own value on any axis the
        // plane is exactly aligned to. Without this, a long edge crossing an
        // axial plane lands a fraction of a unit off it and the error
        // compounds through every later split.
        let mut p = a + (b - a) * t;
        for axis in 0..3 {
            if self.normal[axis] == 1.0 { p[axis] = self.dist; }
            else if self.normal[axis] == -1.0 { p[axis] = -self.dist; }
        }
        Some(p)
    }

    /// Cached orientation, used to pick trace fast paths and split axes.
    pub fn kind(&self) -> PlaneKind {
        let n = self.normal;
        if n.x == 1.0 || n.x == -1.0 { return PlaneKind::X; }
        if n.y == 1.0 || n.y == -1.0 { return PlaneKind::Y; }
        if n.z == 1.0 || n.z == -1.0 { return PlaneKind::Z; }
        match major_axis(n) {
            0 => PlaneKind::AnyX,
            1 => PlaneKind::AnyY,
            _ => PlaneKind::AnyZ,
        }
    }

    /// Distance from the plane to the nearest corner of a box, measured along
    /// the normal. Returns `(min, max)` -- if `min > 0` the box is fully in
    /// front, if `max < 0` fully behind.
    ///
    /// This is the standard "push the box extents onto the normal" test, which
    /// avoids testing all eight corners.
    #[inline]
    pub fn box_distances(&self, center: Vec3, half: Vec3) -> (f32, f32) {
        let d = self.distance_to(center);
        let r = half.x * self.normal.x.abs()
              + half.y * self.normal.y.abs()
              + half.z * self.normal.z.abs();
        (d - r, d + r)
    }

    /// Whether two planes are the same plane, within the dedup tolerances.
    pub fn approx_eq(&self, other: &Plane) -> bool {
        (self.normal.x - other.normal.x).abs() < NORMAL_EPSILON
            && (self.normal.y - other.normal.y).abs() < NORMAL_EPSILON
            && (self.normal.z - other.normal.z).abs() < NORMAL_EPSILON
            && (self.dist - other.dist).abs() < PLANE_DIST_EPSILON
    }
}

/// Interning table that collapses duplicate planes and stores each one
/// alongside its opposite.
///
/// Planes always live in pairs at indices `2k` and `2k + 1`, so flipping a
/// plane reference is `index ^ 1`. The BSP format and every compiler stage
/// rely on that identity: a node stores one plane index and its two children
/// implicitly use the plane and its inverse.
///
/// Interning matters for more than file size. Two brush faces that were meant
/// to be coplanar must end up sharing *one* plane index, or the tree splits
/// along a hair's-width wedge between them and the compile explodes.
#[derive(Default, Clone)]
pub struct PlaneSet {
    planes: Vec<Plane>,
    /// Buckets keyed by truncated `|dist|`; each holds indices into `planes`.
    buckets: HashMap<i64, Vec<u32>>,
}

impl PlaneSet {
    pub fn new() -> Self { Self::default() }

    pub fn len(&self) -> usize { self.planes.len() }
    pub fn is_empty(&self) -> bool { self.planes.is_empty() }
    pub fn planes(&self) -> &[Plane] { &self.planes }

    #[inline]
    pub fn get(&self, index: u32) -> Plane { self.planes[index as usize] }

    /// Intern a plane, returning its index.
    ///
    /// If the plane (or its inverse) is already present the existing index is
    /// reused -- `index ^ 1` when the match was against the inverse.
    pub fn insert(&mut self, plane: Plane) -> u32 {
        let plane = Plane {
            normal: snap_normal(plane.normal.normalize_or_zero()),
            dist: snap_dist(plane.dist),
        };

        if let Some(i) = self.find(&plane) { return i; }
        let flipped = plane.flipped();
        if let Some(i) = self.find(&flipped) { return i ^ 1; }

        // Store the canonical orientation first so that the pair ordering is
        // reproducible between runs -- a compile that shuffles plane indices
        // produces gratuitously different .kerobsp files.
        let base = self.planes.len() as u32;
        let (a, b) = if is_canonical(plane.normal) { (plane, flipped) } else { (flipped, plane) };
        self.push(a);
        self.push(b);
        if is_canonical(plane.normal) { base } else { base + 1 }
    }

    fn push(&mut self, p: Plane) {
        let idx = self.planes.len() as u32;
        self.buckets.entry(bucket_of(p.dist)).or_default().push(idx);
        self.planes.push(p);
    }

    fn find(&self, plane: &Plane) -> Option<u32> {
        let k = bucket_of(plane.dist);
        // A plane sitting near a bucket boundary can legitimately match an
        // entry filed one bucket over, so sweep the neighbours too.
        for key in [k - 1, k, k + 1] {
            if let Some(list) = self.buckets.get(&key) {
                for &i in list {
                    if self.planes[i as usize].approx_eq(plane) { return Some(i); }
                }
            }
        }
        None
    }
}

#[inline]
fn bucket_of(dist: f32) -> i64 { dist.floor() as i64 }

/// Round a plane distance to 1/8 unit when it is very close to it.
///
/// Brush vertices land on integers or simple fractions; letting accumulated
/// float error keep a plane at 63.999996 instead of 64 defeats interning.
#[inline]
fn snap_dist(d: f32) -> f32 {
    let r = (d * 8.0).round() / 8.0;
    if (d - r).abs() < PLANE_DIST_EPSILON { r } else { d }
}

/// Whether a normal is in the orientation we store first in a plane pair.
///
/// The rule only has to be deterministic, not meaningful: positive along the
/// dominant axis, matching Source's habit of filing axial planes normal-positive.
fn is_canonical(n: Vec3) -> bool {
    let ax = major_axis(n);
    if n[ax] > 0.0 { return true; }
    if n[ax] < 0.0 { return false; }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_points_face_outward() {
        // The +Z face of a box. Listed clockwise as seen from the front,
        // i.e. from above looking down -- which reads counter-clockwise on a
        // page with X right and Y up, and is exactly the trap this checks.
        let p = Plane::from_map_points(
            Vec3::new(0.0, 0.0, 64.0),
            Vec3::new(0.0, 64.0, 64.0),
            Vec3::new(64.0, 64.0, 64.0),
        ).unwrap();
        assert_eq!(p.normal, Vec3::Z);
        assert_eq!(p.dist, 64.0);
    }

    #[test]
    fn distance_sign_matches_normal() {
        let p = Plane::new(Vec3::Z, 10.0);
        assert!(p.distance_to(Vec3::new(0.0, 0.0, 20.0)) > 0.0);
        assert!(p.distance_to(Vec3::ZERO) < 0.0);
        assert_eq!(p.classify_point(Vec3::new(5.0, 5.0, 10.0)), PlaneSide::On);
    }

    #[test]
    fn segment_intersection_snaps_to_axial_planes() {
        let p = Plane::new(Vec3::Z, 32.0);
        let hit = p.intersect_segment(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 7.0, 64.0)).unwrap();
        // Exactly on the plane, not merely near it.
        assert_eq!(hit.z, 32.0);
    }

    #[test]
    fn plane_pairs_are_index_xor_one() {
        let mut set = PlaneSet::new();
        let a = set.insert(Plane::new(Vec3::Z, 64.0));
        let b = set.insert(Plane::new(-Vec3::Z, -64.0));
        assert_eq!(a ^ 1, b);
        assert_eq!(set.len(), 2, "the inverse must not allocate a new pair");
        assert_eq!(set.get(a).normal, Vec3::Z);
        assert_eq!(set.get(b).normal, -Vec3::Z);
    }

    #[test]
    fn near_identical_planes_intern_together() {
        let mut set = PlaneSet::new();
        let a = set.insert(Plane::new(Vec3::new(0.0, 0.0, 1.0), 64.0));
        let b = set.insert(Plane::new(Vec3::new(1e-7, -1e-7, 1.0).normalize(), 64.000_004));
        assert_eq!(a, b);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn distinct_planes_stay_distinct() {
        let mut set = PlaneSet::new();
        let a = set.insert(Plane::new(Vec3::Z, 64.0));
        let b = set.insert(Plane::new(Vec3::Z, 65.0));
        assert_ne!(a, b);
        assert_ne!(a ^ 1, b);
        assert_eq!(set.len(), 4);
    }

    #[test]
    fn interning_survives_bucket_boundaries() {
        // dist just under and just over an integer boundary -- different
        // buckets, same plane.
        let mut set = PlaneSet::new();
        let a = set.insert(Plane::new(Vec3::X, 63.999_9));
        let b = set.insert(Plane::new(Vec3::X, 64.000_1));
        assert_eq!(a, b, "bucket boundary must not split one plane in two");
    }

    #[test]
    fn box_distances_bracket_the_corners() {
        let p = Plane::new(Vec3::new(1.0, 1.0, 0.0).normalize(), 0.0);
        let (lo, hi) = p.box_distances(Vec3::ZERO, Vec3::splat(8.0));
        assert!(lo < 0.0 && hi > 0.0, "box straddling the plane: {lo} {hi}");
        let (lo, _) = p.box_distances(Vec3::new(100.0, 100.0, 0.0), Vec3::splat(8.0));
        assert!(lo > 0.0, "box fully in front");
    }
}
