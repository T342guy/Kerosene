// SPDX-License-Identifier: LGPL-3.0-or-later
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

use void_bsp::{Bsp, Trace, contents};
use void_entity::EntityWorld;
use void_math::Vec3;
use void_physics::CollisionWorld;

/// One brush model that is not part of the static world.
#[derive(Clone, Copy, Debug)]
pub struct Mover {
    pub model: usize,
    /// Where the model has moved to, relative to where it was compiled.
    pub offset: Vec3,
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

            movers.push(Mover { model, offset: entity.origin });
        }
        LevelCollision { bsp, movers }
    }

    pub fn mover_count(&self) -> usize { self.movers.len() }

    /// Trace against the world and every mover, keeping the nearest hit.
    pub fn trace(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: u32) -> Trace {
        let mut best = self.bsp.trace_box(start, end, mins, maxs, mask);

        for mover in &self.movers {
            // Trace in the model's own space by subtracting where it has moved
            // to, then move the answer back.
            let hit = self.bsp.trace_model(
                mover.model,
                start - mover.offset,
                end - mover.offset,
                mins,
                maxs,
                mask,
            );
            if hit.start_solid { best.start_solid = true; }
            if hit.fraction < best.fraction {
                best.fraction = hit.fraction;
                best.plane = hit.plane;
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
        // moving water brush still reads as water.
        for mover in &self.movers {
            let local = point - mover.offset;
            let Some(model) = self.bsp.models.get(mover.model) else { continue };
            if model.bounds().contains_point(local) {
                let trace = self.bsp.trace_model(
                    mover.model,
                    local,
                    local,
                    Vec3::ZERO,
                    Vec3::ZERO,
                    contents::MASK_PLAYER_SOLID | contents::MASK_WATER,
                );
                if trace.start_solid { out |= trace.contents; }
            }
        }
        out
    }
}
