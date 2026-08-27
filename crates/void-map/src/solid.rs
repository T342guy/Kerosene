//! Brush solids and their faces.

use crate::texture::{TextureAxis, default_axes_for_plane};
use crate::{DEFAULT_LIGHTMAP_SCALE, read_id};
use thiserror::Error;
use void_kv::{KeyValues, Vec3Value};
use void_math::{Aabb, MAX_MAP_COORD, ON_EPSILON, Plane, Vec3, Winding};

#[derive(Debug, Error)]
pub enum SolidError {
    #[error("has {0} faces; a closed convex solid needs at least 4")]
    TooFewFaces(usize),
    #[error("face {id} has a degenerate plane (its three points are collinear)")]
    DegeneratePlane { id: u32 },
    #[error("face {id} could not be spelled as three points")]
    UnparseablePlane { id: u32 },
    #[error("encloses no volume -- only {0} of its faces bound anything")]
    NotClosed(usize),
    #[error("extends beyond the {MAX_MAP_COORD}-unit world boundary")]
    OutOfBounds,
}

/// One face of a brush.
///
/// The plane is stored as three points rather than a normal and distance,
/// exactly as `.map` and `.vmf` do. It is more verbose and it re-derives the
/// normal on every load, but it survives hand editing far better: three
/// integer points describe an exact plane, while a normalised normal written
/// to six decimal places does not.
#[derive(Clone, Debug, PartialEq)]
pub struct Side {
    pub id: u32,
    /// Three points on the plane, clockwise seen from the front of the face.
    pub plane_points: [Vec3; 3],
    /// Material path relative to `materials/`, without extension.
    pub material: String,
    pub uaxis: TextureAxis,
    pub vaxis: TextureAxis,
    pub rotation: f32,
    /// World units per lightmap luxel on this face.
    pub lightmap_scale: f32,
    /// Faces sharing a smoothing group get their vertex normals averaged, so a
    /// faceted curve shades as a smooth one.
    pub smoothing_groups: u32,
}

impl Side {
    /// A face on the given plane, with default world-aligned texture axes.
    pub fn from_plane(id: u32, plane: Plane, material: &str) -> Side {
        let (uaxis, vaxis) = default_axes_for_plane(&plane, 0.25);
        Side {
            id,
            plane_points: points_for_plane(&plane),
            material: material.to_string(),
            uaxis,
            vaxis,
            rotation: 0.0,
            lightmap_scale: DEFAULT_LIGHTMAP_SCALE,
            smoothing_groups: 0,
        }
    }

    /// The plane this face lies in, or `None` if its points are collinear.
    pub fn plane(&self) -> Option<Plane> {
        Plane::from_map_points(self.plane_points[0], self.plane_points[1], self.plane_points[2])
    }

    /// Texture coordinates of a world point, in texels.
    pub fn texcoord(&self, point: Vec3) -> (f32, f32) {
        (self.uaxis.project(point), self.vaxis.project(point))
    }

    /// Whether this face uses a `tools/` material, which never renders.
    pub fn is_tool_material(&self) -> bool {
        self.material.to_lowercase().starts_with("tools/")
    }

    pub(crate) fn from_kv(kv: &KeyValues) -> Result<Side, SolidError> {
        let id = read_id(kv);
        let plane_points = kv
            .get("plane")
            .and_then(parse_plane_points)
            .ok_or(SolidError::UnparseablePlane { id })?;

        let plane = Plane::from_map_points(plane_points[0], plane_points[1], plane_points[2])
            .ok_or(SolidError::DegeneratePlane { id })?;
        let (default_u, default_v) = default_axes_for_plane(&plane, 0.25);

        Ok(Side {
            id,
            plane_points,
            material: kv.get("material").unwrap_or("dev/grid").to_string(),
            uaxis: kv.get("uaxis").and_then(TextureAxis::parse).unwrap_or(default_u),
            vaxis: kv.get("vaxis").and_then(TextureAxis::parse).unwrap_or(default_v),
            rotation: kv.get_or("rotation", 0.0f32),
            lightmap_scale: kv.get_or("lightmapscale", DEFAULT_LIGHTMAP_SCALE),
            smoothing_groups: kv.get_or("smoothing_groups", 0u32),
        })
    }

    pub(crate) fn to_kv(&self) -> KeyValues {
        use void_kv::format_float as f;
        let mut kv = KeyValues::new("side");
        kv.push_value("id", self.id);
        let p = &self.plane_points;
        kv.push(
            "plane",
            format!(
                "({} {} {}) ({} {} {}) ({} {} {})",
                f(p[0].x), f(p[0].y), f(p[0].z),
                f(p[1].x), f(p[1].y), f(p[1].z),
                f(p[2].x), f(p[2].y), f(p[2].z),
            ),
        );
        kv.push("material", self.material.clone());
        kv.push("uaxis", self.uaxis.to_kv());
        kv.push("vaxis", self.vaxis.to_kv());
        kv.push_value("rotation", self.rotation);
        kv.push_value("lightmapscale", self.lightmap_scale);
        kv.push_value("smoothing_groups", self.smoothing_groups);
        kv
    }
}

/// Parse `"(x y z) (x y z) (x y z)"`.
fn parse_plane_points(s: &str) -> Option<[Vec3; 3]> {
    use void_kv::FromKvValue;
    let mut points = [Vec3::ZERO; 3];
    let mut n = 0;
    for group in s.split(')') {
        let Some(open) = group.find('(') else { continue };
        if n == 3 { return None; }
        let v = Vec3Value::from_kv(&group[open + 1..]).ok()?;
        points[n] = Vec3::from_array(v.to_array());
        n += 1;
    }
    (n == 3).then_some(points)
}

/// Three points spelling out a plane, in the clockwise-from-front order the
/// format expects.
///
/// Chosen so that `(p0 - p1) x (p2 - p1)` reproduces the normal exactly: with
/// `u x v = n`, taking `p0 = o + u`, `p1 = o`, `p2 = o + v` gives back `n`.
fn points_for_plane(plane: &Plane) -> [Vec3; 3] {
    let n = plane.normal;
    let helper = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = helper.cross(n).normalize();
    let v = n.cross(u);
    let org = n * plane.dist;
    const SPAN: f32 = 64.0;
    [org + u * SPAN, org, org + v * SPAN]
}

/// A convex brush: the intersection of its faces' half-spaces.
#[derive(Clone, Debug, PartialEq)]
pub struct Solid {
    pub id: u32,
    pub sides: Vec<Side>,
}

impl Solid {
    pub fn new(id: u32, sides: Vec<Side>) -> Self { Self { id, sides } }

    /// An axis-aligned box brush -- what the block tool produces.
    ///
    /// Points are written as real corners rather than derived from the planes,
    /// so a generated `.voidmap` reads the way a hand-authored one does.
    pub fn cube(bounds: Aabb, material: &str) -> Solid {
        let (lo, hi) = (bounds.min, bounds.max);
        let d = bounds.size();
        // Per face: an origin corner and two tangents whose cross product is
        // the outward normal.
        let faces: [(Vec3, Vec3, Vec3); 6] = [
            (Vec3::new(lo.x, lo.y, hi.z), Vec3::X * d.x, Vec3::Y * d.y),   // +Z
            (Vec3::new(lo.x, lo.y, lo.z), Vec3::Y * d.y, Vec3::X * d.x),   // -Z
            (Vec3::new(hi.x, lo.y, lo.z), Vec3::Y * d.y, Vec3::Z * d.z),   // +X
            (Vec3::new(lo.x, lo.y, lo.z), Vec3::Z * d.z, Vec3::Y * d.y),   // -X
            (Vec3::new(lo.x, hi.y, lo.z), Vec3::Z * d.z, Vec3::X * d.x),   // +Y
            (Vec3::new(lo.x, lo.y, lo.z), Vec3::X * d.x, Vec3::Z * d.z),   // -Y
        ];

        let sides = faces
            .iter()
            .enumerate()
            .map(|(i, &(o, a, b))| {
                let points = [o + a, o, o + b];
                let plane = Plane::from_map_points(points[0], points[1], points[2])
                    .expect("box faces are never degenerate");
                let (uaxis, vaxis) = default_axes_for_plane(&plane, 0.25);
                Side {
                    id: i as u32 + 1,
                    plane_points: points,
                    material: material.to_string(),
                    uaxis,
                    vaxis,
                    rotation: 0.0,
                    lightmap_scale: DEFAULT_LIGHTMAP_SCALE,
                    smoothing_groups: 0,
                }
            })
            .collect();

        Solid { id: 0, sides }
    }

    /// The planes of every face, outward facing.
    pub fn planes(&self) -> Vec<Plane> {
        self.sides.iter().filter_map(|s| s.plane()).collect()
    }

    /// Compute the actual polygon of each face.
    ///
    /// This is where a plane-defined brush becomes geometry: start each face as
    /// the whole of its plane, then cut it back with every *other* face turned
    /// inward. Whatever survives is the face. A face that survives as nothing
    /// was redundant -- its plane never reached the hull -- and yields `None`,
    /// which is normal and not an error.
    ///
    /// Returned in the same order as [`Solid::sides`], so a caller can pair a
    /// winding back to the side that produced it.
    pub fn windings(&self) -> Vec<Option<Winding>> {
        let planes = self.planes();
        if planes.len() != self.sides.len() {
            // A degenerate face means we cannot trust the half-space set;
            // validate() reports it properly.
            return vec![None; self.sides.len()];
        }

        planes
            .iter()
            .enumerate()
            .map(|(i, plane)| {
                let mut w = Winding::base_for_plane(plane);
                for (j, other) in planes.iter().enumerate() {
                    if i == j { continue; }
                    // Keep the half we are inside: the other face's plane,
                    // flipped to point into the brush.
                    match w.clipped(&other.flipped(), ON_EPSILON) {
                        Some(next) => w = next,
                        None => return None,
                    }
                }
                w.remove_collinear();
                (!w.is_tiny()).then_some(w)
            })
            .collect()
    }

    /// Faces paired with their polygons, skipping faces that bound nothing.
    pub fn face_windings(&self) -> Vec<(&Side, Winding)> {
        self.sides
            .iter()
            .zip(self.windings())
            .filter_map(|(s, w)| w.map(|w| (s, w)))
            .collect()
    }

    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for w in self.windings().into_iter().flatten() {
            for p in &w.points { b.add_point(*p); }
        }
        b
    }

    pub fn center(&self) -> Vec3 { self.bounds().center() }

    /// Total surface area of the brush.
    pub fn area(&self) -> f32 {
        self.windings().into_iter().flatten().map(|w| w.area()).sum()
    }

    /// Whether a point is inside the brush.
    pub fn contains_point(&self, p: Vec3) -> bool {
        self.planes().iter().all(|plane| plane.distance_to(p) <= ON_EPSILON)
    }

    /// Check that this brush is something the compiler can use.
    pub fn validate(&self) -> Result<(), SolidError> {
        if self.sides.len() < 4 { return Err(SolidError::TooFewFaces(self.sides.len())); }
        for side in &self.sides {
            if side.plane().is_none() { return Err(SolidError::DegeneratePlane { id: side.id }); }
        }
        let real_faces = self.windings().into_iter().flatten().count();
        if real_faces < 4 { return Err(SolidError::NotClosed(real_faces)); }
        let b = self.bounds();
        if b.is_empty()
            || b.min.min_element() < -MAX_MAP_COORD
            || b.max.max_element() > MAX_MAP_COORD
        {
            return Err(SolidError::OutOfBounds);
        }
        Ok(())
    }

    /// Move the brush with texture lock on -- the texture travels with the
    /// geometry, so a given surface point keeps its texel.
    ///
    /// This is Hammer's default and the behaviour you almost always want:
    /// nudging a wall one unit should not smear its texture. Achieving it
    /// means *changing* the offsets, because the axes are world vectors: the
    /// point that was at `p` is now at `p + delta`, so the offset has to
    /// absorb `delta` projected onto each axis.
    pub fn translate(&mut self, delta: Vec3) {
        for side in &mut self.sides {
            for p in &mut side.plane_points { *p += delta; }
            side.uaxis.offset -= delta.dot(side.uaxis.axis) / side.uaxis.safe_scale();
            side.vaxis.offset -= delta.dot(side.vaxis.axis) / side.vaxis.safe_scale();
        }
    }

    /// Move the brush with texture lock off -- the texture stays pinned to
    /// world space and appears to slide across the moving surface.
    ///
    /// Occasionally what you want: sliding a brush along a tiled wall to
    /// realign it against the world grid rather than against itself.
    pub fn translate_world_locked(&mut self, delta: Vec3) {
        for side in &mut self.sides {
            for p in &mut side.plane_points { *p += delta; }
        }
    }

    /// Assign every face the same material.
    pub fn set_material(&mut self, material: &str) {
        for side in &mut self.sides { side.material = material.to_string(); }
    }

    pub(crate) fn from_kv(kv: &KeyValues) -> Result<Solid, SolidError> {
        let id = read_id(kv);
        let mut sides = Vec::new();
        for side_kv in kv.blocks("side") {
            sides.push(Side::from_kv(side_kv)?);
        }
        Ok(Solid { id, sides })
    }

    pub(crate) fn to_kv(&self) -> KeyValues {
        let mut kv = KeyValues::new("solid");
        kv.push_value("id", self.id);
        for side in &self.sides { kv.push_block(side.to_kv()); }
        kv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube64() -> Solid {
        Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid")
    }

    #[test]
    fn cube_faces_all_point_outward() {
        let c = cube64();
        let center = Vec3::splat(32.0);
        for side in &c.sides {
            let plane = side.plane().expect("box face is well formed");
            assert!(
                plane.distance_to(center) < 0.0,
                "face {:?} points inward, which would invert the brush",
                plane.normal
            );
        }
    }

    #[test]
    fn cube_has_six_square_faces() {
        let c = cube64();
        let windings: Vec<_> = c.windings().into_iter().flatten().collect();
        assert_eq!(windings.len(), 6);
        for w in &windings {
            assert_eq!(w.len(), 4);
            assert!((w.area() - 4096.0).abs() < 1e-2, "area {}", w.area());
        }
        assert!((c.area() - 6.0 * 4096.0).abs() < 1e-1);
    }

    #[test]
    fn cube_bounds_match_what_it_was_built_from() {
        let b = Aabb::new(Vec3::new(-32.0, 0.0, 16.0), Vec3::new(96.0, 128.0, 48.0));
        let c = Solid::cube(b, "dev/grid");
        let got = c.bounds();
        assert!((got.min - b.min).length() < 1e-3, "{:?}", got.min);
        assert!((got.max - b.max).length() < 1e-3, "{:?}", got.max);
    }

    #[test]
    fn contains_point_agrees_with_the_box() {
        let c = cube64();
        assert!(c.contains_point(Vec3::splat(32.0)));
        assert!(!c.contains_point(Vec3::splat(-1.0)));
        assert!(!c.contains_point(Vec3::new(32.0, 32.0, 100.0)));
    }

    #[test]
    fn a_redundant_face_yields_no_winding() {
        // Add a plane that sits outside the cube: it bounds nothing, so it
        // must drop out rather than produce a stray face.
        let mut c = cube64();
        c.sides.push(Side::from_plane(99, Plane::new(Vec3::Z, 500.0), "dev/grid"));
        let windings = c.windings();
        assert_eq!(windings.len(), 7);
        assert_eq!(windings.into_iter().flatten().count(), 6);
        assert!(c.validate().is_ok(), "a redundant face is legal, not an error");
    }

    #[test]
    fn a_cutting_plane_produces_a_wedge() {
        let mut c = cube64();
        // Slice the corner off with a 45-degree plane.
        let n = Vec3::new(1.0, 1.0, 0.0).normalize();
        c.sides.push(Side::from_plane(99, Plane::from_point_normal(Vec3::new(48.0, 48.0, 0.0), n), "dev/grid"));
        assert_eq!(c.windings().into_iter().flatten().count(), 7);
        assert!(c.area() < 6.0 * 4096.0, "cutting a corner should reduce area");
        assert!(!c.contains_point(Vec3::new(60.0, 60.0, 32.0)));
        assert!(c.contains_point(Vec3::new(8.0, 8.0, 32.0)));
    }

    #[test]
    fn too_few_faces_is_rejected() {
        let mut c = cube64();
        c.sides.truncate(3);
        assert!(matches!(c.validate(), Err(SolidError::TooFewFaces(3))));
    }

    #[test]
    fn a_brush_that_encloses_nothing_is_rejected() {
        // Six planes, but the +Z and -Z ones are swapped so nothing is inside.
        let mut c = cube64();
        c.sides[0] = Side::from_plane(1, Plane::new(-Vec3::Z, 0.0), "dev/grid");
        c.sides[1] = Side::from_plane(2, Plane::new(Vec3::Z, -64.0), "dev/grid");
        assert!(matches!(c.validate(), Err(SolidError::NotClosed(_))));
    }

    #[test]
    fn degenerate_plane_points_are_rejected() {
        let mut c = cube64();
        c.sides[0].plane_points = [Vec3::ZERO, Vec3::X, Vec3::X * 2.0]; // collinear
        assert!(matches!(c.validate(), Err(SolidError::DegeneratePlane { .. })));
    }

    #[test]
    fn out_of_bounds_brushes_are_rejected() {
        let far = MAX_MAP_COORD + 1000.0;
        let c = Solid::cube(Aabb::new(Vec3::splat(far), Vec3::splat(far + 64.0)), "dev/grid");
        assert!(matches!(c.validate(), Err(SolidError::OutOfBounds)));
    }

    #[test]
    fn translate_carries_the_texture_with_the_brush() {
        // Texture lock on: the surface point that moves from p to p + delta
        // must keep the same texel.
        let mut c = cube64();
        let probe = Vec3::new(16.0, 16.0, 64.0);
        let delta = Vec3::new(32.0, 8.0, 0.0);
        let before = c.sides[0].texcoord(probe);

        c.translate(delta);
        let after = c.sides[0].texcoord(probe + delta);
        assert!((before.0 - after.0).abs() < 1e-3, "{before:?} vs {after:?}");
        assert!((before.1 - after.1).abs() < 1e-3, "{before:?} vs {after:?}");
    }

    #[test]
    fn world_locked_translate_leaves_the_texture_in_world_space() {
        // Texture lock off: a fixed *world* point keeps the same texel while
        // the brush slides underneath it.
        let mut c = cube64();
        let probe = Vec3::new(16.0, 16.0, 64.0);
        let before = c.sides[0].texcoord(probe);

        c.translate_world_locked(Vec3::new(32.0, 8.0, 0.0));
        let after = c.sides[0].texcoord(probe);
        assert_eq!(before, after);
    }

    #[test]
    fn round_trips_through_keyvalues() {
        let c = cube64();
        let kv = c.to_kv();
        let back = Solid::from_kv(&kv).unwrap();
        assert_eq!(back.sides.len(), 6);
        for (a, b) in c.sides.iter().zip(back.sides.iter()) {
            assert_eq!(a.plane().unwrap().normal, b.plane().unwrap().normal);
            assert_eq!(a.material, b.material);
            assert_eq!(a.uaxis, b.uaxis);
        }
    }

    #[test]
    fn from_plane_reproduces_the_plane_it_was_given() {
        for n in [Vec3::Z, -Vec3::X, Vec3::new(1.0, 2.0, 3.0).normalize()] {
            let want = Plane::new(n, 37.0);
            let side = Side::from_plane(1, want, "dev/grid");
            let got = side.plane().unwrap();
            assert!((got.normal - want.normal).length() < 1e-4, "{got:?} vs {want:?}");
            assert!((got.dist - want.dist).abs() < 1e-2, "{got:?} vs {want:?}");
        }
    }
}
