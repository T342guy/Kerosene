// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "console/console.hpp"
#include "engine/host.hpp"
#include "game/game.hpp"
#include "math/units.hpp"

#include <string>

using namespace kero;
using namespace kero::engine;

namespace {

std::string sample_path() {
    return std::string(KEROSENE_SOURCE_DIR) + "/content/maps/kero_start.kbsp";
}

/// A host with the sample level loaded.
Host make_host() {
    Host host;
    auto loaded = host.load_map(sample_path());
    if (!loaded) {
        FAIL("could not load the sample level: ", loaded.error());
    }
    return host;
}

Command walk(f32 yaw, f32 forward = 1.0f, bool jump = false) {
    Command command;
    command.move.forward = forward;
    command.move.jump = jump;
    command.move.view = math::Angles(0.0f, yaw, 0.0f);
    return command;
}

/// Runs a fixed number of ticks with one input. This is the shape a recorded
/// demo has, and the reason the tick is fixed: the same input sequence must
/// produce the same result every time, on every machine.
void run(Host& host, const Command& command, i32 ticks) {
    for (i32 i = 0; i < ticks; ++i) {
        host.tick(command);
    }
}

}  // namespace

TEST_CASE("the host loads a level and spawns the player where the map says") {
    Host host = make_host();

    REQUIRE(host.has_map());
    CHECK(host.entities().size() == 8);
    CHECK(host.tick_count() == 0);

    // info_player_start is at (64 128 8), a little above the floor so it is
    // visibly not embedded in it. The player is dropped onto the floor rather
    // than beginning the level in mid-air.
    CHECK(host.player().origin.x == doctest::Approx(64.0f));
    CHECK(host.player().origin.y == doctest::Approx(128.0f));
    CHECK(host.player().origin.z < 1.0f);
    CHECK(host.player().on_ground);

    // And in open space, not embedded in the level.
    CHECK_FALSE(any(host.level().contents_at(host.player().origin) & bsp::Contents::Solid));
}

TEST_CASE("the view is at eye height and knows which cluster it is in") {
    Host host = make_host();
    const ViewState view = host.view();

    CHECK(view.eye.z > host.player().origin.z);
    CHECK(view.eye.z ==
          doctest::Approx(host.player().origin.z + units::kPlayerEyeHeight));
    CHECK(view.cluster >= 0);
}

TEST_CASE("the simulation is deterministic") {
    // The precondition for client prediction, for demo playback, and for this
    // test suite being worth anything. Two runs of the same inputs must agree
    // exactly, not approximately.
    Host first = make_host();
    Host second = make_host();

    const Command forward = walk(90.0f);
    run(first, forward, 120);
    run(second, forward, 120);

    CHECK(first.player().origin.x == second.player().origin.x);
    CHECK(first.player().origin.y == second.player().origin.y);
    CHECK(first.player().origin.z == second.player().origin.z);
    CHECK(first.player().velocity.y == second.player().velocity.y);
    CHECK(first.tick_count() == second.tick_count());
}

TEST_CASE("a scripted playthrough walks from one room to the other") {
    // The end-to-end check the whole milestone exists for: a compiled level,
    // loaded, walked through, with collision working the entire way.
    Host host = make_host();

    const Vec3 start = host.player().origin;
    const i32 start_cluster = host.view().cluster;

    // East along the corridor into room B. Three seconds at 160 ku/s carries
    // the player through the doorway, along the corridor and up onto the 8 ku
    // step, which sits at x 512..640.
    run(host, walk(0.0f), 200);

    CHECK(host.player().origin.x > 512.0f);
    CHECK(host.player().on_ground);
    // Up on the step, having climbed it without jumping.
    CHECK(host.player().origin.z > 7.0f);

    // Three seconds more takes it off the far side of the step and across the
    // rest of room B, to the wall.
    run(host, walk(0.0f), 200);

    CHECK(host.player().origin.x > 640.0f);
    CHECK(host.player().origin.x < 704.0f);
    CHECK(host.player().origin.z < 1.0f);
    CHECK(host.player().on_ground);
    CHECK(host.player().origin.x > start.x + 400.0f);

    // Somewhere new, and somewhere real.
    CHECK(host.view().cluster != start_cluster);
    CHECK(host.view().cluster >= 0);
    CHECK_FALSE(any(host.level().contents_at(host.player().origin) & bsp::Contents::Solid));
}

TEST_CASE("the player cannot walk out of the level") {
    Host host = make_host();

    // Straight at the -X wall for ten seconds.
    run(host, walk(180.0f), 660);

    CHECK(host.player().origin.x > 0.0f);
    CHECK_FALSE(any(host.level().contents_at(host.player().origin) & bsp::Contents::Solid));
    CHECK(host.view().cluster >= 0);
}

TEST_CASE("walking through the corridor trigger fires the relay it is wired to") {
    Host host = make_host();
    REQUIRE(host.entities().delivered() == 0);

    // The trigger volume sits in the corridor at x 296..408.
    run(host, walk(0.0f), 200);

    CHECK(host.player().origin.x > 296.0f);
    // The trigger fired, and the relay it names received the input.
    CHECK(host.entities().delivered() >= 2);

    entity::Entity* relay = host.entities().find_by_name("corridor_relay").front();
    REQUIRE(relay != nullptr);
    CHECK(relay->classname() == "logic_relay");
}

TEST_CASE("a trigger does not re-fire while its wait is running") {
    Host host = make_host();

    // Stand in the trigger for four seconds. `wait` is 5, so it fires once.
    host.player().origin = Vec3(352.0f, 128.0f, 4.0f);
    run(host, walk(0.0f, 0.0f), 260);

    // One touch delivered to the trigger, and one OnStartTouch to the relay.
    CHECK(host.entities().delivered() <= 4);
    CHECK(host.entities().delivered() >= 2);
}

TEST_CASE("run_frame turns wall time into a whole number of fixed ticks") {
    Host host = make_host();
    const Command idle;

    // Half a tick's worth: not enough to run one.
    host.run_frame(idle, Host::tick_interval() * 0.5f);
    CHECK(host.tick_count() == 0);

    // The other half completes it.
    host.run_frame(idle, Host::tick_interval() * 0.6f);
    CHECK(host.tick_count() == 1);

    // A whole second at 66 Hz.
    host.run_frame(idle, 1.0f);
    CHECK(host.tick_count() > 1);
}

TEST_CASE("a stalled frame does not try to catch up all at once") {
    // A frame that has to run a hundred ticks makes the next frame longer
    // still, and the one after that longer again. The backlog is dropped
    // instead, which is a visible hitch rather than a spiral into a freeze.
    Host host = make_host();
    const Command idle;

    host.run_frame(idle, 10.0f);
    CHECK(host.tick_count() <= 8);
}

TEST_CASE("the view interpolates between ticks") {
    Host host = make_host();
    const Command forward = walk(90.0f);

    run(host, forward, 60);
    const Vec3 settled = host.view().eye;

    // Part of a tick's worth of wall time: the eye should move, smoothly,
    // without the simulation having advanced.
    host.run_frame(forward, Host::tick_interval() * 0.5f);
    const ViewState mid = host.view();
    CHECK(mid.interpolation > 0.0f);
    CHECK(mid.interpolation < 1.0f);
    CHECK(mid.eye.y >= settled.y);
}

// ---------------------------------------------------------------------------
// Entity I/O
// ---------------------------------------------------------------------------

namespace {

/// Builds a world from entity text alone, with no level. The I/O graph is
/// independent of geometry, so it can be tested that way.
entity::World world_from(std::string_view text) {
    entity::World world;
    std::vector<std::string> unknown;
    world.load(text, game::factory(), unknown);
    return world;
}

}  // namespace

TEST_CASE("a wire fires its target") {
    entity::World world = world_from(R"KV(
entity
{
	"classname" "logic_relay"
	"targetname" "first"
	connections
	{
		"OnTrigger" "second,Trigger,,0,-1"
	}
}
entity
{
	"classname" "logic_relay"
	"targetname" "second"
}
)KV");

    REQUIRE(world.size() == 2);
    world.queue("first", "Trigger", {}, 0.0f, Index::kNone, Index::kNone);

    world.tick(0.1f);
    CHECK(world.delivered() == 1);  // "first" received Trigger.

    world.tick(0.1f);
    CHECK(world.delivered() == 2);  // "second" received the relayed Trigger.
}

TEST_CASE("a delay is honoured") {
    entity::World world = world_from(R"KV(
entity
{
	"classname" "logic_relay"
	"targetname" "first"
	connections
	{
		"OnTrigger" "second,Trigger,,1.5,-1"
	}
}
entity
{
	"classname" "logic_relay"
	"targetname" "second"
}
)KV");

    world.queue("first", "Trigger", {}, 0.0f, Index::kNone, Index::kNone);
    world.tick(0.1f);
    REQUIRE(world.delivered() == 1);

    // Not yet.
    for (int i = 0; i < 10; ++i) {
        world.tick(0.1f);
    }
    CHECK(world.delivered() == 1);
    CHECK(world.pending_events() == 1);

    // And now.
    for (int i = 0; i < 10; ++i) {
        world.tick(0.1f);
    }
    CHECK(world.delivered() == 2);
    CHECK(world.pending_events() == 0);
}

TEST_CASE("a wire with a firing limit stops after it") {
    entity::World world = world_from(R"KV(
entity
{
	"classname" "logic_relay"
	"targetname" "once"
	connections
	{
		"OnTrigger" "target,Trigger,,0,1"
	}
}
entity
{
	"classname" "logic_relay"
	"targetname" "target"
}
)KV");

    for (int i = 0; i < 5; ++i) {
        world.queue("once", "Trigger", {}, 0.0f, Index::kNone, Index::kNone);
        world.tick(0.1f);
        world.tick(0.1f);
    }

    // Five triggers reached "once", but only one was relayed onward.
    CHECK(world.delivered() == 6);
}

TEST_CASE("logic_branch is the otherwise that removes the need for a script") {
    entity::World world = world_from(R"KV(
entity
{
	"classname" "logic_branch"
	"targetname" "branch"
	"InitialValue" "0"
	connections
	{
		"OnTrue"  "yes,Trigger,,0,-1"
		"OnFalse" "no,Trigger,,0,-1"
	}
}
entity
{
	"classname" "logic_relay"
	"targetname" "yes"
}
entity
{
	"classname" "logic_relay"
	"targetname" "no"
}
)KV");

    world.queue("branch", "Test", {}, 0.0f, Index::kNone, Index::kNone);
    world.tick(0.1f);
    world.tick(0.1f);
    const u64 after_false = world.delivered();
    CHECK(after_false == 2);  // Test, then "no".

    world.queue("branch", "SetValue", "1", 0.0f, Index::kNone, Index::kNone);
    world.queue("branch", "Test", {}, 0.0f, Index::kNone, Index::kNone);
    world.tick(0.1f);
    world.tick(0.1f);
    CHECK(world.delivered() == after_false + 3);  // SetValue, Test, then "yes".
}

TEST_CASE("a wire that names nothing is reported at load") {
    entity::World world = world_from(R"KV(
entity
{
	"classname" "logic_relay"
	"targetname" "orphan"
	connections
	{
		"OnTrigger" "a_door_that_was_renamed,Open,,0,-1"
	}
}
)KV");

    // A designer wants to know now, not when the button turns out to do
    // nothing.
    REQUIRE(world.dangling_wires().size() == 1);
    CHECK(world.dangling_wires()[0].find("a_door_that_was_renamed") != std::string::npos);
}

TEST_CASE("an entity class this build does not implement is left out, not fatal") {
    entity::World world;
    std::vector<std::string> unknown;
    world.load(R"KV(
entity
{
	"classname" "logic_relay"
	"targetname" "known"
}
entity
{
	"classname" "npc_something_from_a_later_build"
	"targetname" "unknown"
}
)KV", game::factory(), unknown);

    CHECK(world.size() == 1);
    REQUIRE(unknown.size() == 1);
    CHECK(unknown[0] == "npc_something_from_a_later_build");
}

TEST_CASE("a relay wired to itself does not hang the tick") {
    // Delivering an input that queues another within the same tick would spin
    // forever here. Batching by tick makes it a slow loop instead of a freeze,
    // which is a bug you can see and stop.
    entity::World world = world_from(R"KV(
entity
{
	"classname" "logic_relay"
	"targetname" "loop"
	connections
	{
		"OnTrigger" "loop,Trigger,,0,-1"
	}
}
)KV");

    world.queue("loop", "Trigger", {}, 0.0f, Index::kNone, Index::kNone);
    for (int i = 0; i < 20; ++i) {
        world.tick(0.1f);
    }
    // One delivery per tick, not an unbounded cascade inside one.
    CHECK(world.delivered() == 20);
}

TEST_CASE("an entity can be addressed by classname as well as by name") {
    entity::World world = world_from(R"KV(
entity
{
	"classname" "logic_relay"
	"targetname" "a"
}
entity
{
	"classname" "logic_relay"
	"targetname" "b"
}
)KV");

    // How a level says "every light" without naming them all.
    world.queue("logic_relay", "Trigger", {}, 0.0f, Index::kNone, Index::kNone);
    world.tick(0.1f);
    CHECK(world.delivered() == 2);
}

TEST_CASE("entities keep the properties their class does not understand") {
    entity::World world = world_from(R"KV(
entity
{
	"classname" "logic_relay"
	"targetname" "keeper"
	"a_key_from_a_later_build" "kept"
}
)KV");

    entity::Entity* entity = world.find_by_name("keeper").front();
    REQUIRE(entity != nullptr);
    // What lets the game code gain a feature without recompiling every map.
    CHECK(entity->property("a_key_from_a_later_build") == "kept");
}

TEST_CASE("the tick rate is a convar, and the host follows it") {
    console::ConVar* tickrate = console::find_var("sv_tickrate");
    REQUIRE(tickrate != nullptr);
    const std::string original(tickrate->string());

    (void)tickrate->set("100");
    CHECK(Host::tick_interval() == doctest::Approx(0.01f));

    // And it is bounded, so nobody sets it to zero and divides by it.
    (void)tickrate->set("0");
    CHECK(Host::tick_interval() > 0.0f);
    CHECK(Host::tick_interval() < 1.0f);

    (void)tickrate->set(original);
}
