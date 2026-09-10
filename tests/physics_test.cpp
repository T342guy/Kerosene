// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "console/console.hpp"
#include "math/units.hpp"
#include "physics/movement.hpp"

#include <string>

using namespace kero;
using namespace kero::physics;

namespace {

const Level& sample() {
    static const Level level = [] {
        auto loaded = Level::load(std::string(KEROSENE_SOURCE_DIR) +
                                  "/content/maps/kero_start.kbsp");
        if (!loaded) {
            FAIL("could not load the compiled sample level: ", loaded.error());
            return Level{};
        }
        return std::move(*loaded);
    }();
    return level;
}

/// One server tick. 66 Hz, which is what the engine runs at.
constexpr f32 kTick = 1.0f / 66.0f;

/// Somewhere in room A with a clear run along +Y and no ceiling in the way.
///
/// Room A's interior is 256 ku square and 128 ku tall, and the func_detail
/// pillar occupies 96..160 on both horizontal axes. A player is 36 ku tall, so
/// a start height above 92 puts their head in the ceiling -- which is a
/// perfectly correct start-solid, and measures nothing.
constexpr Vec3 kOpenFloor{64, 32, 8};
constexpr Vec3 kOpenAir{460, 32, 64};  // Room B, clear in every direction.

/// Facing +Y: 224 ku of clear room from kOpenFloor, which is more than a
/// second at the run speed.
const math::Angles kAlongRoom(0, 90, 0);

MoveState standing_at(Vec3 origin) {
    MoveState state;
    state.origin = origin;
    return state;
}

/// Runs `ticks` ticks and returns the final state.
MoveState simulate(MoveState state, const MoveInput& input, i32 ticks,
                   MoveResult* last = nullptr) {
    const Mover mover;
    for (i32 i = 0; i < ticks; ++i) {
        const MoveResult result = mover.move(sample(), state, input, kTick);
        if (last != nullptr) {
            *last = result;
        }
    }
    return state;
}

f32 horizontal_speed(const Vec3& velocity) {
    return std::sqrt(velocity.x * velocity.x + velocity.y * velocity.y);
}

}  // namespace

TEST_CASE("a mover dropped in a room lands on the floor and stays there") {
    MoveState state = standing_at(Vec3(64, 32, 64));

    MoveResult last;
    state = simulate(state, MoveInput{}, 120, &last);

    CHECK(state.on_ground);
    // Resting a hair above the floor, by design: sweeps stop just short of
    // what they hit.
    CHECK(state.origin.z >= 0.0f);
    CHECK(state.origin.z < 0.5f);
    CHECK(std::abs(state.velocity.z) < 1.0f);
}

TEST_CASE("standing still stays still") {
    MoveState state = standing_at(kOpenFloor);
    state = simulate(state, MoveInput{}, 60);

    const Vec3 settled = state.origin;
    state = simulate(state, MoveInput{}, 60);

    CHECK(state.origin.x == doctest::Approx(settled.x));
    CHECK(state.origin.y == doctest::Approx(settled.y));
    CHECK(state.origin.z == doctest::Approx(settled.z));
}

TEST_CASE("walking reaches the run speed and no more") {
    MoveState state = standing_at(kOpenFloor);
    state = simulate(state, MoveInput{}, 30);  // Settle onto the floor.

    MoveInput input;
    input.forward = 1.0f;
    input.view = kAlongRoom;

    state = simulate(state, input, 60);

    CHECK(state.on_ground);
    // sv_maxspeed is 160 ku/s, which is 160 * 2 inches, about 8 m/s.
    CHECK(horizontal_speed(state.velocity) ==
          doctest::Approx(units::kPlayerSpeed).epsilon(0.02));
    CHECK(state.origin.y > kOpenFloor.y);
}

TEST_CASE("diagonal movement is not faster than straight") {
    // The classic bug this rules out: normalising the wish direction *after*
    // scaling by max speed lets forward+strafe reach 1.41x the run speed.
    //
    // Kept short and started in the corner of the room, so neither run reaches
    // the func_detail pillar. Clipping off a wall and re-accelerating is a real
    // way to exceed the run speed and would mask the thing being measured. The
    // two runs are compared with each other, not with the maximum, so they need
    // not have converged.
    const Vec3 corner{32, 32, 8};
    MoveState straight = standing_at(corner);
    straight = simulate(straight, MoveInput{}, 30);
    MoveInput forward_only;
    forward_only.forward = 1.0f;
    forward_only.view = kAlongRoom;
    straight = simulate(straight, forward_only, 20);

    MoveState diagonal = standing_at(corner);
    diagonal = simulate(diagonal, MoveInput{}, 30);
    MoveInput both;
    both.forward = 1.0f;
    both.side = 1.0f;
    both.view = kAlongRoom;
    diagonal = simulate(diagonal, both, 20);

    CHECK(horizontal_speed(diagonal.velocity) ==
          doctest::Approx(horizontal_speed(straight.velocity)).epsilon(0.02));
}

TEST_CASE("letting go comes to a stop, and quickly") {
    MoveState state = standing_at(kOpenFloor);
    state = simulate(state, MoveInput{}, 30);

    MoveInput input;
    input.forward = 1.0f;
    input.view = kAlongRoom;
    state = simulate(state, input, 60);
    REQUIRE(horizontal_speed(state.velocity) > 100.0f);

    // Under a second: the stop-speed term makes the tail of a walk decay
    // linearly rather than asymptotically.
    state = simulate(state, MoveInput{}, 60);
    CHECK(horizontal_speed(state.velocity) < 1.0f);
}

TEST_CASE("a wall stops a walk without the mover entering it") {
    const Level& level = sample();
    MoveState state = standing_at(kOpenFloor);
    state = simulate(state, MoveInput{}, 30);

    MoveInput input;
    input.forward = 1.0f;
    input.view = math::Angles(0, 180, 0);  // Straight at the -X wall.

    state = simulate(state, input, 240);

    // Room A's -X wall is at x = 0 and the player is 16 ku across.
    CHECK(state.origin.x >= 7.5f);
    CHECK(state.origin.x < 10.0f);
    CHECK_FALSE(any(level.contents_at(state.origin + Vec3(0, 0, 4)) & bsp::Contents::Solid));
}

TEST_CASE("walking into a wall at an angle slides along it") {
    MoveState state = standing_at(kOpenFloor);
    state = simulate(state, MoveInput{}, 30);

    MoveInput input;
    input.forward = 1.0f;
    input.view = math::Angles(0, 150, 0);  // Into the -X wall, but angled +Y.

    const f32 start_y = state.origin.y;
    state = simulate(state, input, 180);

    // Stopped by the wall in X...
    CHECK(state.origin.x < 12.0f);
    // ...but still travelling along it, which is what sliding means.
    CHECK(state.origin.y > start_y + 32.0f);
}

TEST_CASE("a jump leaves the ground and comes back") {
    MoveState state = standing_at(kOpenFloor);
    state = simulate(state, MoveInput{}, 30);
    REQUIRE(state.on_ground);

    MoveInput jump;
    jump.jump = true;

    const Mover mover;
    MoveResult first = mover.move(sample(), state, jump, kTick);
    CHECK(first.left_ground);
    CHECK_FALSE(state.on_ground);
    CHECK(state.velocity.z > 0.0f);

    f32 highest = state.origin.z;
    bool landed = false;
    for (i32 i = 0; i < 200 && !landed; ++i) {
        const MoveResult result = mover.move(sample(), state, MoveInput{}, kTick);
        highest = std::max(highest, state.origin.z);
        landed = result.touched_ground;
    }

    CHECK(landed);
    // High enough to clear a 36 ku player's own height would be silly; high
    // enough to matter is the point. At 150 ku/s into 300 ku/s^2 that is 37 ku.
    CHECK(highest > 24.0f);
    CHECK(highest < 64.0f);
    CHECK(state.origin.z == doctest::Approx(0.0f).epsilon(0.1));
}

TEST_CASE("holding jump does not auto-bounce") {
    MoveState state = standing_at(kOpenFloor);
    state = simulate(state, MoveInput{}, 30);

    MoveInput held;
    held.jump = true;

    const Mover mover;
    (void)mover.move(sample(), state, held, kTick);
    REQUIRE_FALSE(state.on_ground);

    // Land with the key still down.
    i32 ticks = 0;
    while (!state.on_ground && ticks < 300) {
        (void)mover.move(sample(), state, held, kTick);
        ++ticks;
    }
    REQUIRE(state.on_ground);

    // Still holding: it must not jump again on its own. Each hop is a press.
    for (i32 i = 0; i < 20; ++i) {
        (void)mover.move(sample(), state, held, kTick);
        CHECK(state.on_ground);
    }
}

TEST_CASE("the 8 ku step is walked up without jumping") {
    MoveState state = standing_at(Vec3(490, 128, 8));
    state = simulate(state, MoveInput{}, 40);
    REQUIRE(state.on_ground);
    const f32 floor_height = state.origin.z;

    MoveInput input;
    input.forward = 1.0f;  // +X, towards the step at x = 512.

    // Long enough to reach the step at x = 512 and get well onto it, short
    // enough not to run off its far edge at x = 640.
    MoveResult last;
    state = simulate(state, input, 30, &last);

    // Up on top of it, without ever leaving the ground.
    CHECK(state.origin.x > 512.0f);
    CHECK(state.origin.z > floor_height + 7.0f);
    CHECK(state.origin.z < floor_height + 9.0f);
    CHECK(state.on_ground);
    CHECK(8.0f < units::kStepHeight);
}

TEST_CASE("the air-speed cap allows steering but not straight-line acceleration") {
    // The mechanic, stated as a test. In the air, aiming where you are already
    // going adds nothing; aiming across your motion adds velocity. That
    // asymmetry is bunny-hopping.
    const Mover mover;

    MoveState forward_state;
    forward_state.origin = kOpenAir;
    forward_state.velocity = Vec3(300, 0, 0);
    forward_state.on_ground = false;

    MoveState across_state = forward_state;

    MoveInput straight_ahead;
    straight_ahead.forward = 1.0f;
    straight_ahead.view = math::Angles(0, 0, 0);  // Along +X, the way we are going.

    MoveInput sideways;
    sideways.forward = 1.0f;
    sideways.view = math::Angles(0, 88, 0);  // Nearly perpendicular.

    const f32 before = horizontal_speed(forward_state.velocity);
    for (i32 i = 0; i < 6; ++i) {
        (void)mover.move(sample(), forward_state, straight_ahead, kTick);
        (void)mover.move(sample(), across_state, sideways, kTick);
    }

    // Aiming along the motion: the cap denies it, so no speed is gained.
    CHECK(horizontal_speed(forward_state.velocity) <= doctest::Approx(before).epsilon(0.001));
    // Aiming across it: speed goes up. Remove the cap and both would.
    CHECK(horizontal_speed(across_state.velocity) > before);
}

TEST_CASE("air control cannot be used to hover") {
    const Mover mover;
    MoveState state;
    state.origin = kOpenAir;
    state.on_ground = false;

    MoveInput input;
    input.forward = 1.0f;

    const f32 start = state.origin.z;
    for (i32 i = 0; i < 30; ++i) {
        (void)mover.move(sample(), state, input, kTick);
    }
    CHECK(state.origin.z < start);
    CHECK(state.velocity.z < 0.0f);
}

TEST_CASE("ducking is slower and shorter") {
    MoveState standing = standing_at(kOpenFloor);
    standing = simulate(standing, MoveInput{}, 30);

    MoveInput walk;
    walk.forward = 1.0f;
    walk.view = kAlongRoom;
    standing = simulate(standing, walk, 60);

    MoveState ducked = standing_at(kOpenFloor);
    ducked = simulate(ducked, MoveInput{}, 30);
    MoveInput crouch_walk = walk;
    crouch_walk.duck = true;
    ducked = simulate(ducked, crouch_walk, 60);

    CHECK(horizontal_speed(ducked.velocity) < horizontal_speed(standing.velocity));
    CHECK(Mover::hull(true).maxs.z < Mover::hull(false).maxs.z);
}

TEST_CASE("movement follows the view's yaw and ignores its pitch") {
    MoveState looking_down = standing_at(kOpenFloor);
    looking_down = simulate(looking_down, MoveInput{}, 30);

    MoveInput input;
    input.forward = 1.0f;
    input.view = math::Angles(80, 90, 0);  // Nearly straight at the floor.
    looking_down = simulate(looking_down, input, 45);

    MoveState looking_flat = standing_at(kOpenFloor);
    looking_flat = simulate(looking_flat, MoveInput{}, 30);
    MoveInput flat = input;
    flat.view = kAlongRoom;
    looking_flat = simulate(looking_flat, flat, 45);

    // Looking at the floor must not walk you into it, and must not slow you
    // down either.
    CHECK(looking_down.origin.y == doctest::Approx(looking_flat.origin.y).epsilon(0.02));
    CHECK(looking_down.on_ground);
}

TEST_CASE("the movement constants are convars, and changing one changes the movement") {
    console::ConVar* gravity = console::find_var("sv_gravity");
    REQUIRE(gravity != nullptr);
    const std::string original(gravity->string());

    MoveState normal = standing_at(Vec3(64, 32, 80));
    normal = simulate(normal, MoveInput{}, 20);

    (void)gravity->set("50");
    MoveState floaty = standing_at(Vec3(64, 32, 80));
    floaty = simulate(floaty, MoveInput{}, 20);
    (void)gravity->set(original);

    CHECK(floaty.origin.z > normal.origin.z);

    // And every one of them is documented where it is declared.
    CHECK_FALSE(gravity->help().empty());
    CHECK_FALSE(console::find_var("sv_air_max_wishspeed")->help().empty());
}

TEST_CASE("a mover started inside a wall reports being stuck rather than escaping") {
    const Mover mover;
    MoveState state;
    state.origin = Vec3(-8, 128, 32);  // Inside room A's -X wall.

    MoveInput input;
    input.forward = 1.0f;

    const MoveResult result = mover.move(sample(), state, input, kTick);

    // What it must not do is quietly pass through the world into the room
    // beyond. Either it reports being wedged, or it has barely moved -- a
    // player pushed into a wall by a door has to be told, not teleported.
    const f32 travelled = (state.origin - Vec3(-8, 128, 32)).length();
    CHECK((result.wedged || travelled < 4.0f));
}
