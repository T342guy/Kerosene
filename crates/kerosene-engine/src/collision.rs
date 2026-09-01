// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Collision against the world *and* the things moving through it.
//!
//! The world model is static, so tracing against it is a tree walk. Brush
//! entities -- doors, platforms, switchable walls -- are not: they are
//! separate models that move, which is precisely why the compiler keeps them
//! out of the world tree. Nothing re-splits the BSP when a door opens.
//!
//! The cost is that they have to be traced separately and the nearest hit
//! taken. Each is a handful of brushes, so this is cheap, and it is the reason
//! a door can move at all.

use kerosene_bsp::{Bsp, Trace, contents};
use kerosene_entity::EntityWorld;
use kerosene_math::{Pose, Vec3};
use kerosene_physics::CollisionWorld;

/// One brush model that is not part of the static world.
#[derive(Clone, Copy, Debug)]
pub struct Mover {
    pub model: usize,
    /// Where the model has got to, relative to where it was compiled.
    pub pose: Pose,
}

/// The world model plus every solid brush entity currently in it.
pub struct LevelCollision<'a> {
    bsp: &'a Bsp,
    movers: Vec<Mover>,
}

impl<'a> LevelCollision<'a> {
    /// Gather the solid brush entities from an entity world.
    ///
    /// Triggers are excluded: they are brush models, but a player walks
    /// through them by definition. Detail brushes are excluded too -- they are
    /// already part of the world model, and tracing them again would make the
    /// player collide with the same geometry twice.
    pub fn new(bsp: &'a Bsp, entities: &EntityWorld) -> LevelCollision<'a> {
        let mut movers = Vec::new();
        for entity in entities.iter() {
            let Some(model) = entity.brush_model else { continue };
            // Model 0 is the world, already traced directly.
            if model == 0 { continue; }

            let class = entity.classname.to_lowercase();
            if class.starts_with("trigger_") { continue; }
            if class == "func_detail" || class == "func_illusionary" { continue; }
            // A disabled func_brush is not there.
            if entity.fields.bool("disabled", false) { continue; }

            movers.push(Mover {
                model,
                // The same function the renderer's poses come from, so a
                // turned door blocks exactly where it is drawn.
                pose: crate::engine::brush_pose(Some(bsp), model, entity.origin, entity.angles),
            });
        }
        LevelCollision { bsp, movers }
    }

    pub fn mover_count(&self) -> usize { self.movers.len() }

    /// Trace against the world and every mover, keeping the nearest hit.
    ///
    /// A mover is traced in its *own* space: the ray goes in through the
    /// inverse of its pose and the plane it hits comes back out through the
    /// rotation. That is what lets a brush model turn at all, since the BSP
    /// nodes it is made of were built once, at compile time, in that space.
    ///
    /// The hull stays axis aligned on the way in, which for a turned model is
    /// an approximation -- a box rotated into local space is not a box. It is
    /// the same approximation Source makes, and it is the right one: the
    /// alternative is a swept-OBB test costing far more than the error, and
    /// the error is bounded by how far a 32-unit hull's corners move.
    pub fn trace(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: u32) -> Trace {
        let mut best = self.bsp.trace_box(start, end, mins, maxs, mask);

        for mover in &self.movers {
            let hit = self.bsp.trace_model(
                mover.model,
                mover.pose.to_local(start),
                mover.pose.to_local(end),
                mins,
                maxs,
                mask,
            );
            if hit.start_solid { best.start_solid = true; }
            if hit.fraction < best.fraction {
                best.fraction = hit.fraction;
                best.plane = hit.plane;
                // The plane came out of the model's space and has to be put
                // back into the world's, or the player would slide along the
                // direction the surface faced before it moved. Both halves:
                // the normal turns, and the distance shifts by how far the
                // turned normal reaches to the model's origin.
                best.plane = hit.plane.map(|plane| {
                    let normal = mover.pose.direction_to_world(plane.normal);
                    kerosene_math::Plane::new(normal, plane.dist + normal.dot(mover.pose.origin))
                });
                best.contents = hit.contents;
                best.surface_flags = hit.surface_flags;
                best.model = mover.model;
                best.endpos = start + (end - start) * hit.fraction;
            }
        }

        best
    }
}

impl CollisionWorld for LevelCollision<'_> {
    fn trace_hull(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: u32) -> Trace {
        self.trace(start, end, mins, maxs, mask)
    }

    fn contents_at(&self, point: Vec3) -> u32 {
        let mut out = self.bsp.point_contents_brushes(point);
        // A point inside a mover picks up its contents too, so standing in a
        // moving water brush still reads as water -- and so a func_ladder,
        // which is a brush entity that nothing collides with, is found at all.
        for mover in &self.movers {
            let local = mover.pose.to_local(point);
            let Some(model) = self.bsp.models.get(mover.model) else { continue };
            if model.bounds().contains_point(local) {
                let trace = self.bsp.trace_model(
                    mover.model,
                    local,
                    local,
                    Vec3::ZERO,
                    Vec3::ZERO,
                    contents::MASK_PLAYER_SOLID | contents::MASK_VOLUMES,
                );
                if trace.start_solid { out |= trace.contents; }
            }
        }
        out
    }
}
