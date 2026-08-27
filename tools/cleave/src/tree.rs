//! Building the BSP tree.
//!
//! The tree is built top-down out of the brushes' own planes. At each step we
//! pick the plane that best partitions the remaining brushes, split them
//! against it, and recurse. When no usable plane is left, the region is convex
//! and uniform, and becomes a leaf.
//!
//! The plane choice is the whole game. A bad split carves brushes into slivers
//! that then get split again, and compile time and file size run away. The
//! heuristic here is Quake's, and it balances three things:
//!
//! * **Don't split brushes.** Each split is a permanent cost paid by every
//!   later stage.
//! * **Prefer axial planes.** They stay exact under floating point and they
//!   tend to align with how rooms are actually built.
//! * **Prefer planes many faces already lie on.** One plane doing the work of
//!   twenty coplanar faces is the best possible outcome.
//!
//! Leaves learn they are solid the way Quake's compiler does: a solid brush
//! whose every side has been used as a node plane on the path down has been
//! completely carved out of space, so whatever is left inside it *is* it.

use crate::brush::BrushWork;
use std::collections::HashSet;
use void_math::{Aabb, PlaneSide, PlaneSet, Vec3};

/// One node or leaf. Interior nodes have a `plane`; leaves do not.
#[derive(Clone, Debug, Default)]
pub struct TreeNode {
    pub plane: Option<u32>,
    pub children: [usize; 2],
    pub parent: Option<usize>,
    pub bounds: Aabb,

    // ---- leaf-only ----
    pub contents: u32,
    /// Brush fragments that ended up here, for the collision lumps.
    pub brushes: Vec<BrushWork>,
    /// Portal indices touching this leaf; filled by the portal pass.
    pub portals: Vec<usize>,
    /// Non-zero once the flood fill has reached this leaf from a spawn point.
    pub occupied: u32,
    pub cluster: i16,
    /// Index this node or leaf gets in the output lumps.
    pub output_index: i32,
}

impl TreeNode {
    pub fn is_leaf(&self) -> bool { self.plane.is_none() }
}

/// A built tree, plus the synthetic node standing for everything outside the
/// world box. Reaching it during the flood fill is what "leak" means.
pub struct Tree {
    pub nodes: Vec<TreeNode>,
    pub root: usize,
    pub outside: usize,
    pub max_depth: usize,
    /// How many times a brush had to be cut to fit the tree.
    pub splits: usize,
}

/// Hard cap on tree depth.
///
/// A pathological map -- hundreds of near-parallel planes a hair apart -- can
/// otherwise recurse until the stack gives out. Stopping produces a slightly
/// worse tree; running out of stack produces no map at all.
const MAX_DEPTH: usize = 96;

/// Padding around the world box so the outer portals are never coincident
/// with real geometry.
const SIDE_SPACE: f32 = 8.0;

impl Tree {
    /// Build a tree from the structural brushes.
    pub fn build(brushes: Vec<BrushWork>, planes: &PlaneSet) -> Tree {
        let mut world = Aabb::EMPTY;
        for b in &brushes { world = world.union(&b.bounds); }
        if world.is_empty() {
            world = Aabb::new(Vec3::splat(-SIDE_SPACE), Vec3::splat(SIDE_SPACE));
        }
        let world = world.expanded(SIDE_SPACE);

        let mut tree = Tree {
            nodes: Vec::new(),
            root: 0,
            outside: 0,
            max_depth: 0,
            splits: 0,
        };

        tree.root = tree.alloc(TreeNode { bounds: world, ..Default::default() });
        let mut used: HashSet<u32> = HashSet::new();
        tree.build_recursive(tree.root, brushes, planes, &mut used, 0);

        // The outside node is a leaf standing for everything beyond the world
        // box. Its contents are *empty*, not solid: the flood fill detects a
        // leak by reaching it, and a solid node is impassable, so marking it
        // solid would make every map look sealed.
        tree.outside = tree.alloc(TreeNode {
            contents: void_bsp::contents::EMPTY,
            bounds: Aabb::WORLD,
            ..Default::default()
        });
        tree
    }

    fn alloc(&mut self, node: TreeNode) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn build_recursive(
        &mut self,
        node: usize,
        brushes: Vec<BrushWork>,
        planes: &PlaneSet,
        used: &mut HashSet<u32>,
        depth: usize,
    ) {
        self.max_depth = self.max_depth.max(depth);

        let split = if depth >= MAX_DEPTH { None } else { select_split(&brushes, planes, used) };

        let Some(plane_index) = split else {
            self.make_leaf(node, brushes);
            return;
        };

        // Plane pairs share one identity: using `p` also rules out `p ^ 1`.
        let pair = plane_index & !1;
        used.insert(pair);

        let (mut front_list, mut back_list) = (Vec::new(), Vec::new());
        for mut b in brushes {
            // Mark every side lying on this plane as consumed, in both halves.
            for s in &mut b.sides {
                if s.plane & !1 == pair { s.used_as_node = true; }
            }
            let (f, bk) = b.split(plane_index, planes);
            if f.is_some() && bk.is_some() { self.splits += 1; }
            front_list.extend(f);
            back_list.extend(bk);
        }

        let bounds = self.nodes[node].bounds;
        let front = self.alloc(TreeNode { parent: Some(node), bounds, ..Default::default() });
        let back = self.alloc(TreeNode { parent: Some(node), bounds, ..Default::default() });
        self.nodes[node].plane = Some(plane_index);
        self.nodes[node].children = [front, back];

        self.build_recursive(front, front_list, planes, used, depth + 1);
        self.build_recursive(back, back_list, planes, used, depth + 1);

        used.remove(&pair);
    }

    /// Turn a node into a leaf and work out what it is made of.
    fn make_leaf(&mut self, node: usize, brushes: Vec<BrushWork>) {
        use void_bsp::contents as c;
        let mut contents = 0u32;

        for b in &brushes {
            // A solid brush all of whose sides became node planes has been
            // fully carved out of space: everything still here is its inside.
            // Nothing can be more solid than that, so stop looking.
            if b.contents & c::SOLID != 0 && b.sides.iter().all(|s| s.used_as_node) {
                contents = c::SOLID;
                break;
            }
            contents |= b.contents;
        }

        let mut bounds = Aabb::EMPTY;
        for b in &brushes { bounds = bounds.union(&b.bounds); }

        let leaf = &mut self.nodes[node];
        leaf.plane = None;
        leaf.contents = contents;
        leaf.brushes = brushes;
        leaf.cluster = -1;
        if !bounds.is_empty() { leaf.bounds = bounds; }
    }

    pub fn leaves(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.nodes.len()).filter(move |&i| self.nodes[i].is_leaf() && i != self.outside)
    }

    pub fn leaf_count(&self) -> usize { self.leaves().count() }

    pub fn node_count(&self) -> usize {
        self.nodes.iter().filter(|n| !n.is_leaf()).count()
    }

    /// Walk the tree to the leaf containing `point`.
    pub fn point_leaf(&self, point: Vec3, planes: &PlaneSet) -> usize {
        let mut at = self.root;
        for _ in 0..self.nodes.len() + 1 {
            let node = &self.nodes[at];
            let Some(plane_index) = node.plane else { return at };
            let plane = planes.get(plane_index);
            at = if plane.distance_to(point) >= 0.0 { node.children[0] } else { node.children[1] };
        }
        at
    }

    /// Node indices with children before parents.
    pub fn post_order(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![(self.root, false)];
        while let Some((n, expanded)) = stack.pop() {
            if self.nodes[n].is_leaf() { out.push(n); continue; }
            if expanded {
                out.push(n);
            } else {
                stack.push((n, true));
                let [f, b] = self.nodes[n].children;
                stack.push((f, false));
                stack.push((b, false));
            }
        }
        out
    }
}

/// Score every candidate plane and take the best.
///
/// Returns `None` when nothing is left to split by, which makes the node a leaf.
fn select_split(brushes: &[BrushWork], planes: &PlaneSet, used: &HashSet<u32>) -> Option<u32> {
    if brushes.is_empty() { return None; }

    let mut best_value = i32::MIN;
    let mut best_plane = None;

    for brush in brushes {
        for side in &brush.sides {
            if side.winding.is_none() { continue; }
            let pair = side.plane & !1;
            if used.contains(&pair) { continue; }

            let plane = planes.get(side.plane);
            let (mut front, mut back, mut splits) = (0i32, 0i32, 0i32);
            for other in brushes {
                match other.classify(&plane) {
                    PlaneSide::Front => front += 1,
                    PlaneSide::Back | PlaneSide::On => back += 1,
                    PlaneSide::Cross => splits += 1,
                }
            }

            // Note there is deliberately no "must have brushes on both
            // sides" test here. A convex brush lies entirely *behind* every
            // one of its own outward-facing planes, so such a test would
            // reject every candidate and no tree would ever be built. Carving
            // those half-spaces out of the surrounding space is exactly the
            // work the tree is doing.

            // How many faces across all brushes lie on this same plane. One
            // plane serving a whole wall built from several brushes is the
            // best case there is.
            let coplanar = brushes
                .iter()
                .flat_map(|b| b.sides.iter())
                .filter(|s| s.plane & !1 == pair && s.winding.is_some())
                .count() as i32;

            // Quake's weighting: coplanar faces are worth a great deal,
            // splits cost the same amount, and balance is a mild tiebreak.
            let mut value = 5 * coplanar - 5 * splits - (front - back).abs() / 2;
            if plane.kind().is_axial() { value += 5; }
            // Prefer planes that carry real surfaces: faces then land on node
            // planes, where the renderer can cull them per node.
            if side.is_visible_surface() { value += 3; }

            if value > best_value {
                best_value = value;
                best_plane = Some(side.plane);
            }
        }
    }

    best_plane
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Warning;
    use void_map::Solid;

    fn brushes_from(boxes: &[(Aabb, &str)]) -> (Vec<BrushWork>, PlaneSet) {
        let mut planes = PlaneSet::new();
        let mut warnings: Vec<Warning> = Vec::new();
        let mut out = Vec::new();
        for (i, (b, material)) in boxes.iter().enumerate() {
            let mut solid = Solid::cube(*b, material);
            solid.id = i as u32 + 1;
            out.push(BrushWork::from_solid(&solid, 0, "worldspawn", &mut planes, &mut warnings).unwrap());
        }
        (out, planes)
    }

    /// A hollow room: six slabs enclosing an empty 256-cube.
    fn room() -> (Vec<BrushWork>, PlaneSet) {
        let t = 16.0;
        let (lo, hi) = (0.0f32, 256.0);
        brushes_from(&[
            (Aabb::new(Vec3::new(lo - t, lo - t, lo - t), Vec3::new(hi + t, hi + t, lo)), "dev/grid"), // floor
            (Aabb::new(Vec3::new(lo - t, lo - t, hi), Vec3::new(hi + t, hi + t, hi + t)), "dev/grid"), // ceiling
            (Aabb::new(Vec3::new(lo - t, lo - t, lo), Vec3::new(lo, hi + t, hi)), "dev/grid"),         // -X
            (Aabb::new(Vec3::new(hi, lo - t, lo), Vec3::new(hi + t, hi + t, hi)), "dev/grid"),         // +X
            (Aabb::new(Vec3::new(lo, lo - t, lo), Vec3::new(hi, lo, hi)), "dev/grid"),                 // -Y
            (Aabb::new(Vec3::new(lo, hi, lo), Vec3::new(hi, hi + t, hi)), "dev/grid"),                 // +Y
        ])
    }

    #[test]
    fn a_single_cube_builds_a_tree() {
        let (brushes, planes) = brushes_from(&[(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid")]);
        let tree = Tree::build(brushes, &planes);
        assert!(tree.node_count() >= 3, "a box needs at least 3 splitting planes");
        assert!(tree.leaf_count() >= 4);
    }

    #[test]
    fn inside_a_solid_cube_is_solid_and_outside_is_not() {
        let (brushes, planes) = brushes_from(&[(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid")]);
        let tree = Tree::build(brushes, &planes);

        let inside = tree.point_leaf(Vec3::splat(32.0), &planes);
        assert!(
            tree.nodes[inside].contents & void_bsp::contents::SOLID != 0,
            "the middle of a solid brush must be a solid leaf"
        );
        for p in [Vec3::splat(-32.0), Vec3::new(100.0, 32.0, 32.0), Vec3::splat(200.0)] {
            let leaf = tree.point_leaf(p, &planes);
            assert_eq!(
                tree.nodes[leaf].contents & void_bsp::contents::SOLID,
                0,
                "{p:?} is outside the brush and must be empty"
            );
        }
    }

    #[test]
    fn a_hollow_room_has_empty_air_inside_and_solid_walls() {
        let (brushes, planes) = room();
        let tree = Tree::build(brushes, &planes);

        let air = tree.point_leaf(Vec3::splat(128.0), &planes);
        assert_eq!(tree.nodes[air].contents, void_bsp::contents::EMPTY, "the room's air");

        // A point inside each wall slab.
        for p in [
            Vec3::new(128.0, 128.0, -8.0),  // floor
            Vec3::new(128.0, 128.0, 264.0), // ceiling
            Vec3::new(-8.0, 128.0, 128.0),  // -X wall
            Vec3::new(264.0, 128.0, 128.0), // +X wall
        ] {
            let leaf = tree.point_leaf(p, &planes);
            assert!(
                tree.nodes[leaf].contents & void_bsp::contents::SOLID != 0,
                "{p:?} should be inside a wall"
            );
        }
    }

    #[test]
    fn every_leaf_is_reachable_and_the_tree_is_acyclic() {
        let (brushes, planes) = room();
        let tree = Tree::build(brushes, &planes);
        let mut seen = vec![false; tree.nodes.len()];
        let mut stack = vec![tree.root];
        while let Some(n) = stack.pop() {
            assert!(!seen[n], "node {n} reached twice: the tree has a cycle");
            seen[n] = true;
            if !tree.nodes[n].is_leaf() {
                stack.extend(tree.nodes[n].children);
            }
        }
        // Everything but the outside node hangs off the root.
        let reached = seen.iter().filter(|&&s| s).count();
        assert_eq!(reached, tree.nodes.len() - 1);
    }

    #[test]
    fn axial_planes_are_preferred_over_slanted_ones() {
        // A box plus a 45-degree wedge. The first split should still be axial:
        // slanted planes cost precision and split more.
        let mut planes = PlaneSet::new();
        let mut warnings = Vec::new();
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        solid.sides.push(void_map::Side::from_plane(
            99,
            void_math::Plane::from_point_normal(
                Vec3::new(48.0, 48.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0).normalize(),
            ),
            "dev/grid",
        ));
        let b = BrushWork::from_solid(&solid, 0, "worldspawn", &mut planes, &mut warnings).unwrap();
        let tree = Tree::build(vec![b], &planes);
        let root_plane = planes.get(tree.nodes[tree.root].plane.unwrap());
        assert!(root_plane.kind().is_axial(), "root split was {:?}", root_plane.normal);
    }

    #[test]
    fn depth_stays_bounded_on_a_pathological_map() {
        // Fifty thin slabs stacked a unit apart: a worst case for tree depth.
        let boxes: Vec<(Aabb, &str)> = (0..50)
            .map(|i| {
                let z = i as f32 * 2.0;
                (Aabb::new(Vec3::new(0.0, 0.0, z), Vec3::new(64.0, 64.0, z + 1.0)), "dev/grid")
            })
            .collect();
        let (brushes, planes) = brushes_from(&boxes);
        let tree = Tree::build(brushes, &planes);
        assert!(tree.max_depth <= MAX_DEPTH, "depth {} exceeded the cap", tree.max_depth);
        assert!(tree.leaf_count() > 50);
    }

    #[test]
    fn a_shared_wall_plane_is_used_once_for_several_brushes() {
        // Four brushes whose tops are all at z = 64. That plane should be
        // chosen early, because it serves four faces at once.
        let boxes: Vec<(Aabb, &str)> = (0..4)
            .map(|i| {
                let x = i as f32 * 64.0;
                (Aabb::new(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 64.0, 64.0, 64.0)), "dev/grid")
            })
            .collect();
        let (brushes, planes) = brushes_from(&boxes);
        let tree = Tree::build(brushes, &planes);
        // With a good heuristic this should not need many more nodes than the
        // handful of distinct planes involved.
        assert!(tree.node_count() < 24, "used {} nodes for 4 aligned boxes", tree.node_count());
        assert_eq!(tree.splits, 0, "aligned boxes should never need splitting");
    }

    #[test]
    fn post_order_visits_children_before_parents() {
        let (brushes, planes) = room();
        let tree = Tree::build(brushes, &planes);
        let order = tree.post_order();
        let mut position = vec![usize::MAX; tree.nodes.len()];
        for (i, &n) in order.iter().enumerate() { position[n] = i; }
        for (i, node) in tree.nodes.iter().enumerate() {
            if node.is_leaf() || position[i] == usize::MAX { continue; }
            for c in node.children {
                assert!(position[c] < position[i], "child {c} came after parent {i}");
            }
        }
    }
}
