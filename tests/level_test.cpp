// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "bsp/level.hpp"
#include "kv/keyvalues.hpp"
#include "math/units.hpp"

#include <cstring>
#include <string>

using namespace kero;
using namespace kero::bsp;

namespace {

/// The compiled sample level, loaded once. Compiled by scripts/build-content.sh
/// or by `kerosene-tools cleave`, which the test asks for by name if it is
/// missing -- an unhelpful "file not found" is a bad way to learn that a build
/// step was skipped.
const Level& sample() {
    static const Level level = [] {
        const std::string path =
            std::string(KEROSENE_SOURCE_DIR) + "/content/maps/kero_start.kbsp";
        auto loaded = Level::load(path);
        if (!loaded) {
            FAIL("could not load the compiled sample level: ", loaded.error(),
                 "\nRun: kerosene-tools cleave content/maps/kero_start.kmap");
            return Level{};
        }
        return std::move(*loaded);
    }();
    return level;
}

/// A player-shaped box, relative to the point being moved: 16 ku across, 36 ku
/// tall, with the origin at the feet.
Aabb player_box() {
    return Aabb(Vec3(-units::kPlayerWidth * 0.5f, -units::kPlayerWidth * 0.5f, 0.0f),
                Vec3(units::kPlayerWidth * 0.5f, units::kPlayerWidth * 0.5f,
                     units::kPlayerHeight));
}

}  // namespace

TEST_CASE("the compiled level loads and reports what it holds") {
    const Level& level = sample();

    CHECK(level.nodes().size() > 0);
    CHECK(level.leaves().size() > 0);
    CHECK(level.faces().size() > 0);
    CHECK(level.planes().size() > 0);
    CHECK(level.models().size() == 1);
    CHECK(level.cluster_count() > 0);

    // The level spans both rooms.
    CHECK(level.bounds().mins.x <= -16.0f);
    CHECK(level.bounds().maxs.x >= 704.0f);
}

TEST_CASE("the entity lump survived the compile") {
    auto document = kv::parse(sample().entities(), "kero_start.kbsp");
    REQUIRE(document);

    usize starts = 0;
    usize lights = 0;
    for (const kv::Block& block : document->blocks) {
        const std::string_view classname = block.get("classname");
        if (classname == "info_player_start") {
            ++starts;
        }
        if (classname == "light") {
            ++lights;
        }
    }
    CHECK(starts == 1);
    CHECK(lights == 3);

    // The wiring came through too, which is what the game code reads to build
    // the I/O graph.
    bool found_wire = false;
    for (const kv::Block& block : document->blocks) {
        if (const kv::Block* connections = block.first_child("connections")) {
            for (const kv::Pair& pair : connections->pairs) {
                if (pair.key == "OnStartTouch") {
                    found_wire = true;
                }
            }
        }
    }
    CHECK(found_wire);
}

TEST_CASE("materials are readable through the texinfo table") {
    const Level& level = sample();
    REQUIRE(!level.texinfos().empty());

    bool found_a_dev_material = false;
    for (const DiskTexInfo& texinfo : level.texinfos()) {
        const std::string_view material = level.material_of(texinfo);
        CHECK_FALSE(material.empty());
        if (material.starts_with("dev/")) {
            found_a_dev_material = true;
        }
    }
    CHECK(found_a_dev_material);
}

TEST_CASE("a point lands in the leaf it should") {
    const Level& level = sample();

    SUBCASE("inside the rooms and the corridor") {
        for (const Vec3& point : {Vec3(128, 128, 32), Vec3(352, 128, 32),
                                  Vec3(576, 128, 32)}) {
            CAPTURE(point.x);
            CHECK_FALSE(any(level.contents_at(point) & Contents::Solid));
            CHECK(level.cluster_at(point) >= 0);
        }
    }

    SUBCASE("inside a wall") {
        CHECK(any(level.contents_at(Vec3(-8, 128, 64)) & Contents::Solid));
        CHECK(level.cluster_at(Vec3(-8, 128, 64)) == -1);
    }

    SUBCASE("outside the level, which was filled in") {
        CHECK(any(level.contents_at(Vec3(-400, 128, 64)) & Contents::Solid));
        CHECK(any(level.contents_at(Vec3(352, 128, 600)) & Contents::Solid));
    }
}

TEST_CASE("faces are reachable from the leaves and reference valid geometry") {
    const Level& level = sample();

    usize with_faces = 0;
    for (u32 leaf = 0; leaf < level.leaves().size(); ++leaf) {
        const std::span<const u32> faces = level.leaf_faces(leaf);
        if (!faces.empty()) {
            ++with_faces;
        }
        for (u32 index : faces) {
            const DiskFace& face = level.faces()[index];
            CHECK(face.vertex_count >= 3);
            for (u32 v = 0; v < face.vertex_count; ++v) {
                CHECK(level.face_vertices()[face.first_vertex + v] <
                      level.vertices().size());
            }
        }
    }
    CHECK(with_faces > 0);
}

TEST_CASE("a ray straight down finds the floor at the right distance") {
    const Level& level = sample();

    // Clear of the func_detail pillar, which stands at x 96..160, y 176..240.
    // A ray started inside it would correctly report start-solid and measure
    // nothing.
    const Trace trace =
        level.trace_ray(Vec3(64, 48, 64), Vec3(64, 48, -64), Contents::SolidMask);

    REQUIRE(trace.hit());
    CHECK_FALSE(trace.start_solid);
    // Room A's floor is at z = 0, so 64 ku of a 128 ku sweep -- landing a hair
    // above it, by design.
    CHECK(trace.end.z >= 0.0f);
    CHECK(trace.end.z < 0.5f);
    CHECK(trace.fraction == doctest::Approx(0.5f).epsilon(0.01));
    // And it hit a surface facing up.
    CHECK(trace.plane.normal.z == doctest::Approx(1.0f));
}

TEST_CASE("a ray into open space hits nothing") {
    const Trace trace = sample().trace_ray(Vec3(64, 48, 32), Vec3(200, 48, 32),
                                           Contents::SolidMask);
    CHECK_FALSE(trace.hit());
    CHECK(trace.fraction == 1.0f);
    CHECK(trace.end.x == doctest::Approx(200.0f));
}

TEST_CASE("a ray at a wall stops at it, not in it") {
    const Level& level = sample();
    const Trace trace =
        level.trace_ray(Vec3(128, 48, 64), Vec3(-64, 48, 64), Contents::SolidMask);

    REQUIRE(trace.hit());
    // Room A's -X wall has its inner face at x = 0, and the sweep deliberately
    // stops a hair short of it rather than exactly on it.
    CHECK(trace.end.x > 0.0f);
    CHECK(trace.end.x < 0.5f);
    // The normal points out of the solid, so the wall bounding the room on its
    // -X side faces +X -- back towards where the ray came from.
    CHECK(trace.plane.normal.x == doctest::Approx(1.0f));
    // And the endpoint really is in open space, not embedded in the wall. That
    // is what the surface gap is for.
    CHECK_FALSE(any(level.contents_at(trace.end) & Contents::Solid));
}

TEST_CASE("a swept player box stops a body-width short of the wall") {
    const Level& level = sample();
    const Aabb box = player_box();

    const Trace trace =
        level.trace(Vec3(128, 48, 4), Vec3(-64, 48, 4), box, Contents::SolidMask);

    REQUIRE(trace.hit());
    CHECK_FALSE(trace.start_solid);
    // The box is 16 ku across, so its centre stops 8 ku from the wall at x = 0.
    CHECK(trace.end.x == doctest::Approx(8.0f).epsilon(0.02));

    // The box's own volume is clear of the wall, which is the property that
    // makes the arbitrary-size sweep worth having.
    CHECK_FALSE(any(level.contents_at(trace.end + Vec3(-7.5f, 0, 1)) & Contents::Solid));
}

TEST_CASE("the box size actually changes where the sweep stops") {
    const Level& level = sample();

    const Trace narrow = level.trace(Vec3(128, 48, 4), Vec3(-64, 48, 4),
                                     Aabb(Vec3(-4, -4, 0), Vec3(4, 4, 36)),
                                     Contents::SolidMask);
    const Trace wide = level.trace(Vec3(128, 48, 4), Vec3(-64, 48, 4),
                                   Aabb(Vec3(-24, -24, 0), Vec3(24, 24, 36)),
                                   Contents::SolidMask);

    REQUIRE(narrow.hit());
    REQUIRE(wide.hit());
    // Quake and Source snap every entity to one of a few precomputed hull
    // sizes; here the size is whatever was asked for, and the answer follows it.
    CHECK(narrow.end.x == doctest::Approx(4.0f).epsilon(0.05));
    CHECK(wide.end.x == doctest::Approx(24.0f).epsilon(0.05));
    CHECK(wide.end.x > narrow.end.x);
}

TEST_CASE("the func_detail pillar is solid, even though it is not in the tree") {
    const Level& level = sample();

    // Kept out of the visibility tree, filed back into the leaves it touches.
    // A trace has to find it, or a player walks through the scenery. The pillar
    // stands at x 96..160, y 176..240 in room A.
    const Trace into_pillar =
        level.trace_ray(Vec3(64, 208, 64), Vec3(200, 208, 64), Contents::SolidMask);
    REQUIRE(into_pillar.hit());
    CHECK(into_pillar.end.x == doctest::Approx(96.0f).epsilon(0.01));

    // And it does not block the route the player start actually faces.
    const Trace along_the_route =
        level.trace_ray(Vec3(64, 128, 32), Vec3(240, 128, 32), Contents::SolidMask);
    CHECK_FALSE(along_the_route.hit());
}

TEST_CASE("a player box walks up the 8 ku step") {
    const Level& level = sample();
    const Aabb box = player_box();

    // The step in room B is 8 ku, below the 9 ku step height. A mover walking
    // into it is stopped...
    const Trace into_step =
        level.trace(Vec3(500, 128, 0), Vec3(560, 128, 0), box, Contents::SolidMask);
    CHECK(into_step.hit());

    // ...but the same move from 8 ku up passes over it, which is what makes the
    // step walkable rather than a wall.
    const Trace over_step =
        level.trace(Vec3(500, 128, 8), Vec3(560, 128, 8), box, Contents::SolidMask);
    CHECK_FALSE(over_step.hit());
    CHECK(8.0f < units::kStepHeight);
}

TEST_CASE("a box that does not fit reports start-solid rather than tunnelling") {
    const Level& level = sample();

    // A box wider than the 64 ku corridor, started inside the corridor.
    const Aabb oversized(Vec3(-64, -64, 0), Vec3(64, 64, 36));
    const Trace trace = level.trace(Vec3(352, 128, 4), Vec3(420, 128, 4), oversized,
                                    Contents::SolidMask);

    // The important property is that it does not sail through the walls: it
    // either reports being stuck or refuses to move.
    CHECK((trace.start_solid || trace.fraction < 1.0f));
}

TEST_CASE("a trace starting inside a wall says so") {
    const Trace trace = sample().trace_ray(Vec3(-8, 128, 64), Vec3(-8, 128, 32),
                                           Contents::SolidMask);
    CHECK(trace.start_solid);
}

TEST_CASE("the content mask decides what is collided with") {
    const Level& level = sample();

    // The trigger volume in the corridor is not solid, so a solid-only sweep
    // passes through it...
    const Trace solid_only = level.trace_ray(Vec3(300, 128, 32), Vec3(404, 128, 32),
                                             Contents::SolidMask);
    CHECK_FALSE(solid_only.hit());

    // ...while a sweep that asks about triggers finds it.
    const Trace triggers =
        level.trace_ray(Vec3(280, 128, 32), Vec3(420, 128, 32), Contents::Trigger);
    CHECK(triggers.hit());
}

TEST_CASE("every open leaf can see itself, and visibility is readable") {
    const Level& level = sample();
    REQUIRE(level.has_visibility());

    std::vector<u8> visible;
    usize checked = 0;
    for (const DiskLeaf& leaf : level.leaves()) {
        if (leaf.cluster < 0) {
            continue;
        }
        level.visible_clusters(leaf.cluster, kVisPvs, visible);
        CHECK(Level::cluster_in_set(visible, leaf.cluster));
        ++checked;
    }
    CHECK(checked > 0);
}

TEST_CASE("a level compiled without umbra still loads, and sees everything") {
    // The property that lets you walk a level thirty seconds after drawing it.
    auto file = File::load(std::string(KEROSENE_SOURCE_DIR) +
                           "/content/maps/kero_start.kbsp");
    REQUIRE(file);
    file->set_lump(LumpId::Visibility, std::vector<std::byte>{});

    auto level = Level::from_file(std::move(*file));
    REQUIRE(level);
    CHECK_FALSE(level->has_visibility());

    std::vector<u8> visible;
    level->visible_clusters(0, kVisPvs, visible);
    for (u8 byte : visible) {
        CHECK(byte == 0xFF);
    }

    // And it is still walkable.
    const Trace trace =
        level->trace_ray(Vec3(128, 128, 64), Vec3(128, 128, -64), Contents::SolidMask);
    CHECK(trace.hit());
}

TEST_CASE("a corrupt file is refused with something a person can act on") {
    SUBCASE("not a .kbsp at all") {
        const std::string text = "this is not a bsp";
        const auto* bytes = reinterpret_cast<const std::byte*>(text.data());
        auto file = File::from_bytes({bytes, text.size()}, "junk.kbsp");
        REQUIRE_FALSE(file);
        CHECK(file.error().find("Kerosene .kbsp") != std::string::npos);
    }

    SUBCASE("the wrong version") {
        auto good = File::load(std::string(KEROSENE_SOURCE_DIR) +
                               "/content/maps/kero_start.kbsp");
        REQUIRE(good);
        std::vector<std::byte> bytes = good->to_bytes();
        // Bump the version field.
        const u32 wrong = kVersion + 1;
        std::memcpy(bytes.data() + sizeof(u32), &wrong, sizeof(wrong));

        auto file = File::from_bytes(bytes, "old.kbsp");
        REQUIRE_FALSE(file);
        // It should say what to do, not just that it failed.
        CHECK(file.error().find("Recompile") != std::string::npos);
    }

    SUBCASE("truncated") {
        auto good = File::load(std::string(KEROSENE_SOURCE_DIR) +
                               "/content/maps/kero_start.kbsp");
        REQUIRE(good);
        std::vector<std::byte> bytes = good->to_bytes();
        bytes.resize(bytes.size() / 2);

        auto file = File::from_bytes(bytes, "cut.kbsp");
        REQUIRE_FALSE(file);
        CHECK(file.error().find("truncated") != std::string::npos);
    }

    SUBCASE("a header but no geometry") {
        File empty;
        auto level = Level::from_file(empty);
        REQUIRE_FALSE(level);
        CHECK(level.error().find("did not compile") != std::string::npos);
    }
}
