// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! From leaves to rooms.
//!
//! A BSP carves a hall into dozens of leaves, and the ear hears one hall.
//! Storing a record per leaf would be dozens of copies of the same numbers
//! with a little noise in each, and walking across a leaf boundary would
//! nudge the reverb for no reason a player could see. So leaves that touch
//! and sound alike are gathered into rooms: union-find over the portal
//! graph, widest portals first, merging only while the merged average still
//! sounds like both halves. A doorway between a hall and a cupboard stays a
//! boundary because the two sides disagree; a hall split by the tree into
//! twenty leaves comes back together.
//!
//! Leaves too small to probe take the figures of the neighbour they share
//! the most portal with, which is almost always the room they are a corner
//! of.

use crate::probe::{LeafAcoustics, OUTDOOR_OPENNESS, Skipped};
use kerosene_bsp::Bsp;
use kerosene_bsp::acoustics::{AcousticRoom, Acoustics, MAX_ROOMS, NO_ROOM, room_flags};
use kerosene_math::Aabb;
use umbra::prt::PortalGraph;

/// Which leaves touch which, and how much.
#[derive(Clone, Debug, Default)]
pub struct Adjacency {
    /// `(leaf a, leaf b, weight)`; weight is the portal's area, or a
    /// nominal one when adjacency was guessed from bounds.
    pub edges: Vec<(usize, usize, f32)>,
}

impl Adjacency {
    /// From the portal graph Cleave wrote. Portals join clusters; every
    /// leaf with vis is in exactly one, and normally is one.
    pub fn from_portals(bsp: &Bsp, graph: &PortalGraph) -> Adjacency {
        let mut cluster_leaves: Vec<Vec<usize>> = vec![Vec::new(); graph.clusters];
        for (i, leaf) in bsp.leaves.iter().enumerate() {
            if leaf.cluster >= 0 && (leaf.cluster as usize) < graph.clusters {
                cluster_leaves[leaf.cluster as usize].push(i);
            }
        }
        let mut edges = Vec::with_capacity(graph.portals.len() / 2);
        // Directed portals come in pairs; one direction is enough.
        for portal in graph.portals.iter().step_by(2) {
            let area = portal.winding.area().max(1.0);
            for &a in &cluster_leaves[portal.from_cluster] {
                for &b in &cluster_leaves[portal.into_cluster] {
                    if a != b {
                        edges.push((a, b, area));
                    }
                }
            }
        }
        Adjacency { edges }
    }

    /// From leaf bounds alone, for a map with no portal file: leaves whose
    /// boxes touch are taken to be joined, weighted by the overlap of their
    /// shared face.
    pub fn from_bounds(bounds: &[Aabb], usable: &[bool]) -> Adjacency {
        const EPSILON: f32 = 1.0;
        let mut order: Vec<usize> = (0..bounds.len()).filter(|&i| usable[i]).collect();
        order.sort_by(|&a, &b| bounds[a].min.x.total_cmp(&bounds[b].min.x));
        let mut edges = Vec::new();
        for (n, &a) in order.iter().enumerate() {
            let box_a = bounds[a].expanded(EPSILON);
            for &b in &order[n + 1..] {
                if bounds[b].min.x > box_a.max.x {
                    break;
                }
                let box_b = bounds[b];
                if !box_a.intersects(&box_b) {
                    continue;
                }
                // The face they share: the overlap in the two axes they do
                // not meet along.
                let overlap = |axis: usize| {
                    (bounds[a].max[axis].min(box_b.max[axis])
                        - bounds[a].min[axis].max(box_b.min[axis]))
                    .max(0.0)
                };
                let o = [overlap(0), overlap(1), overlap(2)];
                let mut sorted = o;
                sorted.sort_by(f32::total_cmp);
                let weight = (sorted[1] * sorted[2]).max(1.0);
                edges.push((a, b, weight));
            }
        }
        Adjacency { edges }
    }

    /// The neighbours of every leaf, with the weight of each join.
    fn neighbours(&self, leaves: usize) -> Vec<Vec<(usize, f32)>> {
        let mut out = vec![Vec::new(); leaves];
        for &(a, b, w) in &self.edges {
            out[a].push((b, w));
            out[b].push((a, w));
        }
        out
    }
}

/// Give every unprobed but non-solid leaf the figures of the neighbour it
/// shares the widest join with, repeating until nothing changes so a chain
/// of slivers fills in from the far end.
pub fn inherit(leaves: &mut [Result<LeafAcoustics, Skipped>], adjacency: &Adjacency) -> usize {
    let neighbours = adjacency.neighbours(leaves.len());
    let mut inherited = 0;
    loop {
        let mut changed = false;
        for i in 0..leaves.len() {
            if !matches!(leaves[i], Err(Skipped::Tiny) | Err(Skipped::NoPoint)) {
                continue;
            }
            let best = neighbours[i]
                .iter()
                .filter_map(|&(n, w)| leaves[n].ok().map(|a| (w, a)))
                .max_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, mut acoustics)) = best {
                acoustics.inherited = true;
                leaves[i] = Ok(acoustics);
                inherited += 1;
                changed = true;
            }
        }
        if !changed {
            return inherited;
        }
    }
}

/// How different two leaves may be and still be one room.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    /// Ratio of decay times, per band.
    pub rt60_ratio: f32,
    /// Ratio of mean free paths.
    pub path_ratio: f32,
    /// Difference in openness.
    pub openness: f32,
    /// Difference in pre-delay, seconds.
    pub predelay: f32,
}

impl Tolerance {
    pub const DEFAULT: Tolerance = Tolerance {
        rt60_ratio: 1.25,
        path_ratio: 1.5,
        openness: 0.15,
        predelay: 0.008,
    };

    /// The same, `k` times looser.
    pub fn scaled(&self, k: f32) -> Tolerance {
        Tolerance {
            rt60_ratio: self.rt60_ratio.powf(k),
            path_ratio: self.path_ratio.powf(k),
            openness: self.openness * k,
            predelay: self.predelay * k,
        }
    }
}

/// The volume-weighted sum of a set's figures, so the mean of a merge is a
/// sum and a division rather than a walk over its leaves.
#[derive(Clone, Copy, Debug, Default)]
struct Sums {
    volume: f64,
    ln_rt60: [f64; 4],
    absorption: [f64; 4],
    ln_path: f64,
    predelay: f64,
    openness: f64,
    diffusion: f64,
    wet: f64,
    water: bool,
    all_inherited: bool,
    leaves: u32,
}

impl Sums {
    fn of(a: &LeafAcoustics, volume: f32) -> Sums {
        let v = volume.max(1.0) as f64;
        let mut s = Sums {
            volume: v,
            ln_path: (a.mean_free_path.max(1.0) as f64).ln() * v,
            predelay: a.predelay as f64 * v,
            openness: a.openness as f64 * v,
            diffusion: a.diffusion as f64 * v,
            wet: a.wet as f64 * v,
            water: a.water,
            all_inherited: a.inherited,
            leaves: 1,
            ..Default::default()
        };
        for b in 0..4 {
            s.ln_rt60[b] = (a.rt60[b] as f64).ln() * v;
            s.absorption[b] = a.absorption[b] as f64 * v;
        }
        s
    }

    fn plus(&self, other: &Sums) -> Sums {
        let mut s = *self;
        s.volume += other.volume;
        for b in 0..4 {
            s.ln_rt60[b] += other.ln_rt60[b];
            s.absorption[b] += other.absorption[b];
        }
        s.ln_path += other.ln_path;
        s.predelay += other.predelay;
        s.openness += other.openness;
        s.diffusion += other.diffusion;
        s.wet += other.wet;
        s.water |= other.water;
        s.all_inherited &= other.all_inherited;
        s.leaves += other.leaves;
        s
    }

    fn mean(&self) -> Mean {
        let v = self.volume.max(1.0);
        let mut m = Mean {
            ln_rt60: [0.0; 4],
            ln_path: (self.ln_path / v) as f32,
            openness: (self.openness / v) as f32,
            predelay: (self.predelay / v) as f32,
        };
        for b in 0..4 {
            m.ln_rt60[b] = (self.ln_rt60[b] / v) as f32;
        }
        m
    }

    fn compatible(&self, other: &Sums, tolerance: &Tolerance) -> bool {
        if self.water != other.water {
            return false;
        }
        let a = self.mean();
        let b = other.mean();
        a.ln_rt60
            .iter()
            .zip(b.ln_rt60)
            .all(|(x, y)| (x - y).abs() < tolerance.rt60_ratio.ln())
            && (a.ln_path - b.ln_path).abs() < tolerance.path_ratio.ln()
            && (a.openness - b.openness).abs() < tolerance.openness
            && (a.predelay - b.predelay).abs() < tolerance.predelay
    }

    fn room(&self, bounds: &Aabb) -> AcousticRoom {
        let v = self.volume.max(1.0);
        let mut rt60 = [0.0f32; 4];
        let mut absorption = [0.0f32; 4];
        for b in 0..4 {
            rt60[b] = (self.ln_rt60[b] / v).exp() as f32;
            absorption[b] = (self.absorption[b] / v) as f32;
        }
        let openness = (self.openness / v) as f32;
        let mut flags = 0;
        if self.water {
            flags |= room_flags::WATER;
        }
        if openness > OUTDOOR_OPENNESS {
            flags |= room_flags::OUTDOOR;
        }
        if self.all_inherited {
            flags |= room_flags::INHERITED;
        }
        AcousticRoom {
            rt60,
            absorption,
            mean_free_path: (self.ln_path / v).exp() as f32,
            predelay: (self.predelay / v) as f32,
            openness,
            wet: (self.wet / v) as f32,
            diffusion: (self.diffusion / v) as f32,
            centre: bounds.center().to_array(),
            radius: bounds.size().length() * 0.5,
            volume: self.volume as f32,
            flags,
            leaf_count: self.leaves,
        }
    }
}

struct Mean {
    ln_rt60: [f32; 4],
    ln_path: f32,
    openness: f32,
    predelay: f32,
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> UnionFind {
        UnionFind {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) -> usize {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
        ra
    }
}

/// Sets with less volume than this are absorbed into the neighbour they
/// share the most portal with, whatever it sounds like: a 64-unit cube is a
/// niche or a doorway, not a room, and it sounds like the room it opens
/// onto. A doorway in particular measures as a blend of both sides and
/// would otherwise be a third room a pace wide.
const SMALL_ROOM: f64 = 64.0 * 64.0 * 64.0;

/// Gather the leaves into rooms.
///
/// `leaves` is one entry per BSP leaf; `Err` leaves end up in [`NO_ROOM`].
pub fn cluster(
    leaves: &[Result<LeafAcoustics, Skipped>],
    bounds: &[Aabb],
    adjacency: &Adjacency,
    tolerance: Tolerance,
) -> Acoustics {
    let mut tolerance = tolerance;
    loop {
        let acoustics = cluster_once(leaves, bounds, adjacency, &tolerance);
        if acoustics.rooms.len() <= MAX_ROOMS {
            return acoustics;
        }
        log::warn!(
            "resonance: {} rooms is more than the format holds; merging more loosely",
            acoustics.rooms.len()
        );
        tolerance = tolerance.scaled(1.5);
    }
}

fn cluster_once(
    leaves: &[Result<LeafAcoustics, Skipped>],
    bounds: &[Aabb],
    adjacency: &Adjacency,
    tolerance: &Tolerance,
) -> Acoustics {
    let n = leaves.len();
    let volume = |i: usize| {
        let s = bounds[i].size();
        (s.x * s.y * s.z).max(1.0)
    };
    let mut sums: Vec<Option<Sums>> = leaves
        .iter()
        .enumerate()
        .map(|(i, l)| l.ok().map(|a| Sums::of(&a, volume(i))))
        .collect();
    let mut sets = UnionFind::new(n);

    let mut edges: Vec<(usize, usize, f32)> = adjacency
        .edges
        .iter()
        .copied()
        .filter(|&(a, b, _)| sums[a].is_some() && sums[b].is_some())
        .collect();
    edges.sort_by(|x, y| y.2.total_cmp(&x.2).then(x.0.cmp(&y.0)).then(x.1.cmp(&y.1)));

    // Join two sets if they are alike within `tolerance`, or regardless
    // when no tolerance is given -- except that water and air never mix.
    let try_merge = |sets: &mut UnionFind,
                     sums: &mut Vec<Option<Sums>>,
                     a: usize,
                     b: usize,
                     tolerance: Option<&Tolerance>|
     -> bool {
        let (ra, rb) = (sets.find(a), sets.find(b));
        if ra == rb {
            return false;
        }
        let (Some(sa), Some(sb)) = (sums[ra], sums[rb]) else {
            return false;
        };
        if sa.water != sb.water {
            return false;
        }
        let merged = sa.plus(&sb);
        if let Some(tolerance) = tolerance
            && (!sa.compatible(&sb, tolerance)
                || !merged.compatible(&sa, tolerance)
                || !merged.compatible(&sb, tolerance))
        {
            return false;
        }
        let root = sets.union(ra, rb);
        sums[root] = Some(merged);
        sums[if root == ra { rb } else { ra }] = None;
        true
    };

    for &(a, b, _) in &edges {
        try_merge(&mut sets, &mut sums, a, b, Some(tolerance));
    }

    // Niches: a set too small to be a room joins the neighbouring set it
    // shares the most portal with.
    loop {
        let mut merged_any = false;
        let mut small: Vec<usize> = (0..n)
            .filter(|&i| sets.find(i) == i && sums[i].is_some_and(|s| s.volume < SMALL_ROOM))
            .collect();
        small.sort_unstable();
        for root in small {
            if sets.find(root) != root {
                continue;
            }
            let mut best: Option<(f32, usize)> = None;
            let mut weight_to: std::collections::HashMap<usize, f32> =
                std::collections::HashMap::new();
            for &(a, b, w) in &edges {
                let (ra, rb) = (sets.find(a), sets.find(b));
                let other = if ra == root && rb != root {
                    rb
                } else if rb == root && ra != root {
                    ra
                } else {
                    continue;
                };
                *weight_to.entry(other).or_insert(0.0) += w;
            }
            for (other, w) in weight_to {
                if best.is_none_or(|(bw, bo)| w > bw || (w == bw && other < bo)) {
                    best = Some((w, other));
                }
            }
            if let Some((_, other)) = best
                && try_merge(&mut sets, &mut sums, root, other, None)
            {
                merged_any = true;
            }
        }
        if !merged_any {
            break;
        }
    }

    // Number the rooms, largest first, and point every leaf at its own.
    let mut roots: Vec<usize> = (0..n)
        .filter(|&i| sets.find(i) == i && sums[i].is_some())
        .collect();
    roots.sort_by(|&a, &b| {
        sums[b]
            .unwrap()
            .volume
            .total_cmp(&sums[a].unwrap().volume)
            .then(a.cmp(&b))
    });
    let mut room_of_root = vec![NO_ROOM; n];
    for (index, &root) in roots.iter().enumerate() {
        room_of_root[root] = index as u16;
    }
    let mut leaf_room = vec![NO_ROOM; n];
    let mut room_bounds: Vec<Aabb> = vec![Aabb::EMPTY; roots.len()];
    for i in 0..n {
        if leaves[i].is_err() {
            continue;
        }
        let room = room_of_root[sets.find(i)];
        leaf_room[i] = room;
        if room != NO_ROOM {
            let rb = &mut room_bounds[room as usize];
            *rb = rb.union(&bounds[i]);
        }
    }
    let rooms = roots
        .iter()
        .enumerate()
        .map(|(index, &root)| sums[root].unwrap().room(&room_bounds[index]))
        .collect();

    Acoustics {
        rooms,
        leaf_room,
        paths: Vec::new(),
    }
}
