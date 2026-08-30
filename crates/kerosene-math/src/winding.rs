// SPDX-License-Identifier: LGPL-3.0-or-later
use crate::{Aabb, MAX_MAP_COORD, Plane, PlaneSide, major_axis};
use glam::Vec3;

/// A convex polygon in 3D, stored as an ordered ring of points.
///
/// Windings are the working currency of the map compiler: every brush face
/// starts as an infinite plane, gets cut down to a polygon by its sibling
/// faces, and is then split repeatedly as it is filed into the BSP tree.
///
/// **Point order is clockwise when viewed from the front** (the side the
/// normal points at), matching the `.map`/`.keromap` brush convention. Renderers
/// that want counter-clockwise triangles must reverse, and
/// [`Winding::triangulate_ccw`] does exactly that.
#[derive(Clone, Debug, PartialEq)]
pub struct Winding {
    pub points: Vec<Vec3>,
}

/// Edges shorter than this do not count toward a winding being real geometry.
const EDGE_LENGTH: f32 = 0.2;

impl Winding {
    pub fn new(points: Vec<Vec3>) -> Self { Self { points } }

    pub fn len(&self) -> usize { self.points.len() }
    pub fn is_empty(&self) -> bool { self.points.len() < 3 }

    /// The largest polygon that fits on `plane` inside the legal world.
    ///
    /// Face construction works subtractively: start with this, then clip it by
    /// every other plane of the brush. What survives is the face.
    pub fn base_for_plane(plane: &Plane) -> Self {
        let n = plane.normal;
        // Pick any axis not parallel to the normal, then orthogonalise it.
        let mut up = match major_axis(n) {
            0 | 1 => Vec3::Z,
            _ => Vec3::X,
        };
        up -= n * up.dot(n);
        let up = up.normalize();
        let right = up.cross(n);

        let org = n * plane.dist;
        // Generous enough to cover the world cube from any orientation.
        let r = MAX_MAP_COORD * 2.0;
        let (up, right) = (up * r, right * r);

        Self::new(vec![
            org - right + up,
            org + right + up,
            org + right - up,
            org - right - up,
        ])
    }

    /// Plane this winding lies in, derived from its first three points.
    pub fn plane(&self) -> Option<Plane> {
        if self.points.len() < 3 { return None; }
        // Scan for a non-degenerate triple; the first three points can be
        // nearly collinear on a sliver that survived clipping.
        let p0 = self.points[0];
        for i in 1..self.points.len() - 1 {
            if let Some(p) = Plane::from_map_points(p0, self.points[i], self.points[i + 1]) {
                return Some(p);
            }
        }
        None
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::from_points(&self.points)
    }

    pub fn center(&self) -> Vec3 {
        if self.points.is_empty() { return Vec3::ZERO; }
        self.points.iter().copied().sum::<Vec3>() / self.points.len() as f32
    }

    /// Surface area, via a triangle fan from the first vertex.
    pub fn area(&self) -> f32 {
        let mut total = 0.0;
        for i in 2..self.points.len() {
            let a = self.points[i - 1] - self.points[0];
            let b = self.points[i] - self.points[0];
            total += a.cross(b).length() * 0.5;
        }
        total
    }

    /// Reverse the ring, flipping which side counts as the front.
    pub fn reverse(&mut self) { self.points.reverse(); }

    pub fn reversed(&self) -> Self {
        let mut w = self.clone();
        w.reverse();
        w
    }

    /// Classify the whole winding against a plane.
    pub fn classify(&self, plane: &Plane, epsilon: f32) -> PlaneSide {
        let (mut front, mut back) = (false, false);
        for &p in &self.points {
            let d = plane.distance_to(p);
            if d > epsilon { front = true; }
            else if d < -epsilon { back = true; }
            if front && back { return PlaneSide::Cross; }
        }
        match (front, back) {
            (true, false) => PlaneSide::Front,
            (false, true) => PlaneSide::Back,
            (false, false) => PlaneSide::On,
            (true, true) => PlaneSide::Cross,
        }
    }

    /// Split into the parts in front of and behind `plane`.
    ///
    /// This is Quake's `ClipWindingEpsilon`, kept faithful down to the
    /// axial-snap in the interpolation: when the cutting plane is axis
    /// aligned, the generated point takes the plane's coordinate *exactly*
    /// rather than an interpolated approximation of it. Without that, a
    /// polygon that gets split a dozen times drifts off its own plane and the
    /// tree develops leaks.
    pub fn split(&self, plane: &Plane, epsilon: f32) -> (Option<Winding>, Option<Winding>) {
        let n = self.points.len();
        if n < 3 { return (None, None); }

        let mut dists = Vec::with_capacity(n + 1);
        let mut sides = Vec::with_capacity(n + 1);
        let (mut counts_front, mut counts_back) = (0usize, 0usize);

        for &p in &self.points {
            let d = plane.distance_to(p);
            let s = if d > epsilon {
                counts_front += 1;
                PlaneSide::Front
            } else if d < -epsilon {
                counts_back += 1;
                PlaneSide::Back
            } else {
                PlaneSide::On
            };
            dists.push(d);
            sides.push(s);
        }
        // Wrap-around sentinel so the edge loop can look at `i + 1` freely.
        sides.push(sides[0]);
        dists.push(dists[0]);

        if counts_front == 0 { return (None, Some(self.clone())); }
        if counts_back == 0 { return (Some(self.clone()), None); }

        let mut front = Vec::with_capacity(n + 4);
        let mut back = Vec::with_capacity(n + 4);

        for i in 0..n {
            let p1 = self.points[i];
            match sides[i] {
                PlaneSide::On => {
                    front.push(p1);
                    back.push(p1);
                    continue;
                }
                PlaneSide::Front => front.push(p1),
                PlaneSide::Back => back.push(p1),
                PlaneSide::Cross => unreachable!("per-point classification is never Cross"),
            }

            // Only emit a crossing point when this edge actually changes side.
            if sides[i + 1] == PlaneSide::On || sides[i + 1] == sides[i] { continue; }

            let p2 = self.points[(i + 1) % n];
            let t = dists[i] / (dists[i] - dists[i + 1]);
            let mut mid = Vec3::ZERO;
            for axis in 0..3 {
                mid[axis] = if plane.normal[axis] == 1.0 {
                    plane.dist
                } else if plane.normal[axis] == -1.0 {
                    -plane.dist
                } else {
                    p1[axis] + t * (p2[axis] - p1[axis])
                };
            }
            front.push(mid);
            back.push(mid);
        }

        let f = (front.len() >= 3).then(|| Winding::new(front));
        let b = (back.len() >= 3).then(|| Winding::new(back));
        (f, b)
    }

    /// Keep only the part in front of `plane`. Returns `None` if nothing is left.
    pub fn clipped(&self, plane: &Plane, epsilon: f32) -> Option<Winding> {
        self.split(plane, epsilon).0
    }

    /// Clip in place, reporting whether anything survived.
    pub fn clip(&mut self, plane: &Plane, epsilon: f32) -> bool {
        match self.clipped(plane, epsilon) {
            Some(w) => { *self = w; true }
            None => { self.points.clear(); false }
        }
    }

    /// Drop vertices that sit on the straight line between their neighbours.
    ///
    /// Repeated splitting leaves behind points that no longer turn a corner.
    /// They are harmless geometrically but they inflate every downstream lump,
    /// so the compiler sheds them before writing faces out.
    pub fn remove_collinear(&mut self) {
        if self.points.len() < 3 { return; }
        let mut out: Vec<Vec3> = Vec::with_capacity(self.points.len());
        let n = self.points.len();
        for i in 0..n {
            let prev = self.points[(i + n - 1) % n];
            let cur = self.points[i];
            let next = self.points[(i + 1) % n];
            let a = (cur - prev).normalize_or_zero();
            let b = (next - cur).normalize_or_zero();
            // Keep the vertex if the direction actually changes there.
            if a.dot(b) < 0.999_99 { out.push(cur); }
        }
        if out.len() >= 3 { self.points = out; }
    }

    /// Whether this winding is too small to be real geometry.
    ///
    /// A polygon needs three edges of meaningful length; anything less is a
    /// sliver thrown off by a near-tangent split and is discarded rather than
    /// carried through the compile.
    pub fn is_tiny(&self) -> bool {
        let n = self.points.len();
        if n < 3 { return true; }
        let mut edges = 0;
        for i in 0..n {
            let d = self.points[(i + 1) % n] - self.points[i];
            if d.length() > EDGE_LENGTH {
                edges += 1;
                if edges == 3 { return false; }
            }
        }
        true
    }

    /// Whether any vertex has escaped the legal world -- a sign the winding
    /// was never properly bounded.
    pub fn is_huge(&self) -> bool {
        self.points.iter().any(|p| {
            p.x.abs() > MAX_MAP_COORD || p.y.abs() > MAX_MAP_COORD || p.z.abs() > MAX_MAP_COORD
        })
    }

    /// Triangle indices into `points`, wound counter-clockwise from the front.
    ///
    /// Windings are stored clockwise (the brush-file convention) but GPUs
    /// default to counter-clockwise front faces, so the fan is emitted in
    /// reverse here rather than making every call site remember.
    pub fn triangulate_ccw(&self) -> Vec<[u32; 3]> {
        (2..self.points.len())
            .map(|i| [0u32, i as u32, (i - 1) as u32])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ON_EPSILON;

    fn unit_square() -> Winding {
        // Clockwise seen from +Z, i.e. facing up.
        Winding::new(vec![
            Vec3::new(0.0, 16.0, 0.0),
            Vec3::new(16.0, 16.0, 0.0),
            Vec3::new(16.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
        ])
    }

    #[test]
    fn base_winding_faces_its_plane() {
        for n in [Vec3::Z, -Vec3::Z, Vec3::X, Vec3::new(1.0, 1.0, 1.0).normalize()] {
            let plane = Plane::new(n, 32.0);
            let w = Winding::base_for_plane(&plane);
            assert_eq!(w.len(), 4);
            let derived = w.plane().unwrap();
            assert!((derived.normal - n).length() < 1e-4, "{:?} vs {n:?}", derived.normal);
            assert!((derived.dist - 32.0).abs() < 1e-2);
        }
    }

    #[test]
    fn winding_order_is_clockwise_from_front() {
        let w = unit_square();
        assert_eq!(w.plane().unwrap().normal, Vec3::Z);
    }

    #[test]
    fn area_of_square() {
        assert!((unit_square().area() - 256.0).abs() < 1e-3);
    }

    #[test]
    fn split_halves_a_square() {
        let w = unit_square();
        let plane = Plane::new(Vec3::X, 8.0);
        let (front, back) = w.split(&plane, ON_EPSILON);
        let (front, back) = (front.unwrap(), back.unwrap());
        assert!((front.area() - 128.0).abs() < 1e-3, "{}", front.area());
        assert!((back.area() - 128.0).abs() < 1e-3, "{}", back.area());
        // The two halves must add back up to the original.
        assert!((front.area() + back.area() - 256.0).abs() < 1e-3);
    }

    #[test]
    fn split_that_misses_returns_the_whole_thing() {
        let w = unit_square();
        let (f, b) = w.split(&Plane::new(Vec3::X, -100.0), ON_EPSILON);
        assert!(f.is_some() && b.is_none());
        let (f, b) = w.split(&Plane::new(Vec3::X, 100.0), ON_EPSILON);
        assert!(f.is_none() && b.is_some());
    }

    #[test]
    fn split_points_land_exactly_on_axial_planes() {
        let w = Winding::new(vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 100.0, 0.0),
            Vec3::new(97.3, 100.0, 0.0),
            Vec3::new(97.3, 0.0, 0.0),
        ]);
        let plane = Plane::new(Vec3::X, 31.0);
        let (front, _) = w.split(&plane, ON_EPSILON);
        for p in &front.unwrap().points {
            assert!(p.x >= 31.0 - 1e-6, "point drifted behind the cut: {p:?}");
        }
    }

    #[test]
    fn repeated_splits_stay_on_plane() {
        // The failure this guards: drift off the source plane after many cuts.
        let plane = Plane::new(Vec3::Z, 0.0);
        let mut w = Winding::base_for_plane(&plane);
        for i in 1..40 {
            let cut = Plane::new(Vec3::new(1.0, (i as f32) * 0.05, 0.0).normalize(), i as f32);
            w = w.clipped(&cut, ON_EPSILON).expect("should still have area");
        }
        for p in &w.points {
            assert!(p.z.abs() < 1e-3, "drifted off plane after 40 splits: {p:?}");
        }
    }

    #[test]
    fn collinear_points_are_dropped() {
        let mut w = Winding::new(vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(8.0, 0.0, 0.0),   // sits on the edge 0 -> 16
            Vec3::new(16.0, 0.0, 0.0),
            Vec3::new(16.0, 16.0, 0.0),
            Vec3::new(0.0, 16.0, 0.0),
        ]);
        w.remove_collinear();
        assert_eq!(w.len(), 4);
    }

    #[test]
    fn tiny_windings_are_detected() {
        assert!(Winding::new(vec![Vec3::ZERO, Vec3::X * 0.01, Vec3::Y * 0.01]).is_tiny());
        assert!(!unit_square().is_tiny());
    }

    #[test]
    fn clipping_by_six_planes_makes_a_box_face() {
        // Build the +Z face of a 64-cube exactly the way Cleave does: start
        // with the whole plane, then clip by every *other* face of the brush
        // turned inward.
        let top = Plane::new(Vec3::Z, 64.0);
        let mut w = Winding::base_for_plane(&top);
        let side_faces = [
            Plane::new(-Vec3::X, 0.0),   // outward normal of the -X face
            Plane::new(Vec3::X, 64.0),   // outward normal of the +X face
            Plane::new(-Vec3::Y, 0.0),
            Plane::new(Vec3::Y, 64.0),
        ];
        for face in side_faces {
            w = w.clipped(&face.flipped(), ON_EPSILON).expect("face survives");
        }
        assert_eq!(w.len(), 4);
        assert!((w.area() - 64.0 * 64.0).abs() < 1e-2, "area {}", w.area());
    }
}
