//! The compile, start to finish.
//!
//! ```text
//!   .voidmap  ->  brushes  ->  CSG  ->  BSP tree  ->  portals
//!                                                     |
//!                     .voidprt  <----  clusters  <---  flood fill
//!                                                     |
//!                                        fill outside  ->  emit  ->  .voidbsp
//! ```
//!
//! Each stage is separately testable and reports its own numbers, because
//! "the compile got slower" is a question a level designer asks often and the
//! only useful answer names a stage.

use crate::brush::{BrushWork, Warning};
use crate::emit::{self, BrushModel};
use crate::portal::{self, LeakPath, PortalSet};
use crate::tree::Tree;
use crate::{csg, material};
use void_bsp::Bsp;
use void_kv::KeyValues;
use void_map::Map;
use void_math::{PlaneSet, Vec3};

#[derive(Clone, Debug, Default)]
pub struct CompileOptions {
    /// Compile even if the world leaks. The result is playable but vis will
    /// be nearly useless, so this is off by default.
    pub ignore_leaks: bool,
    /// Skip removing the space outside the map. Useful when debugging a leak.
    pub no_fill: bool,
    pub verbose: bool,
}

/// Per-stage numbers, printed after a compile.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub source_brushes: usize,
    pub world_brushes: usize,
    pub detail_brushes: usize,
    pub entity_brushes: usize,
    pub faces_removed_by_csg: usize,
    pub tree_nodes: usize,
    pub tree_leaves: usize,
    pub tree_depth: usize,
    pub brush_splits: usize,
    pub portals: usize,
    pub tiny_portals: usize,
    pub leaves_filled: usize,
    pub clusters: usize,
    pub faces: usize,
    pub vertices: usize,
}

pub struct CompileOutput {
    pub bsp: Bsp,
    /// Portal graph for Umbra.
    pub prt: String,
    /// Set when the world is not sealed.
    pub leak: Option<LeakPath>,
    pub warnings: Vec<Warning>,
    pub stats: Stats,
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("the map has no brushes to compile")]
    NoBrushes,
    #[error("the map leaks: {0} entity could reach the void. Compile with --ignore-leaks to build it anyway.")]
    Leaked(String),
}

pub fn compile(map: &Map, options: &CompileOptions) -> Result<CompileOutput, CompileError> {
    let mut planes = PlaneSet::new();
    let mut warnings: Vec<Warning> = Vec::new();
    let mut stats = Stats::default();

    // ---- brushes ----
    let mut all: Vec<BrushWork> = Vec::new();
    for solid in &map.world.solids {
        stats.source_brushes += 1;
        if let Some(b) = BrushWork::from_solid(solid, 0, "worldspawn", &mut planes, &mut warnings) {
            all.push(b);
        }
    }
    // Entity indices start at 1; index 0 is worldspawn.
    let mut model_entities: Vec<usize> = Vec::new();
    for (i, entity) in map.entities.iter().enumerate() {
        if entity.solids.is_empty() { continue; }
        model_entities.push(i);
        let entity_slot = model_entities.len();
        for solid in &entity.solids {
            stats.source_brushes += 1;
            if let Some(b) =
                BrushWork::from_solid(solid, entity_slot, entity.classname(), &mut planes, &mut warnings)
            {
                all.push(b);
            }
        }
    }
    if all.is_empty() { return Err(CompileError::NoBrushes); }

    for (i, b) in all.iter_mut().enumerate() { b.original = i; }

    // ---- CSG ----
    stats.faces_removed_by_csg = csg::chop_brushes(&mut all, &planes);

    let world: Vec<BrushWork> = all.iter().filter(|b| b.entity == 0).cloned().collect();
    stats.world_brushes = world.len();
    stats.detail_brushes = world.iter().filter(|b| b.is_detail()).count();
    stats.entity_brushes = all.len() - world.len();

    // ---- tree ----
    let structural: Vec<BrushWork> = world.iter().filter(|b| b.is_structural()).cloned().collect();
    let mut tree = Tree::build(structural, &planes);
    stats.tree_nodes = tree.node_count();
    stats.tree_leaves = tree.leaf_count();
    stats.tree_depth = tree.max_depth;
    stats.brush_splits = tree.splits;

    // ---- portals and flood fill ----
    let portals: PortalSet = portal::build_portals(&mut tree, &mut planes);
    stats.portals = portals.portals.len();
    stats.tiny_portals = portals.tiny;

    let origins = entity_origins(map);
    let flood = portal::flood_entities(&mut tree, &portals, &planes, &origins);
    for origin in &flood.entities_in_solid {
        warnings.push(Warning {
            brush_id: 0,
            message: format!("an entity at {origin:?} is inside solid geometry"),
        });
    }

    if let Some(leak) = &flood.leak {
        if !options.ignore_leaks {
            return Err(CompileError::Leaked(format!("{:?}", leak.from)));
        }
    }

    // Only fill outside when the map is actually sealed. Filling a leaking map
    // turns the whole level solid, which is far more confusing than leaving it.
    if !options.no_fill && flood.leak.is_none() {
        stats.leaves_filled = portal::fill_outside(&mut tree);
    }
    stats.clusters = portal::assign_clusters(&mut tree);

    // ---- non-structural brushes into leaves ----
    // Detail, clips, triggers and water never split the tree, so they have to
    // be filed into whichever leaves they sit in for collision to find them.
    let root = tree.root;
    for b in world.iter().filter(|b| !b.is_structural()) {
        filter_brush(&mut tree, &planes, root, b.clone());
    }

    // ---- entities and brush models ----
    let mut models: Vec<BrushModel> = Vec::new();
    for &entity_index in &model_entities {
        let slot = models.len() + 1;
        let brushes: Vec<BrushWork> =
            all.iter().filter(|b| b.entity == slot).cloned().collect();
        models.push(BrushModel {
            brushes,
            origin: map.entities[entity_index].get_vec3("origin").unwrap_or(Vec3::ZERO),
        });
    }

    let entities_text = build_entity_lump(map, &model_entities);
    let prt = portal::write_prt(&tree, &portals, stats.clusters);

    let bsp = emit::emit(&tree, &planes, &world, &models, entities_text, 1);
    stats.faces = bsp.faces.len();
    stats.vertices = bsp.vertices.len();

    Ok(CompileOutput { bsp, prt, leak: flood.leak, warnings, stats })
}

/// Points the flood fill starts from.
///
/// Every point entity counts, not just spawn points: an entity anywhere in the
/// map means that spot is meant to be inside the world, so a hole near it is a
/// leak worth reporting.
fn entity_origins(map: &Map) -> Vec<Vec3> {
    map.entities
        .iter()
        .filter(|e| e.solids.is_empty())
        .filter_map(|e| e.get_vec3("origin"))
        .collect()
}

/// Push a brush down the tree, recording it in every open leaf it reaches.
fn filter_brush(tree: &mut Tree, planes: &PlaneSet, node: usize, brush: BrushWork) {
    if tree.nodes[node].is_leaf() {
        // Solid leaves are inside rock; nothing ever traces against them.
        if tree.nodes[node].contents & void_bsp::contents::SOLID == 0 {
            tree.nodes[node].brushes.push(brush);
        }
        return;
    }
    let plane_index = tree.nodes[node].plane.expect("interior node has a plane");
    let [front_child, back_child] = tree.nodes[node].children;
    let (f, b) = brush.split(plane_index, planes);
    if let Some(fb) = f { filter_brush(tree, planes, front_child, fb); }
    if let Some(bb) = b { filter_brush(tree, planes, back_child, bb); }
}

/// Serialise the entity lump.
///
/// Brush entities gain a `model` key naming their brush model -- `"*1"`,
/// `"*2"` and so on. That indirection is how a `func_door` moves: the entity
/// carries a model index, and moving it moves the model, leaving the world
/// tree untouched.
fn build_entity_lump(map: &Map, model_entities: &[usize]) -> String {
    let mut root = KeyValues::new("");

    let mut world = KeyValues::new("entity");
    for (k, v) in &map.world.properties { world.push(k.clone(), v.clone()); }
    if !world.contains_key("classname") { world.push("classname", "worldspawn"); }
    world.set("model", "*0");
    root.push_block(world);

    for (i, entity) in map.entities.iter().enumerate() {
        let mut kv = KeyValues::new("entity");
        for (k, v) in &entity.properties { kv.push(k.clone(), v.clone()); }
        if let Some(slot) = model_entities.iter().position(|&e| e == i) {
            kv.set("model", format!("*{}", slot + 1));
        }
        if !entity.connections.is_empty() {
            let mut conn = KeyValues::new("connections");
            for c in &entity.connections { conn.push(c.output.clone(), c.to_value()); }
            kv.push_block(conn);
        }
        root.push_block(kv);
    }

    root.to_document()
}

/// Warn about tool materials that will not do what the designer expects.
pub fn lint_materials(map: &Map) -> Vec<String> {
    let mut out = Vec::new();
    for (_, solid) in map.all_solids() {
        for side in &solid.sides {
            if !material::is_known_tool(&side.material) {
                out.push(format!(
                    "brush {} face {}: '{}' is not a known tools/ material",
                    solid.id, side.id, side.material
                ));
            }
        }
    }
    out
}
