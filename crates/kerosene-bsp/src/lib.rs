// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `.kerobsp` -- Kerosene's compiled map format.
//!
//! A `.keromap` is what a designer edits; a `.kerobsp` is what the engine runs. The
//! compile turns overlapping convex brushes into a binary space partition:
//! a tree of planes whose leaves are convex, non-overlapping regions of space.
//! That single structure answers most of the questions a level needs answered:
//!
//! * *Where am I?* Walk the tree; you land in exactly one leaf.
//! * *What can I see?* Each leaf names a visibility cluster, and the PVS says
//!   which clusters are reachable by sight (see [`vis`]).
//! * *Did I hit anything?* Traces walk the same tree, testing brush planes
//!   rather than triangles.
//! * *How is this surface lit?* Lightmaps are baked per face into [`Bsp::lighting`].
//! * *What does it sound like here?* Each leaf names an acoustic room, and
//!   the room says how long sound lingers (see [`acoustics`]).
//!
//! The format is a header, a lump directory, and flat arrays of `#[repr(C)]`
//! records, so loading is a bounds check and a cast rather than a parse.
//!
//! ## Detail brushes
//!
//! Brushes marked [`contents::DETAIL`] are kept out of the tree entirely. A
//! railing or a pillar would otherwise carve the world into dozens of slivers
//! and inflate the vis compile enormously, for no visibility benefit -- you
//! cannot hide behind a railing. Detail geometry renders and collides but does
//! not split space, which is the single biggest lever a level designer has
//! over compile times.

pub mod acoustics;
pub mod cubemaps;
pub mod io;
pub mod sections;
pub mod trace;
pub mod types;
pub mod vis;

pub use acoustics::{AcousticPath, AcousticRoom, Acoustics, NO_ROOM, room_flags};
pub use cubemaps::{Cubemaps, Probe, decode_rgb9e5, encode_rgb9e5};
pub use io::{BspError, LumpDir, MAGIC, VERSION, write_bsp};
pub use sections::{Section, SectionRecord, WORLD_SECTION};
pub use trace::Trace;
pub use types::*;
pub use vis::{VisBuilder, VisData, VisKind};

use kerosene_kv::KeyValues;
use kerosene_math::{Aabb, Plane, Vec3};

/// Lump indices, and the names used in error messages.
pub mod lumps {
    pub const ENTITIES: usize = 0;
    pub const PLANES: usize = 1;
    pub const VERTICES: usize = 2;
    pub const EDGES: usize = 3;
    pub const SURFEDGES: usize = 4;
    pub const FACES: usize = 5;
    pub const NODES: usize = 6;
    pub const LEAVES: usize = 7;
    pub const LEAFFACES: usize = 8;
    pub const LEAFBRUSHES: usize = 9;
    pub const MODELS: usize = 10;
    pub const BRUSHES: usize = 11;
    pub const BRUSHSIDES: usize = 12;
    pub const TEXINFO: usize = 13;
    pub const TEXDATA: usize = 14;
    pub const TEXDATA_STRINGS: usize = 15;
    pub const VISIBILITY: usize = 16;
    pub const LIGHTING: usize = 17;
    /// Rooms; see [`crate::acoustics`]. Took a spare slot, so no version bump.
    pub const ACOUSTICS: usize = 18;
    /// One `u16` per leaf: which room it is in.
    pub const ACOUSTIC_LEAFS: usize = 19;
    /// Streamed sections; see [`crate::sections`].
    pub const SECTIONS: usize = 20;
    /// One `u16` per face: its section.
    pub const FACE_SECTIONS: usize = 21;
    /// One `u16` per brush: its section.
    pub const BRUSH_SECTIONS: usize = 22;
    /// Cubemap probes; see [`crate::cubemaps`]. This was the spare slot, so
    /// a map from before probes has it empty and reads as having none,
    /// without a format version bump.
    pub const CUBEMAPS: usize = 23;

    pub const NAMES: [&str; super::LUMP_COUNT] = [
        "entities",
        "planes",
        "vertices",
        "edges",
        "surfedges",
        "faces",
        "nodes",
        "leaves",
        "leaffaces",
        "leafbrushes",
        "models",
        "brushes",
        "brushsides",
        "texinfo",
        "texdata",
        "texdata_strings",
        "visibility",
        "lighting",
        "acoustics",
        "acoustic_leafs",
        "sections",
        "face_sections",
        "brush_sections",
        "cubemaps",
    ];
}

/// Slots in the lump directory. Version 1 had twenty, with two spare that
/// the acoustics lumps took; version 2 added four for the section lumps and
/// one spare, which the cubemap probes took. The next lump needs a version
/// bump.
pub const LUMP_COUNT: usize = 24;

/// A compiled map.
#[derive(Clone, Default)]
pub struct Bsp {
    /// Bumped by each compile; lets a client tell whether its map matches the
    /// server's.
    pub revision: u32,
    /// The entity lump: KeyValues text, one block per entity.
    pub entities: String,
    pub planes: Vec<BspPlane>,
    pub vertices: Vec<[f32; 3]>,
    pub edges: Vec<Edge>,
    /// Signed indices into `edges`; negative means traverse backwards.
    pub surfedges: Vec<i32>,
    pub faces: Vec<Face>,
    pub nodes: Vec<Node>,
    pub leaves: Vec<Leaf>,
    pub leaffaces: Vec<u32>,
    pub leafbrushes: Vec<u32>,
    pub models: Vec<Model>,
    pub brushes: Vec<Brush>,
    pub brushsides: Vec<BrushSide>,
    pub texinfo: Vec<TexInfo>,
    pub texdata: Vec<TexData>,
    /// Null-separated material names, indexed by [`TexData::name_offset`].
    pub texdata_strings: Vec<u8>,
    /// Compiled PVS; empty until Umbra runs.
    pub visibility: Vec<u8>,
    /// Baked lightmap samples; empty until Radiance runs.
    pub lighting: Vec<ColorRgbExp32>,
    /// Rooms and which leaf is in which; `None` until Resonance runs.
    pub acoustics: Option<Acoustics>,
    /// The streamed sections; always at least the world, section 0.
    pub sections: Vec<Section>,
    /// One per face: which section it belongs to. Empty means all in 0.
    pub face_sections: Vec<u16>,
    /// One per brush: which section it belongs to. Empty means all in 0.
    pub brush_sections: Vec<u16>,
    /// Reflection probes; `None` until Radiance finds an `env_cubemap`.
    pub cubemaps: Option<Cubemaps>,
}

impl Bsp {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- tree queries ----------------------------------------------------

    /// Walk the tree from `head_node` and return the leaf containing `point`.
    ///
    /// The workhorse query: every position update, every sound, every
    /// visibility test starts here. It is a handful of dot products deep --
    /// tree depth, not geometry count -- which is the whole reason the BSP
    /// exists.
    pub fn point_leaf_from(&self, point: Vec3, head_node: i32) -> usize {
        let mut child = head_node;
        // Bounded rather than `loop`, so a cyclic tree from a broken compile
        // returns a wrong answer instead of hanging the engine.
        for _ in 0..self.nodes.len() + 1 {
            match decode_child(child) {
                Child::Leaf(leaf) => return leaf.min(self.leaves.len().saturating_sub(1)),
                Child::Node(n) => {
                    let Some(node) = self.nodes.get(n) else {
                        return 0;
                    };
                    let plane = self.planes[node.plane as usize].to_plane();
                    child = if plane.distance_to(point) >= 0.0 {
                        node.children[0]
                    } else {
                        node.children[1]
                    };
                }
            }
        }
        0
    }

    /// Leaf containing `point` in the world model.
    /// The acoustic room around a point, if the map has acoustics and the
    /// point is somewhere sound was measured.
    pub fn room_at(&self, point: Vec3) -> Option<(u16, &AcousticRoom)> {
        self.acoustics
            .as_ref()?
            .room_of_leaf(self.point_leaf(point))
    }

    pub fn point_leaf(&self, point: Vec3) -> usize {
        let head = self.models.first().map_or(0, |m| m.head_node);
        self.point_leaf_from(point, head)
    }

    /// Visibility cluster at `point`, or `-1` inside solid geometry.
    pub fn point_cluster(&self, point: Vec3) -> i16 {
        self.leaves
            .get(self.point_leaf(point))
            .map_or(-1, |l| l.cluster)
    }

    /// Contents of the leaf at `point` -- solid, water, and so on.
    pub fn point_contents(&self, point: Vec3) -> u32 {
        self.leaves
            .get(self.point_leaf(point))
            .map_or(contents::SOLID, |l| l.contents)
    }

    /// Whether `point` is inside solid world geometry.
    pub fn point_is_solid(&self, point: Vec3) -> bool {
        self.point_contents(point) & contents::SOLID != 0
    }

    /// Leaves whose cluster is visible from `from_cluster`.
    ///
    /// With no compiled PVS every leaf comes back, so an unlit, un-vised map
    /// still renders -- just without the culling.
    pub fn visible_leaves(&self, from_cluster: i16) -> Vec<usize> {
        let Some(visdata) = VisData::new(&self.visibility) else {
            return (0..self.leaves.len()).collect();
        };
        if from_cluster < 0 {
            return (0..self.leaves.len()).collect();
        }
        let row = visdata.decompress(from_cluster as usize, VisKind::Pvs);
        (0..self.leaves.len())
            .filter(|&i| {
                let c = self.leaves[i].cluster;
                c >= 0 && vis::row_test(&row, c as usize)
            })
            .collect()
    }

    /// Whether one cluster can see another.
    pub fn cluster_visible(&self, from: i16, to: i16) -> bool {
        if from < 0 || to < 0 {
            return true;
        }
        match VisData::new(&self.visibility) {
            Some(v) => v.is_visible(from as usize, to as usize, VisKind::Pvs),
            None => true,
        }
    }

    /// Whether sound from one cluster can reach another at all: the PAS,
    /// which is the PVS grown by one room. A doorway you cannot see
    /// through is still one you can hear through.
    pub fn cluster_audible(&self, from: i16, to: i16) -> bool {
        if from < 0 || to < 0 {
            return true;
        }
        match VisData::new(&self.visibility) {
            Some(v) => v.is_visible(from as usize, to as usize, VisKind::Pas),
            None => true,
        }
    }

    pub fn num_clusters(&self) -> usize {
        // Filter before the cast: a solid leaf's cluster is -1, and casting
        // that to usize first would wrap to the largest possible count.
        self.leaves
            .iter()
            .filter(|&l| l.cluster >= 0)
            .map(|l| l.cluster as usize + 1)
            .max()
            .unwrap_or(0)
    }

    // ---- geometry --------------------------------------------------------

    /// World-space vertices of a face, in order.
    ///
    /// Walks the surfedge indirection: each entry indexes an edge, negatively
    /// if the edge should be traversed backwards. Faces sharing an edge
    /// therefore share *identical* vertex positions, which is what keeps their
    /// seam from cracking open under floating-point rounding.
    pub fn face_vertices(&self, face_index: usize) -> Vec<Vec3> {
        let Some(face) = self.faces.get(face_index) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(face.num_surfedges as usize);
        for i in 0..face.num_surfedges as usize {
            let Some(&se) = self.surfedges.get(face.first_surfedge as usize + i) else {
                break;
            };
            let (edge_index, end) = if se >= 0 {
                (se as usize, 0)
            } else {
                ((-se) as usize, 1)
            };
            let Some(edge) = self.edges.get(edge_index) else {
                break;
            };
            let Some(v) = self.vertices.get(edge.v[end] as usize) else {
                break;
            };
            out.push(Vec3::from_array(*v));
        }
        out
    }

    /// The plane a face lies in, already flipped if the face is on the back side.
    pub fn face_plane(&self, face_index: usize) -> Option<Plane> {
        let face = self.faces.get(face_index)?;
        let plane = self.planes.get(face.plane as usize)?.to_plane();
        Some(if face.side != 0 {
            plane.flipped()
        } else {
            plane
        })
    }

    pub fn face_bounds(&self, face_index: usize) -> Aabb {
        Aabb::from_points(&self.face_vertices(face_index))
    }

    /// Material name for a texdata index.
    pub fn texdata_name(&self, index: usize) -> &str {
        let Some(td) = self.texdata.get(index) else {
            return "";
        };
        read_c_string(&self.texdata_strings, td.name_offset as usize)
    }

    /// Material name for a texinfo index.
    pub fn texinfo_name(&self, index: usize) -> &str {
        match self.texinfo.get(index) {
            Some(ti) => self.texdata_name(ti.texdata as usize),
            None => "",
        }
    }

    /// Material name a face draws with.
    pub fn face_material(&self, face_index: usize) -> &str {
        match self.faces.get(face_index) {
            Some(f) => self.texinfo_name(f.texinfo as usize),
            None => "",
        }
    }

    /// Every distinct material the map references.
    pub fn materials(&self) -> Vec<&str> {
        let mut names: Vec<&str> = (0..self.texdata.len())
            .map(|i| self.texdata_name(i))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Add a material name to the string lump, reusing an existing entry.
    pub fn intern_texdata_string(&mut self, name: &str) -> u32 {
        let needle = name.as_bytes();
        let mut offset = 0usize;
        while offset < self.texdata_strings.len() {
            let existing = read_c_string(&self.texdata_strings, offset);
            if existing.as_bytes() == needle {
                return offset as u32;
            }
            offset += existing.len() + 1;
        }
        let at = self.texdata_strings.len() as u32;
        self.texdata_strings.extend_from_slice(needle);
        self.texdata_strings.push(0);
        at
    }

    /// Lightmap samples for a face, if it has any.
    pub fn face_lightmap(&self, face_index: usize) -> Option<&[ColorRgbExp32]> {
        let face = self.faces.get(face_index)?;
        if face.lightmap_offset < 0 {
            return None;
        }
        let start = face.lightmap_offset as usize;
        let count = (face.lightmap_size[0] as usize) * (face.lightmap_size[1] as usize);
        self.lighting.get(start..start + count)
    }

    // ---- entities --------------------------------------------------------

    /// Parse the entity lump.
    pub fn entities_kv(&self) -> Result<KeyValues, kerosene_kv::ParseError> {
        KeyValues::parse(&self.entities)
    }

    pub fn world_bounds(&self) -> Aabb {
        self.models.first().map_or(Aabb::EMPTY, |m| m.bounds())
    }

    // ---- integrity -------------------------------------------------------

    /// Check that every index in the file points at something real.
    ///
    /// Runs at load. A dangling index here becomes an out-of-bounds read or a
    /// nonsense trace deep inside the renderer, where the cause is invisible;
    /// catching it at the door costs one pass over the lumps and names the
    /// actual problem.
    pub fn validate(&self) -> Result<(), String> {
        let (np, nv, ne, nse) = (
            self.planes.len(),
            self.vertices.len(),
            self.edges.len(),
            self.surfedges.len(),
        );

        for (i, e) in self.edges.iter().enumerate() {
            if e.v[0] as usize >= nv || e.v[1] as usize >= nv {
                return Err(format!("edge {i} references vertex {:?} of {nv}", e.v));
            }
        }
        for (i, &se) in self.surfedges.iter().enumerate() {
            if se.unsigned_abs() as usize >= ne {
                return Err(format!("surfedge {i} references edge {se} of {ne}"));
            }
        }
        for (i, f) in self.faces.iter().enumerate() {
            if f.plane as usize >= np {
                return Err(format!("face {i} references plane {} of {np}", f.plane));
            }
            if f.first_surfedge as usize + f.num_surfedges as usize > nse {
                return Err(format!("face {i} surfedge range runs past the lump"));
            }
            if f.texinfo as usize >= self.texinfo.len() {
                return Err(format!(
                    "face {i} references texinfo {} of {}",
                    f.texinfo,
                    self.texinfo.len()
                ));
            }
        }
        for (i, ti) in self.texinfo.iter().enumerate() {
            if ti.texdata as usize >= self.texdata.len() {
                return Err(format!(
                    "texinfo {i} references texdata {} of {}",
                    ti.texdata,
                    self.texdata.len()
                ));
            }
        }
        for (i, n) in self.nodes.iter().enumerate() {
            if n.plane as usize >= np {
                return Err(format!("node {i} references plane {} of {np}", n.plane));
            }
            for (side, &c) in n.children.iter().enumerate() {
                match decode_child(c) {
                    Child::Node(x) if x >= self.nodes.len() => {
                        return Err(format!(
                            "node {i} child {side} references node {x} of {}",
                            self.nodes.len()
                        ));
                    }
                    Child::Leaf(x) if x >= self.leaves.len() => {
                        return Err(format!(
                            "node {i} child {side} references leaf {x} of {}",
                            self.leaves.len()
                        ));
                    }
                    _ => {}
                }
            }
        }
        for (i, l) in self.leaves.iter().enumerate() {
            if l.first_leafface as usize + l.num_leaffaces as usize > self.leaffaces.len() {
                return Err(format!("leaf {i} leafface range runs past the lump"));
            }
            if l.first_leafbrush as usize + l.num_leafbrushes as usize > self.leafbrushes.len() {
                return Err(format!("leaf {i} leafbrush range runs past the lump"));
            }
        }
        for (i, &lf) in self.leaffaces.iter().enumerate() {
            if lf as usize >= self.faces.len() {
                return Err(format!(
                    "leafface {i} references face {lf} of {}",
                    self.faces.len()
                ));
            }
        }
        for (i, &lb) in self.leafbrushes.iter().enumerate() {
            if lb as usize >= self.brushes.len() {
                return Err(format!(
                    "leafbrush {i} references brush {lb} of {}",
                    self.brushes.len()
                ));
            }
        }
        for (i, b) in self.brushes.iter().enumerate() {
            if b.first_side as usize + b.num_sides as usize > self.brushsides.len() {
                return Err(format!("brush {i} brushside range runs past the lump"));
            }
        }
        for (i, bs) in self.brushsides.iter().enumerate() {
            if bs.plane as usize >= np {
                return Err(format!(
                    "brushside {i} references plane {} of {np}",
                    bs.plane
                ));
            }
        }
        for (i, m) in self.models.iter().enumerate() {
            match decode_child(m.head_node) {
                Child::Node(x) if x >= self.nodes.len() && !self.nodes.is_empty() => {
                    return Err(format!("model {i} head node {x} is out of range"));
                }
                Child::Leaf(x) if x >= self.leaves.len() => {
                    return Err(format!("model {i} head leaf {x} is out of range"));
                }
                _ => {}
            }
            if m.first_face as usize + m.num_faces as usize > self.faces.len() {
                return Err(format!("model {i} face range runs past the lump"));
            }
        }
        if let Some(acoustics) = &self.acoustics {
            acoustics.validate(self.leaves.len())?;
        }
        if !self.face_sections.is_empty() && self.face_sections.len() != self.faces.len() {
            return Err(format!(
                "{} face sections for {} faces",
                self.face_sections.len(),
                self.faces.len()
            ));
        }
        if !self.brush_sections.is_empty() && self.brush_sections.len() != self.brushes.len() {
            return Err(format!(
                "{} brush sections for {} brushes",
                self.brush_sections.len(),
                self.brushes.len()
            ));
        }
        // A `Bsp` built in memory may not have named its sections yet; an
        // empty list is the one-section world, as an empty lump is.
        let n = self.sections.len().max(1);
        if let Some(bad) = self
            .face_sections
            .iter()
            .chain(self.brush_sections.iter())
            .find(|&&s| s as usize >= n)
        {
            return Err(format!("something is in section {bad} of {n}"));
        }
        Ok(())
    }

    /// How many sections there are: at least the world.
    pub fn section_count(&self) -> usize {
        self.sections.len().max(1)
    }

    /// The section a face is in.
    pub fn face_section(&self, face: usize) -> u16 {
        self.face_sections.get(face).copied().unwrap_or(0)
    }

    /// The section a brush is in.
    pub fn brush_section(&self, brush: usize) -> u16 {
        self.brush_sections.get(brush).copied().unwrap_or(0)
    }

    /// For each section, a bit per cluster: the clusters that hold any of
    /// its faces. This is what streaming intersects with the PVS to decide
    /// which sections the player might see.
    pub fn section_cluster_masks(&self) -> Vec<Vec<u8>> {
        let clusters = self.num_clusters().max(1);
        let bytes = vis::row_bytes(clusters);
        let mut masks = vec![vec![0u8; bytes]; self.section_count()];
        for leaf in &self.leaves {
            if leaf.cluster < 0 {
                continue;
            }
            let cluster = leaf.cluster as usize;
            if cluster >= clusters {
                continue;
            }
            let first = leaf.first_leafface as usize;
            for i in first..first + leaf.num_leaffaces as usize {
                let Some(&face) = self.leaffaces.get(i) else {
                    continue;
                };
                let section = self.face_section(face as usize) as usize;
                if let Some(mask) = masks.get_mut(section) {
                    mask[cluster / 8] |= 1 << (cluster % 8);
                }
            }
        }
        masks
    }

    /// A one-line-per-lump summary, for the compilers' output and the engine's
    /// `map_stats` command.
    pub fn stats(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("planes", self.planes.len()),
            ("vertices", self.vertices.len()),
            ("edges", self.edges.len()),
            ("surfedges", self.surfedges.len()),
            ("faces", self.faces.len()),
            ("nodes", self.nodes.len()),
            ("leaves", self.leaves.len()),
            ("leaffaces", self.leaffaces.len()),
            ("leafbrushes", self.leafbrushes.len()),
            ("models", self.models.len()),
            ("brushes", self.brushes.len()),
            ("brushsides", self.brushsides.len()),
            ("texinfo", self.texinfo.len()),
            ("texdata", self.texdata.len()),
            ("clusters", self.num_clusters()),
            ("vis bytes", self.visibility.len()),
            ("lightmap samples", self.lighting.len()),
            (
                "acoustic rooms",
                self.acoustics.as_ref().map_or(0, |a| a.rooms.len()),
            ),
            ("sections", self.section_count()),
            (
                "cubemaps",
                self.cubemaps.as_ref().map_or(0, |c| c.probes.len()),
            ),
        ]
    }
}

/// Read a NUL-terminated string out of the texdata string lump.
fn read_c_string(buf: &[u8], offset: usize) -> &str {
    if offset >= buf.len() {
        return "";
    }
    let rest = &buf[offset..];
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    std::str::from_utf8(&rest[..end]).unwrap_or("")
}

#[cfg(test)]
mod tests;
