// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Tracing rays and boxes through the world.
//!
//! Every "did it hit anything" question in the engine funnels through here:
//! player movement, bullets, line of sight, and the shadow rays the lighting
//! compile fires at every luxel.
//!
//! Traces work against **brushes**, not triangles. A brush is a handful of
//! planes, so testing one is a handful of dot products regardless of how
//! detailed its surface is. The BSP tree narrows the search to the brushes
//! actually along the path, so a trace across a whole level touches a few
//! dozen planes rather than the map's hundred thousand triangles.
//!
//! Box traces use the standard trick of pushing each brush plane outward by
//! the box's extent along that plane's normal, which turns a swept-box test
//! back into a swept-point test against a fattened brush.

use crate::types::*;
use crate::{Bsp, contents as content_flags};
use kerosene_math::{DIST_EPSILON, Plane, Vec3};

/// What a trace found.
#[derive(Clone, Copy, Debug)]
pub struct Trace {
    /// How far along the path the trace got, in `[0, 1]`. `1.0` means it
    /// reached the end without hitting anything.
    pub fraction: f32,
    /// Where it stopped.
    pub endpos: Vec3,
    /// The surface it stopped against, if any.
    pub plane: Option<Plane>,
    /// Contents of whatever it hit.
    pub contents: u32,
    /// Surface flags of the face it hit, when known.
    pub surface_flags: u32,
    /// The trace began inside solid geometry.
    pub start_solid: bool,
    /// The entire path was inside solid geometry.
    pub all_solid: bool,
    /// Index of the brush model hit; 0 is the world.
    pub model: usize,
}

impl Trace {
    /// A trace that reached its destination.
    pub fn miss(end: Vec3) -> Trace {
        Trace {
            fraction: 1.0,
            endpos: end,
            plane: None,
            contents: 0,
            surface_flags: 0,
            start_solid: false,
            all_solid: false,
            model: 0,
        }
    }

    pub fn hit(&self) -> bool { self.fraction < 1.0 || self.start_solid }
}

struct Work<'a> {
    bsp: &'a Bsp,
    start: Vec3,
    end: Vec3,
    mins: Vec3,
    maxs: Vec3,
    /// Half-extents used to offset planes. Zero for a ray.
    extents: Vec3,
    is_point: bool,
    mask: u32,
    trace: Trace,
}

impl Bsp {
    /// Trace a ray against the world.
    pub fn trace_ray(&self, start: Vec3, end: Vec3, mask: u32) -> Trace {
        self.trace_box(start, end, Vec3::ZERO, Vec3::ZERO, mask)
    }

    /// Trace an axis-aligned box against the world.
    ///
    /// `mins`/`maxs` are relative to the traced point, so a standing player is
    /// roughly `(-16, -16, 0)` to `(16, 16, 72)`.
    pub fn trace_box(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: u32) -> Trace {
        let head = self.models.first().map_or(0, |m| m.head_node);
        self.trace_node(head, start, end, mins, maxs, mask, 0)
    }

    /// Trace against one brush model -- a door, a platform, the world itself.
    ///
    /// Brush models are stored as a single leaf, so this tests their brushes
    /// directly rather than walking a tree.
    pub fn trace_model(
        &self,
        model: usize,
        start: Vec3,
        end: Vec3,
        mins: Vec3,
        maxs: Vec3,
        mask: u32,
    ) -> Trace {
        let Some(m) = self.models.get(model) else { return Trace::miss(end) };
        self.trace_node(m.head_node, start, end, mins, maxs, mask, model)
    }

    fn trace_node(
        &self,
        head: i32,
        start: Vec3,
        end: Vec3,
        mins: Vec3,
        maxs: Vec3,
        mask: u32,
        model: usize,
    ) -> Trace {
        let is_point = mins == Vec3::ZERO && maxs == Vec3::ZERO;
        let mut work = Work {
            bsp: self,
            start,
            end,
            mins,
            maxs,
            extents: Vec3::new(
                (-mins.x).max(maxs.x),
                (-mins.y).max(maxs.y),
                (-mins.z).max(maxs.z),
            ),
            is_point,
            mask,
            trace: Trace {
                fraction: 1.0,
                endpos: end,
                plane: None,
                contents: 0,
                surface_flags: 0,
                // Cleared as soon as any non-solid space is seen. Starting
                // true means a trace that never leaves rock reports it.
                start_solid: false,
                all_solid: true,
                model,
            },
        };

        work.recurse(head, 0.0, 1.0, start, end);

        if work.trace.fraction == 1.0 {
            work.trace.endpos = end;
        } else {
            work.trace.endpos = start + (end - start) * work.trace.fraction;
        }
        work.trace
    }
}

impl Work<'_> {
    fn recurse(&mut self, node: i32, p1f: f32, p2f: f32, p1: Vec3, p2: Vec3) {
        // Something nearer has already been hit; nothing beyond can matter.
        if self.trace.fraction <= p1f { return; }

        let node = match decode_child(node) {
            Child::Leaf(leaf) => return self.test_leaf(leaf),
            Child::Node(n) => n,
        };
        let Some(node) = self.bsp.nodes.get(node) else { return };
        let Some(bsp_plane) = self.bsp.planes.get(node.plane as usize) else { return };
        let plane = bsp_plane.to_plane();

        let (t1, t2, offset) = {
            let t1 = plane.distance_to(p1);
            let t2 = plane.distance_to(p2);
            // How far the box sticks out along this normal.
            let offset = if self.is_point {
                0.0
            } else {
                self.extents.x * plane.normal.x.abs()
                    + self.extents.y * plane.normal.y.abs()
                    + self.extents.z * plane.normal.z.abs()
            };
            (t1, t2, offset)
        };

        if t1 >= offset && t2 >= offset {
            return self.recurse(node.children[0], p1f, p2f, p1, p2);
        }
        if t1 < -offset && t2 < -offset {
            return self.recurse(node.children[1], p1f, p2f, p1, p2);
        }

        // The path crosses the plane: split it and walk the near side first,
        // so an early hit prunes the far side.
        let (side, frac_near, frac_far) = if t1 < t2 {
            let idist = 1.0 / (t1 - t2);
            (1usize, (t1 - offset + DIST_EPSILON) * idist, (t1 + offset + DIST_EPSILON) * idist)
        } else if t1 > t2 {
            let idist = 1.0 / (t1 - t2);
            (0usize, (t1 + offset + DIST_EPSILON) * idist, (t1 - offset - DIST_EPSILON) * idist)
        } else {
            (0usize, 1.0, 0.0)
        };

        let frac_near = frac_near.clamp(0.0, 1.0);
        let frac_far = frac_far.clamp(0.0, 1.0);

        let midf = p1f + (p2f - p1f) * frac_near;
        let mid = p1 + (p2 - p1) * frac_near;
        self.recurse(node.children[side], p1f, midf, p1, mid);

        let midf = p1f + (p2f - p1f) * frac_far;
        let mid = p1 + (p2 - p1) * frac_far;
        self.recurse(node.children[side ^ 1], midf, p2f, mid, p2);
    }

    fn test_leaf(&mut self, leaf: usize) {
        let Some(l) = self.bsp.leaves.get(leaf) else { return };
        if l.contents & self.mask == 0 {
            // Passing through open space: the trace is not entirely in solid.
            self.trace.all_solid = false;
        }

        let first = l.first_leafbrush as usize;
        let count = l.num_leafbrushes as usize;
        for i in first..first + count {
            let Some(&bi) = self.bsp.leafbrushes.get(i) else { continue };
            let Some(brush) = self.bsp.brushes.get(bi as usize).copied() else { continue };
            if brush.contents & self.mask == 0 { continue; }
            self.clip_to_brush(&brush);
            if self.trace.all_solid { return; }
        }
    }

    /// Clip the swept box against one convex brush.
    ///
    /// Walks the brush's planes tracking where the path *enters* the brush
    /// (the latest front-to-back crossing) and where it *leaves* (the earliest
    /// back-to-front one). If it enters before it leaves, it is inside, and
    /// the entry point is the hit.
    fn clip_to_brush(&mut self, brush: &Brush) {
        if brush.num_sides == 0 { return; }

        let mut enter_frac = -1.0f32;
        let mut leave_frac = 1.0f32;
        let mut clip_plane: Option<Plane> = None;
        let mut clip_surface = 0u32;
        let mut started_outside = false;
        let mut ends_outside = false;

        let first = brush.first_side as usize;
        for i in first..first + brush.num_sides as usize {
            let Some(side) = self.bsp.brushsides.get(i) else { continue };
            let Some(bp) = self.bsp.planes.get(side.plane as usize) else { continue };
            let plane = bp.to_plane();

            let dist = if self.is_point {
                plane.dist
            } else {
                // Push the plane out by the corner of the box that leads along
                // this normal, turning the swept box into a swept point.
                let ofs = Vec3::new(
                    if plane.normal.x < 0.0 { self.maxs.x } else { self.mins.x },
                    if plane.normal.y < 0.0 { self.maxs.y } else { self.mins.y },
                    if plane.normal.z < 0.0 { self.maxs.z } else { self.mins.z },
                );
                plane.dist - ofs.dot(plane.normal)
            };

            let d1 = plane.normal.dot(self.start) - dist;
            let d2 = plane.normal.dot(self.end) - dist;

            if d2 > 0.0 { ends_outside = true; }
            if d1 > 0.0 { started_outside = true; }

            // Entirely in front of this plane and moving no closer: the path
            // never enters the brush.
            if d1 > 0.0 && (d2 >= DIST_EPSILON || d2 >= d1) { return; }
            // Behind this plane the whole way: it constrains nothing.
            if d1 <= 0.0 && d2 <= 0.0 { continue; }

            if d1 > d2 {
                // Crossing front to back: a candidate entry point.
                let f = (d1 - DIST_EPSILON) / (d1 - d2);
                if f > enter_frac {
                    enter_frac = f;
                    clip_plane = Some(plane);
                    clip_surface = self
                        .bsp
                        .texinfo
                        .get(side.texinfo.max(0) as usize)
                        .map_or(0, |t| t.flags);
                }
            } else {
                // Back to front: an exit point.
                let f = (d1 + DIST_EPSILON) / (d1 - d2);
                if f < leave_frac { leave_frac = f; }
            }
        }

        if !started_outside {
            self.trace.start_solid = true;
            self.trace.contents |= brush.contents;
            if !ends_outside {
                self.trace.all_solid = true;
                self.trace.fraction = 0.0;
            }
            return;
        }

        if enter_frac < leave_frac && enter_frac > -1.0 && enter_frac < self.trace.fraction {
            self.trace.fraction = enter_frac.max(0.0);
            self.trace.plane = clip_plane;
            self.trace.contents = brush.contents;
            self.trace.surface_flags = clip_surface;
        }
    }
}

impl Bsp {
    /// Whether a straight line between two points is unobstructed.
    ///
    /// Used for shadow rays and for line-of-sight checks.
    pub fn is_visible_between(&self, a: Vec3, b: Vec3, mask: u32) -> bool {
        !self.trace_ray(a, b, mask).hit()
    }

    /// Contents at a point, tested against brushes rather than just the leaf.
    ///
    /// Detail brushes, clips and triggers sit *inside* open leaves, so the
    /// leaf's own contents do not mention them. Anything that cares about
    /// standing in water or inside a trigger has to ask this.
    pub fn point_contents_brushes(&self, p: Vec3) -> u32 {
        let leaf_index = self.point_leaf(p);
        let Some(leaf) = self.leaves.get(leaf_index) else { return content_flags::SOLID };
        let mut out = leaf.contents;

        let first = leaf.first_leafbrush as usize;
        for i in first..first + leaf.num_leafbrushes as usize {
            let Some(&bi) = self.leafbrushes.get(i) else { continue };
            let Some(brush) = self.brushes.get(bi as usize) else { continue };
            let inside = (brush.first_side as usize..brush.first_side as usize + brush.num_sides as usize)
                .all(|s| {
                    self.brushsides
                        .get(s)
                        .and_then(|side| self.planes.get(side.plane as usize))
                        .is_some_and(|bp| bp.to_plane().distance_to(p) <= 0.0)
                });
            if inside { out |= brush.contents; }
        }
        out
    }
}

#[cfg(test)]
mod tests;
