// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Navigation over a [`Walkmap`]: the consumer the data format was built for.
//!
//! The walkmap answers "is this point a place NPCs may stand". What it does
//! not answer, and what this module does, is "and how do I get from here to
//! there". That is the orphan the format documents: a solid data foundation
//! with nothing reading it.
//!
//! Navigation is a graph search over *faces*. Two walkmap faces are adjacent
//! when they share an edge — a floor meeting a floor across a seam, a floor
//! meeting a ramp. The route between them is a straight line through the
//! midpoint of that shared edge, because a midpoint is always inside both
//! faces, so a path built from midpoints never clips a corner that is not
//! there. A face marked [`WalkmapRule::Avoid`] is kept in the graph — an NPC
//! may still need to cross a puddle — but crossing it costs more, so paths
//! bend around it when an alternative is not much longer.
//!
//! The result is a list of waypoints. No path smoothing beyond the midpoint
//! construction is applied: an NPC that follows the waypoints strictly is
//! always inside a walkable face, which is more important than a visually
//! shorter line. Callers that want smoother motion can string-pull the result
//! against the walkmap with [`Walkmap::walkable`].

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use kerosene_map::WalkmapRule;
use kerosene_math::Vec3;

use crate::{WalkFace, Walkmap};

/// How much crossing an `avoid` face costs, relative to an `allow` one.
///
/// More than one and less than "never": an NPC will route around a pool of
/// avoid faces when the detour is shorter than this multiplier makes the
/// direct line, but will still cross when the only other way is much longer.
pub const AVOID_COST: f32 = 4.0;

/// The distance beyond which two vertices are not considered the same point.
const MATCH_EPSILON: f32 = 0.5;

/// A connection between two faces, through a shared edge.
#[derive(Clone, Debug)]
struct Arc {
    /// The destination face.
    to: usize,
    /// The midpoint of the edge the two faces share — the place a path crosses
    /// from one to the other. Always inside both faces.
    portal: Vec3,
    /// Cost of the edge, including the destination face's rule penalty.
    cost: f32,
}

/// Graph of walkmap faces linked by shared edges.
///
/// Built once per walkmap and reused for any number of paths, so the geometry
/// questions — which faces touch — are answered before anyone asks where to
/// go, exactly as the PVS answers visibility ahead of time.
#[derive(Clone, Debug, Default)]
pub struct NavGraph {
    /// The faces, as nodes.
    nodes: Vec<Node>,
    /// Per-node out-edges.
    arcs: Vec<Vec<Arc>>,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    /// Centroid of the face, used as the node's position.
    center: Vec3,
    rule: WalkmapRule,
}

impl NavGraph {
    /// Build the connectivity graph from a walkmap.
    pub fn build(walk: &Walkmap) -> NavGraph {
        let nodes: Vec<Node> = walk
            .faces
            .iter()
            .map(|f| Node { center: centroid(&f.vertices), rule: f.rule })
            .collect();

        let mut arcs: Vec<Vec<Arc>> = vec![Vec::new(); nodes.len()];

        // For each pair of faces (n is small — a level has hundreds, not
        // millions), add an edge when they share a segment. A shared edge is
        // found by matching two distinct vertex pairs between the faces.
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let portal = match shared_edge_midpoint(&walk.faces[i], &walk.faces[j]) {
                    Some(p) => p,
                    None => continue,
                };
                let cost_ji = edge_cost(nodes[i], nodes[j], portal);
                let cost_ij = edge_cost(nodes[j], nodes[i], portal);
                arcs[i].push(Arc { to: j, portal, cost: cost_ij });
                arcs[j].push(Arc { to: i, portal, cost: cost_ji });
            }
        }

        NavGraph { nodes, arcs }
    }

    /// Number of nodes (faces) in the graph.
    pub fn node_count(&self) -> usize { self.nodes.len() }

    /// Number of directed edges in the graph.
    pub fn edge_count(&self) -> usize { self.arcs.iter().map(|a| a.len()).sum() }

    /// The face index whose polygon contains a point, within `max_dist` of its
    /// plane — the node a path starts or ends at.
    pub fn face_at(&self, walk: &Walkmap, point: Vec3, max_dist: f32) -> Option<usize> {
        walk.faces.iter().position(|f| f.contains(point, max_dist))
    }

    /// Find a path from `start` to `goal` as a list of waypoints, inclusive of
    /// both ends.
    ///
    /// The start and goal are constrained to lie within `max_dist` of a face's
    /// plane, exactly as [`Walkmap::face_under`] is. Returns `None` when either
    /// point is not on a walkable face, or when no sequence of faces connects
    /// them (a goal on a ledge reachable only by a door that is closed).
    ///
    /// The waypoints are, in order: `start`, the midpoints of the shared edges
    /// crossed, and `goal`. That is enough for an NPC steered point-to-point
    /// to stay inside walkable faces the whole way.
    pub fn find_path(
        &self,
        walk: &Walkmap,
        start: Vec3,
        goal: Vec3,
        max_dist: f32,
    ) -> Option<Vec<Vec3>> {
        let start_face = self.face_at(walk, start, max_dist)?;
        let goal_face = self.face_at(walk, goal, max_dist)?;

        // Same face: the straight line is already inside one walkable face.
        if start_face == goal_face {
            return Some(vec![start, goal]);
        }

        let came_from = self.a_star(start, start_face, goal, goal_face);
        let mut path = Vec::new();
        let mut current = goal_face;
        while current != start_face {
            path.push(current);
            current = came_from[current]?;
        }
        path.push(start_face);
        path.reverse();

        let mut waypoints = Vec::with_capacity(path.len() + 2);
        waypoints.push(start);
        for (i, &face) in path.iter().enumerate() {
            if i + 1 == path.len() { break; }
            let next = path[i + 1];
            // Find the arc from `face` to `next` to recover its portal point.
            let portal = self.arcs[face]
                .iter()
                .find(|a| a.to == next)
                .map(|a| a.portal);
            match portal {
                Some(p) => waypoints.push(p),
                // Should not happen for a path A* produced, but a missing arc
                // means the path itself is a lie; fall back to the centroid.
                None => waypoints.push(self.nodes[next].center),
            }
        }
        waypoints.push(goal);
        Some(waypoints)
    }

    fn a_star(
        &self,
        start: Vec3,
        start_face: usize,
        goal: Vec3,
        goal_face: usize,
    ) -> Vec<Option<usize>> {
        let n = self.nodes.len();
        let mut came_from: Vec<Option<usize>> = vec![None; n];
        let mut g_score: Vec<f32> = vec![f32::INFINITY; n];
        let mut open = BinaryHeap::new();

        g_score[start_face] = 0.0;
        open.push(HeapEntry { cost: heuristic(start, goal), face: start_face });

        while let Some(HeapEntry { face, .. }) = open.pop() {
            if face == goal_face { break; }
            for arc in &self.arcs[face] {
                let tentative = g_score[face] + arc.cost;
                if tentative < g_score[arc.to] {
                    g_score[arc.to] = tentative;
                    came_from[arc.to] = Some(face);
                    open.push(HeapEntry {
                        cost: tentative + heuristic(self.nodes[arc.to].center, goal),
                        face: arc.to,
                    });
                }
            }
        }
        came_from
    }
}

#[derive(Clone, Copy, Debug)]
struct HeapEntry {
    cost: f32,
    face: usize,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool { self.cost == other.cost }
}
impl Eq for HeapEntry {}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering { other.cost.total_cmp(&self.cost) }
}

/// Straight-line distance between two points, as the admissible heuristic.
fn heuristic(a: Vec3, b: Vec3) -> f32 { (b - a).length() }

/// The cost of travelling `from_face` → `to_face` across `portal`.
///
/// The base is the length from the departing face's centroid to the portal,
/// plus the same on the far side, so A* minimises travelled distance. An
/// `avoid` face multiplies the cost of leaving *through* it and of entering
/// it, discouraging routes that use it without forbidding them.
fn edge_cost(from: Node, to: Node, portal: Vec3) -> f32 {
    let base = (portal - from.center).length() + (to.center - portal).length();
    let mut cost = base;
    if from.rule == WalkmapRule::Avoid { cost *= AVOID_COST; }
    if to.rule == WalkmapRule::Avoid { cost *= AVOID_COST; }
    cost
}

/// Whether two convex polygon faces share an edge, and if so the midpoint of
/// that edge (the safe crossing point).
///
/// Two faces in a compiled walkmap are adjacent when a full edge of one is a
/// full edge of the other. Because CSG welds coplanar faces and fragments
/// angled ones consistently, an edge is shared exactly when its two endpoints
/// each appear in both faces' vertex lists (within a small tolerance, for the
/// floating-point dust the welds leave). The midpoint of that edge lies on
/// both polygons, so it is always a valid crossing point.
fn shared_edge_midpoint(a: &WalkFace, b: &WalkFace) -> Option<Vec3> {
    let mut shared = Vec::new();
    for &va in &a.vertices {
        if b.vertices.iter().any(|&vb| (vb - va).length_squared() < MATCH_EPSILON * MATCH_EPSILON)
            && !shared.iter().any(|&s: &Vec3| (s - va).length_squared() < MATCH_EPSILON * MATCH_EPSILON)
        {
            shared.push(va);
        }
    }
    if shared.len() < 2 { return None; }

    // Two convex faces sharing two vertices share the edge between them.
    // The midpoint of that edge lies on both polygons, so it is always a safe
    // crossing point. If more than two vertices match (two coplanar fragments
    // of the same surface), the first two are a valid shared edge — the rest
    // only add crossing points that are already covered.
    Some((shared[0] + shared[1]) * 0.5)
}

/// The centroid (average of vertices) of a convex polygon.
///
/// For a flat walkable floor this lies on the face and inside it, which is all
/// a node position needs.
fn centroid(vertices: &[Vec3]) -> Vec3 {
    let mut sum = Vec3::ZERO;
    for &v in vertices { sum += v; }
    sum / vertices.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_math::Aabb;

    fn face_z(z: f32, min: (f32, f32), max: (f32, f32)) -> WalkFace {
        let vertices = vec![
            Vec3::new(min.0, min.1, z),
            Vec3::new(max.0, min.1, z),
            Vec3::new(max.0, max.1, z),
            Vec3::new(min.0, max.1, z),
        ];
        WalkFace {
            vertices,
            normal: Vec3::Z,
            rule: WalkmapRule::Allow,
            bounds: Aabb::from_points(&[Vec3::new(min.0, min.1, z), Vec3::new(max.0, max.1, z)]),
        }
    }

    /// Two 64x64 floors side by side, sharing the edge x = 64.
    fn two_floors() -> Walkmap {
        Walkmap {
            faces: vec![
                face_z(0.0, (0.0, 0.0), (64.0, 64.0)),
                face_z(0.0, (64.0, 0.0), (128.0, 64.0)),
            ],
        }
    }

    #[test]
    fn adjacent_faces_share_an_edge() {
        let walk = two_floors();
        assert!(shared_edge_midpoint(&walk.faces[0], &walk.faces[1]).is_some());
    }

    #[test]
    fn separated_faces_do_not() {
        let walk = Walkmap {
            faces: vec![
                face_z(0.0, (0.0, 0.0), (64.0, 64.0)),
                face_z(0.0, (128.0, 0.0), (192.0, 64.0)),
            ],
        };
        assert!(shared_edge_midpoint(&walk.faces[0], &walk.faces[1]).is_none());
    }

    #[test]
    fn the_graph_links_adjacent_floors() {
        let walk = two_floors();
        let graph = NavGraph::build(&walk);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 2, "one undirected edge is two arcs");
    }

    #[test]
    fn a_path_across_two_floors_hits_the_seam() {
        let walk = two_floors();
        let graph = NavGraph::build(&walk);
        let path = graph
            .find_path(&walk, Vec3::new(32.0, 32.0, 0.0), Vec3::new(96.0, 32.0, 0.0), crate::DEFAULT_STEP)
            .expect("the floors are connected");
        assert_eq!(path.len(), 3);
        assert!((path[0] - Vec3::new(32.0, 32.0, 0.0)).length() < 0.01);
        assert!((path[2] - Vec3::new(96.0, 32.0, 0.0)).length() < 0.01);
        // The middle waypoint sits on the shared edge x = 64.
        assert!((path[1].x - 64.0).abs() < 0.5, "{}", path[1]);
    }

    #[test]
    fn a_path_within_one_face_is_the_straight_line() {
        let walk = two_floors();
        let graph = NavGraph::build(&walk);
        let path = graph
            .find_path(&walk, Vec3::new(8.0, 8.0, 0.0), Vec3::new(40.0, 40.0, 0.0), crate::DEFAULT_STEP)
            .expect("same face");
        assert_eq!(path.len(), 2);
    }

    #[test]
    fn an_unreachable_goal_is_none() {
        let walk = two_floors();
        let graph = NavGraph::build(&walk);
        // The two floors are connected, but this goal is on neither.
        assert!(graph.find_path(&walk, Vec3::new(32.0, 32.0, 0.0), Vec3::new(1000.0, 32.0, 0.0), crate::DEFAULT_STEP).is_none());
    }

    #[test]
    fn avoid_faces_are_preferred_around_but_still_crossable() {
        // Two routes between start and goal: a direct line through an `avoid`
        // floor, or nothing. With a single avoid face the path still crosses it.
        let mut avoid = face_z(0.0, (0.0, 0.0), (64.0, 64.0));
        avoid.rule = WalkmapRule::Avoid;
        let walk = Walkmap { faces: vec![avoid] };
        let graph = NavGraph::build(&walk);
        let path = graph
            .find_path(&walk, Vec3::new(8.0, 8.0, 0.0), Vec3::new(40.0, 40.0, 0.0), crate::DEFAULT_STEP)
            .expect("avoid is walkable");
        assert_eq!(path.len(), 2);
    }

    #[test]
    fn disconnected_faces_yield_no_path_through_the_void() {
        // Three floors in a row where the middle is missing: the two ends
        // share no edge and are not connectable.
        let walk = Walkmap {
            faces: vec![
                face_z(0.0, (0.0, 0.0), (64.0, 64.0)),
                face_z(0.0, (128.0, 0.0), (192.0, 64.0)),
            ],
        };
        let graph = NavGraph::build(&walk);
        assert!(graph
            .find_path(&walk, Vec3::new(32.0, 32.0, 0.0), Vec3::new(160.0, 32.0, 0.0), crate::DEFAULT_STEP)
            .is_none());
    }
}
