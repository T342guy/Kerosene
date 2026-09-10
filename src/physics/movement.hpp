// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "bsp/level.hpp"
#include "math/aabb.hpp"
#include "math/angles.hpp"
#include "math/vec.hpp"

namespace kero::physics {

using bsp::Level;
using math::Aabb;
using math::Angles;
using math::Vec3;

/// What the player is asking for this tick.
struct MoveInput {
    /// -1 to 1 each, before being rotated into the view's frame.
    f32 forward = 0.0f;
    f32 side = 0.0f;
    bool jump = false;
    bool duck = false;
    /// Where the player is looking. Movement is relative to the yaw only:
    /// looking down does not walk you into the floor.
    Angles view;
};

/// Everything about a mover that survives between ticks.
struct MoveState {
    Vec3 origin;      ///< At the feet.
    Vec3 velocity;
    bool on_ground = false;
    bool ducking = false;
    /// Set while the jump key is held, so holding it does not auto-bounce and
    /// each hop is a deliberate press. Releasing it clears the latch.
    bool jump_held = false;

    /// The surface being stood on, when on_ground.
    Vec3 ground_normal{0, 0, 1};
};

/// What happened, for the caller to react to -- a footstep sound, a landing
/// animation, fall damage.
struct MoveResult {
    bool touched_ground = false;   ///< Landed this tick.
    bool left_ground = false;      ///< Jumped or walked off an edge.
    bool stepped_up = false;
    f32 landing_speed = 0.0f;      ///< Downward speed at the moment of landing.
    /// Blocked with nowhere to slide. Movement code that ignores this leaves a
    /// player vibrating in a corner.
    bool wedged = false;
};

/// The player movement model.
///
/// Reproduced from the Quake lineage rather than reinvented, and the air-speed
/// cap is reproduced *deliberately*. In the air, acceleration is applied
/// against a wish-speed clamped to a small constant, so a player can always add
/// a little velocity perpendicular to their current motion but never along it.
/// That single clamp is what makes bunny-hopping and surfing possible, and it
/// looks exactly like a bug until you notice that removing it changes what the
/// game is. It is not a bug to be fixed; it is the movement model.
///
/// The constants are re-derived for the two-inch unit, not halved blindly.
/// Speeds and accelerations are lengths per unit time, so those halve; friction
/// and the acceleration coefficients are dimensionless rates, so those do not.
class Mover {
public:
    /// Advances `state` by `dt` seconds against the level.
    MoveResult move(const Level& level, MoveState& state, const MoveInput& input,
                    f32 dt) const;

    /// The collision box for a mover, relative to its origin at the feet.
    [[nodiscard]] static Aabb hull(bool ducking);

    /// Finds the ground under `state` and sets `on_ground` accordingly.
    void categorise_position(const Level& level, MoveState& state) const;

private:
    /// v -= n * (v . n), the standard slide. `overbounce` above 1 would make a
    /// surface springy; player movement wants exactly 1.
    [[nodiscard]] static Vec3 clip_velocity(const Vec3& velocity, const Vec3& normal,
                                            f32 overbounce);

    void apply_friction(MoveState& state, f32 dt) const;
    static void accelerate(MoveState& state, const Vec3& direction, f32 wish_speed,
                           f32 acceleration, f32 dt);
    static void air_accelerate(MoveState& state, const Vec3& direction, f32 wish_speed,
                               f32 acceleration, f32 dt);

    /// Sweeps along the velocity, sliding off whatever it hits, up to a few
    /// times. Returns false if it ended up wedged.
    bool slide_move(const Level& level, MoveState& state, f32 dt, bool allow_step,
                    MoveResult& result) const;

    /// Tries the same move up-over-and-down, and keeps whichever went further.
    bool step_move(const Level& level, MoveState& state, f32 dt,
                   MoveResult& result) const;
};

}  // namespace kero::physics
