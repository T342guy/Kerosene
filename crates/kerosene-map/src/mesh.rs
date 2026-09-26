// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Meshes: world geometry stored as vertices and faces rather than planes.
//!
//! A brush is the intersection of half-spaces, which makes it convex, sealed
//! and exact -- everything the BSP needs -- and makes an arch, a pipe or a
//! sloped rock a pile of fiddly wedges. A mesh is the other trade, and it is
//! the one Source 2's Hammer made for everything: a list of points and the
//! polygons between them, any shape, open or closed, concave if it likes.
//!
//! Kerosene keeps both. Brushes stay the *structure* -- they seal the map,
//! cut the tree and decide what can see what -- and meshes are *detail*, like
//! `func_detail`: Cleave draws them, lights them and collides with them, but
//! never splits the tree on them and never lets them block visibility. Block
//! a room out in brushes; dress it in meshes.
//!
//! ```text
//! mesh
//! {
//!     "id" "40"
//!     vertices
//!     {
//!         "v" "0 0 0"
//!         "v" "0 64 0"
//!         "v" "64 64 0"
//!         "v" "64 0 0"
//!     }
//!     face
//!     {
//!         "id" "41"
//!         "v" "0 1 2 3"
//!         "material" "dev/grid"
//!         "uaxis" "[1 0 0 0] 0.25"
//!         "vaxis" "[0 -1 0 0] 0.25"
//!     }
//! }
//! ```
//!
//! One vertex a line, so moving a vertex is a one-line diff. A face lists its
//! vertices **clockwise seen from the front**, as a brush face's plane points
//! and a compiled face are -- one winding convention for the whole pipeline.
//! Faces may be any polygon; a face that is not flat and convex is cut into
//! triangles when it is compiled (see [`Mesh::face_pieces`]).

use crate::editor::{EditorData, kv_get, kv_set, unknown_pairs};
use crate::texture::{TextureAxis, default_axes_for_plane};
use crate::{DEFAULT_LIGHTMAP_SCALE, Side, Solid, WalkmapRule, read_id};
use kerosene_kv::{FromKvValue, KeyValues, Vec3Value};
use kerosene_math::{Aabb, MAX_MAP_COORD, Plane, Vec3};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum MeshError {
    #[error("has no faces")]
    NoFaces,
    #[error("face {face} has {count} vertices; a face needs at least 3")]
    TooFewVertices { face: u32, count: usize },
    #[error("face {face} uses vertex {index}, but the mesh has {count}")]
    BadIndex { face: u32, index: u32, count: usize },
    #[error("face {face} has no area")]
    Degenerate { face: u32 },
    #[error("extends beyond the {MAX_MAP_COORD}-unit world boundary")]
    OutOfBounds,
}

/// A polygon mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub id: u32,
    pub vertices: Vec<Vec3>,
    pub faces: Vec<MeshFace>,
    /// Key-values on the mesh itself, round-tripped for tools and games.
    pub properties: Vec<(String, String)>,
    pub editor: EditorData,
}

/// One polygon of a mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshFace {
    pub id: u32,
    /// Indices into [`Mesh::vertices`], clockwise seen from the front.
    pub indices: Vec<u32>,
    /// Material path relative to `materials/`, without extension.
    pub material: String,
    pub uaxis: TextureAxis,
    pub vaxis: TextureAxis,
    /// World units per lightmap luxel.
    pub lightmap_scale: f32,
    pub walkmap: WalkmapRule,
    pub properties: Vec<(String, String)>,
}

/// The keys [`MeshFace`] reads into typed fields.
const FACE_KEYS: &[&str] = &[
    "id",
    "v",
    "material",
    "uaxis",
    "vaxis",
    "lightmapscale",
    "walkmap",
];

impl MeshFace {
    /// A face over `indices`, textured world-aligned to its own plane.
    pub fn new(id: u32, indices: Vec<u32>, points: &[Vec3], material: &str) -> MeshFace {
        let normal = polygon_normal(points);
        let dist = points.first().map_or(0.0, |p| normal.dot(*p));
        let (uaxis, vaxis) = default_axes_for_plane(&Plane::new(normal, dist), 0.25);
        MeshFace {
            id,
            indices,
            material: material.to_string(),
            uaxis,
            vaxis,
            lightmap_scale: DEFAULT_LIGHTMAP_SCALE,
            walkmap: WalkmapRule::Allow,
            properties: Vec::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        kv_get(&self.properties, key)
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        kv_set(&mut self.properties, key, value.into());
        self
    }

    /// Whether this face uses a `tools/` material, which never renders.
    pub fn is_tool_material(&self) -> bool {
        self.material.to_lowercase().starts_with("tools/")
    }

    fn from_kv(kv: &KeyValues) -> MeshFace {
        let indices = kv
            .get("v")
            .map(|v| {
                v.split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        // Axes default to world-aligned once the vertices are known; until
        // then a placeholder, fixed up by `Mesh::from_kv`.
        let placeholder = TextureAxis::new(Vec3::X, 0.0, 0.25);
        MeshFace {
            id: read_id(kv),
            indices,
            material: kv.get("material").unwrap_or("dev/grid").to_string(),
            uaxis: kv
                .get("uaxis")
                .and_then(TextureAxis::parse)
                .unwrap_or(placeholder),
            vaxis: kv
                .get("vaxis")
                .and_then(TextureAxis::parse)
                .unwrap_or(placeholder),
            lightmap_scale: kv.get_or("lightmapscale", DEFAULT_LIGHTMAP_SCALE),
            walkmap: kv
                .get("walkmap")
                .map(WalkmapRule::parse)
                .unwrap_or_default(),
            properties: unknown_pairs(kv, FACE_KEYS),
        }
    }

    fn to_kv(&self) -> KeyValues {
        let mut kv = KeyValues::new("face");
        kv.push_value("id", self.id);
        kv.push(
            "v",
            self.indices
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        );
        kv.push("material", self.material.clone());
        kv.push("uaxis", self.uaxis.to_kv());
        kv.push("vaxis", self.vaxis.to_kv());
        kv.push_value("lightmapscale", self.lightmap_scale);
        if self.walkmap != WalkmapRule::Allow {
            kv.push("walkmap", self.walkmap.as_str());
        }
        for (k, v) in &self.properties {
            kv.push(k.clone(), v.clone());
        }
        kv
    }
}

impl Mesh {
    pub fn new(id: u32) -> Mesh {
        Mesh {
            id,
            vertices: Vec::new(),
            faces: Vec::new(),
            properties: Vec::new(),
            editor: EditorData::default(),
        }
    }

    /// The same shape as a brush: its polygons, with shared corners welded
    /// into one vertex and each face keeping its material, texture axes and
    /// lightmap scale. This is how a blocked-out brush becomes something to
    /// sculpt, and ids are handed out from `next_id`.
    pub fn from_solid(solid: &Solid, mut next_id: impl FnMut() -> u32) -> Mesh {
        let mut mesh = Mesh::new(next_id());
        mesh.editor = solid.editor.clone();
        for (side, winding) in solid.sides.iter().zip(solid.windings()) {
            let Some(winding) = winding else {
                continue;
            };
            // Windings are clockwise from the front already: the format's
            // convention is the brush file's.
            let indices = winding.points.iter().map(|&p| mesh.weld(p)).collect();
            mesh.faces.push(face_from_side(next_id(), indices, side));
        }
        mesh
    }

    /// The index of a vertex at `p`, adding it if there is none within a
    /// hundredth of a unit.
    pub fn weld(&mut self, p: Vec3) -> u32 {
        const WELD: f32 = 0.01;
        if let Some(i) = self.vertices.iter().position(|v| v.distance(p) <= WELD) {
            return i as u32;
        }
        self.vertices.push(p);
        (self.vertices.len() - 1) as u32
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        kv_get(&self.properties, key)
    }

    /// A face's corners in order, skipping any index out of range.
    pub fn face_points(&self, face: &MeshFace) -> Vec<Vec3> {
        face.indices
            .iter()
            .filter_map(|&i| self.vertices.get(i as usize).copied())
            .collect()
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::from_points(&self.vertices)
    }

    /// Move every vertex by `delta`, and the textures with them so they stay
    /// stuck to the surface.
    pub fn translate(&mut self, delta: Vec3) {
        for v in &mut self.vertices {
            *v += delta;
        }
        // As `Solid::translate`: the axes are world vectors, so the offsets
        // absorb the move for a point on the surface to keep its texel.
        for face in &mut self.faces {
            face.uaxis.offset -= delta.dot(face.uaxis.axis) / face.uaxis.safe_scale();
            face.vaxis.offset -= delta.dot(face.vaxis.axis) / face.vaxis.safe_scale();
        }
    }

    pub fn validate(&self) -> Result<(), MeshError> {
        if self.faces.is_empty() {
            return Err(MeshError::NoFaces);
        }
        for face in &self.faces {
            if face.indices.len() < 3 {
                return Err(MeshError::TooFewVertices {
                    face: face.id,
                    count: face.indices.len(),
                });
            }
            if let Some(&index) = face
                .indices
                .iter()
                .find(|&&i| i as usize >= self.vertices.len())
            {
                return Err(MeshError::BadIndex {
                    face: face.id,
                    index,
                    count: self.vertices.len(),
                });
            }
            if polygon_area(&self.face_points(face)) < 1e-4 {
                return Err(MeshError::Degenerate { face: face.id });
            }
        }
        if self
            .vertices
            .iter()
            .any(|v| v.abs().max_element() > MAX_MAP_COORD)
        {
            return Err(MeshError::OutOfBounds);
        }
        Ok(())
    }

    /// A face as polygons that are each flat and convex, clockwise from the
    /// front: the face itself when it already is one, otherwise triangles.
    ///
    /// What the compiler needs -- a compiled face is a flat convex polygon --
    /// and what a face with a bent or notched outline is not.
    pub fn face_pieces(&self, face: &MeshFace) -> Vec<Vec<Vec3>> {
        let points = self.face_points(face);
        if points.len() < 3 {
            return Vec::new();
        }
        if is_flat_and_convex(&points) {
            return vec![points];
        }
        triangulate(&points)
            .into_iter()
            .map(|[a, b, c]| vec![points[a], points[b], points[c]])
            .collect()
    }

    pub(crate) fn from_kv(kv: &KeyValues) -> Mesh {
        let vertices = kv
            .block("vertices")
            .map(|b| {
                b.pairs()
                    .filter(|(k, _)| *k == "v")
                    .filter_map(|(_, v)| Vec3Value::from_kv(v).ok())
                    .map(|v| Vec3::from_array(v.to_array()))
                    .collect()
            })
            .unwrap_or_default();
        let mut mesh = Mesh {
            id: read_id(kv),
            vertices,
            faces: kv.blocks("face").map(MeshFace::from_kv).collect(),
            properties: unknown_pairs(kv, &["id"]),
            editor: kv
                .block("editor")
                .map(EditorData::from_kv)
                .unwrap_or_default(),
        };
        // A face written without axes gets the world-aligned ones for its
        // own plane, the same as a brush face would.
        for (i, block) in kv.blocks("face").enumerate() {
            if block.get("uaxis").is_some() && block.get("vaxis").is_some() {
                continue;
            }
            let points = mesh.face_points(&mesh.faces[i]);
            if points.len() < 3 {
                continue;
            }
            let normal = polygon_normal(&points);
            let (u, v) = default_axes_for_plane(&Plane::new(normal, normal.dot(points[0])), 0.25);
            let face = &mut mesh.faces[i];
            if block.get("uaxis").is_none() {
                face.uaxis = u;
            }
            if block.get("vaxis").is_none() {
                face.vaxis = v;
            }
        }
        mesh
    }

    pub(crate) fn to_kv(&self) -> KeyValues {
        use kerosene_kv::format_float as f;
        let mut kv = KeyValues::new("mesh");
        kv.push_value("id", self.id);
        for (k, v) in &self.properties {
            kv.push(k.clone(), v.clone());
        }
        let mut vertices = KeyValues::new("vertices");
        for v in &self.vertices {
            vertices.push("v", format!("{} {} {}", f(v.x), f(v.y), f(v.z)));
        }
        kv.push_block(vertices);
        for face in &self.faces {
            kv.push_block(face.to_kv());
        }
        if let Some(editor) = self.editor.to_kv() {
            kv.push_block(editor);
        }
        kv
    }
}

fn face_from_side(id: u32, indices: Vec<u32>, side: &Side) -> MeshFace {
    MeshFace {
        id,
        indices,
        material: side.material.clone(),
        uaxis: side.uaxis,
        vaxis: side.vaxis,
        lightmap_scale: side.lightmap_scale,
        walkmap: side.walkmap,
        properties: side.properties.clone(),
    }
}

/// The front-facing unit normal of a polygon wound clockwise from the front.
///
/// Newell's method, which averages over every edge and so is right for a
/// polygon that is not quite flat. Newell's normal points out of the side the
/// polygon is counter-clockwise from; this winding is the other way, hence
/// the minus.
pub fn polygon_normal(points: &[Vec3]) -> Vec3 {
    let mut n = Vec3::ZERO;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    (-n).normalize_or_zero()
}

fn polygon_area(points: &[Vec3]) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut sum = Vec3::ZERO;
    for i in 1..points.len() - 1 {
        sum += (points[i] - points[0]).cross(points[i + 1] - points[0]);
    }
    sum.length() * 0.5
}

/// Whether every corner lies on the polygon's plane and turns the same way.
fn is_flat_and_convex(points: &[Vec3]) -> bool {
    const FLAT: f32 = 0.01;
    let n = polygon_normal(points);
    if n == Vec3::ZERO {
        return false;
    }
    let d = n.dot(points[0]);
    if points.iter().any(|p| (n.dot(*p) - d).abs() > FLAT) {
        return false;
    }
    let count = points.len();
    (0..count).all(|i| {
        let a = points[i];
        let b = points[(i + 1) % count];
        let c = points[(i + 2) % count];
        // Clockwise from the front: each turn is clockwise about n.
        (b - a).cross(c - b).dot(n) <= 1e-4
    })
}

/// Ear clipping, in the plane the polygon best fits. Triangles keep the
/// polygon's winding.
///
/// Quadratic, which is nothing for a face a person drew. A polygon that
/// ear clipping cannot finish -- self-intersecting, say -- falls back to a
/// fan over what is left rather than dropping the rest of the face.
fn triangulate(points: &[Vec3]) -> Vec<[usize; 3]> {
    let n = polygon_normal(points);
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    let mut out = Vec::new();

    let convex = |a: Vec3, b: Vec3, c: Vec3| (b - a).cross(c - b).dot(n) < -1e-6;
    let inside = |p: Vec3, a: Vec3, b: Vec3, c: Vec3| {
        let s1 = (b - a).cross(p - a).dot(n);
        let s2 = (c - b).cross(p - b).dot(n);
        let s3 = (a - c).cross(p - c).dot(n);
        s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0
    };

    while remaining.len() > 3 {
        let m = remaining.len();
        let ear = (0..m).find(|&i| {
            let (ia, ib, ic) = (
                remaining[(i + m - 1) % m],
                remaining[i],
                remaining[(i + 1) % m],
            );
            let (a, b, c) = (points[ia], points[ib], points[ic]);
            convex(a, b, c)
                && remaining
                    .iter()
                    .filter(|&&j| j != ia && j != ib && j != ic)
                    .all(|&j| !inside(points[j], a, b, c))
        });
        let Some(i) = ear else {
            break;
        };
        let m = remaining.len();
        out.push([
            remaining[(i + m - 1) % m],
            remaining[i],
            remaining[(i + 1) % m],
        ]);
        remaining.remove(i);
    }
    // Three left -- the normal end -- or ear clipping gave up: fan the rest.
    for i in 1..remaining.len().saturating_sub(1) {
        out.push([remaining[0], remaining[i], remaining[i + 1]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-unit floor quad, clockwise from above.
    fn floor() -> Mesh {
        let mut m = Mesh::new(1);
        m.vertices = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 64.0, 0.0),
            Vec3::new(64.0, 64.0, 0.0),
            Vec3::new(64.0, 0.0, 0.0),
        ];
        let points = m.vertices.clone();
        m.faces
            .push(MeshFace::new(2, vec![0, 1, 2, 3], &points, "dev/grid"));
        m
    }

    #[test]
    fn a_clockwise_floor_faces_up() {
        let m = floor();
        assert_eq!(polygon_normal(&m.face_points(&m.faces[0])), Vec3::Z);
    }

    #[test]
    fn a_mesh_round_trips_through_text() {
        let mut m = floor();
        m.properties.push(("mymod_key".into(), "kept".into()));
        m.faces[0].walkmap = WalkmapRule::Deny;
        let text = m.to_kv().to_text();
        let back = Mesh::from_kv(
            &KeyValues::parse(&text)
                .unwrap()
                .blocks("mesh")
                .next()
                .unwrap()
                .clone(),
        );
        assert_eq!(back, m, "{text}");
    }

    #[test]
    fn a_face_written_without_axes_gets_world_aligned_ones() {
        let text = r#"mesh { "id" "1" vertices { "v" "0 0 0" "v" "0 64 0" "v" "64 64 0" } face { "id" "2" "v" "0 1 2" } }"#;
        let kv = KeyValues::parse(text).unwrap();
        let m = Mesh::from_kv(kv.blocks("mesh").next().unwrap());
        let (u, v) = default_axes_for_plane(&Plane::new(Vec3::Z, 0.0), 0.25);
        assert_eq!(m.faces[0].uaxis, u);
        assert_eq!(m.faces[0].vaxis, v);
        assert_eq!(m.faces[0].material, "dev/grid");
    }

    #[test]
    fn validation_catches_what_a_compiler_cannot_use() {
        assert!(floor().validate().is_ok());
        let mut bad = floor();
        bad.faces[0].indices = vec![0, 1];
        assert!(matches!(
            bad.validate(),
            Err(MeshError::TooFewVertices { .. })
        ));
        let mut bad = floor();
        bad.faces[0].indices[2] = 9;
        assert!(matches!(
            bad.validate(),
            Err(MeshError::BadIndex { index: 9, .. })
        ));
        let mut bad = floor();
        bad.vertices = vec![Vec3::ZERO; 4];
        assert!(matches!(bad.validate(), Err(MeshError::Degenerate { .. })));
        assert_eq!(Mesh::new(1).validate(), Err(MeshError::NoFaces));
    }

    #[test]
    fn a_flat_convex_face_compiles_whole() {
        let m = floor();
        assert_eq!(m.face_pieces(&m.faces[0]).len(), 1);
    }

    #[test]
    fn a_notched_face_is_cut_into_triangles_that_cover_it_and_face_the_same_way() {
        // An L, clockwise from above: six corners, concave at one.
        let mut m = Mesh::new(1);
        m.vertices = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 64.0, 0.0),
            Vec3::new(32.0, 64.0, 0.0),
            Vec3::new(32.0, 32.0, 0.0),
            Vec3::new(64.0, 32.0, 0.0),
            Vec3::new(64.0, 0.0, 0.0),
        ];
        let points = m.vertices.clone();
        m.faces
            .push(MeshFace::new(2, (0..6).collect(), &points, "dev/grid"));
        let pieces = m.face_pieces(&m.faces[0]);
        assert_eq!(pieces.len(), 4, "n - 2 triangles");
        let area: f32 = pieces.iter().map(|p| polygon_area(p)).sum();
        assert!((area - (64.0 * 64.0 - 32.0 * 32.0)).abs() < 1e-2, "{area}");
        for p in &pieces {
            assert!(polygon_normal(p).dot(Vec3::Z) > 0.99);
        }
    }

    #[test]
    fn a_bent_quad_is_cut_into_two_flat_triangles() {
        let mut m = floor();
        m.vertices[2].z = 16.0;
        let pieces = m.face_pieces(&m.faces[0]);
        assert_eq!(pieces.len(), 2);
    }

    #[test]
    fn a_brush_becomes_a_closed_mesh_with_welded_corners() {
        let solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/wall");
        let mut next = 100;
        let mesh = Mesh::from_solid(&solid, || {
            next += 1;
            next
        });
        assert_eq!(mesh.vertices.len(), 8, "a cube has eight corners");
        assert_eq!(mesh.faces.len(), 6);
        assert!(mesh.validate().is_ok());
        // Every face still faces outward.
        let centre = Vec3::splat(32.0);
        for face in &mesh.faces {
            let points = mesh.face_points(face);
            let mid = points.iter().copied().sum::<Vec3>() / points.len() as f32;
            assert!(polygon_normal(&points).dot(mid - centre) > 0.0);
            assert_eq!(face.material, "dev/wall");
        }
    }
}
