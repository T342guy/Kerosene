// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Traces for a game: what a line or a box meets, entities included.
//!
//! [`LevelCollision`](crate::LevelCollision) answers for the movement
//! solver, which only needs to know where to stop: the world and the brush
//! entities moving through it. A game asks a different question -- *what*
//! did the shot hit, what is the player looking at, is anything between
//! the guard and the player -- and the answer has to include the props, and
//! say which entity it was.

use crate::engine::Engine;
use kerosene_bsp::contents;
use kerosene_entity::EntityId;
use kerosene_math::Vec3;

/// What a trace met.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TraceHit {
    /// Where the trace stopped: the point on the surface for a ray, the
    /// box's centre for a box.
    pub pos: Vec3,
    /// The surface's normal, facing back along the trace.
    pub normal: Vec3,
    /// How far along the trace, from 0 at the start to 1 at the end.
    pub fraction: f32,
    /// How far along the trace, in units.
    pub distance: f32,
    /// The entity it hit: a door, a prop, anything with a body. `None` for
    /// the world.
    pub entity: Option<EntityId>,
}

impl Engine {
    /// Sweep a box from `start` to `end` and say what it hit first: the
    /// world, a brush entity (a door, a platform), or a prop's body. `None`
    /// when nothing was in the way, or no map is loaded.
    ///
    /// `mins`/`maxs` are the box around the trace's point; zero for a ray.
    /// `mask` says which contents stop it -- `contents::MASK_SOLID` for what
    /// a bullet stops at. Props stop every trace.
    pub fn trace(
        &self,
        start: Vec3,
        end: Vec3,
        mins: Vec3,
        maxs: Vec3,
        mask: u32,
    ) -> Option<TraceHit> {
        let level = self.level.as_ref()?;
        let length = (end - start).length();
        let world = crate::LevelCollision::new(&level.bsp, &self.entities);
        let t = world.trace(start, end, mins, maxs, mask);

        let mut hit = (t.fraction < 1.0).then(|| TraceHit {
            pos: t.endpos,
            normal: t
                .plane
                .map_or(-(end - start).normalize_or_zero(), |p| p.normal),
            fraction: t.fraction,
            distance: t.fraction * length,
            // A brush model other than the world is an entity's.
            entity: (t.model != 0)
                .then(|| {
                    self.entities
                        .iter()
                        .find(|e| e.brush_model == Some(t.model))
                        .map(|e| e.id)
                })
                .flatten(),
        });

        if let Some((fraction, id, normal)) = self.physics.trace_bodies(start, end, mins, maxs)
            && hit.is_none_or(|h| fraction < h.fraction)
        {
            hit = Some(TraceHit {
                pos: start + (end - start) * fraction,
                normal,
                fraction,
                distance: fraction * length,
                entity: Some(id),
            });
        }
        hit
    }

    /// A ray from `start` to `end` against what a bullet stops at.
    pub fn trace_ray(&self, start: Vec3, end: Vec3) -> Option<TraceHit> {
        self.trace(start, end, Vec3::ZERO, Vec3::ZERO, contents::MASK_SOLID)
    }

    /// A ray from the player's eye along their view, turned by
    /// `(yaw, pitch)` degrees and `range` long: what a shot, the use key or
    /// a `decal` command meets.
    pub fn trace_view(&self, offset: (f32, f32), range: f32) -> Option<TraceHit> {
        let mut angles = self.player.view_angles;
        angles.yaw += offset.0;
        angles.pitch += offset.1;
        let eye = self.player.movement.eye_position();
        self.trace_ray(eye, eye + angles.forward() * range)
    }
}

#[cfg(test)]
mod tests;
