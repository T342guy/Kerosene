// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Listening to one leaf: rays out, a room's figures back.
//!
//! Sound in a room is energy bouncing between surfaces and losing a share at
//! each. Rather than solve that, this throws a few hundred rays from inside
//! the leaf and lets them do it: each hit records the distance travelled and
//! how much the surface soaked up per band, and the averages are what
//! Eyring's formula wants -- the mean free path and the mean absorption --
//! measured rather than assumed, in a shape no formula assumes.
//!
//! The decay comes out of the ray statistics directly. A ray meets a surface
//! every `mean_free_path` units, losing `ln(1 - absorption)` of its energy
//! each time, so energy falls off along its path as
//! `exp(distance * mean_ln / mean_free_path)`; 60 dB is `ln(10^-6)` of that,
//! and distance is time at the speed of sound. Air absorption is one more
//! loss per unit distance on top.

use crate::materials::Absorption;
use crate::rng::Pcg32;
use kerosene_bsp::{Bsp, contents, surf};
use kerosene_math::{Aabb, Vec3};

/// Speed of sound in kerosene units (inches) per second: 343 m/s.
pub const SPEED_OF_SOUND: f32 = 13_504.0;

/// Air absorption per unit of travel, per band, as an energy coefficient.
///
/// Roughly 0.1, 0.5, 2.5 and 12 dB over a hundred metres. Nothing in a room
/// notices; a hangar or a canyon does.
pub const AIR: [f32; 4] = [2e-6, 1e-5, 6e-5, 3e-4];

/// The most a surface absorbs, and what an escaped ray counts as.
const MAX_ABSORPTION: f32 = kerosene_asset::MAX_ABSORPTION;

/// How far a ray is followed before it is taken to have found nothing.
const RAY_CAP: f32 = 16_384.0;

/// How far off a surface the next ray starts, so it does not hit it again.
const NUDGE: f32 = 0.5;

/// A ray is followed until it has lost this much of its 500 Hz energy.
const NEGLIGIBLE: f32 = 1e-3;

/// How much of a reflection is matte rather than mirror-like. Real walls are
/// somewhere between; half spreads the rays without losing the shape of the
/// room.
const SCATTER: f32 = 0.5;

/// Leaves whose diagonal is under this are too small to probe and take a
/// neighbour's figures instead.
pub const TINY: f32 = 16.0;

/// A leaf whose rays mostly escape is outdoors.
pub const OUTDOOR_OPENNESS: f32 = 0.35;

/// The loudest a room can be next to the dry sound.
const WET_CEILING: f32 = 0.6;

/// The reverb time that counts as fully wet: longer rings no louder.
const WET_RT60: f32 = 1.5;

pub const MIN_RT60: f32 = 0.05;
pub const MAX_RT60: f32 = 20.0;
pub const MIN_PREDELAY: f32 = 0.002;
pub const MAX_PREDELAY: f32 = 0.080;

/// How hard to look.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Rays per probe point.
    pub rays: usize,
    /// Bounces per ray before giving up on it.
    pub max_bounces: usize,
}

impl Options {
    pub const DEFAULT: Options = Options {
        rays: 256,
        max_bounces: 24,
    };
    pub const FAST: Options = Options {
        rays: 96,
        max_bounces: 16,
    };
    pub const EXTRA: Options = Options {
        rays: 1024,
        max_bounces: 32,
    };
}

impl Default for Options {
    fn default() -> Self {
        Options::DEFAULT
    }
}

/// What one leaf sounds like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeafAcoustics {
    pub rt60: [f32; 4],
    pub absorption: [f32; 4],
    pub mean_free_path: f32,
    pub predelay: f32,
    pub openness: f32,
    pub diffusion: f32,
    pub wet: f32,
    pub water: bool,
    /// Taken from a neighbour rather than measured.
    pub inherited: bool,
}

impl LeafAcoustics {
    pub fn is_outdoor(&self) -> bool {
        self.openness > OUTDOOR_OPENNESS
    }
}

/// Why a leaf was not probed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skipped {
    /// Solid, or outside the map.
    Solid,
    /// Under [`TINY`]: takes a neighbour's figures.
    Tiny,
    /// No probe point inside it could be found: a sliver too thin for its
    /// own centre to land in. Takes a neighbour's figures.
    NoPoint,
}

/// The running totals over every ray from a leaf.
#[derive(Default)]
struct Tally {
    rays: usize,
    hits: usize,
    escaped: usize,
    free_path: f64,
    free_path_sq: f64,
    ln_sum: [f64; 4],
    first_hit: f64,
    first_hits: usize,
}

impl Tally {
    fn hit(&mut self, distance: f32, absorption: [f32; 4], first: bool) {
        self.hits += 1;
        self.free_path += distance as f64;
        self.free_path_sq += (distance as f64) * (distance as f64);
        for (sum, a) in self.ln_sum.iter_mut().zip(absorption) {
            *sum += (1.0 - a.clamp(0.0, MAX_ABSORPTION) as f64).ln();
        }
        if first {
            self.first_hit += distance as f64;
            self.first_hits += 1;
        }
    }

    fn finish(&self, water: bool) -> Option<LeafAcoustics> {
        if self.hits == 0 || self.rays == 0 {
            return None;
        }
        let hits = self.hits as f64;
        let mean_free_path = (self.free_path / hits).max(1.0);
        let variance = (self.free_path_sq / hits - mean_free_path * mean_free_path).max(0.0);
        let diffusion = (variance.sqrt() / mean_free_path).clamp(0.0, 1.0) as f32;

        let mut rt60 = [0.0f32; 4];
        let mut absorption = [0.0f32; 4];
        for b in 0..4 {
            let mean_ln = self.ln_sum[b] / hits;
            absorption[b] = (1.0 - mean_ln.exp()) as f32;
            let per_unit = -mean_ln / mean_free_path + AIR[b] as f64;
            let seconds = (1e-6f64).ln().abs() / (SPEED_OF_SOUND as f64 * per_unit.max(1e-12));
            rt60[b] = (seconds as f32).clamp(MIN_RT60, MAX_RT60);
        }

        let openness = (self.escaped as f32 / self.rays as f32).clamp(0.0, 1.0);
        let first = if self.first_hits > 0 {
            self.first_hit / self.first_hits as f64
        } else {
            mean_free_path
        };
        let predelay = (first as f32 / SPEED_OF_SOUND).clamp(MIN_PREDELAY, MAX_PREDELAY);
        let wet = WET_CEILING * (1.0 - openness).powf(1.5) * (rt60[1] / WET_RT60).clamp(0.15, 1.0);

        Some(LeafAcoustics {
            rt60,
            absorption,
            mean_free_path: mean_free_path as f32,
            predelay,
            openness,
            diffusion,
            wet,
            water,
            inherited: false,
        })
    }
}

/// Where in a leaf to listen from. The centre when the centre is inside it,
/// otherwise the first of a few jittered points that is; a sliver can have
/// its centre in the next leaf over.
pub fn probe_points(bsp: &Bsp, leaf: usize, rng: &mut Pcg32) -> Vec<Vec3> {
    let bounds = bsp.leaves[leaf].bounds();
    let diagonal = bounds.size().length();
    let wanted = ((1.0 + diagonal / 512.0) as usize).clamp(1, 8);
    let inside =
        |p: Vec3| bsp.point_leaf(p) == leaf && bsp.point_contents(p) & contents::SOLID == 0;

    let mut points = Vec::with_capacity(wanted);
    let centre = bounds.center();
    if inside(centre) {
        points.push(centre);
    }
    // Stratified: the box in eight cells, a jittered point in each, twice
    // over if the first pass found nothing.
    let mut attempts = 0;
    while points.len() < wanted && attempts < 32 {
        let cell = attempts % 8;
        let corner = Vec3::new(
            (cell & 1) as f32,
            ((cell >> 1) & 1) as f32,
            ((cell >> 2) & 1) as f32,
        );
        let jitter = Vec3::new(rng.next_f32(), rng.next_f32(), rng.next_f32());
        let t = (corner + jitter) * 0.5;
        let p = bounds.min + (bounds.max - bounds.min) * t;
        if inside(p) && !points.iter().any(|q| q.distance(p) < 1.0) {
            points.push(p);
        }
        attempts += 1;
    }
    points
}

/// Probe one leaf.
pub fn probe_leaf(
    bsp: &Bsp,
    leaf: usize,
    absorption: &Absorption,
    options: &Options,
) -> Result<LeafAcoustics, Skipped> {
    let record = &bsp.leaves[leaf];
    if record.is_solid() || !record.has_vis() {
        return Err(Skipped::Solid);
    }
    let bounds = record.bounds();
    if bounds.size().length() < TINY {
        return Err(Skipped::Tiny);
    }
    let mut rng = Pcg32::new(leaf as u64, 0x5eed);
    let points = probe_points(bsp, leaf, &mut rng);
    if points.is_empty() {
        return Err(Skipped::NoPoint);
    }

    let mask = contents::SOLID | contents::MOVEABLE | contents::WINDOW;
    let mut tally = Tally::default();
    for origin in points {
        for _ in 0..options.rays {
            tally.rays += 1;
            trace_ray(
                bsp,
                origin,
                rng.unit_sphere(),
                absorption,
                options,
                mask,
                &mut rng,
                &mut tally,
            );
        }
    }
    let water = record.contents & contents::WATER != 0;
    tally.finish(water).ok_or(Skipped::NoPoint)
}

/// Follow one ray until it has nothing left, has escaped, or has bounced
/// too often.
#[allow(clippy::too_many_arguments)]
fn trace_ray(
    bsp: &Bsp,
    mut position: Vec3,
    mut direction: Vec3,
    absorption: &Absorption,
    options: &Options,
    mask: u32,
    rng: &mut Pcg32,
    tally: &mut Tally,
) {
    let mut energy = 1.0f32;
    for bounce in 0..options.max_bounces {
        let first = bounce == 0;
        let end = position + direction * RAY_CAP;
        let trace = bsp.trace_ray(position, end, mask);
        if trace.start_solid || trace.all_solid {
            // Started in a wall: the nudge landed somewhere it should not
            // have. Nothing to learn from this ray.
            return;
        }
        let distance = trace.fraction * RAY_CAP;
        let escaped = trace.fraction >= 1.0 || trace.surface_flags & surf::SKY != 0;
        if escaped {
            // Open sky is the perfect absorber: nothing comes back.
            tally.hit(distance, [MAX_ABSORPTION; 4], first);
            tally.escaped += 1;
            return;
        }
        let Some(plane) = trace.plane else {
            return;
        };
        if distance < NUDGE {
            // Grazing the surface it just left; give up rather than loop.
            return;
        }
        let alpha = absorption.of(trace.texture_index);
        tally.hit(distance, alpha, first);
        energy *= 1.0 - alpha[1];
        if energy < NEGLIGIBLE {
            return;
        }

        // Reflect: part mirror, part matte.
        let normal = plane.normal;
        let mirror = direction - normal * (2.0 * direction.dot(normal));
        let matte = rng.cosine_hemisphere(normal);
        let mut next = (mirror * (1.0 - SCATTER) + matte * SCATTER).normalize_or_zero();
        if next.dot(normal) <= 1e-3 {
            next = matte;
        }
        direction = next;
        position = trace.endpos + normal * NUDGE;
    }
}

/// Every leaf's bounds as an [`Aabb`], for the clustering pass.
pub fn leaf_bounds(bsp: &Bsp) -> Vec<Aabb> {
    bsp.leaves.iter().map(|l| l.bounds()).collect()
}
