// SPDX-License-Identifier: MPL-2.0
//! Computing the Potentially Visible Set.
//!
//! The question is: standing anywhere in cluster A, can you see any part of
//! cluster B? Answering it exactly is expensive, so it is answered in two
//! passes that tighten on each other.
//!
//! **Base vis** is cheap and generous. Two portals might see each other if
//! each has some point on the visible side of the other. Flooding that
//! relation transitively gives an over-estimate: never hides something you
//! could actually see, but lets far too much through.
//!
//! **Portal flow** is the real answer. Sight from the base portal, through a
//! chain of portals, is a shrinking sight-cone. At each step the cone is
//! narrowed by *separating planes* -- the planes touching one edge of the
//! source and one vertex of the portal being passed through, which bound
//! exactly what the source can see through it. When the cone closes to
//! nothing, everything beyond is invisible and the recursion stops.
//!
//! This is Quake's algorithm, and it is why a Source map can hold a whole
//! building and still only draw the room you are in.

use crate::bitset::BitSet;
use crate::prt::PortalGraph;
use kerosene_math::{ON_EPSILON, Plane, Vec3, Winding};

/// Recursion cap. A degenerate portal graph could otherwise chain forever;
/// stopping early is conservative (it leaves extra clusters visible) rather
/// than wrong.
const MAX_DEPTH: usize = 128;

pub struct VisResult {
    /// Which clusters each cluster can see.
    pub cluster_vis: Vec<BitSet>,
    /// Total cluster-pairs visible after base vis, for reporting.
    pub base_visible: usize,
    /// Total after the full flow.
    pub final_visible: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status { Pending, Done }

/// Compute visibility. With `fast`, stops after base vis -- a much quicker
/// compile that leaves too much visible, for iterating on a level's layout.
pub fn compute(graph: &PortalGraph, fast: bool) -> VisResult {
    let n = graph.portal_count();

    let portal_front = base_portal_vis(graph);
    let portal_flood = flood(graph, &portal_front);

    let base_visible: usize = merge_clusters(graph, &portal_flood)
        .iter()
        .map(BitSet::count)
        .sum();

    let portal_vis = if fast {
        portal_flood.clone()
    } else {
        let mut vis: Vec<BitSet> = vec![BitSet::new(n); n];
        let mut status = vec![Status::Pending; n];
        // Sequential rather than threaded: each portal's result tightens the
        // pruning available to every later one, and reading a finished
        // neighbour's answer is what makes the flow converge quickly.
        for i in 0..n {
            let result = {
                let mut flow = Flow {
                    graph,
                    portal_flood: &portal_flood,
                    portal_vis: &vis,
                    status: &status,
                    base: i,
                    base_plane: graph.portals[i].plane,
                    base_center: graph.portals[i].center,
                    base_radius: graph.portals[i].radius,
                    vis: BitSet::new(n),
                };
                flow.run();
                flow.vis
            };
            vis[i] = result;
            status[i] = Status::Done;
        }
        vis
    };

    let cluster_vis = merge_clusters(graph, &portal_vis);
    let final_visible = cluster_vis.iter().map(BitSet::count).sum();

    VisResult { cluster_vis, base_visible, final_visible }
}

/// Which portals each portal could conceivably see.
///
/// Two portals can only see each other if each pokes out on the far side of
/// the other: a portal facing away is looking at the back of a wall.
fn base_portal_vis(graph: &PortalGraph) -> Vec<BitSet> {
    let n = graph.portal_count();
    let mut out = vec![BitSet::new(n); n];

    for i in 0..n {
        let p = &graph.portals[i];
        for j in 0..n {
            if i == j { continue; }
            let q = &graph.portals[j];

            // q must have at least one point in front of p, or p cannot see
            // any of it.
            if !q.winding.points.iter().any(|&pt| p.plane.distance_to(pt) > ON_EPSILON) {
                continue;
            }
            // And p must have at least one point behind q, or q is looking
            // away from p entirely.
            if !p.winding.points.iter().any(|&pt| q.plane.distance_to(pt) < -ON_EPSILON) {
                continue;
            }
            out[i].set(j);
        }
    }
    out
}

/// Transitive closure of the base relation: everything reachable by chaining
/// portals that might see each other.
fn flood(graph: &PortalGraph, front: &[BitSet]) -> Vec<BitSet> {
    let n = graph.portal_count();
    let mut out = vec![BitSet::new(n); n];

    for base in 0..n {
        // Iterative rather than recursive: a large open map floods very wide.
        let mut stack = vec![graph.portals[base].into_cluster];
        while let Some(cluster) = stack.pop() {
            for &pnum in &graph.by_cluster[cluster] {
                if !front[base].test(pnum) { continue; }
                if out[base].test(pnum) { continue; }
                out[base].set(pnum);
                stack.push(graph.portals[pnum].into_cluster);
            }
        }
    }
    out
}

/// Turn per-portal visibility into per-cluster visibility.
fn merge_clusters(graph: &PortalGraph, portal_vis: &[BitSet]) -> Vec<BitSet> {
    let mut out = vec![BitSet::new(graph.clusters); graph.clusters];
    for c in 0..graph.clusters {
        // A cluster always sees itself.
        out[c].set(c);
        for &pnum in &graph.by_cluster[c] {
            // And it always sees whatever is directly through its own
            // portals. The flow starts *beyond* each portal, so without this
            // the immediate neighbour never appears and adjacent rooms would
            // cull each other away.
            out[c].set(graph.portals[pnum].into_cluster);
            for seen in portal_vis[pnum].iter_set() {
                out[c].set(graph.portals[seen].into_cluster);
            }
        }
    }
    out
}

/// One level of the sight-cone recursion.
struct Stack {
    mightsee: BitSet,
    /// The base portal's winding, clipped down by everything passed so far.
    source: Winding,
    /// The last portal passed through, clipped. `None` at the first step.
    pass: Option<Winding>,
}

struct Flow<'a> {
    graph: &'a PortalGraph,
    portal_flood: &'a [BitSet],
    portal_vis: &'a [BitSet],
    status: &'a [Status],
    base: usize,
    base_plane: Plane,
    base_center: Vec3,
    base_radius: f32,
    vis: BitSet,
}

impl Flow<'_> {
    fn run(&mut self) {
        let head = Stack {
            mightsee: self.portal_flood[self.base].clone(),
            source: self.graph.portals[self.base].winding.clone(),
            pass: None,
        };
        let start = self.graph.portals[self.base].into_cluster;
        self.recurse(start, &head, 0);
    }

    fn recurse(&mut self, cluster: usize, prev: &Stack, depth: usize) {
        if depth >= MAX_DEPTH { return; }

        for idx in 0..self.graph.by_cluster[cluster].len() {
            let pnum = self.graph.by_cluster[cluster][idx];
            if !prev.mightsee.test(pnum) { continue; }

            // A portal already solved gives its exact answer; one still
            // pending gives its over-estimate. Either way it prunes.
            let test = if self.status[pnum] == Status::Done {
                &self.portal_vis[pnum]
            } else {
                &self.portal_flood[pnum]
            };

            let mut mightsee = BitSet::new(self.vis.len());
            let more = mightsee.intersect_into(&prev.mightsee, test, &self.vis);
            if !more && self.vis.test(pnum) {
                // Nothing new lies beyond, and we have already recorded this
                // portal. Going further cannot change the answer.
                continue;
            }

            let p = &self.graph.portals[pnum];

            // Clip the portal we are stepping through by the base portal's
            // plane: we cannot see anything behind ourselves.
            let d = self.base_plane.distance_to(p.center);
            let pass = if d < -p.radius {
                continue;
            } else if d > p.radius {
                p.winding.clone()
            } else {
                match p.winding.clipped(&self.base_plane, ON_EPSILON) {
                    Some(w) => w,
                    None => continue,
                }
            };

            // Clip the source by the back of this portal's plane: only the
            // part of the base portal behind it can see through it.
            let d = p.plane.distance_to(self.base_center);
            let source = if d > self.base_radius {
                continue;
            } else if d < -self.base_radius {
                prev.source.clone()
            } else {
                match prev.source.clipped(&p.plane.flipped(), ON_EPSILON) {
                    Some(w) => w,
                    None => continue,
                }
            };

            let Some(prev_pass) = &prev.pass else {
                // First step out of the base portal: there is no earlier
                // portal to form separating planes with, so nothing can be
                // clipped away yet.
                self.vis.set(pnum);
                let stack = Stack { mightsee, source, pass: Some(pass) };
                self.recurse(p.into_cluster, &stack, depth + 1);
                continue;
            };

            // Narrow the sight cone from both directions.
            let Some(pass) = clip_to_separators(&source, prev_pass, pass, false) else { continue };
            let Some(pass) = clip_to_separators(prev_pass, &source, pass, true) else { continue };

            self.vis.set(pnum);
            let stack = Stack { mightsee, source, pass: Some(pass) };
            self.recurse(p.into_cluster, &stack, depth + 1);
        }
    }
}

/// Clip `target` by every separating plane between `source` and `pass`.
///
/// A separating plane touches one edge of `source` and one vertex of `pass`,
/// with all of `source` on one side and all of `pass` on the other. Such a
/// plane bounds what `source` can possibly see through `pass`, so anything of
/// `target` beyond it is hidden. Returns `None` when nothing survives, which
/// means the sight line is fully blocked.
fn clip_to_separators(
    source: &Winding,
    pass: &Winding,
    mut target: Winding,
    flip_clip: bool,
) -> Option<Winding> {
    let sn = source.points.len();
    if sn < 3 || pass.points.len() < 3 { return Some(target); }

    for i in 0..sn {
        let l = (i + 1) % sn;
        let v1 = source.points[l] - source.points[i];

        for j in 0..pass.points.len() {
            let v2 = pass.points[j] - source.points[i];
            let normal = v1.cross(v2);
            let length = normal.length();
            if length < ON_EPSILON { continue; }
            let normal = normal / length;
            let mut plane = Plane::new(normal, pass.points[j].dot(normal));

            // Which side is the source on? The two points forming the edge lie
            // on the plane by construction, so look at the others.
            let mut flip_test = None;
            for k in 0..sn {
                if k == i || k == l { continue; }
                let d = plane.distance_to(source.points[k]);
                if d < -ON_EPSILON { flip_test = Some(false); break; }
                if d > ON_EPSILON { flip_test = Some(true); break; }
            }
            // Every remaining point is on the plane: the source is coplanar
            // with it, so it separates nothing.
            let Some(flip_test) = flip_test else { continue };

            // Orient the plane so the source is behind it.
            if flip_test { plane = plane.flipped(); }

            // For this to separate, all of `pass` must be in front.
            let mut any_front = false;
            let mut blocked = false;
            for k in 0..pass.points.len() {
                if k == j { continue; }
                let d = plane.distance_to(pass.points[k]);
                if d < -ON_EPSILON { blocked = true; break; }
                if d > ON_EPSILON { any_front = true; }
            }
            if blocked { continue; }
            // Coplanar with `pass` too: still not a separator.
            if !any_front { continue; }

            if flip_clip { plane = plane.flipped(); }
            target = target.clipped(&plane, ON_EPSILON)?;
        }
    }

    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prt::PortalGraph;

    /// Four clusters in a chain: 0 -A- 1 -B- 2 -C- 3.
    ///
    /// A and B are aligned; C is offset far enough in +Y to sit outside the
    /// sight cone that A casts through B. Cluster 0 must therefore be unable
    /// to see cluster 3, which is precisely what separating planes are for.
    ///
    /// Each winding's own plane normal points at the first cluster listed,
    /// matching what Cleave writes.
    fn occluded_chain() -> PortalGraph {
        let prt = "VPRT1\n4\n3\n\
            4 0 1 (0 0 32) (0 0 0) (0 32 0) (0 32 32)\n\
            4 1 2 (64 0 32) (64 0 0) (64 32 0) (64 32 32)\n\
            4 2 3 (128 80 32) (128 80 0) (128 112 0) (128 112 32)\n";
        PortalGraph::parse(prt).unwrap()
    }

    /// The same chain, but with C aligned with A and B so everything is in
    /// plain sight down a straight corridor.
    fn open_chain() -> PortalGraph {
        let prt = "VPRT1\n4\n3\n\
            4 0 1 (0 0 32) (0 0 0) (0 32 0) (0 32 32)\n\
            4 1 2 (64 0 32) (64 0 0) (64 32 0) (64 32 32)\n\
            4 2 3 (128 0 32) (128 0 0) (128 32 0) (128 32 32)\n";
        PortalGraph::parse(prt).unwrap()
    }

    /// Two clusters sharing one portal.
    fn two_rooms() -> PortalGraph {
        PortalGraph::parse("VPRT1\n2\n1\n4 0 1 (0 0 0) (0 0 64) (0 64 64) (0 64 0)\n").unwrap()
    }

    #[test]
    fn adjacent_clusters_see_each_other() {
        let g = two_rooms();
        let r = compute(&g, false);
        assert!(r.cluster_vis[0].test(0), "a cluster always sees itself");
        assert!(r.cluster_vis[0].test(1));
        assert!(r.cluster_vis[1].test(0));
    }

    #[test]
    fn every_cluster_sees_itself_even_with_no_portals() {
        let g = PortalGraph::parse("VPRT1\n4\n0\n").unwrap();
        let r = compute(&g, false);
        for c in 0..4 {
            assert_eq!(r.cluster_vis[c].count(), 1);
            assert!(r.cluster_vis[c].test(c));
        }
    }

    #[test]
    fn visibility_is_symmetric() {
        // If A can see B then B can see A; an asymmetric PVS makes geometry
        // pop in and out depending on which way you walked into a room.
        let g = occluded_chain();
        let r = compute(&g, false);
        for a in 0..g.clusters {
            for b in 0..g.clusters {
                assert_eq!(
                    r.cluster_vis[a].test(b),
                    r.cluster_vis[b].test(a),
                    "clusters {a} and {b} disagree"
                );
            }
        }
    }

    #[test]
    fn full_vis_is_never_more_permissive_than_base_vis() {
        // The full flow may only ever *remove* visibility. If it added any,
        // it would be hiding a bug that shows up as geometry disappearing.
        let g = occluded_chain();
        let full = compute(&g, false);
        let fast = compute(&g, true);
        for c in 0..g.clusters {
            assert!(
                full.cluster_vis[c].is_subset_of(&fast.cluster_vis[c]),
                "cluster {c}: full vis saw something base vis did not"
            );
        }
        assert!(full.final_visible <= fast.final_visible);
    }

    #[test]
    fn fast_vis_matches_the_base_estimate() {
        let g = occluded_chain();
        let fast = compute(&g, true);
        assert_eq!(fast.final_visible, fast.base_visible);
    }

    #[test]
    fn a_chain_of_rooms_stays_connected_through_its_neighbours() {
        let g = occluded_chain();
        let r = compute(&g, false);
        assert!(r.cluster_vis[0].test(1), "adjacent rooms must see each other");
        assert!(r.cluster_vis[1].test(2));
        assert!(r.cluster_vis[2].test(3));
    }

    #[test]
    fn a_straight_corridor_is_visible_end_to_end() {
        let r = compute(&open_chain(), false);
        assert!(r.cluster_vis[0].test(3), "you can see straight down an aligned corridor");
    }

    #[test]
    fn an_offset_opening_is_culled_away() {
        // The headline behaviour: the third opening lies outside the cone the
        // first casts through the second, so the far room is invisible even
        // though a chain of portals connects them.
        let g = occluded_chain();
        let full = compute(&g, false);
        let fast = compute(&g, true);

        assert!(fast.cluster_vis[0].test(3), "base vis floods through and over-estimates");
        assert!(
            !full.cluster_vis[0].test(3),
            "full vis should cull cluster 3 from cluster 0"
        );
        assert!(!full.cluster_vis[3].test(0), "and symmetrically");
        assert!(full.final_visible < fast.final_visible, "the full pass must cull something");
    }

    #[test]
    fn separators_do_not_clip_when_the_view_is_wide_open() {
        // A big source looking through a portal it entirely contains: nothing
        // should be clipped away.
        let source = Winding::new(vec![
            Vec3::new(0.0, -100.0, -100.0),
            Vec3::new(0.0, -100.0, 100.0),
            Vec3::new(0.0, 100.0, 100.0),
            Vec3::new(0.0, 100.0, -100.0),
        ]);
        let pass = Winding::new(vec![
            Vec3::new(64.0, -8.0, -8.0),
            Vec3::new(64.0, -8.0, 8.0),
            Vec3::new(64.0, 8.0, 8.0),
            Vec3::new(64.0, 8.0, -8.0),
        ]);
        let target = pass.clone();
        let out = clip_to_separators(&source, &pass, target, false);
        assert!(out.is_some());
    }

    #[test]
    fn separators_close_off_a_target_hidden_behind_a_wall() {
        // Source on the left, a narrow slot, and a target entirely off to one
        // side of the slot: no sight line reaches it.
        let source = Winding::new(vec![
            Vec3::new(0.0, -4.0, -4.0),
            Vec3::new(0.0, -4.0, 4.0),
            Vec3::new(0.0, 4.0, 4.0),
            Vec3::new(0.0, 4.0, -4.0),
        ]);
        let slot = Winding::new(vec![
            Vec3::new(64.0, -4.0, -4.0),
            Vec3::new(64.0, -4.0, 4.0),
            Vec3::new(64.0, 4.0, 4.0),
            Vec3::new(64.0, 4.0, -4.0),
        ]);
        // Far off to +Y, well outside any cone from source through slot.
        let target = Winding::new(vec![
            Vec3::new(128.0, 400.0, -4.0),
            Vec3::new(128.0, 400.0, 4.0),
            Vec3::new(128.0, 500.0, 4.0),
            Vec3::new(128.0, 500.0, -4.0),
        ]);
        let out = clip_to_separators(&source, &slot, target, false);
        assert!(out.is_none(), "a target outside the sight cone must be clipped away");
    }

    #[test]
    fn results_are_deterministic() {
        let g = occluded_chain();
        let a = compute(&g, false);
        let b = compute(&g, false);
        assert_eq!(a.cluster_vis, b.cluster_vis);
    }
}
