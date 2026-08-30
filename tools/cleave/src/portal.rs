// SPDX-License-Identifier: LGPL-3.0-or-later
//! Portals, flood filling, and leak detection.
//!
//! A *portal* is the polygon where two leaves touch -- the doorway between two
//! convex cells. Portals are what turn a tree of planes into a connectivity
//! graph, and three things fall out of having them:
//!
//! * **Leak detection.** Flood outward from every entity through passable
//!   portals. If the flood reaches the outside of the world, the map has a
//!   hole in it, and Cleave says exactly where.
//! * **Outside removal.** Leaves the flood never reached are outside the
//!   sealed world. Turning them solid deletes the entire outer surface of the
//!   map from the compile.
//! * **Visibility.** Umbra computes the PVS by asking which portals can see
//!   through which others, so Cleave writes the portal graph out as a `.keroprt`.
//!
//! The construction is Quake's: start with six portals on the world box, then
//! walk down the tree. At each node, build a new portal on the node's plane
//! bounded by the portals already touching that node, and hand each existing
//! portal down to whichever children it now touches, splitting it if it
//! straddles the plane.

use crate::tree::Tree;
use std::collections::VecDeque;
use std::fmt::Write as _;
use kerosene_bsp::contents;
use kerosene_math::{Aabb, ON_EPSILON, Plane, PlaneSet, Vec3, Winding};

/// The polygon where two leaves meet.
#[derive(Clone, Debug)]
pub struct Portal {
    /// Plane the portal lies on. `nodes[0]` is in front of it.
    pub plane: u32,
    pub winding: Winding,
    /// `[front, back]` relative to `plane`.
    pub nodes: [usize; 2],
    /// The node whose split created this portal, if any.
    pub on_node: Option<usize>,
}

impl Portal {
    /// Whether anything can pass through: both sides must be non-solid.
    pub fn passable(&self, tree: &Tree) -> bool {
        let a = tree.nodes[self.nodes[0]].contents;
        let b = tree.nodes[self.nodes[1]].contents;
        a & contents::SOLID == 0 && b & contents::SOLID == 0
    }

    /// The other side of the portal from `node`.
    pub fn other(&self, node: usize) -> usize {
        if self.nodes[0] == node { self.nodes[1] } else { self.nodes[0] }
    }
}

/// All the portals in a compile.
#[derive(Default)]
pub struct PortalSet {
    pub portals: Vec<Portal>,
    /// Portals discarded for being slivers.
    pub tiny: usize,
}

/// Slack when splitting a portal, looser than [`ON_EPSILON`].
///
/// Portal windings have been through many clips by the time they are split
/// again, so a tighter epsilon produces hairline fragments that are not real
/// geometry but do cost a portal each.
const SPLIT_EPSILON: f32 = 0.4;

/// Build every portal in the tree.
pub fn build_portals(tree: &mut Tree, planes: &mut PlaneSet) -> PortalSet {
    let mut set = PortalSet::default();
    make_head_portals(tree, planes, &mut set);
    make_tree_portals(tree, planes, &mut set, tree.root);
    set
}

/// Six portals on the world box, each joining the root to the outside node.
fn make_head_portals(tree: &mut Tree, planes: &mut PlaneSet, set: &mut PortalSet) {
    let bounds = tree.nodes[tree.root].bounds;
    let (root, outside) = (tree.root, tree.outside);

    let mut box_planes: Vec<Plane> = Vec::with_capacity(6);
    for axis in 0..3 {
        for far in [false, true] {
            let mut normal = Vec3::ZERO;
            // The interior is in front of every one of these, so a portal's
            // front side is always the world and its back is the outside.
            normal[axis] = if far { -1.0 } else { 1.0 };
            let dist = if far { -bounds.max[axis] } else { bounds.min[axis] };
            box_planes.push(Plane::new(normal, dist));
        }
    }

    let mut windings: Vec<Winding> =
        box_planes.iter().map(Winding::base_for_plane).collect();
    // Cut each face of the box back by the other five.
    for i in 0..6 {
        for j in 0..6 {
            if i == j { continue; }
            match windings[i].clipped(&box_planes[j], ON_EPSILON) {
                Some(w) => windings[i] = w,
                None => { windings[i] = Winding::new(Vec::new()); break; }
            }
        }
    }

    for (plane, winding) in box_planes.into_iter().zip(windings) {
        if winding.is_empty() { continue; }
        let plane_index = planes.insert(plane);
        let portal = Portal { plane: plane_index, winding, nodes: [root, outside], on_node: None };
        add_portal(tree, set, portal);
    }
}

fn add_portal(tree: &mut Tree, set: &mut PortalSet, portal: Portal) -> usize {
    let index = set.portals.len();
    tree.nodes[portal.nodes[0]].portals.push(index);
    tree.nodes[portal.nodes[1]].portals.push(index);
    set.portals.push(portal);
    index
}

fn attach(tree: &mut Tree, set: &mut PortalSet, portal: usize, front: usize, back: usize) {
    set.portals[portal].nodes = [front, back];
    tree.nodes[front].portals.push(portal);
    tree.nodes[back].portals.push(portal);
}

fn detach(tree: &mut Tree, portal: usize, node: usize) {
    tree.nodes[node].portals.retain(|&p| p != portal);
}

fn make_tree_portals(tree: &mut Tree, planes: &mut PlaneSet, set: &mut PortalSet, node: usize) {
    calc_node_bounds(tree, set, node);
    if tree.nodes[node].is_leaf() { return; }
    make_node_portal(tree, planes, set, node);
    split_node_portals(tree, planes, set, node);
    let [front, back] = tree.nodes[node].children;
    make_tree_portals(tree, planes, set, front);
    make_tree_portals(tree, planes, set, back);
}

/// Set a node's bounds from the portals that touch it.
///
/// This is what gives an *empty* leaf a real bounding box: it has no brushes
/// to derive one from, but its portals describe its convex volume exactly.
fn calc_node_bounds(tree: &mut Tree, set: &PortalSet, node: usize) {
    let mut bounds = Aabb::EMPTY;
    for &p in &tree.nodes[node].portals {
        for pt in &set.portals[p].winding.points { bounds.add_point(*pt); }
    }
    if !bounds.is_empty() { tree.nodes[node].bounds = bounds; }
}

/// Create the portal lying on a node's own split plane.
fn make_node_portal(tree: &mut Tree, planes: &mut PlaneSet, set: &mut PortalSet, node: usize) {
    let Some(plane_index) = tree.nodes[node].plane else { return };
    let Some(mut w) = base_winding_for_node(tree, planes, node) else { return };

    // Cut it back by every portal already bounding this node, each oriented so
    // that the node's own side is kept.
    let touching = tree.nodes[node].portals.clone();
    for p in touching {
        let portal = &set.portals[p];
        let plane = planes.get(portal.plane);
        let clip = if portal.nodes[0] == node { plane } else { plane.flipped() };
        match w.clipped(&clip, 0.1) {
            Some(next) => w = next,
            None => return,
        }
    }

    if w.is_tiny() { set.tiny += 1; return; }

    let [front, back] = tree.nodes[node].children;
    let portal = Portal {
        plane: plane_index,
        winding: w,
        nodes: [front, back],
        on_node: Some(node),
    };
    add_portal(tree, set, portal);
}

/// A node's plane, cut down by every ancestor's plane.
fn base_winding_for_node(tree: &Tree, planes: &PlaneSet, node: usize) -> Option<Winding> {
    let plane_index = tree.nodes[node].plane?;
    let mut w = Winding::base_for_plane(&planes.get(plane_index));

    let mut child = node;
    let mut parent = tree.nodes[node].parent;
    while let Some(p) = parent {
        let plane = planes.get(tree.nodes[p].plane?);
        // Keep the side of the ancestor that leads back down to `child`.
        let clip = if tree.nodes[p].children[0] == child { plane } else { plane.flipped() };
        w = w.clipped(&clip, ON_EPSILON)?;
        child = p;
        parent = tree.nodes[p].parent;
    }
    Some(w)
}

/// Hand each of a node's portals down to its children, splitting as needed.
fn split_node_portals(tree: &mut Tree, planes: &PlaneSet, set: &mut PortalSet, node: usize) {
    let Some(plane_index) = tree.nodes[node].plane else { return };
    let plane = planes.get(plane_index);
    let [front_child, back_child] = tree.nodes[node].children;

    let touching = tree.nodes[node].portals.clone();
    for p in touching {
        // The portal created on this very node already joins the children.
        if set.portals[p].on_node == Some(node) { continue; }

        let node_is_front = set.portals[p].nodes[0] == node;
        let other = set.portals[p].other(node);

        detach(tree, p, set.portals[p].nodes[0]);
        detach(tree, p, set.portals[p].nodes[1]);

        let (f, b) = set.portals[p].winding.split(&plane, SPLIT_EPSILON);
        let f = f.filter(|w| !w.is_tiny());
        let b = b.filter(|w| !w.is_tiny());

        match (f, b) {
            (None, None) => { set.tiny += 1; }
            (Some(w), None) => {
                set.portals[p].winding = w;
                if node_is_front { attach(tree, set, p, front_child, other); }
                else { attach(tree, set, p, other, front_child); }
            }
            (None, Some(w)) => {
                set.portals[p].winding = w;
                if node_is_front { attach(tree, set, p, back_child, other); }
                else { attach(tree, set, p, other, back_child); }
            }
            (Some(fw), Some(bw)) => {
                // Straddles the plane: one portal becomes two.
                let mut clone = set.portals[p].clone();
                clone.winding = bw;
                set.portals[p].winding = fw;
                let new_index = set.portals.len();
                set.portals.push(clone);
                if node_is_front {
                    attach(tree, set, p, front_child, other);
                    attach(tree, set, new_index, back_child, other);
                } else {
                    attach(tree, set, p, other, front_child);
                    attach(tree, set, new_index, other, back_child);
                }
            }
        }
    }
}

// ---- flood filling -------------------------------------------------------

/// What the flood fill found.
pub struct FloodResult {
    /// Leaves the flood reached.
    pub occupied_leaves: usize,
    /// Entity origins that were inside solid geometry.
    pub entities_in_solid: Vec<Vec3>,
    /// If the world is not sealed, a path from an entity to the outside.
    pub leak: Option<LeakPath>,
}

/// A trace from an entity out through the hole in the map.
pub struct LeakPath {
    /// The entity that leaked.
    pub from: Vec3,
    /// Portal centres from the entity out to the void.
    pub points: Vec<Vec3>,
}

impl LeakPath {
    /// The `.keroleak` point-file format Chisel loads to draw the leak.
    ///
    /// One `x y z` per line. Deliberately trivial: a leak file is read by a
    /// person as often as by a program, and being able to eyeball the
    /// coordinates has saved more time than any structure would.
    pub fn to_lin(&self) -> String {
        let mut out = String::new();
        for p in &self.points {
            let _ = writeln!(out, "{} {} {}", p.x, p.y, p.z);
        }
        out
    }
}

/// Flood outward from every entity, marking reachable leaves.
///
/// Any leaf not reached is outside the sealed world. If the flood escapes to
/// the outside node, the map leaks -- and the returned path shows the route
/// out, which is the only practical way to find a one-unit gap in a large map.
pub fn flood_entities(
    tree: &mut Tree,
    set: &PortalSet,
    planes: &PlaneSet,
    entity_origins: &[Vec3],
) -> FloodResult {
    let mut result = FloodResult {
        occupied_leaves: 0,
        entities_in_solid: Vec::new(),
        leak: None,
    };

    for &origin in entity_origins {
        let leaf = tree.point_leaf(origin, planes);
        if tree.nodes[leaf].contents & contents::SOLID != 0 {
            result.entities_in_solid.push(origin);
            continue;
        }
        if tree.nodes[leaf].occupied != 0 { continue; }

        // Breadth-first so the recorded route out is the shortest one, and so
        // depth cannot blow the stack on a large open map.
        let mut came_from: Vec<Option<(usize, usize)>> = vec![None; tree.nodes.len()];
        let mut queue = VecDeque::new();
        tree.nodes[leaf].occupied = 1;
        queue.push_back(leaf);

        let mut escaped = false;
        while let Some(node) = queue.pop_front() {
            let dist = tree.nodes[node].occupied;
            for &p in &tree.nodes[node].portals.clone() {
                let portal = &set.portals[p];
                if !portal.passable(tree) { continue; }
                let other = portal.other(node);
                if tree.nodes[other].occupied != 0 { continue; }
                tree.nodes[other].occupied = dist + 1;
                came_from[other] = Some((node, p));
                if other == tree.outside { escaped = true; }
                queue.push_back(other);
            }
        }

        if escaped && result.leak.is_none() {
            result.leak = Some(build_leak_path(tree, set, &came_from, origin));
        }
    }

    result.occupied_leaves = tree.leaves().filter(|&l| tree.nodes[l].occupied != 0).count();
    result
}

fn build_leak_path(
    tree: &Tree,
    set: &PortalSet,
    came_from: &[Option<(usize, usize)>],
    origin: Vec3,
) -> LeakPath {
    let mut points = Vec::new();
    let mut at = tree.outside;
    // Walk the breadcrumb trail back to the entity, collecting the portal
    // each hop went through.
    while let Some((prev, portal)) = came_from[at] {
        points.push(set.portals[portal].winding.center());
        at = prev;
        if points.len() > tree.nodes.len() { break; }
    }
    points.push(origin);
    points.reverse();
    LeakPath { from: origin, points }
}

/// Turn every leaf the flood never reached into solid rock.
///
/// This is what "sealing" a map buys: the entire outer surface of the level --
/// the backs of every wall, the underside of the floor -- stops existing.
/// Returns how many leaves were filled.
pub fn fill_outside(tree: &mut Tree) -> usize {
    let mut filled = 0;
    for leaf in tree.leaves().collect::<Vec<_>>() {
        if tree.nodes[leaf].occupied == 0 && tree.nodes[leaf].contents & contents::SOLID == 0 {
            tree.nodes[leaf].contents |= contents::SOLID;
            filled += 1;
        }
    }
    filled
}

/// Number every non-solid leaf with a visibility cluster.
///
/// Returns how many clusters exist. Solid leaves keep `-1`: nothing can see
/// out of solid rock, so they need no row in the PVS.
pub fn assign_clusters(tree: &mut Tree) -> usize {
    let mut next = 0i16;
    for leaf in tree.leaves().collect::<Vec<_>>() {
        if tree.nodes[leaf].contents & contents::SOLID != 0 {
            tree.nodes[leaf].cluster = -1;
        } else {
            tree.nodes[leaf].cluster = next;
            next += 1;
        }
    }
    next as usize
}

/// Write the portal graph for Umbra.
///
/// The shape follows Quake's portal file -- readable, diffable, and exactly
/// what a separate visibility tool needs -- but the header is `VPRT1`, not
/// `PRT1`: this is not that format and must not be handed to a tool expecting
/// one.
///
/// ```text
/// VPRT1
/// <cluster count>
/// <portal count>
/// <points> <cluster a> <cluster b> (x y z) (x y z) ...
/// ```
///
/// Only portals between two non-solid leaves appear: sight does not travel
/// through rock, so a portal with a solid side is not a portal at all.
pub fn write_prt(tree: &Tree, set: &PortalSet, clusters: usize) -> String {
    let mut body = String::new();
    let mut count = 0usize;

    for portal in &set.portals {
        let (a, b) = (portal.nodes[0], portal.nodes[1]);
        if a == tree.outside || b == tree.outside { continue; }
        if !tree.nodes[a].is_leaf() || !tree.nodes[b].is_leaf() { continue; }
        let (ca, cb) = (tree.nodes[a].cluster, tree.nodes[b].cluster);
        if ca < 0 || cb < 0 { continue; }
        if portal.winding.is_empty() { continue; }

        let _ = write!(body, "{} {} {}", portal.winding.len(), ca, cb);
        for p in &portal.winding.points {
            let _ = write!(body, " ({:.6} {:.6} {:.6})", p.x, p.y, p.z);
        }
        body.push('\n');
        count += 1;
    }

    format!("VPRT1\n{clusters}\n{count}\n{body}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::{BrushWork, Warning};
    use kerosene_map::Solid;

    fn build(boxes: &[Aabb]) -> (Tree, PortalSet, PlaneSet) {
        let mut planes = PlaneSet::new();
        let mut warnings: Vec<Warning> = Vec::new();
        let mut brushes = Vec::new();
        for (i, b) in boxes.iter().enumerate() {
            let mut solid = Solid::cube(*b, "dev/grid");
            solid.id = i as u32 + 1;
            brushes.push(
                BrushWork::from_solid(&solid, 0, "worldspawn", &mut planes, &mut warnings).unwrap(),
            );
        }
        let mut tree = Tree::build(brushes, &planes);
        let set = build_portals(&mut tree, &mut planes);
        (tree, set, planes)
    }

    /// Six slabs enclosing an empty 256-cube, with `gap` left out of one wall.
    fn room_boxes(gap: bool) -> Vec<Aabb> {
        let t = 16.0;
        let (lo, hi) = (0.0f32, 256.0);
        let x_wall_hi = if gap { hi - 32.0 } else { hi + t };
        vec![
            Aabb::new(Vec3::new(lo - t, lo - t, lo - t), Vec3::new(hi + t, hi + t, lo)),
            Aabb::new(Vec3::new(lo - t, lo - t, hi), Vec3::new(hi + t, hi + t, hi + t)),
            Aabb::new(Vec3::new(lo - t, lo - t, lo), Vec3::new(lo, hi + t, hi)),
            Aabb::new(Vec3::new(hi, lo - t, lo), Vec3::new(hi + t, hi + t, hi)),
            // Front wall: shortened when `gap` is set, leaving a hole.
            Aabb::new(Vec3::new(lo, lo - t, lo), Vec3::new(x_wall_hi, lo, hi)),
            Aabb::new(Vec3::new(lo, hi, lo), Vec3::new(hi, hi + t, hi)),
        ]
    }

    #[test]
    fn portals_are_created_and_join_real_leaves() {
        let (tree, set, _) = build(&[Aabb::new(Vec3::ZERO, Vec3::splat(64.0))]);
        assert!(!set.portals.is_empty());
        for p in &set.portals {
            assert!(p.nodes[0] < tree.nodes.len() && p.nodes[1] < tree.nodes.len());
            assert_ne!(p.nodes[0], p.nodes[1], "a portal must separate two different nodes");
            assert!(!p.winding.is_empty());
        }
    }

    #[test]
    fn every_portal_is_listed_by_both_its_leaves() {
        let (tree, set, _) = build(&room_boxes(false));
        for (i, p) in set.portals.iter().enumerate() {
            assert!(tree.nodes[p.nodes[0]].portals.contains(&i), "portal {i} missing from front node");
            assert!(tree.nodes[p.nodes[1]].portals.contains(&i), "portal {i} missing from back node");
        }
    }

    #[test]
    fn a_sealed_room_does_not_leak() {
        let (mut tree, set, planes) = build(&room_boxes(false));
        let inside = vec![Vec3::splat(128.0)];
        let result = flood_entities(&mut tree, &set, &planes, &inside);
        assert!(result.leak.is_none(), "a sealed room must not leak");
        assert!(result.entities_in_solid.is_empty());
        assert!(result.occupied_leaves > 0);
    }

    #[test]
    fn a_hole_in_the_wall_leaks_and_reports_a_route() {
        let (mut tree, set, planes) = build(&room_boxes(true));
        let inside = vec![Vec3::splat(128.0)];
        let result = flood_entities(&mut tree, &set, &planes, &inside);
        let leak = result.leak.expect("an open wall must be reported as a leak");
        assert_eq!(leak.from, Vec3::splat(128.0));
        assert!(leak.points.len() >= 2, "the route needs at least a start and an exit");
        assert_eq!(leak.points[0], Vec3::splat(128.0), "the route starts at the entity");
        let lin = leak.to_lin();
        assert_eq!(lin.lines().count(), leak.points.len());
    }

    #[test]
    fn an_entity_buried_in_a_wall_is_reported() {
        let (mut tree, set, planes) = build(&room_boxes(false));
        let buried = vec![Vec3::new(128.0, -8.0, 128.0)]; // inside the -Y slab
        let result = flood_entities(&mut tree, &set, &planes, &buried);
        assert_eq!(result.entities_in_solid.len(), 1);
    }

    #[test]
    fn filling_outside_seals_a_good_map_and_keeps_the_room() {
        let (mut tree, set, planes) = build(&room_boxes(false));
        flood_entities(&mut tree, &set, &planes, &[Vec3::splat(128.0)]);
        let filled = fill_outside(&mut tree);
        assert!(filled > 0, "the space outside the room should have been filled");

        // The room's air survives; the space beyond the walls does not.
        let air = tree.point_leaf(Vec3::splat(128.0), &planes);
        assert_eq!(tree.nodes[air].contents & contents::SOLID, 0);
        let beyond = tree.point_leaf(Vec3::new(128.0, -200.0, 128.0), &planes);
        assert!(tree.nodes[beyond].contents & contents::SOLID != 0);
    }

    #[test]
    fn clusters_are_assigned_only_to_open_leaves() {
        let (mut tree, set, planes) = build(&room_boxes(false));
        flood_entities(&mut tree, &set, &planes, &[Vec3::splat(128.0)]);
        fill_outside(&mut tree);
        let n = assign_clusters(&mut tree);
        assert!(n > 0);
        for leaf in tree.leaves().collect::<Vec<_>>() {
            let node = &tree.nodes[leaf];
            if node.contents & contents::SOLID != 0 {
                assert_eq!(node.cluster, -1, "solid leaves get no cluster");
            } else {
                assert!(node.cluster >= 0 && (node.cluster as usize) < n);
            }
        }
    }

    #[test]
    fn the_prt_file_is_well_formed() {
        let (mut tree, set, planes) = build(&room_boxes(false));
        flood_entities(&mut tree, &set, &planes, &[Vec3::splat(128.0)]);
        fill_outside(&mut tree);
        let clusters = assign_clusters(&mut tree);
        let prt = write_prt(&tree, &set, clusters);

        let mut lines = prt.lines();
        assert_eq!(lines.next(), Some("VPRT1"));
        assert_eq!(lines.next().unwrap().parse::<usize>().unwrap(), clusters);
        let count: usize = lines.next().unwrap().parse().unwrap();
        assert_eq!(lines.clone().count(), count, "header count must match the body");
        for line in lines {
            let n: usize = line.split_whitespace().next().unwrap().parse().unwrap();
            assert!(n >= 3, "a portal needs at least three points");
            assert_eq!(line.matches('(').count(), n);
        }
    }

    #[test]
    fn portals_touching_solid_do_not_reach_the_prt() {
        let (mut tree, set, planes) = build(&room_boxes(false));
        flood_entities(&mut tree, &set, &planes, &[Vec3::splat(128.0)]);
        fill_outside(&mut tree);
        let clusters = assign_clusters(&mut tree);
        let prt = write_prt(&tree, &set, clusters);
        for line in prt.lines().skip(3) {
            let mut f = line.split_whitespace();
            f.next();
            let a: i32 = f.next().unwrap().parse().unwrap();
            let b: i32 = f.next().unwrap().parse().unwrap();
            assert!(a >= 0 && b >= 0, "a prt portal must join two real clusters");
        }
    }
}
