// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! `.kerowalk` -- the compiled NPC walkmap.
//!
//! A walkmap is the answer to "where can NPCs go", built once at compile time
//! from the flat walkable faces of the world and then read by NPC navigation.
//! It is to movement what the PVS is to rendering: the expensive question is
//! answered ahead of time, and the runtime only ever asks the cheap one --
//! *is this point on a face NPCs may stand on, and what is that face's rule*.
//!
//! The format is binary and little-endian, on purpose: the engine reads it,
//! and reading it should be a cast and a bounds check rather than a parse.
//!
//! ```text
//!   [ 4 bytes magic "KRWL" ][ u32 version ][ u32 face count ]
//!   per face:
//!     [ u8 rule ][ u32 vertex count ][ vertices as [f32;3] ]
//!     [ normal [f32;3] ][ bounds min [f32;3] ][ bounds max [f32;3] ]
//! ```

use std::io::{self, Write};
use kerosene_math::{Aabb, Vec3};

pub use kerosene_map::WalkmapRule;

const MAGIC: [u8; 4] = *b"KRWL";
const VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    #[error("not a .kerowalk file (bad magic)")]
    BadMagic,
    #[error("unsupported walkmap version {0}")]
    BadVersion(u32),
    #[error("truncated walkmap file")]
    Truncated,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// How far a query point may sit from a face's plane and still count as on it.
///
/// A footstep lands slightly above the floor, and the radius of that error is
/// the reason `face_under` takes a distance rather than testing the plane
/// exactly.
pub const DEFAULT_STEP: f32 = 1.0;

/// One walkable face in the compiled map.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkFace {
    /// Polygon, wound consistently with `normal`.
    pub vertices: Vec<Vec3>,
    /// The face's plane normal -- upward for a floor.
    pub normal: Vec3,
    pub rule: WalkmapRule,
    /// Bounding box, for a cheap first rejection before the polygon test.
    pub bounds: Aabb,
}

impl WalkFace {
    pub fn area(&self) -> f32 {
        // Newell's method: half the length of the summed edge cross products.
        if self.vertices.len() < 3 { return 0.0 }
        let mut acc = Vec3::ZERO;
        for i in 0..self.vertices.len() {
            let a = self.vertices[i];
            let b = self.vertices[(i + 1) % self.vertices.len()];
            acc += a.cross(b);
        }
        acc.dot(self.normal).abs() * 0.5
    }

    /// Whether a point lies within `max_dist` of this face's plane and inside
    /// its polygon. NPCs call this with the point at their feet.
    pub fn contains(&self, point: Vec3, max_dist: f32) -> bool {
        if self.vertices.len() < 3 { return false; }
        let dist = (point - self.vertices[0]).dot(self.normal);
        if dist.abs() > max_dist { return false; }
        // The bounds is degenerate along the normal -- a floor has zero
        // thickness -- so pad it before using it as a fast reject. The
        // polygon test below is authoritative; this only skips the work.
        if !self.bounds.expanded(max_dist).contains_point(point) { return false; }
        polygon_contains(&self.vertices, self.normal, point)
    }
}

/// The compiled walkmap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Walkmap {
    pub faces: Vec<WalkFace>,
}

impl Walkmap {
    pub fn len(&self) -> usize { self.faces.len() }
    pub fn is_empty(&self) -> bool { self.faces.is_empty() }

    /// The face under a point, if any.
    pub fn face_under(&self, point: Vec3, max_dist: f32) -> Option<&WalkFace> {
        self.faces.iter().find(|f| f.contains(point, max_dist))
    }

    /// Whether a point stands on a face NPCs may walk on.
    pub fn walkable(&self, point: Vec3, max_dist: f32) -> bool {
        self.face_under(point, max_dist).is_some()
    }

    /// The rule of the face under a point, or `None` on empty space.
    pub fn rule_at(&self, point: Vec3, max_dist: f32) -> Option<WalkmapRule> {
        self.face_under(point, max_dist).map(|f| f.rule)
    }

    /// Serialise to bytes, ready to be written or packed into a `.vault`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.faces.len() * 64);
        out.extend_from_slice(&MAGIC);
        push_u32(&mut out, VERSION);
        push_u32(&mut out, self.faces.len() as u32);
        for face in &self.faces {
            out.push(rule_byte(face.rule));
            push_u32(&mut out, face.vertices.len() as u32);
            for v in &face.vertices { push_vec3(&mut out, *v); }
            push_vec3(&mut out, face.normal);
            push_vec3(&mut out, face.bounds.min);
            push_vec3(&mut out, face.bounds.max);
        }
        out
    }

    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(&self.to_bytes())
    }

    /// Write to a file path.
    pub fn write(&self, path: impl AsRef<std::path::Path>) -> io::Result<()> {
        let bytes = self.to_bytes();
        std::fs::write(path, bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Walkmap, WalkError> {
        let mut r = Reader { bytes, at: 0 };
        if r.read_bytes(4)? != MAGIC { return Err(WalkError::BadMagic); }
        let version = r.read_u32()?;
        if version != VERSION { return Err(WalkError::BadVersion(version)); }
        let count = r.read_u32()? as usize;

        // Guard against a corrupt count claiming more faces than the bytes
        // could possibly hold, which would make the loop below read garbage.
        if count > bytes.len() / 24 {
            return Err(WalkError::Truncated);
        }

        let mut faces = Vec::with_capacity(count);
        for _ in 0..count {
            let rule = rule_from_byte(r.read_u8()?);
            let n = r.read_u32()? as usize;
            if n < 3 || n > bytes.len() {
                return Err(WalkError::Truncated);
            }
            let mut vertices = Vec::with_capacity(n);
            for _ in 0..n { vertices.push(r.read_vec3()?); }
            let normal = r.read_vec3()?;
            let min = r.read_vec3()?;
            let max = r.read_vec3()?;
            faces.push(WalkFace { vertices, normal, rule, bounds: Aabb::new(min, max) });
        }
        Ok(Walkmap { faces })
    }

    /// Load from a file path.
    pub fn read(path: impl AsRef<std::path::Path>) -> Result<Walkmap, WalkError> {
        let bytes = std::fs::read(path)?;
        Self::parse(&bytes)
    }
}

/// Whether a point on a convex polygon's plane is inside it.
///
/// The winding is convex, so the point is inside when it falls on the same
/// side of every edge. Which side that is depends on the winding order, so the
/// first edge that says anything decides and the rest must agree.
fn polygon_contains(verts: &[Vec3], normal: Vec3, point: Vec3) -> bool {
    let n = verts.len();
    if n < 3 { return false }
    let mut sign = 0.0f32;
    for i in 0..n {
        let a = verts[i];
        let b = verts[(i + 1) % n];
        let side = (b - a).cross(point - a).dot(normal);
        if side.abs() <= 0.05 { continue }
        if sign == 0.0 {
            sign = side.signum();
        } else if side.signum() != sign {
            return false;
        }
    }
    true
}

// ---- little-endian primitives -------------------------------------------

fn push_u32(out: &mut Vec<u8>, v: u32) { out.extend_from_slice(&v.to_le_bytes()); }
fn push_vec3(out: &mut Vec<u8>, v: Vec3) {
    out.extend_from_slice(&v.x.to_le_bytes());
    out.extend_from_slice(&v.y.to_le_bytes());
    out.extend_from_slice(&v.z.to_le_bytes());
}

fn rule_byte(rule: WalkmapRule) -> u8 {
    match rule {
        WalkmapRule::Allow => 0,
        WalkmapRule::Deny => 1,
        WalkmapRule::Avoid => 2,
        WalkmapRule::Always => 3,
    }
}

fn rule_from_byte(b: u8) -> WalkmapRule {
    match b {
        1 => WalkmapRule::Deny,
        2 => WalkmapRule::Avoid,
        3 => WalkmapRule::Always,
        _ => WalkmapRule::Allow,
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], WalkError> {
        let end = self.at.checked_add(n).ok_or(WalkError::Truncated)?;
        if end > self.bytes.len() { return Err(WalkError::Truncated); }
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, WalkError> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32, WalkError> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_vec3(&mut self) -> Result<Vec3, WalkError> {
        let x = f32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap());
        let y = f32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap());
        let z = f32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap());
        Ok(Vec3::new(x, y, z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64x64 floor at z = 0, normal +Z, wound clockwise from above.
    fn floor() -> WalkFace {
        let vertices = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(64.0, 0.0, 0.0),
            Vec3::new(64.0, 64.0, 0.0),
            Vec3::new(0.0, 64.0, 0.0),
        ];
        WalkFace {
            vertices,
            normal: Vec3::Z,
            rule: WalkmapRule::Allow,
            bounds: Aabb::new(Vec3::ZERO, Vec3::new(64.0, 64.0, 0.0)),
        }
    }

    #[test]
    fn a_point_on_the_floor_is_walkable() {
        let map = Walkmap { faces: vec![floor()] };
        assert!(map.walkable(Vec3::new(32.0, 32.0, 0.0), DEFAULT_STEP));
        assert!(map.walkable(Vec3::new(32.0, 32.0, 0.5), DEFAULT_STEP));
    }

    #[test]
    fn a_point_off_the_face_is_not() {
        let map = Walkmap { faces: vec![floor()] };
        assert!(!map.walkable(Vec3::new(100.0, 32.0, 0.0), DEFAULT_STEP));
        assert!(!map.walkable(Vec3::new(32.0, 32.0, 5.0), DEFAULT_STEP));
        assert!(!map.walkable(Vec3::new(32.0, 32.0, -2.0), DEFAULT_STEP));
    }

    #[test]
    fn the_rule_survives_a_round_trip() {
        for rule in WalkmapRule::all() {
            let face = WalkFace { rule, ..floor() };
            let bytes = Walkmap { faces: vec![face.clone()] }.to_bytes();
            let back = Walkmap::parse(&bytes).unwrap();
            assert_eq!(back.faces[0].rule, rule);
            assert_eq!(back.faces[0].vertices, face.vertices);
            assert_eq!(back.faces[0].normal, face.normal);
        }
    }

    #[test]
    fn an_avoid_face_is_still_walkable_but_reports_avoid() {
        let face = WalkFace { rule: WalkmapRule::Avoid, ..floor() };
        let map = Walkmap { faces: vec![face] };
        assert!(map.walkable(Vec3::new(8.0, 8.0, 0.0), DEFAULT_STEP));
        assert_eq!(map.rule_at(Vec3::new(8.0, 8.0, 0.0), DEFAULT_STEP), Some(WalkmapRule::Avoid));
    }

    #[test]
    fn a_bad_magic_is_rejected() {
        let mut bytes = Walkmap { faces: vec![floor()] }.to_bytes();
        bytes[0] = b'X';
        assert!(matches!(Walkmap::parse(&bytes), Err(WalkError::BadMagic)));
    }

    #[test]
    fn a_truncated_file_is_rejected() {
        let bytes = Walkmap { faces: vec![floor()] }.to_bytes();
        for cut in [0, 4, 8, 12, bytes.len() - 1] {
            assert!(
                Walkmap::parse(&bytes[..cut]).is_err(),
                "cutting at {cut} should not parse"
            );
        }
    }

    #[test]
    fn an_empty_walkmap_round_trips() {
        let bytes = Walkmap::default().to_bytes();
        let back = Walkmap::parse(&bytes).unwrap();
        assert!(back.is_empty());
    }

    #[test]
    fn face_area_matches_the_rectangle() {
        assert!((floor().area() - 4096.0).abs() < 0.01, "{}", floor().area());
    }
}
