// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "physics/movement.hpp"

#include "console/console.hpp"
#include "math/units.hpp"

#include <algorithm>
#include <cmath>

namespace kero::physics {
namespace {

using console::ConVar;
using console::VarFlags;

// Every one of these is a convar, because every one of them is something
// somebody will want to change -- for a game mode, for a debug session, or
// because they disagree. Source's mistake was never exposing too many of these.
//
// The values are the Quake lineage's, converted to the two-inch unit. Lengths
// per unit time halve; dimensionless rates do not.

ConVar sv_gravity("sv_gravity", "300",
                  "Downward acceleration, ku/s^2. 300 is the Quake value at this scale.");
ConVar sv_maxspeed("sv_maxspeed", "160", "How fast a player runs on the flat, ku/s.");
ConVar sv_friction("sv_friction", "4",
                   "Ground friction. Dimensionless, so the unit scale does not touch it.");
ConVar sv_stopspeed("sv_stopspeed", "50",
                    "Below this speed, friction is applied as though you were at it -- "
                    "which is what stops a slow walk taking forever to end.");
ConVar sv_accelerate("sv_accelerate", "10", "Ground acceleration rate.");
ConVar sv_airaccelerate("sv_airaccelerate", "10", "Air acceleration rate.");
ConVar sv_air_max_wishspeed(
    "sv_air_max_wishspeed", "15",
    "The air-speed cap, ku/s. In the air you may accelerate towards a wish "
    "direction only up to this speed along it -- so you can always steer, but "
    "never add speed in the direction you are already going. This is what makes "
    "bunny-hopping and surfing work. It looks like a bug and is not: removing "
    "it changes what the game is.");
ConVar sv_jump_impulse("sv_jump_impulse", "150",
                       "Upward speed a jump adds, ku/s. Roughly 40 ku of height at the "
                       "default gravity, which clears a 36 ku player.");
ConVar sv_stepsize("sv_stepsize", "9",
                   "The tallest ledge walked up without jumping, ku. Half a stair riser "
                   "over the player's knee.");
ConVar sv_maxvelocity("sv_maxvelocity", "1750",
                      "Speed clamp, ku/s. A backstop against a physics bug launching "
                      "someone out of the world, not a gameplay limit.");
ConVar sv_ground_normal(
    "sv_ground_normal", "0.7",
    "How steep a surface can be and still count as ground, as the cosine of its "
    "angle from vertical. 0.7 is about 45 degrees; steeper than this and you "
    "slide, which is the other half of what makes surfing possible.");

/// How many times a slide will bounce off a surface before giving up.
///
/// Four is enough for a floor, a wall and the crease between two walls, with
/// one spare. More would let a mover thread a corner it should be stopped by.
constexpr i32 kMaxBumps = 4;

/// How many distinct planes are tracked while sliding.
constexpr usize kMaxPlanes = 5;

/// How far down to look for the ground.
constexpr f32 kGroundProbe = 2.0f;

/// Velocity components smaller than this are snapped to zero, so a mover
/// resting on a floor does not jitter on the last bit of a float.
constexpr f32 kStopEpsilon = 0.1f;

Vec3 wish_direction(const MoveInput& input, Vec3& out_forward, Vec3& out_right) {
    // Only the yaw is used. Looking at the floor must not walk you into it, and
    // looking at the sky must not lift you -- the movement frame is horizontal
    // whatever the view is doing.
    const Angles flat(0.0f, input.view.yaw, 0.0f);
    math::angle_vectors(flat, &out_forward, &out_right, nullptr);

    Vec3 wish = out_forward * input.forward + out_right * input.side;
    wish.z = 0.0f;
    return wish;
}

}  // namespace

Aabb Mover::hull(bool ducking) {
    const f32 half = units::kPlayerWidth * 0.5f;
    const f32 height = ducking ? units::kPlayerDuckHeight : units::kPlayerHeight;
    return Aabb(Vec3(-half, -half, 0.0f), Vec3(half, half, height));
}

Vec3 Mover::clip_velocity(const Vec3& velocity, const Vec3& normal, f32 overbounce) {
    const f32 backoff = dot(velocity, normal) * overbounce;
    Vec3 out = velocity - normal * backoff;

    // Snap the remainder to zero on each axis. Without this a mover standing on
    // a floor keeps a few thousandths of a unit of downward velocity, and the
    // next tick's ground test can go either way.
    for (usize axis = 0; axis < 3; ++axis) {
        if (out[axis] > -kStopEpsilon && out[axis] < kStopEpsilon) {
            out[axis] = 0.0f;
        }
    }
    return out;
}

void Mover::categorise_position(const Level& level, MoveState& state) const {
    // Moving up fast enough means you are not standing on anything, whatever is
    // beneath you. Without this, the tick after a jump finds the floor still
    // within the probe distance and cancels the jump.
    if (state.velocity.z > sv_jump_impulse.number() * 0.5f) {
        state.on_ground = false;
        return;
    }

    const Vec3 down = state.origin - Vec3(0.0f, 0.0f, kGroundProbe);
    const bsp::Trace trace =
        level.trace(state.origin, down, hull(state.ducking), bsp::Contents::SolidMask);

    if (!trace.hit() || trace.plane.normal.z < sv_ground_normal.number()) {
        // Nothing under us, or what is under us is too steep to stand on. The
        // second case is what lets a player slide down -- and along -- a ramp.
        state.on_ground = false;
        return;
    }

    state.on_ground = true;
    state.ground_normal = trace.plane.normal;
    // Settle onto the surface rather than hovering the probe distance above it.
    state.origin = trace.end;
    if (state.velocity.z < 0.0f) {
        state.velocity.z = 0.0f;
    }
}

void Mover::apply_friction(MoveState& state, f32 dt) const {
    const f32 speed = state.velocity.length();
    if (speed < 0.1f) {
        state.velocity = Vec3{};
        return;
    }
    if (!state.on_ground) {
        return;  // No air friction: the air cap is the only air control there is.
    }

    // Below the stop speed, friction is applied as though you were *at* it. That
    // makes the last of a walk decay linearly instead of exponentially, so
    // coming to a halt takes a fixed short time rather than an asymptote.
    const f32 control = std::max(speed, sv_stopspeed.number());
    const f32 drop = control * sv_friction.number() * dt;

    const f32 scale = std::max(speed - drop, 0.0f) / speed;
    state.velocity *= scale;
}

void Mover::accelerate(MoveState& state, const Vec3& direction, f32 wish_speed,
                       f32 acceleration, f32 dt) {
    // How much of the wish speed is already accounted for by the current
    // velocity along that direction.
    const f32 current = dot(state.velocity, direction);
    const f32 missing = wish_speed - current;
    if (missing <= 0.0f) {
        return;
    }

    const f32 added = std::min(acceleration * wish_speed * dt, missing);
    state.velocity += direction * added;
}

void Mover::air_accelerate(MoveState& state, const Vec3& direction, f32 wish_speed,
                           f32 acceleration, f32 dt) {
    // The whole of air control, and the whole of bunny-hopping and surfing,
    // is this clamp.
    //
    // The wish speed used for the "how much am I already going this way"
    // comparison is capped, but the acceleration applied is *not* scaled down
    // to match. So a player already moving at 300 ku/s can still gain speed --
    // as long as they aim nearly perpendicular to their motion, where the
    // projection onto the wish direction is small enough to be under the cap.
    // Aim straight ahead and the cap denies you; aim across and it does not.
    // Turning while airborne therefore adds velocity, which is the mechanic.
    const f32 capped = std::min(wish_speed, sv_air_max_wishspeed.number());

    const f32 current = dot(state.velocity, direction);
    const f32 missing = capped - current;
    if (missing <= 0.0f) {
        return;
    }

    const f32 added = std::min(acceleration * wish_speed * dt, missing);
    state.velocity += direction * added;
}

bool Mover::slide_move(const Level& level, MoveState& state, f32 dt, bool allow_step,
                       MoveResult& result) const {
    (void)allow_step;

    const Vec3 original_velocity = state.velocity;
    Vec3 planes[kMaxPlanes];
    usize plane_count = 0;
    f32 time_left = dt;
    bool blocked = false;

    for (i32 bump = 0; bump < kMaxBumps; ++bump) {
        if (state.velocity.is_zero(1e-4f)) {
            break;
        }

        const Vec3 end = state.origin + state.velocity * time_left;
        const bsp::Trace trace = level.trace(state.origin, end, hull(state.ducking),
                                             bsp::Contents::SolidMask);

        if (trace.all_solid) {
            // Nowhere to go at all. Stopping dead is the honest answer; the
            // caller sees `wedged` and can decide to unstick.
            state.velocity = Vec3{};
            result.wedged = true;
            return false;
        }

        if (trace.fraction > 0.0f) {
            state.origin = trace.end;
            // Progress was made, so the planes recorded before no longer
            // constrain us.
            plane_count = 0;
        }
        if (trace.fraction == 1.0f) {
            return !blocked;
        }

        blocked = true;
        time_left -= time_left * trace.fraction;

        if (plane_count >= kMaxPlanes) {
            state.velocity = Vec3{};
            result.wedged = true;
            return false;
        }
        planes[plane_count++] = trace.plane.normal;

        // Find a velocity that slides along every plane hit so far.
        bool found = false;
        for (usize i = 0; i < plane_count; ++i) {
            Vec3 candidate = clip_velocity(original_velocity, planes[i], 1.0f);

            bool into_another = false;
            for (usize j = 0; j < plane_count; ++j) {
                if (j != i && dot(candidate, planes[j]) < 0.0f) {
                    into_another = true;
                    break;
                }
            }
            if (!into_another) {
                state.velocity = candidate;
                found = true;
                break;
            }
        }

        if (found) {
            continue;
        }

        if (plane_count == 2) {
            // A crease between two surfaces: the only direction left is along
            // the line where they meet.
            Vec3 along = cross(planes[0], planes[1]);
            if (along.normalize() < 1e-4f) {
                state.velocity = Vec3{};
                result.wedged = true;
                return false;
            }
            state.velocity = along * dot(along, original_velocity);
            continue;
        }

        // Three or more planes with no direction satisfying all of them: a
        // corner. Stop rather than squeeze through it.
        state.velocity = Vec3{};
        result.wedged = true;
        return false;
    }

    return !blocked;
}

bool Mover::step_move(const Level& level, MoveState& state, f32 dt,
                      MoveResult& result) const {
    const MoveState before = state;

    // First, the plain slide, and remember where it got to.
    MoveResult flat_result;
    MoveState flat = state;
    (void)slide_move(level, flat, dt, false, flat_result);

    // Then the same move lifted over the step: up, along, and back down.
    MoveState stepped = before;
    const Aabb box = hull(stepped.ducking);
    const f32 step = sv_stepsize.number();

    const bsp::Trace up = level.trace(stepped.origin, stepped.origin + Vec3(0, 0, step),
                                      box, bsp::Contents::SolidMask);
    if (up.start_solid) {
        state = flat;
        return flat_result.wedged;
    }
    stepped.origin = up.end;

    MoveResult stepped_result;
    (void)slide_move(level, stepped, dt, false, stepped_result);

    const bsp::Trace down = level.trace(stepped.origin, stepped.origin - Vec3(0, 0, step),
                                        box, bsp::Contents::SolidMask);
    if (!down.start_solid) {
        stepped.origin = down.end;
    }
    // Landing on something too steep to stand on is not a step, it is a ramp
    // being climbed by accident.
    const bool landed_flat =
        !down.hit() || down.plane.normal.z >= sv_ground_normal.number();

    // Take whichever went further horizontally. Comparing the distance actually
    // travelled, rather than trying to predict which case applies, is what makes
    // this work on stairs, on ledges and on the join between them.
    const Vec3 flat_delta = flat.origin - before.origin;
    const Vec3 step_delta = stepped.origin - before.origin;
    const f32 flat_distance = flat_delta.x * flat_delta.x + flat_delta.y * flat_delta.y;
    const f32 step_distance = step_delta.x * step_delta.x + step_delta.y * step_delta.y;

    if (landed_flat && step_distance > flat_distance) {
        state = stepped;
        // The vertical velocity from the lift is not real motion.
        state.velocity.z = flat.velocity.z;
        result.stepped_up = true;
        return stepped_result.wedged;
    }

    state = flat;
    return flat_result.wedged;
}

MoveResult Mover::move(const Level& level, MoveState& state, const MoveInput& input,
                       f32 dt) const {
    MoveResult result;
    if (dt <= 0.0f) {
        return result;
    }

    const bool was_on_ground = state.on_ground;
    f32 falling_speed = state.velocity.z;

    categorise_position(level, state);
    if (state.on_ground && !was_on_ground) {
        // Landed before moving at all -- the fall finished last tick.
        falling_speed = state.velocity.z;
    }

    state.ducking = input.duck;

    // Gravity in two halves, before and after the move. Integrating it all at
    // one end makes a jump's height depend on the tick rate; splitting it makes
    // the trajectory correct to second order, so a 66 Hz server and a 128 Hz one
    // agree about how high you jumped.
    if (!state.on_ground) {
        state.velocity.z -= sv_gravity.number() * 0.5f * dt;
    }

    if (input.jump && state.on_ground && !state.jump_held) {
        state.velocity.z = sv_jump_impulse.number();
        state.on_ground = false;
    }
    state.jump_held = input.jump;

    apply_friction(state, dt);

    Vec3 forward;
    Vec3 right;
    Vec3 wish = wish_direction(input, forward, right);
    f32 wish_speed = wish.normalize();
    wish_speed = std::min(wish_speed, 1.0f) * sv_maxspeed.number();
    if (state.ducking) {
        // Ducking is slower, which is the only reason anyone would stand up.
        wish_speed *= 0.34f;
    }

    if (state.on_ground) {
        // On a slope, the wish direction follows the surface rather than the
        // horizontal, so walking up a ramp is not fighting gravity every tick.
        wish = clip_velocity(wish, state.ground_normal, 1.0f);
        wish.normalize();
        accelerate(state, wish, wish_speed, sv_accelerate.number(), dt);
        state.velocity.z = std::min(state.velocity.z, 0.0f);
    } else {
        air_accelerate(state, wish, wish_speed, sv_airaccelerate.number(), dt);
    }

    const f32 limit = sv_maxvelocity.number();
    for (usize axis = 0; axis < 3; ++axis) {
        state.velocity[axis] = std::clamp(state.velocity[axis], -limit, limit);
    }

    if (state.on_ground) {
        (void)step_move(level, state, dt, result);
    } else {
        (void)slide_move(level, state, dt, false, result);
    }

    if (!state.on_ground) {
        state.velocity.z -= sv_gravity.number() * 0.5f * dt;
    }

    const f32 speed_before_landing = state.velocity.z;
    categorise_position(level, state);

    // The transitions are reported against where the tick *started*, not
    // against the intermediate state.
    //
    // Landing is normally discovered by this second categorise -- the mover
    // falls during the move and meets the floor at the end of it. Comparing
    // against the first categorise instead means the flag is computed before
    // the landing has happened, so it never fires at all and nothing can play a
    // footstep or take fall damage.
    if (state.on_ground && !was_on_ground) {
        result.touched_ground = true;
        result.landing_speed = -std::min(falling_speed, speed_before_landing);
    }
    if (!state.on_ground && was_on_ground) {
        result.left_ground = true;
    }

    return result;
}

}  // namespace kero::physics
