// SPDX-License-Identifier: MPL-2.0
//! What movement code needs from the world.

use kerosene_bsp::{Bsp, Trace};
use kerosene_math::Vec3;

/// Everything player movement needs to know about its surroundings.
///
/// A trait rather than a concrete `Bsp` so that movement can be tested against
/// a hand-made world -- a single floor plane, one step, one ramp -- where the
/// expected answer is obvious. Testing movement against a compiled map means
/// debugging the map when a test fails.
pub trait CollisionWorld {
    /// Sweep the player's box from `start` to `end`.
    fn trace_hull(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: u32) -> Trace;

    /// Contents at a point, for water and trigger detection.
    fn contents_at(&self, point: Vec3) -> u32;
}

/// The world as a compiled map.
pub struct BspWorld<'a> {
    pub bsp: &'a Bsp,
}

impl<'a> BspWorld<'a> {
    pub fn new(bsp: &'a Bsp) -> Self { BspWorld { bsp } }
}

impl CollisionWorld for BspWorld<'_> {
    fn trace_hull(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: u32) -> Trace {
        self.bsp.trace_box(start, end, mins, maxs, mask)
    }

    fn contents_at(&self, point: Vec3) -> u32 {
        self.bsp.point_contents_brushes(point)
    }
}

/// A world made of axis-aligned boxes, for testing movement in isolation.
///
/// Testing movement against a compiled map means debugging the map when a test
/// fails. A floor, a step and a wall are enough to pin down every rule in the
/// solver, and the expected answer is obvious by inspection.
#[cfg(any(test, feature = "test-world"))]
#[derive(Default)]
pub struct BoxWorld {
    /// `(mins, maxs, contents)`.
    pub boxes: Vec<(Vec3, Vec3, u32)>,
}

#[cfg(any(test, feature = "test-world"))]
impl BoxWorld {
    pub fn new() -> Self { Self::default() }

    /// Add a solid box.
    pub fn solid(mut self, mins: Vec3, maxs: Vec3) -> Self {
        self.boxes.push((mins, maxs, kerosene_bsp::contents::SOLID));
        self
    }

    /// Add a box with specific contents, such as water.
    pub fn volume(mut self, mins: Vec3, maxs: Vec3, contents: u32) -> Self {
        self.boxes.push((mins, maxs, contents));
        self
    }

    /// A large floor slab at `z <= 0`.
    pub fn with_floor(self) -> Self {
        self.solid(Vec3::new(-4096.0, -4096.0, -64.0), Vec3::new(4096.0, 4096.0, 0.0))
    }
}

#[cfg(any(test, feature = "test-world"))]
impl CollisionWorld for BoxWorld {
    fn trace_hull(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: u32) -> Trace {
        let mut best = Trace::miss(end);
        best.all_solid = false;

        for &(bmin, bmax, box_contents) in &self.boxes {
            if box_contents & mask == 0 { continue; }
            // Minkowski expansion: grow the box by the hull and sweep a point.
            let expanded_min = bmin - maxs;
            let expanded_max = bmax - mins;
            if let Some(hit) = sweep_point_vs_box(start, end, expanded_min, expanded_max) {
                if hit.0 < best.fraction {
                    best.fraction = hit.0;
                    best.plane = Some(kerosene_math::Plane::from_point_normal(Vec3::ZERO, hit.1));
                    best.contents = box_contents;
                }
            }
            // Starting inside counts as solid, matching the BSP tracer.
            let inside = (0..3).all(|i| start[i] > expanded_min[i] && start[i] < expanded_max[i]);
            if inside {
                best.start_solid = true;
                best.contents |= box_contents;
            }
        }

        best.endpos = if best.fraction >= 1.0 { end } else { start + (end - start) * best.fraction };
        best
    }

    fn contents_at(&self, point: Vec3) -> u32 {
        let mut out = 0;
        for &(bmin, bmax, c) in &self.boxes {
            if (0..3).all(|i| point[i] >= bmin[i] && point[i] <= bmax[i]) { out |= c; }
        }
        out
    }
}

#[cfg(any(test, feature = "test-world"))]
/// Slab method: the entry and exit times of a ray against a box.
///
/// A start point lying exactly *on* a face while moving inward counts as an
/// immediate hit at fraction zero. That case is not an edge case here -- it is
/// a player standing on the floor, which is most of them -- and rejecting it
/// makes the ground vanish from under everyone.
fn sweep_point_vs_box(start: Vec3, end: Vec3, min: Vec3, max: Vec3) -> Option<(f32, Vec3)> {
    let delta = end - start;
    let mut enter = 0.0f32;
    let mut exit = 1.0f32;
    let mut normal = Vec3::ZERO;

    for axis in 0..3 {
        if delta[axis].abs() < 1e-9 {
            // Parallel to this slab: either always within it or never.
            if start[axis] < min[axis] || start[axis] > max[axis] { return None; }
            continue;
        }
        let inv = 1.0 / delta[axis];
        let mut t0 = (min[axis] - start[axis]) * inv;
        let mut t1 = (max[axis] - start[axis]) * inv;
        // After the swap, t0 is always the near face; `sign` records which of
        // the two it was, so the reported normal points out of the box.
        let mut sign = -1.0;
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
            sign = 1.0;
        }
        // `>=` rather than `>` so that a contact exactly at t = 0 still
        // records its normal.
        if t0 >= enter {
            enter = t0;
            normal = Vec3::ZERO;
            normal[axis] = sign;
        }
        exit = exit.min(t1);
        if enter > exit { return None; }
    }

    // A zero normal means no face was crossed: the ray began inside the box.
    // That is start-solid, which the caller detects separately.
    if normal == Vec3::ZERO { return None; }
    if enter >= 1.0 { return None; }

    // Stop a hair short of the surface, matching the BSP tracer, so the next
    // tick does not begin inside what was just hit.
    let backed = (enter - kerosene_math::DIST_EPSILON / delta.length().max(1e-6)).max(0.0);
    Some((backed, normal))
}
