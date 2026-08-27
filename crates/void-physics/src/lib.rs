// SPDX-License-Identifier: LGPL-3.0-or-later
//! Player movement and collision response.
//!
//! This is a faithful reimplementation of Source's `gamemovement`, because the
//! way a Source game *feels* is not an accident of its renderer -- it is this
//! code. The acceleration model, the air-speed cap, the stair-stepping
//! algorithm and the sliding solver together produce a specific, recognisable
//! way of moving through a level, and reproducing that means reproducing the
//! algorithm rather than approximating it.
//!
//! The pieces, in the order a tick runs them:
//!
//! 1. [`categorize_position`] -- am I standing on something?
//! 2. [`apply_friction`] -- ground friction, with a floor under it so slow
//!    movement still stops promptly.
//! 3. [`accelerate`] or [`air_accelerate`] -- add speed toward the wish
//!    direction, never exceeding what the wish speed allows.
//! 4. [`try_move`] -- slide along whatever is in the way, up to four times.
//! 5. [`step_move`] -- try the same move stepped up, and take whichever got
//!    further.
//!
//! ## Why air control works the way it does
//!
//! [`air_accelerate`] caps the *wish speed* it considers at
//! [`MoveParams::air_speed_cap`] (30 units/s), not the resulting speed. So the
//! "have I got enough speed already" test is against 30 rather than against
//! the run speed, and a player already moving at 400 can still gain speed by
//! steering sideways. That single clamp is the origin of bunny-hopping and
//! surfing, and it is preserved deliberately: it is not a bug, and removing it
//! would change the game.

mod movement;
mod world;

pub use movement::{
    MoveInput, MoveParams, MoveResult, MoveState, PlayerHull, WaterLevel, accelerate,
    air_accelerate, apply_friction, categorize_position, clip_velocity, player_move, step_move,
    try_move,
};
pub use world::{BspWorld, CollisionWorld};
#[cfg(any(test, feature = "test-world"))]
pub use world::BoxWorld;

use void_math::Vec3;

/// A surface this steep or shallower counts as ground.
///
/// `cos(45.57 degrees)`. Source's value: steeper than this and a player slides
/// off rather than standing. It is the single number that decides which ramps
/// are stairs and which are slides.
pub const MAX_STANDABLE_Z: f32 = 0.7;

/// Standing player hull, in inches: 32 wide and 72 tall.
pub const STANDING_HULL: PlayerHull = PlayerHull {
    mins: Vec3::new(-16.0, -16.0, 0.0),
    maxs: Vec3::new(16.0, 16.0, 72.0),
    view_height: 64.0,
};

/// Ducked player hull: same width, half the height.
pub const DUCKED_HULL: PlayerHull = PlayerHull {
    mins: Vec3::new(-16.0, -16.0, 0.0),
    maxs: Vec3::new(16.0, 16.0, 36.0),
    view_height: 28.0,
};
