// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "map/map.hpp"
#include "math/aabb.hpp"
#include "math/units.hpp"
#include "math/winding.hpp"

#include <optional>
#include <string>

using namespace kero;
using namespace kero::map;
using math::Windingd;

namespace {

std::string sample_map_path() {
    return std::string(KEROSENE_SOURCE_DIR) + "/content/maps/kero_start.kmap";
}

/// Turns a brush's planes into its faces, the way Cleave will: start each side
/// from a base winding and clip it by every other side's half-space.
///
/// Doing it here as well is deliberate. It is the check that the *stored* form
/// -- three points per side, in a text file -- really does describe the solid
/// that was intended, independently of any compiler code.
std::vector<Windingd> faces_of(const Solid& solid) {
    std::vector<Windingd> faces;
    for (usize i = 0; i < solid.sides.size(); ++i) {
        std::optional<Windingd> winding = Windingd::from_plane(solid.sides[i].plane);
        for (usize j = 0; j < solid.sides.size() && winding; ++j) {
            if (i != j) {
                winding = winding->clipped(solid.sides[j].plane.flipped());
            }
        }
        if (winding) {
            faces.push_back(std::move(*winding));
        }
    }
    return faces;
}

math::Aabbd bounds_of(const Solid& solid) {
    math::Aabbd box;
    for (const Windingd& face : faces_of(solid)) {
        for (const Vec3d& point : face.points()) {
            box.add(point);
        }
    }
    return box;
}

}  // namespace

TEST_CASE("the sample map loads") {
    auto map = load(sample_map_path());
    REQUIRE_MESSAGE(map, map.error().format());

    CHECK(map->format_version == 1);
    CHECK(map->world.classname == "worldspawn");
    CHECK(map->world.get("skyname") == "sky_kero");
    CHECK(map->brush_count() > 20);
    CHECK(map->side_count() == map->brush_count() * 6);
}

TEST_CASE("every brush in the sample map is a closed solid") {
    auto map = load(sample_map_path());
    REQUIRE(map);

    std::vector<const Solid*> solids;
    for (const Solid& solid : map->world.solids) {
        solids.push_back(&solid);
    }
    for (const Entity& entity : map->entities) {
        for (const Solid& solid : entity.solids) {
            solids.push_back(&solid);
        }
    }
    REQUIRE(solids.size() > 20);

    for (const Solid* solid : solids) {
        CAPTURE(solid->id);
        const std::vector<Windingd> faces = faces_of(*solid);

        // Every side bounds the solid: none was clipped away by the others,
        // which is what happens when a plane is wound the wrong way round.
        REQUIRE(faces.size() == solid->sides.size());

        for (const Windingd& face : faces) {
            CHECK(face.valid());
        }

        // A closed volume: the sum of (face area x distance from an interior
        // point) over all faces is three times the volume, and every face must
        // face away from that point. A brush turned inside out fails here.
        math::Aabbd box;
        for (const Windingd& face : faces) {
            for (const Vec3d& point : face.points()) {
                box.add(point);
            }
        }
        const Vec3d interior = box.centre();
        for (const Side& side : solid->sides) {
            CHECK(side.plane.distance_to(interior) < 0.0);
        }
    }
}

TEST_CASE("the sample map's rooms are where the level says they are") {
    auto map = load(sample_map_path());
    REQUIRE(map);

    // The floor of room A: 288 x 288 x 16, sitting just below z = 0.
    const math::Aabbd floor_box = bounds_of(map->world.solids[0]);
    CHECK(floor_box.mins.x == doctest::Approx(-16.0));
    CHECK(floor_box.maxs.x == doctest::Approx(272.0));
    CHECK(floor_box.maxs.z == doctest::Approx(0.0));
    CHECK(floor_box.size().z == doctest::Approx(16.0));

    // The level spans both rooms and the corridor between them.
    math::Aabbd world_box;
    for (const Solid& solid : map->world.solids) {
        world_box.add(bounds_of(solid));
    }
    CHECK(world_box.mins.x == doctest::Approx(-16.0));
    CHECK(world_box.maxs.x == doctest::Approx(720.0));
    CHECK(world_box.maxs.z == doctest::Approx(144.0));
}

TEST_CASE("the step is walkable, which is the point of it being 8 ku") {
    auto map = load(sample_map_path());
    REQUIRE(map);

    // The last world brush is the step in room B.
    const math::Aabbd step = bounds_of(map->world.solids.back());
    CHECK(step.size().z == doctest::Approx(8.0));
    CHECK(step.size().z < units::kStepHeight);
}

TEST_CASE("entities, their classnames and their origins") {
    auto map = load(sample_map_path());
    REQUIRE(map);

    const std::vector<const Entity*> starts = map->by_classname("info_player_start");
    REQUIRE(starts.size() == 1);
    const std::optional<Vec3d> origin = starts[0]->origin();
    REQUIRE(origin);
    CHECK(origin->x == doctest::Approx(64.0));
    CHECK(origin->z == doctest::Approx(8.0));

    CHECK(map->by_classname("light").size() == 3);

    SUBCASE("a brush entity carries brushes; a point entity does not") {
        const std::vector<const Entity*> detail = map->by_classname("func_detail");
        REQUIRE(detail.size() == 1);
        CHECK(detail[0]->is_brush_entity());
        CHECK(detail[0]->solids.size() == 1);
        CHECK_FALSE(starts[0]->is_brush_entity());
    }
}

TEST_CASE("entity I/O wiring parses into connections") {
    auto map = load(sample_map_path());
    REQUIRE(map);

    const std::vector<const Entity*> triggers = map->by_classname("trigger_multiple");
    REQUIRE(triggers.size() == 1);
    REQUIRE(triggers[0]->connections.size() == 1);

    const Connection& wire = triggers[0]->connections[0];
    CHECK(wire.output == "OnStartTouch");
    CHECK(wire.target == "corridor_relay");
    CHECK(wire.input == "Trigger");
    CHECK(wire.delay == doctest::Approx(0.0f));
    CHECK(wire.times_to_fire == -1);

    // The entity it names exists, which is the check a compiler should make
    // before a designer discovers the wire does nothing at runtime.
    bool found = false;
    for (const Entity& entity : map->entities) {
        if (entity.get("targetname") == wire.target) {
            found = true;
        }
    }
    CHECK(found);
}

TEST_CASE("texture axes parse, including the scale after the bracket") {
    auto map = load(sample_map_path());
    REQUIRE(map);

    const Side& side = map->world.solids[0].sides[0];
    CHECK(side.material == "dev/floor");
    CHECK(side.uaxis.axis == Vec3d(1, 0, 0));
    CHECK(side.uaxis.scale == doctest::Approx(0.5));
    CHECK(side.vaxis.axis == Vec3d(0, -1, 0));
    CHECK(side.lightmap_scale == doctest::Approx(8.0f));
}

TEST_CASE("a map round-trips through text unchanged") {
    auto map = load(sample_map_path());
    REQUIRE(map);

    const kv::Document written = to_document(*map);
    auto reloaded = from_document(written);
    REQUIRE_MESSAGE(reloaded, reloaded.error().format());

    CHECK(reloaded->brush_count() == map->brush_count());
    CHECK(reloaded->side_count() == map->side_count());
    CHECK(reloaded->entities.size() == map->entities.size());
    CHECK(reloaded->world.get("skyname") == map->world.get("skyname"));

    // The planes must survive exactly. A round trip that moved a wall by a
    // rounding error would mean the editor could not safely save a map.
    for (usize i = 0; i < map->world.solids.size(); ++i) {
        for (usize j = 0; j < map->world.solids[i].sides.size(); ++j) {
            const Side& before = map->world.solids[i].sides[j];
            const Side& after = reloaded->world.solids[i].sides[j];
            CHECK(after.plane_points == before.plane_points);
            CHECK(after.plane.normal == before.plane.normal);
            CHECK(after.plane.distance == before.plane.distance);
            CHECK(after.material == before.material);
        }
    }

    // And writing it a second time produces the same bytes.
    CHECK(to_document(*reloaded).to_string() == written.to_string());
}

TEST_CASE("an unknown key survives a load and save") {
    auto map = from_document(*kv::parse(R"KV(
world
{
	"id" "1"
	"classname" "worldspawn"
	"a_key_this_build_does_not_know" "keep me"
}
)KV", "test"));
    REQUIRE(map);
    CHECK(map->world.get("a_key_this_build_does_not_know") == "keep me");
    CHECK(to_document(*map).to_string().find("keep me") != std::string::npos);
}

TEST_CASE("malformed maps are reported against the side that caused them") {
    auto bad_plane = [](std::string_view plane) {
        return from_document(*kv::parse(std::string(R"KV(
world
{
	"id" "1"
	"classname" "worldspawn"
	solid
	{
		"id" "2"
		side
		{
			"id" "3"
			"plane" ")KV") + std::string(plane) + R"KV("
		}
		side
		{ "id" "4" "plane" "(0 0 0) (0 16 0) (16 0 0)" }
		side
		{ "id" "5" "plane" "(0 0 0) (0 16 0) (16 0 0)" }
		side
		{ "id" "6" "plane" "(0 0 0) (0 16 0) (16 0 0)" }
	}
}
)KV", "m.kmap"));
    };

    SUBCASE("collinear points do not define a plane") {
        auto map = bad_plane("(0 0 0) (8 0 0) (16 0 0)");
        REQUIRE_FALSE(map);
        CHECK(map.error().message.find("collinear") != std::string::npos);
        CHECK(map.error().where.line > 1);
    }

    SUBCASE("a plane that is not three points") {
        auto map = bad_plane("(0 0 0) (8 0 0)");
        REQUIRE_FALSE(map);
        CHECK(map.error().message.find("three points") != std::string::npos);
    }

    SUBCASE("a brush with too few sides cannot bound a volume") {
        auto map = from_document(*kv::parse(R"KV(
world
{
	"id" "1"
	"classname" "worldspawn"
	solid
	{
		"id" "2"
		side
		{ "id" "3" "plane" "(0 0 0) (0 16 0) (16 0 0)" }
	}
}
)KV", "m.kmap"));
        REQUIRE_FALSE(map);
        CHECK(map.error().message.find("at least 4") != std::string::npos);
    }

    SUBCASE("a map with no world block") {
        auto map = from_document(*kv::parse("versioninfo\n{\n}\n", "m.kmap"));
        REQUIRE_FALSE(map);
        CHECK(map.error().message.find("no 'world' block") != std::string::npos);
    }

    SUBCASE("an entity with no classname") {
        auto map = from_document(*kv::parse(R"KV(
world
{ "classname" "worldspawn" }
entity
{ "id" "5" "origin" "0 0 0" }
)KV", "m.kmap"));
        REQUIRE_FALSE(map);
        CHECK(map.error().message.find("classname") != std::string::npos);
    }

    SUBCASE("a zero texture scale is rejected rather than dividing by zero later") {
        auto map = from_document(*kv::parse(R"KV(
world
{
	"classname" "worldspawn"
	solid
	{
		side
		{ "plane" "(0 0 0) (0 16 0) (16 0 0)" "uaxis" "[1 0 0 0] 0" }
		side
		{ "plane" "(0 0 0) (0 16 0) (16 0 0)" }
		side
		{ "plane" "(0 0 0) (0 16 0) (16 0 0)" }
		side
		{ "plane" "(0 0 0) (0 16 0) (16 0 0)" }
	}
}
)KV", "m.kmap"));
        REQUIRE_FALSE(map);
        CHECK(map.error().message.find("uaxis") != std::string::npos);
    }
}
