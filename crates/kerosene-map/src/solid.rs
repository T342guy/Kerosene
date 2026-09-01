// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Brush solids and their faces.

use crate::texture::{TextureAxis, default_axes_for_plane};
use crate::{DEFAULT_LIGHTMAP_SCALE, WalkmapRule, read_id};
use thiserror::Error;
use kerosene_kv::{KeyValues, Vec3Value};
use kerosene_math::{Aabb, MAX_MAP_COORD, ON_EPSILON, Plane, Vec3, Winding};

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
    /// How this face participates in the NPC walkmap. See [`WalkmapRule`].
    pub walkmap: WalkmapRule,
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
            walkmap: WalkmapRule::Allow,
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
            walkmap: kv.get("walkmap").map(WalkmapRule::parse).unwrap_or_default(),
        })
    }

    pub(crate) fn to_kv(&self) -> KeyValues {
        use kerosene_kv::format_float as f;
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
        // Only the faces someone changed carry a rule; a floor that says
        // nothing is `allow`, and writing it on every face would bury the few
        // that matter in noise.
        if self.walkmap != WalkmapRule::Allow {
            kv.push("walkmap", self.walkmap.as_str());
        }
        kv
    }
}

/// Parse `"(x y z) (x y z) (x y z)"`.
fn parse_plane_points(s: &str) -> Option<[Vec3; 3]> {
    use kerosene_kv::FromKvValue;
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
    /// so a generated `.keromap` reads the way a hand-authored one does.
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
                    walkmap: WalkmapRule::Allow,
                }
            })
            .collect();

        Solid { id: 0, sides }
    }

    /// A face on the plane through three points, oriented to face `outward`.
    ///
    /// The three points fix the plane; which side is the front depends on the
    /// order they are written in, and getting that wrong produces a brush
    /// that is inside out -- a hole in the world that compiles cleanly and
    /// looks, from most angles, like nothing at all. Rather than reason about
    /// winding at every call site, say which way the face should look and let
    /// this put the points in the order that means it.
    fn facing(id: u32, points: [Vec3; 3], outward: Vec3, material: &str) -> Option<Side> {
        let plane = Plane::from_map_points(points[0], points[1], points[2])?;
        let points = if plane.normal.dot(outward) >= 0.0 {
            points
        } else {
            [points[2], points[1], points[0]]
        };
        let plane = Plane::from_map_points(points[0], points[1], points[2])?;
        let (uaxis, vaxis) = default_axes_for_plane(&plane, 0.25);
        Some(Side {
            id,
            plane_points: points,
            material: material.to_string(),
            uaxis,
            vaxis,
            rotation: 0.0,
            lightmap_scale: DEFAULT_LIGHTMAP_SCALE,
            smoothing_groups: 0,
            walkmap: WalkmapRule::Allow,
        })
    }

    /// A convex polygon swept along an axis: the general prism.
    ///
    /// `profile` is the cross-section in world space, at `low` on `axis`, and
    /// must be convex and wound consistently -- either way round, since the
    /// faces are oriented from the polygon's own centre rather than from its
    /// winding. Everything the shape tool draws that is not a cone is one of
    /// these: a box is a four-sided prism, a wedge a three-sided one, a
    /// cylinder an n-sided one, and one slice of an arch a four-sided one
    /// with two of its corners pushed inward.
    ///
    /// `None` when the profile is not a polygon -- fewer than three points, or
    /// three points in a line.
    pub fn prism(profile: &[Vec3], axis: usize, low: f32, high: f32, material: &str) -> Option<Solid> {
        if profile.len() < 3 || high <= low { return None }

        let mut up = Vec3::ZERO;
        up[axis] = 1.0;
        let at = |p: Vec3, height: f32| {
            let mut p = p;
            p[axis] = height;
            p
        };

        let centre = profile.iter().copied().sum::<Vec3>() / profile.len() as f32;
        let mut sides = Vec::with_capacity(profile.len() + 2);
        let mut id = 1;

        // The two caps, from any three points of the profile: they are
        // coplanar, so which three does not matter.
        for (height, outward) in [(high, up), (low, -up)] {
            let points = [
                at(profile[0], height),
                at(profile[1], height),
                at(profile[2], height),
            ];
            sides.push(Self::facing(id, points, outward, material)?);
            id += 1;
        }

        // One wall per edge, facing away from the middle.
        for (i, &a) in profile.iter().enumerate() {
            let b = profile[(i + 1) % profile.len()];
            let along = at(b, low) - at(a, low);
            let outward = along.cross(up).normalize_or_zero();
            // Away from the centre regardless of which way the profile was
            // wound, so a caller cannot get this wrong.
            let outward = if outward.dot(at(a, low) - at(centre, low)) < 0.0 { -outward } else { outward };
            if outward == Vec3::ZERO { return None }

            let points = [at(a, low), at(b, low), at(b, high)];
            sides.push(Self::facing(id, points, outward, material)?);
            id += 1;
        }

        let solid = Solid { id: 0, sides };
        solid.validate().ok()?;
        Some(solid)
    }

    /// A convex polygon drawn to a point: the general cone.
    ///
    /// `profile` is the base, in world space; `apex` is the tip. A four-sided
    /// base gives the pyramid, a many-sided one the spike the shape tool
    /// calls a cone.
    pub fn pyramid(profile: &[Vec3], axis: usize, base: f32, apex: Vec3, material: &str) -> Option<Solid> {
        if profile.len() < 3 { return None }

        let mut up = Vec3::ZERO;
        up[axis] = 1.0;
        let outward_base = if apex[axis] > base { -up } else { up };
        let at = |p: Vec3| {
            let mut p = p;
            p[axis] = base;
            p
        };

        let mut sides = Vec::with_capacity(profile.len() + 1);
        sides.push(Self::facing(
            1,
            [at(profile[0]), at(profile[1]), at(profile[2])],
            outward_base,
            material,
        )?);

        let centre = profile.iter().copied().map(at).sum::<Vec3>() / profile.len() as f32;
        for (i, &a) in profile.iter().enumerate() {
            let b = profile[(i + 1) % profile.len()];
            let (a, b) = (at(a), at(b));
            // Outward is away from the axis through the middle of the base --
            // the apex being off to one side does not change which way a
            // wall looks.
            let outward = (b - a).cross(apex - a).normalize_or_zero();
            let outward = if outward.dot((a + b) * 0.5 - centre) < 0.0 { -outward } else { outward };
            if outward == Vec3::ZERO { return None }

            sides.push(Self::facing(i as u32 + 2, [a, b, apex], outward, material)?);
        }

        let solid = Solid { id: 0, sides };
        solid.validate().ok()?;
        Some(solid)
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

    /// Scale the brush about a point.
    ///
    /// This is what a resize handle does, and it is why brushes are stored as
    /// plane *points* rather than as plane equations: moving the points and
    /// re-deriving the planes keeps a brush a brush, where scaling a normal
    /// and a distance separately does not.
    ///
    /// The texture is left where it is in world space rather than stretched
    /// with the surface. That is Hammer's behaviour and the one that is nearly
    /// always wanted: making a wall twice as wide should tile the bricks
    /// twice, not draw bricks twice the size.
    pub fn scale(&mut self, anchor: Vec3, factor: Vec3) {
        // A factor with an odd number of negative components mirrors the
        // brush, which reverses every face's winding and turns its normal
        // inward. A brush with inward normals is not a smaller brush, it is a
        // hole in the world -- so the winding goes back the way it was.
        let mirrored = factor.x * factor.y * factor.z < 0.0;

        for side in &mut self.sides {
            for p in &mut side.plane_points {
                *p = anchor + (*p - anchor) * factor;
            }
            if mirrored { side.plane_points.reverse() }
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
    fn a_walkmap_rule_round_trips_and_defaults_to_allow() {
        let mut c = cube64();
        assert!(
            c.sides.iter().all(|s| s.walkmap == WalkmapRule::Allow),
            "a face that says nothing is `allow`"
        );
        c.sides[0].walkmap = WalkmapRule::Deny;

        let kv = c.to_kv();
        let back = Solid::from_kv(&kv).unwrap();
        assert_eq!(back.sides[0].walkmap, WalkmapRule::Deny);
        assert_eq!(back.sides[1].walkmap, WalkmapRule::Allow);

        // Only faces someone changed carry the key, so a diff between saves
        // still shows what actually moved.
        let text = kv.to_document();
        assert_eq!(text.matches("walkmap").count(), 1, "{text}");
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

    // ---- scaling -----------------------------------------------------------

    #[test]
    fn scaling_about_a_corner_leaves_that_corner_where_it_was() {
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        solid.scale(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));

        let bounds = solid.bounds();
        assert_eq!(bounds.min, Vec3::ZERO, "the anchor does not move");
        assert_eq!(bounds.max, Vec3::splat(128.0));
    }

    #[test]
    fn scaling_one_axis_leaves_the_others_alone() {
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        solid.scale(Vec3::ZERO, Vec3::new(1.0, 3.0, 1.0));

        let bounds = solid.bounds();
        assert_eq!(bounds.max, Vec3::new(64.0, 192.0, 64.0));
    }

    #[test]
    fn a_scaled_brush_is_still_a_valid_brush() {
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        solid.scale(Vec3::new(32.0, 32.0, 32.0), Vec3::new(0.5, 4.0, 1.5));

        assert!(solid.validate().is_ok(), "{:?}", solid.validate());
        assert_eq!(solid.windings().iter().filter(|w| w.is_some()).count(), 6);
    }

    #[test]
    fn scaling_about_the_centre_grows_both_ways() {
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        let centre = solid.center();
        solid.scale(centre, Vec3::splat(2.0));

        let bounds = solid.bounds();
        assert_eq!(bounds.min, Vec3::splat(-32.0));
        assert_eq!(bounds.max, Vec3::splat(96.0));
    }

    #[test]
    fn mirroring_a_brush_keeps_its_faces_pointing_outward() {
        // A negative factor turns the brush inside out unless the windings are
        // put back, and an inside-out brush is a hole in the world rather
        // than a solid -- one that compiles, too, which is the worst of it.
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        solid.scale(Vec3::ZERO, Vec3::new(-1.0, 1.0, 1.0));

        assert!(solid.validate().is_ok(), "{:?}", solid.validate());
        assert!(solid.contains_point(Vec3::new(-32.0, 32.0, 32.0)), "the mirrored brush is solid");
        assert_eq!(solid.bounds(), Aabb::new(Vec3::new(-64.0, 0.0, 0.0), Vec3::new(0.0, 64.0, 64.0)));
    }

    #[test]
    fn mirroring_on_two_axes_needs_no_correction_and_gets_none() {
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        solid.scale(Vec3::ZERO, Vec3::new(-1.0, -1.0, 1.0));

        assert!(solid.validate().is_ok(), "{:?}", solid.validate());
        assert!(solid.contains_point(Vec3::new(-32.0, -32.0, 32.0)));
    }

    #[test]
    fn scaling_does_not_stretch_the_texture_with_the_surface() {
        // Making a wall twice as wide should tile the bricks twice, not draw
        // bricks twice the size.
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        let before: Vec<_> = solid.sides.iter().map(|s| (s.uaxis, s.vaxis)).collect();
        solid.scale(Vec3::ZERO, Vec3::new(2.0, 1.0, 1.0));

        for (side, (u, v)) in solid.sides.iter().zip(before) {
            assert_eq!(side.uaxis.scale, u.scale);
            assert_eq!(side.vaxis.scale, v.scale);
            assert_eq!(side.uaxis.offset, u.offset);
            assert_eq!(side.vaxis.offset, v.offset);
        }
    }

    #[test]
    fn scaling_by_one_changes_nothing() {
        let original = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        let mut solid = original.clone();
        solid.scale(Vec3::new(11.0, -3.0, 7.0), Vec3::ONE);
        assert_eq!(solid.bounds(), original.bounds());
    }

    // ---- prisms and cones --------------------------------------------------

    /// A regular polygon in the XY plane, at z = 0.
    fn ngon(sides: usize, radius: f32) -> Vec<Vec3> {
        (0..sides)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / sides as f32;
                Vec3::new(a.cos() * radius, a.sin() * radius, 0.0)
            })
            .collect()
    }

    #[test]
    fn a_four_sided_prism_is_a_box() {
        let square = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(64.0, 0.0, 0.0),
            Vec3::new(64.0, 64.0, 0.0),
            Vec3::new(0.0, 64.0, 0.0),
        ];
        let solid = Solid::prism(&square, 2, 0.0, 32.0, "dev/grid").unwrap();

        assert_eq!(solid.sides.len(), 6);
        assert_eq!(solid.bounds(), Aabb::new(Vec3::ZERO, Vec3::new(64.0, 64.0, 32.0)));
        assert!(solid.contains_point(Vec3::new(32.0, 32.0, 16.0)), "it is solid inside");
        assert!(!solid.contains_point(Vec3::new(32.0, 32.0, 48.0)), "and not outside");
    }

    #[test]
    fn a_prism_is_solid_whichever_way_its_profile_was_wound() {
        // The caller should not have to know, and getting it wrong produces a
        // brush that is inside out: a hole in the world that compiles.
        let clockwise = ngon(8, 64.0);
        let mut anticlockwise = clockwise.clone();
        anticlockwise.reverse();

        for profile in [clockwise, anticlockwise] {
            let solid = Solid::prism(&profile, 2, 0.0, 128.0, "dev/grid").unwrap();
            assert!(solid.validate().is_ok(), "{:?}", solid.validate());
            assert!(solid.contains_point(Vec3::new(0.0, 0.0, 64.0)), "the middle is inside");
            assert!(!solid.contains_point(Vec3::new(0.0, 0.0, 200.0)));
            assert!(!solid.contains_point(Vec3::new(200.0, 0.0, 64.0)));
        }
    }

    #[test]
    fn a_cylinder_has_a_face_per_side_plus_two_caps() {
        for sides in [3usize, 6, 12, 32] {
            let solid = Solid::prism(&ngon(sides, 64.0), 2, 0.0, 128.0, "dev/grid").unwrap();
            assert_eq!(solid.sides.len(), sides + 2, "{sides}-sided");
            let real = solid.windings().iter().filter(|w| w.is_some()).count();
            assert_eq!(real, sides + 2, "every face reaches the hull on a {sides}-gon");
        }
    }

    #[test]
    fn a_prism_can_be_swept_along_any_axis() {
        // A cylinder drawn in the front view lies on its side, and that is
        // the whole reason the axis is a parameter.
        let profile: Vec<Vec3> = ngon(8, 64.0)
            .into_iter()
            .map(|p| Vec3::new(0.0, p.x, p.y))
            .collect();
        let solid = Solid::prism(&profile, 0, 0.0, 128.0, "dev/grid").unwrap();

        assert!(solid.validate().is_ok());
        assert!(solid.contains_point(Vec3::new(64.0, 0.0, 0.0)));
        assert!(!solid.contains_point(Vec3::new(200.0, 0.0, 0.0)));
    }

    #[test]
    fn a_profile_that_is_not_a_polygon_is_refused_rather_than_producing_a_bad_brush() {
        let line = vec![Vec3::ZERO, Vec3::new(64.0, 0.0, 0.0), Vec3::new(128.0, 0.0, 0.0)];
        assert!(Solid::prism(&line, 2, 0.0, 64.0, "dev/grid").is_none(), "collinear");
        assert!(Solid::prism(&[Vec3::ZERO, Vec3::X], 2, 0.0, 64.0, "dev/grid").is_none(), "too few");
    }

    #[test]
    fn a_prism_with_no_height_is_refused() {
        // A brush of zero thickness exists, compiles, and cannot be seen.
        let square = ngon(4, 64.0);
        assert!(Solid::prism(&square, 2, 32.0, 32.0, "dev/grid").is_none());
        assert!(Solid::prism(&square, 2, 32.0, 0.0, "dev/grid").is_none());
    }

    #[test]
    fn a_pyramid_is_a_base_and_a_wall_per_edge() {
        let apex = Vec3::new(0.0, 0.0, 128.0);
        let solid = Solid::pyramid(&ngon(4, 64.0), 2, 0.0, apex, "dev/grid").unwrap();

        assert_eq!(solid.sides.len(), 5);
        assert!(solid.validate().is_ok(), "{:?}", solid.validate());
        assert!(solid.contains_point(Vec3::new(0.0, 0.0, 8.0)), "wide at the bottom");
        assert!(!solid.contains_point(Vec3::new(40.0, 40.0, 120.0)), "and narrow at the top");
    }

    #[test]
    fn a_cone_points_the_way_its_apex_does() {
        // Hanging downward is a stalactite, and it must be solid too.
        let apex = Vec3::new(0.0, 0.0, -128.0);
        let solid = Solid::pyramid(&ngon(12, 64.0), 2, 0.0, apex, "dev/grid").unwrap();

        assert!(solid.validate().is_ok(), "{:?}", solid.validate());
        assert!(solid.contains_point(Vec3::new(0.0, 0.0, -8.0)));
        assert!(!solid.contains_point(Vec3::new(0.0, 0.0, 8.0)));
    }

    #[test]
    fn a_generated_shape_survives_a_round_trip_through_the_file_format() {
        // The point of writing real corner points rather than plane equations
        // is that the file is readable and reloads as the same brush.
        let mut map = crate::Map::new();
        let solid = Solid::prism(&ngon(10, 96.0), 2, 0.0, 160.0, "dev/wall").unwrap();
        let bounds = solid.bounds();
        map.add_world_solid(solid);

        let reloaded = crate::Map::parse(&map.to_text()).unwrap();
        let back = reloaded.world.solids.first().unwrap();
        assert_eq!(back.sides.len(), 12);
        assert!(back.validate().is_ok());
        assert!((back.bounds().min - bounds.min).length() < 0.01);
        assert!((back.bounds().max - bounds.max).length() < 0.01);
    }
}
