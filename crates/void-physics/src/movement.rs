// SPDX-License-Identifier: LGPL-3.0-or-later
//! The movement solver itself.

use crate::world::CollisionWorld;
use crate::{DUCKED_HULL, MAX_STANDABLE_Z, STANDING_HULL};
use void_bsp::contents;
use void_math::{Angles, Vec3};

/// The box a player occupies, and where their eyes sit in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerHull {
    pub mins: Vec3,
    pub maxs: Vec3,
    pub view_height: f32,
}

/// How deep in water a player is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WaterLevel {
    #[default]
    Dry,
    /// Feet in water: slower, but still walking.
    Feet,
    /// Waist deep: swimming physics.
    Waist,
    /// Fully submerged: no jumping, and breath is running out.
    Eyes,
}

/// Tunable movement constants, every one a convar in a running engine.
#[derive(Clone, Copy, Debug)]
pub struct MoveParams {
    /// Units per second squared. 800 is Source's default -- about 20 m/s²,
    /// noticeably heavier than reality, which is what makes jumps feel snappy.
    pub gravity: f32,
    pub max_speed: f32,
    /// Ground acceleration, as a multiple of wish speed per second.
    pub accelerate: f32,
    pub air_accelerate: f32,
    pub friction: f32,
    /// Below this speed, friction is applied as though you were at this speed,
    /// so a slow walk still comes to a stop promptly instead of drifting.
    pub stop_speed: f32,
    /// Tallest step a player walks up without jumping. 18 is why Source
    /// staircases have 8-unit risers -- two per step, comfortably under.
    pub step_size: f32,
    /// Upward speed a jump imparts.
    pub jump_impulse: f32,
    /// The wish-speed clamp that makes air strafing work. See the crate docs.
    pub air_speed_cap: f32,
    /// Multiplier on max speed while ducked.
    pub duck_speed_scale: f32,
    /// Terminal velocity.
    pub max_velocity: f32,
    /// How fast a player moves while on a ladder.
    ///
    /// Slower than running on purpose: a ladder is a place you are committed
    /// to for a moment, and climbing one at full sprint reads as a bug.
    pub ladder_speed: f32,
}

impl Default for MoveParams {
    fn default() -> Self {
        MoveParams {
            gravity: 800.0,
            max_speed: 320.0,
            accelerate: 10.0,
            air_accelerate: 10.0,
            friction: 4.0,
            stop_speed: 100.0,
            step_size: 18.0,
            // sqrt(2 * 800 * 57): enough to reach 57 units, so a player clears
            // a 56-unit crate. Deriving it from the height rather than
            // hard-coding a speed keeps jump height right if gravity changes.
            jump_impulse: (2.0f32 * 800.0 * 57.0).sqrt(),
            air_speed_cap: 30.0,
            duck_speed_scale: 0.34,
            max_velocity: 3500.0,
            ladder_speed: 200.0,
        }
    }
}

impl MoveParams {
    /// Jump impulse that reaches `height` units under this gravity.
    pub fn jump_for_height(&self, height: f32) -> f32 {
        (2.0 * self.gravity * height).sqrt()
    }
}

/// What the player is asking to do this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveInput {
    /// Forward/back, in `[-1, 1]`.
    pub forward: f32,
    /// Right/left, in `[-1, 1]`.
    pub side: f32,
    /// Up/down, used when swimming or flying.
    pub up: f32,
    pub jump: bool,
    pub duck: bool,
    /// Where the player is looking.
    pub view_angles: Angles,
}

/// The player's physical state, carried between ticks.
#[derive(Clone, Copy, Debug)]
pub struct MoveState {
    pub origin: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    /// Normal of whatever is underfoot, for slope handling.
    pub ground_normal: Vec3,
    pub ducked: bool,
    pub water_level: WaterLevel,
    /// Whether the player is inside a ladder volume.
    pub on_ladder: bool,
    /// True while the jump key is still held from a previous jump, so holding
    /// it does not auto-bounce.
    pub jump_held: bool,
    /// Speed at the moment of landing, for fall damage.
    pub fall_speed: f32,
    pub noclip: bool,
}

impl Default for MoveState {
    fn default() -> Self {
        MoveState {
            origin: Vec3::ZERO,
            velocity: Vec3::ZERO,
            on_ground: false,
            ground_normal: Vec3::Z,
            ducked: false,
            water_level: WaterLevel::Dry,
            on_ladder: false,
            jump_held: false,
            fall_speed: 0.0,
            noclip: false,
        }
    }
}

impl MoveState {
    pub fn hull(&self) -> PlayerHull {
        if self.ducked { DUCKED_HULL } else { STANDING_HULL }
    }

    /// Eye position, for the camera and for line-of-sight checks.
    pub fn eye_position(&self) -> Vec3 {
        self.origin + Vec3::Z * self.hull().view_height
    }

    /// Horizontal speed, which is what a speedometer shows.
    pub fn ground_speed(&self) -> f32 {
        self.velocity.truncate().length()
    }
}

/// What happened during a move.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveResult {
    /// The player landed this tick, at this speed. For fall damage.
    pub landed_at_speed: Option<f32>,
    pub jumped: bool,
    /// Ran into a wall.
    pub hit_wall: bool,
    /// Stepped up onto something.
    pub stepped_up: bool,
}

/// Advance a player by one tick.
///
/// Order matters and follows Source's: categorize, then friction, then
/// acceleration, then the actual move. Running friction after acceleration
/// would make ground movement feel mushy and would break the air-strafe
/// behaviour entirely.
pub fn player_move(
    state: &mut MoveState,
    input: &MoveInput,
    params: &MoveParams,
    world: &dyn CollisionWorld,
    dt: f32,
) -> MoveResult {
    let mut result = MoveResult::default();
    if dt <= 0.0 { return result; }

    if state.noclip {
        let basis = input.view_angles.vectors();
        let wish = basis.forward * input.forward + basis.right * input.side + Vec3::Z * input.up;
        state.velocity = wish.normalize_or_zero() * params.max_speed * 4.0;
        state.origin += state.velocity * dt;
        state.on_ground = false;
        return result;
    }

    update_water_level(state, world);
    update_ladder(state, world);

    // Remember how fast the player was falling as the tick began. By the time
    // the move finishes, contact with the ground has already zeroed the
    // downward component, so asking afterwards always reports zero.
    let was_airborne = !state.on_ground;
    let entry_fall_speed = -state.velocity.z;

    categorize_position(state, world, params);

    apply_duck(state, world, input);

    // Jumping is checked before friction so that the jump takes the player's
    // full ground speed with them rather than a decelerated version of it.
    if input.jump && state.on_ground && !state.jump_held && state.water_level != WaterLevel::Eyes {
        state.velocity.z = params.jump_impulse;
        state.on_ground = false;
        state.jump_held = true;
        result.jumped = true;
    }
    if !input.jump { state.jump_held = false; }

    // A ladder replaces the whole ground-or-air decision rather than
    // modifying it: there is no gravity on a ladder, no friction worth
    // modelling, and no acceleration curve -- you move at climbing speed or
    // you do not move.
    if state.on_ladder && !state.noclip {
        ladder_move(state, input, params, world, dt);
        categorize_position(state, world, params);
        return result;
    }

    if state.on_ground {
        apply_friction(state, params, dt);
    }

    let (wish_dir, wish_speed) = wish_direction(state, input, params);

    if state.on_ground {
        accelerate(state, wish_dir, wish_speed, params.accelerate, params, dt);
        // On the ground, vertical velocity is the ground's business.
        state.velocity.z = 0.0;
        if state.velocity.length_squared() > 0.0 {
            let (hit, stepped) = step_move(state, world, params, dt);
            result.hit_wall = hit;
            result.stepped_up = stepped;
        }
        stay_on_ground(state, world, params);
    } else {
        air_accelerate(state, wish_dir, wish_speed, params.air_accelerate, params, dt);
        state.velocity.z -= params.gravity * dt;
        result.hit_wall = try_move(state, world, params, dt).0;
    }

    let speed = state.velocity.length();
    if speed > params.max_velocity {
        state.velocity *= params.max_velocity / speed;
    }

    categorize_position(state, world, params);

    if was_airborne && state.on_ground && entry_fall_speed > 0.0 {
        result.landed_at_speed = Some(entry_fall_speed);
        state.fall_speed = entry_fall_speed;
    }
    result
}

/// Which way the player wants to go, and how fast.
fn wish_direction(state: &MoveState, input: &MoveInput, params: &MoveParams) -> (Vec3, f32) {
    let basis = input.view_angles.vectors();
    let mut forward = basis.forward;
    let mut right = basis.right;
    // Walking is horizontal even when looking up or down; only swimming,
    // climbing and noclip follow the view into the vertical.
    if state.water_level < WaterLevel::Waist && !state.on_ladder {
        forward.z = 0.0;
        right.z = 0.0;
        forward = forward.normalize_or_zero();
        right = right.normalize_or_zero();
    }

    let wish = forward * input.forward + right * input.side;
    let mut max = params.max_speed;
    if state.ducked { max *= params.duck_speed_scale; }
    if state.water_level >= WaterLevel::Waist { max *= 0.8; }

    let length = wish.length();
    if length < 1e-6 { return (Vec3::ZERO, 0.0); }
    (wish / length, (length * max).min(max))
}

/// Slow the player down when they are on the ground and not accelerating.
pub fn apply_friction(state: &mut MoveState, params: &MoveParams, dt: f32) {
    let speed = state.velocity.length();
    if speed < 0.1 { return; }

    // The stop-speed floor: below it, friction is applied as though moving at
    // stop_speed. Without it, deceleration tapers off asymptotically and the
    // player drifts for a long time at walking pace.
    let control = speed.max(params.stop_speed);
    let drop = control * params.friction * dt;

    let new_speed = (speed - drop).max(0.0);
    state.velocity *= new_speed / speed;
}

/// Add speed toward `wish_dir`, never overshooting `wish_speed`.
///
/// The key property: this adds toward the wish direction only up to the point
/// where the *component of velocity along that direction* reaches wish speed.
/// Speed already built up in other directions is untouched, which is what lets
/// a player carry momentum through a turn.
pub fn accelerate(
    state: &mut MoveState,
    wish_dir: Vec3,
    wish_speed: f32,
    accel: f32,
    _params: &MoveParams,
    dt: f32,
) {
    if wish_speed <= 0.0 { return; }
    let current = state.velocity.dot(wish_dir);
    let add = wish_speed - current;
    if add <= 0.0 { return; }

    let accel_speed = (accel * dt * wish_speed).min(add);
    state.velocity += wish_dir * accel_speed;
}

/// Acceleration while airborne.
///
/// Identical to [`accelerate`] except that the wish speed used in the
/// "have I got enough already" test is clamped to
/// [`MoveParams::air_speed_cap`]. Because the clamp applies to the *test*
/// rather than to the result, a fast-moving player steering sideways still
/// gains the full 30 units per second, every second, without limit. This is
/// where bunny-hopping and surfing come from.
pub fn air_accelerate(
    state: &mut MoveState,
    wish_dir: Vec3,
    wish_speed: f32,
    accel: f32,
    params: &MoveParams,
    dt: f32,
) {
    if wish_speed <= 0.0 { return; }
    let capped = wish_speed.min(params.air_speed_cap);

    let current = state.velocity.dot(wish_dir);
    let add = capped - current;
    if add <= 0.0 { return; }

    // Note the uncapped `wish_speed` here: the *rate* scales with how hard
    // the player is pushing, while the *ceiling* stays at the cap.
    let accel_speed = (accel * wish_speed * dt).min(add);
    state.velocity += wish_dir * accel_speed;
}

/// Remove the component of `velocity` heading into a surface.
///
/// `overbounce` above 1 makes the player bounce off; exactly 1 slides along.
/// The second pass matters: floating point can leave a hair of velocity still
/// heading into the plane, and over many ticks that hair is enough to sink
/// through a wall.
pub fn clip_velocity(velocity: Vec3, normal: Vec3, overbounce: f32) -> Vec3 {
    let backoff = velocity.dot(normal) * overbounce;
    let mut out = velocity - normal * backoff;
    let adjust = out.dot(normal);
    if adjust < 0.0 { out -= normal * adjust; }
    out
}

/// Attempts before giving up on a move.
const MAX_BUMPS: usize = 4;
/// Planes remembered while solving a corner.
const MAX_CLIP_PLANES: usize = 5;

/// Move by the current velocity, sliding along whatever is in the way.
///
/// Returns `(hit_something, fraction_of_the_move_completed)`.
pub fn try_move(
    state: &mut MoveState,
    world: &dyn CollisionWorld,
    _params: &MoveParams,
    dt: f32,
) -> (bool, f32) {
    let hull = state.hull();
    let primal_velocity = state.velocity;
    let mut original_velocity = state.velocity;
    let mut planes: Vec<Vec3> = Vec::with_capacity(MAX_CLIP_PLANES);
    let mut time_left = dt;
    let mut all_fraction = 0.0f32;
    let mut blocked = false;

    for _ in 0..MAX_BUMPS {
        if state.velocity.length_squared() == 0.0 { break; }

        let end = state.origin + state.velocity * time_left;
        let trace = world.trace_hull(
            state.origin,
            end,
            hull.mins,
            hull.maxs,
            contents::MASK_PLAYER_SOLID,
        );

        all_fraction += trace.fraction;

        if trace.all_solid {
            // Buried in geometry; stop dead rather than tunnelling further in.
            state.velocity = Vec3::ZERO;
            return (true, 0.0);
        }

        if trace.fraction > 0.0 {
            state.origin = trace.endpos;
            original_velocity = state.velocity;
            planes.clear();
        }

        if trace.fraction >= 1.0 { break; }
        blocked = true;

        time_left -= time_left * trace.fraction;

        let Some(plane) = trace.plane else { break };
        if planes.len() >= MAX_CLIP_PLANES {
            state.velocity = Vec3::ZERO;
            break;
        }
        planes.push(plane.normal);

        // Find a velocity that slides along one plane without heading into
        // any of the others.
        let mut resolved = None;
        for (i, &normal) in planes.iter().enumerate() {
            let candidate = clip_velocity(original_velocity, normal, 1.0);
            if planes
                .iter()
                .enumerate()
                .all(|(j, &other)| j == i || candidate.dot(other) >= 0.0)
            {
                resolved = Some(candidate);
                break;
            }
        }

        state.velocity = match resolved {
            Some(v) => v,
            None => {
                // Wedged between planes. With exactly two, slide along their
                // crease; with more, there is nowhere to go.
                if planes.len() != 2 {
                    state.velocity = Vec3::ZERO;
                    break;
                }
                let crease = planes[0].cross(planes[1]).normalize_or_zero();
                crease * crease.dot(original_velocity)
            }
        };

        // If the solution reverses the original direction, stop. Without this
        // a player in a sharp corner oscillates back and forth every tick.
        if state.velocity.dot(primal_velocity) <= 0.0 {
            state.velocity = Vec3::ZERO;
            break;
        }
    }

    if all_fraction == 0.0 {
        state.velocity = Vec3::ZERO;
    }
    (blocked, all_fraction)
}

/// Move, trying both along the ground and stepped up, and keep whichever got
/// further.
///
/// Trying both is what makes stairs work without the player having to jump,
/// and what stops them from being launched up a wall they merely brushed:
/// the stepped-up attempt only wins if it actually travelled further
/// horizontally and landed on something standable.
///
/// Returns `(hit_something, stepped_up)`.
pub fn step_move(
    state: &mut MoveState,
    world: &dyn CollisionWorld,
    params: &MoveParams,
    dt: f32,
) -> (bool, bool) {
    let hull = state.hull();
    let start_origin = state.origin;
    let start_velocity = state.velocity;

    // Attempt one: straight along the ground.
    let (blocked, _) = try_move(state, world, params, dt);
    let down_origin = state.origin;
    let down_velocity = state.velocity;

    if !blocked {
        return (false, false);
    }

    // Attempt two: step up, move, then drop back down.
    state.origin = start_origin;
    state.velocity = start_velocity;

    let step_up = start_origin + Vec3::Z * params.step_size;
    let up_trace = world.trace_hull(
        start_origin,
        step_up,
        hull.mins,
        hull.maxs,
        contents::MASK_PLAYER_SOLID,
    );
    if !up_trace.start_solid && !up_trace.all_solid {
        state.origin = up_trace.endpos;
    }

    try_move(state, world, params, dt);

    let step_down = state.origin - Vec3::Z * params.step_size;
    let down_trace = world.trace_hull(
        state.origin,
        step_down,
        hull.mins,
        hull.maxs,
        contents::MASK_PLAYER_SOLID,
    );

    // Landing on something too steep means this was a wall, not a step.
    let landed_flat = down_trace.plane.is_none_or(|p| p.normal.z >= MAX_STANDABLE_Z);
    if !landed_flat {
        state.origin = down_origin;
        state.velocity = down_velocity;
        return (true, false);
    }
    if !down_trace.start_solid && !down_trace.all_solid {
        state.origin = down_trace.endpos;
    }
    let up_origin = state.origin;

    // Whichever attempt covered more horizontal ground wins. The comparison
    // is strict, so a tie goes to the stepped-up attempt: sliding to a stop
    // against a step covers exactly as much ground as stepping onto it, and
    // preferring the blocked result there means stairs can never be climbed.
    let down_dist = (down_origin - start_origin).truncate().length_squared();
    let up_dist = (up_origin - start_origin).truncate().length_squared();

    if down_dist > up_dist {
        state.origin = down_origin;
        state.velocity = down_velocity;
        (true, false)
    } else {
        // Keep the stepped position, but take the downward velocity: the step
        // itself must not add upward speed, or walking up stairs launches you.
        state.velocity.z = down_velocity.z;
        (true, up_origin.z > start_origin.z + 0.1)
    }
}

/// Decide whether the player is standing on something.
pub fn categorize_position(
    state: &mut MoveState,
    world: &dyn CollisionWorld,
    _params: &MoveParams,
) {
    // Moving up fast enough means they have definitively left the ground --
    // otherwise a jump would be cancelled by this check on its first tick.
    if state.velocity.z > 140.0 {
        state.on_ground = false;
        return;
    }

    let hull = state.hull();
    let below = state.origin - Vec3::Z * 2.0;
    let trace = world.trace_hull(
        state.origin,
        below,
        hull.mins,
        hull.maxs,
        contents::MASK_PLAYER_SOLID,
    );

    match trace.plane {
        Some(plane) if plane.normal.z >= MAX_STANDABLE_Z && trace.fraction < 1.0 => {
            state.on_ground = true;
            state.ground_normal = plane.normal;
            if !trace.start_solid { state.origin = trace.endpos; }
        }
        _ => {
            state.on_ground = false;
            state.ground_normal = Vec3::Z;
        }
    }
}

/// Keep a walking player glued to the ground over small dips.
///
/// Without it, walking off the lip of a shallow slope launches the player into
/// a brief unwanted hop, and the transition from ground to air movement makes
/// it feel like a stumble.
fn stay_on_ground(state: &mut MoveState, world: &dyn CollisionWorld, params: &MoveParams) {
    if !state.on_ground { return; }
    let hull = state.hull();

    let start = state.origin + Vec3::Z * 2.0;
    let end = state.origin - Vec3::Z * params.step_size;
    let trace = world.trace_hull(start, end, hull.mins, hull.maxs, contents::MASK_PLAYER_SOLID);

    if trace.fraction > 0.0
        && trace.fraction < 1.0
        && !trace.start_solid
        && trace.plane.is_some_and(|p| p.normal.z >= MAX_STANDABLE_Z)
    {
        state.origin = trace.endpos;
    }
}

/// Duck and unduck, refusing to stand up where there is no headroom.
fn apply_duck(state: &mut MoveState, world: &dyn CollisionWorld, input: &MoveInput) {
    if input.duck {
        state.ducked = true;
        return;
    }
    if !state.ducked { return; }

    // Standing up has to be checked: a player who ducked under a pipe must
    // not be able to stand inside it.
    let standing = STANDING_HULL;
    let trace = world.trace_hull(
        state.origin,
        state.origin,
        standing.mins,
        standing.maxs,
        contents::MASK_PLAYER_SOLID,
    );
    if !trace.start_solid && !trace.all_solid {
        state.ducked = false;
    }
}

/// Move while on a ladder.
///
/// The view drives all three axes, exactly as it does underwater: look up and
/// hold forward to climb, look down to descend. Holding jump climbs as well,
/// so a player who has not worked the first part out still gets to the top.
///
/// Leaving is not a special case and does not need to be. Backing away moves
/// the player out of the volume, and walking off the top puts them on the
/// floor; both fall out of ordinary movement, and a dedicated dismount would
/// be a rule to learn for no benefit.
fn ladder_move(
    state: &mut MoveState,
    input: &MoveInput,
    params: &MoveParams,
    world: &dyn CollisionWorld,
    dt: f32,
) {
    let basis = input.view_angles.vectors();
    let vertical = input.up + if input.jump { 1.0 } else { 0.0 } - if input.duck { 1.0 } else { 0.0 };
    let wish = basis.forward * input.forward + basis.right * input.side + Vec3::Z * vertical;

    // Set rather than accelerated toward. A ladder is not a surface you build
    // momentum on, and letting go of the keys should stop you where you are
    // instead of sliding you down it.
    state.velocity = wish.normalize_or_zero() * params.ladder_speed;
    state.on_ground = false;

    if state.velocity.length_squared() > 0.0 {
        try_move(state, world, params, dt);
    }
}

/// Whether the player is inside a ladder volume.
///
/// Tested at the waist rather than the feet, so that stepping off the top of
/// a ladder onto a floor lets go of it instead of leaving the player climbing
/// thin air one unit above the ground.
fn update_ladder(state: &mut MoveState, world: &dyn CollisionWorld) {
    let hull = state.hull();
    let waist = state.origin + Vec3::Z * (hull.maxs.z * 0.5);
    state.on_ladder = world.contents_at(waist) & contents::LADDER != 0;
}

fn update_water_level(state: &mut MoveState, world: &dyn CollisionWorld) {
    let hull = state.hull();
    let feet = state.origin + Vec3::Z * 2.0;
    let waist = state.origin + Vec3::Z * (hull.maxs.z * 0.5);
    let eyes = state.origin + Vec3::Z * hull.view_height;

    let is_water = |p: Vec3| world.contents_at(p) & contents::MASK_WATER != 0;

    state.water_level = if is_water(eyes) {
        WaterLevel::Eyes
    } else if is_water(waist) {
        WaterLevel::Waist
    } else if is_water(feet) {
        WaterLevel::Feet
    } else {
        WaterLevel::Dry
    };
}

impl PartialOrd for WaterLevel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WaterLevel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

#[cfg(test)]
mod tests;
