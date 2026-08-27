//! Turning the compiled tree into `.vbsp` lumps.
//!
//! Three jobs happen here, in order:
//!
//! 1. **Filing faces into the tree.** Each surviving CSG fragment is pushed
//!    down the tree, split where it straddles a node plane, and deposited in
//!    the leaf that can see it. A fragment landing in a solid leaf is dropped
//!    -- which is where outside removal pays off, because after the flood fill
//!    the entire outer shell of the map faces into solid.
//!
//! 2. **Welding vertices and edges.** Faces that meet must share vertex
//!    *records*, not merely equal coordinates, or their seam cracks open under
//!    rounding. Shared edges then let the surfedge indirection do its job.
//!
//! 3. **Writing the lumps**, assigning output indices to nodes and leaves.

use crate::brush::BrushWork;
use crate::tree::Tree;
use std::collections::HashMap;
use void_bsp::{
    Brush, BrushSide, Bsp, ColorRgbExp32, Edge, Face, Leaf, Model, Node, TexData, TexInfo,
    contents, encode_leaf, surf,
};
use void_math::{Aabb, ON_EPSILON, Plane, PlaneSet, PlaneSide, Vec3, Winding};

/// Largest lightmap a single face may claim, in luxels per side.
///
/// A face wanting more gets a coarser scale instead. Without a cap, one
/// enormous floor brush can demand a lightmap larger than the rest of the map
/// put together.
const MAX_LIGHTMAP_DIM: u32 = 64;

/// Vertices closer than this are the same vertex.
///
/// Well under [`ON_EPSILON`], so welding can never move a vertex far enough to
/// be visible, but comfortably above the float noise left by a chain of plane
/// intersections.
const WELD_EPSILON: f32 = 0.05;

/// A face waiting to be written, still holding its polygon.
struct PendingFace {
    winding: Winding,
    plane: u32,
    /// 1 when the face points opposite its plane.
    side: u8,
    texinfo: u32,
    lightmap_scale: f32,
}

/// Merges coincident vertices so that adjacent faces share vertex records.
#[derive(Default)]
struct VertexWelder {
    vertices: Vec<[f32; 3]>,
    /// Bucketed by a coarse grid; a lookup checks the 27 surrounding cells so
    /// a vertex near a cell boundary still finds its twin.
    buckets: HashMap<(i32, i32, i32), Vec<u32>>,
}

impl VertexWelder {
    fn cell(p: Vec3) -> (i32, i32, i32) {
        ((p.x / 1.0).floor() as i32, (p.y / 1.0).floor() as i32, (p.z / 1.0).floor() as i32)
    }

    fn add(&mut self, p: Vec3) -> u32 {
        // Map coordinates are overwhelmingly integers; snapping first makes
        // most welds exact rather than approximate.
        let p = Vec3::new(snap(p.x), snap(p.y), snap(p.z));
        let (cx, cy, cz) = Self::cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = self.buckets.get(&(cx + dx, cy + dy, cz + dz)) else { continue };
                    for &i in list {
                        let q = Vec3::from_array(self.vertices[i as usize]);
                        if (q - p).length() < WELD_EPSILON { return i; }
                    }
                }
            }
        }
        let index = self.vertices.len() as u32;
        self.vertices.push(p.to_array());
        self.buckets.entry((cx, cy, cz)).or_default().push(index);
        index
    }
}

fn snap(v: f32) -> f32 {
    let r = v.round();
    if (v - r).abs() < 0.01 { r } else { v }
}

/// Builds the edge and surfedge lumps, sharing each edge between the two faces
/// that meet along it.
#[derive(Default)]
struct EdgeBuilder {
    edges: Vec<Edge>,
    surfedges: Vec<i32>,
    /// Edges available to be used a second time, in the reverse direction.
    available: HashMap<(u32, u32), u32>,
}

impl EdgeBuilder {
    fn new() -> Self {
        // Edge 0 is a placeholder: surfedges are signed, and -0 cannot mean
        // "edge 0 reversed".
        EdgeBuilder { edges: vec![Edge { v: [0, 0] }], ..Default::default() }
    }

    fn add(&mut self, v0: u32, v1: u32) -> i32 {
        // An edge already recorded as (v1, v0) is this same edge, walked the
        // other way -- exactly what a neighbouring face wants.
        if let Some(index) = self.available.remove(&(v1, v0)) {
            return -(index as i32);
        }
        let index = self.edges.len() as u32;
        self.edges.push(Edge { v: [v0, v1] });
        self.available.insert((v0, v1), index);
        index as i32
    }

    fn push_face(&mut self, verts: &[u32]) -> (u32, u32) {
        let first = self.surfedges.len() as u32;
        for i in 0..verts.len() {
            let se = self.add(verts[i], verts[(i + 1) % verts.len()]);
            self.surfedges.push(se);
        }
        (first, verts.len() as u32)
    }
}

/// Interns texinfo and texdata records.
#[derive(Default)]
struct TexBuilder {
    texinfo: Vec<TexInfo>,
    texdata: Vec<TexData>,
    strings: Vec<u8>,
    texdata_by_name: HashMap<String, u32>,
    texinfo_seen: HashMap<String, u32>,
}

impl TexBuilder {
    fn texdata_for(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.texdata_by_name.get(name) { return i; }
        let offset = self.strings.len() as u32;
        self.strings.extend_from_slice(name.as_bytes());
        self.strings.push(0);
        let index = self.texdata.len() as u32;
        self.texdata.push(TexData {
            // A neutral grey until Radiance reads the real material and
            // learns its actual average colour for bounce lighting.
            reflectivity: [0.5, 0.5, 0.5],
            name_offset: offset,
            width: 512,
            height: 512,
            view_width: 512,
            view_height: 512,
        });
        self.texdata_by_name.insert(name.to_string(), index);
        index
    }

    fn intern(&mut self, side: &crate::brush::SideWork) -> u32 {
        let u = side.uaxis;
        let v = side.vaxis;
        let (us, vs) = (u.safe_scale(), v.safe_scale());
        let lm = side.lightmap_scale.max(1.0);

        let key = format!(
            "{}|{:?}|{}|{:?}|{}|{}|{}",
            side.material, u.axis, u.offset, v.axis, v.offset, lm, side.surface
        );
        if let Some(&i) = self.texinfo_seen.get(&key) { return i; }

        let texdata = self.texdata_for(&side.material);
        let mut ti = TexInfo { flags: side.surface, texdata, ..Default::default() };
        ti.texture_vecs[0] = [u.axis.x / us, u.axis.y / us, u.axis.z / us, u.offset];
        ti.texture_vecs[1] = [v.axis.x / vs, v.axis.y / vs, v.axis.z / vs, v.offset];

        // Lightmap axes point the same way but are scaled to luxels, and are
        // normalised first so the luxel grid is square regardless of how the
        // texture happens to be stretched.
        let un = u.axis.normalize_or_zero() / lm;
        let vn = v.axis.normalize_or_zero() / lm;
        ti.lightmap_vecs[0] = [un.x, un.y, un.z, 0.0];
        ti.lightmap_vecs[1] = [vn.x, vn.y, vn.z, 0.0];

        let index = self.texinfo.len() as u32;
        self.texinfo.push(ti);
        self.texinfo_seen.insert(key, index);
        index
    }
}

/// Everything the emitter needs about one entity's brushes.
pub struct BrushModel {
    /// Brushes belonging to this entity, already CSG'd.
    pub brushes: Vec<BrushWork>,
    /// Rotation origin for movers.
    pub origin: Vec3,
}

/// Assemble the final `.vbsp`.
pub fn emit(
    tree: &Tree,
    planes: &PlaneSet,
    world_brushes: &[BrushWork],
    brush_models: &[BrushModel],
    entities_text: String,
    revision: u32,
) -> Bsp {
    let mut bsp = Bsp::new();
    bsp.revision = revision;
    bsp.entities = entities_text;
    bsp.planes = planes.planes().iter().map(void_bsp::BspPlane::from_plane).collect();

    let mut tex = TexBuilder::default();
    let mut welder = VertexWelder::default();
    let mut edges = EdgeBuilder::new();

    // ---- 1. file world faces into leaves ----
    let mut leaf_faces: HashMap<usize, Vec<PendingFace>> = HashMap::new();
    for brush in world_brushes {
        for side in &brush.sides {
            if !side.is_visible_surface() { continue; }
            let texinfo = tex.intern(side);
            for fragment in &side.fragments {
                let plane = planes.get(side.plane);
                file_face(
                    tree,
                    planes,
                    tree.root,
                    fragment.clone(),
                    side.plane,
                    plane,
                    texinfo,
                    side.lightmap_scale,
                    &mut leaf_faces,
                );
            }
        }
    }

    // ---- 2. walk the tree, assigning output indices ----
    let mut node_index = vec![-1i32; tree.nodes.len()];
    let mut leaf_index = vec![-1i32; tree.nodes.len()];
    let (mut next_node, mut next_leaf) = (0i32, 0i32);
    let mut order: Vec<usize> = Vec::new();
    let mut stack = vec![tree.root];
    while let Some(n) = stack.pop() {
        order.push(n);
        if tree.nodes[n].is_leaf() {
            leaf_index[n] = next_leaf;
            next_leaf += 1;
        } else {
            node_index[n] = next_node;
            next_node += 1;
            let [f, b] = tree.nodes[n].children;
            stack.push(b);
            stack.push(f);
        }
    }

    bsp.nodes = vec![Node::default(); next_node as usize];
    bsp.leaves = vec![Leaf::default(); next_leaf as usize];

    // ---- 3. brushes and brush sides for collision ----
    let mut brush_index_map: HashMap<usize, u32> = HashMap::new();
    for (i, brush) in world_brushes.iter().enumerate() {
        let first_side = bsp.brushsides.len() as u32;
        for side in &brush.sides {
            bsp.brushsides.push(BrushSide {
                plane: side.plane,
                texinfo: if side.generated { -1 } else { tex.intern(side) as i32 },
                bevel: side.generated as u32,
            });
        }
        let index = bsp.brushes.len() as u32;
        bsp.brushes.push(Brush {
            first_side,
            num_sides: brush.sides.len() as u32,
            contents: brush.contents,
        });
        brush_index_map.insert(i, index);
    }

    // ---- 4. emit faces and leaves ----
    for &n in &order {
        let node = &tree.nodes[n];
        if !node.is_leaf() { continue; }

        let first_leafface = bsp.leaffaces.len() as u32;
        let solid = node.contents & contents::SOLID != 0;
        if !solid {
            if let Some(pending) = leaf_faces.remove(&n) {
                for pf in pending {
                    let face = build_face(pf, &mut welder, &mut edges, &tex);
                    bsp.leaffaces.push(bsp.faces.len() as u32);
                    bsp.faces.push(face);
                }
            }
        }
        let num_leaffaces = bsp.leaffaces.len() as u32 - first_leafface;

        let first_leafbrush = bsp.leafbrushes.len() as u32;
        let mut seen: Vec<u32> = Vec::new();
        for fragment in &node.brushes {
            if let Some(&index) = brush_index_map.get(&fragment.original) {
                if !seen.contains(&index) { seen.push(index); }
            }
        }
        seen.sort_unstable();
        bsp.leafbrushes.extend(&seen);
        let num_leafbrushes = bsp.leafbrushes.len() as u32 - first_leafbrush;

        let b = if node.bounds.is_empty() { Aabb::new(Vec3::ZERO, Vec3::ZERO) } else { node.bounds };
        bsp.leaves[leaf_index[n] as usize] = Leaf {
            contents: node.contents,
            first_leafface,
            first_leafbrush,
            num_leaffaces: num_leaffaces.min(u16::MAX as u32) as u16,
            num_leafbrushes: num_leafbrushes.min(u16::MAX as u32) as u16,
            cluster: node.cluster,
            area: 0,
            mins: clamp_i16(b.min.floor()),
            maxs: clamp_i16(b.max.ceil()),
        };
    }

    // ---- 5. emit nodes ----
    for &n in &order {
        let node = &tree.nodes[n];
        let Some(plane) = node.plane else { continue };
        let child_ref = |c: usize| -> i32 {
            if tree.nodes[c].is_leaf() { encode_leaf(leaf_index[c] as usize) } else { node_index[c] }
        };
        let b = node.bounds;
        bsp.nodes[node_index[n] as usize] = Node {
            plane,
            children: [child_ref(node.children[0]), child_ref(node.children[1])],
            mins: clamp_i16(b.min.floor()),
            maxs: clamp_i16(b.max.ceil()),
            first_face: 0,
            num_faces: 0,
            area: 0,
        };
    }

    // ---- 6. models ----
    let world_bounds = tree.nodes[tree.root].bounds;
    bsp.models.push(Model {
        mins: world_bounds.min.to_array(),
        maxs: world_bounds.max.to_array(),
        origin: [0.0; 3],
        head_node: if bsp.nodes.is_empty() { encode_leaf(0) } else { 0 },
        first_face: 0,
        num_faces: bsp.faces.len() as u32,
    });

    // Brush entities get one leaf apiece rather than a tree of their own.
    // A `func_door` is a handful of convex brushes; walking a two-node tree to
    // find them costs more than testing them directly, and it keeps moving
    // geometry out of the world tree where it would have to be re-split every
    // time it moved.
    for model in brush_models {
        let first_face = bsp.faces.len() as u32;
        let mut bounds = Aabb::EMPTY;
        let first_leafbrush = bsp.leafbrushes.len() as u32;

        for brush in &model.brushes {
            bounds = bounds.union(&brush.bounds);
            let first_side = bsp.brushsides.len() as u32;
            for side in &brush.sides {
                bsp.brushsides.push(BrushSide {
                    plane: side.plane,
                    texinfo: if side.generated { -1 } else { tex.intern(side) as i32 },
                    bevel: side.generated as u32,
                });
            }
            bsp.leafbrushes.push(bsp.brushes.len() as u32);
            bsp.brushes.push(Brush {
                first_side,
                num_sides: brush.sides.len() as u32,
                contents: brush.contents,
            });

            for side in &brush.sides {
                if !side.is_visible_surface() { continue; }
                let texinfo = tex.intern(side);
                for fragment in &side.fragments {
                    let pf = PendingFace {
                        winding: fragment.clone(),
                        plane: side.plane,
                        side: 0,
                        texinfo,
                        lightmap_scale: side.lightmap_scale,
                    };
                    let face = build_face(pf, &mut welder, &mut edges, &tex);
                    bsp.faces.push(face);
                }
            }
        }

        let num_leafbrushes = bsp.leafbrushes.len() as u32 - first_leafbrush;
        let leaf = bsp.leaves.len();
        let b = if bounds.is_empty() { Aabb::new(Vec3::ZERO, Vec3::ZERO) } else { bounds };
        bsp.leaves.push(Leaf {
            contents: model.brushes.first().map_or(contents::SOLID, |b| b.contents),
            first_leafface: 0,
            first_leafbrush,
            num_leaffaces: 0,
            num_leafbrushes: num_leafbrushes.min(u16::MAX as u32) as u16,
            cluster: -1,
            area: 0,
            mins: clamp_i16(b.min.floor()),
            maxs: clamp_i16(b.max.ceil()),
        });

        bsp.models.push(Model {
            mins: b.min.to_array(),
            maxs: b.max.to_array(),
            origin: model.origin.to_array(),
            head_node: encode_leaf(leaf),
            first_face,
            num_faces: bsp.faces.len() as u32 - first_face,
        });
    }

    bsp.vertices = welder.vertices;
    bsp.edges = edges.edges;
    bsp.surfedges = edges.surfedges;
    bsp.texinfo = tex.texinfo;
    bsp.texdata = tex.texdata;
    bsp.texdata_strings = tex.strings;
    bsp.lighting = Vec::<ColorRgbExp32>::new();
    bsp
}

fn clamp_i16(v: Vec3) -> [i16; 3] {
    [
        v.x.clamp(i16::MIN as f32, i16::MAX as f32) as i16,
        v.y.clamp(i16::MIN as f32, i16::MAX as f32) as i16,
        v.z.clamp(i16::MIN as f32, i16::MAX as f32) as i16,
    ]
}

/// Push one face fragment down the tree until it reaches the leaves that can
/// see it, splitting it at every node plane it crosses.
#[allow(clippy::too_many_arguments)]
fn file_face(
    tree: &Tree,
    planes: &PlaneSet,
    node: usize,
    winding: Winding,
    plane_index: u32,
    face_plane: Plane,
    texinfo: u32,
    lightmap_scale: f32,
    out: &mut HashMap<usize, Vec<PendingFace>>,
) {
    if winding.is_tiny() { return; }

    let n = &tree.nodes[node];
    let Some(node_plane_index) = n.plane else {
        // A face pointing into solid rock cannot be seen and is dropped. After
        // the flood fill this is what removes the map's entire outer shell.
        if n.contents & contents::SOLID != 0 { return; }
        out.entry(node).or_default().push(PendingFace {
            winding,
            plane: plane_index & !1,
            // The stored plane is the canonical one of the pair, so a face
            // built on the odd half of the pair is flagged as reversed.
            side: (plane_index & 1) as u8,
            texinfo,
            lightmap_scale,
        });
        return;
    };

    let node_plane = planes.get(node_plane_index);
    let recurse = |child: usize, w: Winding, out: &mut HashMap<usize, Vec<PendingFace>>| {
        file_face(tree, planes, child, w, plane_index, face_plane, texinfo, lightmap_scale, out);
    };

    match winding.classify(&node_plane, ON_EPSILON) {
        PlaneSide::Front => recurse(n.children[0], winding, out),
        PlaneSide::Back => recurse(n.children[1], winding, out),
        PlaneSide::On => {
            // Coplanar with the node plane: the face belongs on whichever side
            // it faces, because that is the side that can see it.
            let child = if face_plane.normal.dot(node_plane.normal) > 0.0 {
                n.children[0]
            } else {
                n.children[1]
            };
            recurse(child, winding, out);
        }
        PlaneSide::Cross => {
            let (f, b) = winding.split(&node_plane, ON_EPSILON);
            if let Some(w) = f { recurse(n.children[0], w, out); }
            if let Some(w) = b { recurse(n.children[1], w, out); }
        }
    }
}

/// Weld a pending face's vertices and compute its lightmap extents.
fn build_face(
    pf: PendingFace,
    welder: &mut VertexWelder,
    edges: &mut EdgeBuilder,
    tex: &TexBuilder,
) -> Face {
    let verts: Vec<u32> = pf.winding.points.iter().map(|&p| welder.add(p)).collect();
    let (first_surfedge, num_surfedges) = edges.push_face(&verts);

    let ti = &tex.texinfo[pf.texinfo as usize];
    let (mins, size) = lightmap_extents(&pf.winding, ti, pf.lightmap_scale);

    Face {
        plane: pf.plane,
        side: pf.side,
        on_node: 0,
        _pad: [0; 2],
        first_surfedge,
        num_surfedges,
        texinfo: pf.texinfo,
        dispinfo: -1,
        // Radiance fills these in; until then the face is unlit.
        lightmap_offset: -1,
        lightmap_mins: mins,
        lightmap_size: size,
        light_styles: [0, 255, 255, 255],
        area: pf.winding.area(),
    }
}

/// How many luxels a face needs, and where its luxel grid starts.
fn lightmap_extents(w: &Winding, ti: &TexInfo, scale: f32) -> ([i32; 2], [u32; 2]) {
    if ti.flags & (surf::NOLIGHT | surf::SKY | surf::NODRAW) != 0 {
        return ([0, 0], [0, 0]);
    }

    let (mut min_u, mut min_v) = (f32::INFINITY, f32::INFINITY);
    let (mut max_u, mut max_v) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for &p in &w.points {
        let (u, v) = ti.lightcoord(p);
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    if !min_u.is_finite() || !min_v.is_finite() { return ([0, 0], [0, 0]); }

    let mins = [min_u.floor() as i32, min_v.floor() as i32];
    let mut size = [
        (max_u.ceil() as i32 - mins[0] + 1).max(1) as u32,
        (max_v.ceil() as i32 - mins[1] + 1).max(1) as u32,
    ];

    // A single enormous face must not claim an unbounded lightmap. Clamping
    // the dimension effectively coarsens the scale for that face alone.
    let _ = scale;
    size[0] = size[0].min(MAX_LIGHTMAP_DIM);
    size[1] = size[1].min(MAX_LIGHTMAP_DIM);
    (mins, size)
}

#[cfg(test)]
mod tests;
