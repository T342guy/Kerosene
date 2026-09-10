// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "chisel/document.hpp"

#include <array>

#include <filesystem>
#include <fstream>
#include <format>
#include <set>
#include <sstream>
#include <string>

using namespace kero;
using namespace kero::chisel;
using kero::map::Vec3d;

namespace {

std::string sample_path() {
    return std::string(KEROSENE_SOURCE_DIR) + "/content/maps/kero_start.kmap";
}

std::string read_file(const std::string& path) {
    std::ifstream file(path, std::ios::binary);
    std::ostringstream buffer;
    buffer << file.rdbuf();
    return buffer.str();
}

std::filesystem::path scratch(const char* name) {
    const std::filesystem::path directory =
        std::filesystem::temp_directory_path() / "kerosene-chisel-tests";
    std::error_code code;
    std::filesystem::create_directories(directory, code);
    return directory / name;
}

/// A box brush, as the map format stores one: six planes, each given as three
/// points wound counter-clockwise seen from the front.
map::Solid box(Document& document, Vec3d mins, Vec3d maxs,
               const std::string& material = "dev/wall") {
    const std::array<std::array<Vec3d, 3>, 6> faces{{
        {{{mins.x, maxs.y, maxs.z}, {maxs.x, maxs.y, maxs.z}, {maxs.x, mins.y, maxs.z}}},
        {{{mins.x, mins.y, mins.z}, {maxs.x, mins.y, mins.z}, {maxs.x, maxs.y, mins.z}}},
        {{{maxs.x, maxs.y, maxs.z}, {maxs.x, maxs.y, mins.z}, {maxs.x, mins.y, mins.z}}},
        {{{mins.x, mins.y, maxs.z}, {mins.x, mins.y, mins.z}, {mins.x, maxs.y, mins.z}}},
        {{{mins.x, maxs.y, maxs.z}, {mins.x, maxs.y, mins.z}, {maxs.x, maxs.y, mins.z}}},
        {{{maxs.x, mins.y, maxs.z}, {maxs.x, mins.y, mins.z}, {mins.x, mins.y, mins.z}}},
    }};

    map::Solid solid;
    solid.id = document.allocate_id();
    for (const std::array<Vec3d, 3>& points : faces) {
        map::Side side;
        side.id = document.allocate_id();
        side.plane_points = points;
        REQUIRE(math::Planed::from_points(points[0], points[1], points[2], side.plane));
        side.material = material;
        solid.sides.push_back(std::move(side));
    }
    return solid;
}

map::Entity point_entity(Document& document, const std::string& classname, Vec3d origin) {
    map::Entity entity;
    entity.id = document.allocate_id();
    entity.classname = classname;
    entity.set("id", std::to_string(entity.id));
    entity.set("classname", classname);
    entity.set("origin", std::format("{:g} {:g} {:g}", origin.x, origin.y, origin.z));
    return entity;
}

}  // namespace

TEST_CASE("a new document has a world and nothing else") {
    Document document;

    CHECK(document.map().world.classname == "worldspawn");
    CHECK(document.map().brush_count() == 0);
    CHECK(document.map().entities.empty());
    CHECK_FALSE(document.dirty());
    CHECK_FALSE(document.can_undo());
    CHECK_FALSE(document.has_path());

    // A map with no worldspawn is not a map, and letting the editor represent
    // one would mean every later stage having to cope with it.
    CHECK(document.map().world.id != 0);
}

TEST_CASE("adding a brush is undoable") {
    Document document;
    const map::Solid solid = box(document, {0, 0, 0}, {128, 128, 128});
    const i32 id = solid.id;

    document.apply(std::make_unique<AddSolid>(document.map().world.id, solid,
                                              "Create brush"));

    CHECK(document.map().brush_count() == 1);
    CHECK(document.find_solid(id) != nullptr);
    CHECK(document.dirty());
    REQUIRE(document.can_undo());
    CHECK(document.undo_name() == "Create brush");

    REQUIRE(document.undo());
    CHECK(document.map().brush_count() == 0);
    CHECK(document.find_solid(id) == nullptr);

    REQUIRE(document.redo());
    CHECK(document.map().brush_count() == 1);
    CHECK(document.find_solid(id) != nullptr);
}

TEST_CASE("removing a brush puts it back where it was") {
    Document document;
    const i32 world = document.map().world.id;

    std::vector<i32> ids;
    for (int i = 0; i < 4; ++i) {
        const map::Solid solid =
            box(document, {i * 256.0, 0, 0}, {i * 256.0 + 128, 128, 128});
        ids.push_back(solid.id);
        document.apply(std::make_unique<AddSolid>(world, solid, "Create brush"));
    }

    // The middle one.
    document.apply(std::make_unique<RemoveSolid>(world, ids[1], "Delete brush"));
    CHECK(document.map().brush_count() == 3);
    CHECK(document.map().world.solids[1].id == ids[2]);

    REQUIRE(document.undo());

    // Back in the middle, not appended to the end. A map that reorders itself
    // when you undo and redo is one nobody can diff.
    REQUIRE(document.map().brush_count() == 4);
    for (usize i = 0; i < ids.size(); ++i) {
        CHECK(document.map().world.solids[i].id == ids[i]);
    }
}

TEST_CASE("replacing a brush is how a move or a resize is recorded") {
    Document document;
    const i32 world = document.map().world.id;

    const map::Solid before = box(document, {0, 0, 0}, {128, 128, 128});
    document.apply(std::make_unique<AddSolid>(world, before, "Create brush"));

    // Moved 64 ku east, which for a plane-based brush means moving the planes.
    map::Solid after = before;
    for (map::Side& side : after.sides) {
        for (Vec3d& point : side.plane_points) {
            point.x += 64.0;
        }
        REQUIRE(math::Planed::from_points(side.plane_points[0], side.plane_points[1],
                                          side.plane_points[2], side.plane));
    }

    document.apply(std::make_unique<ReplaceSolid>(world, before, after, "Move brush"));

    CHECK(map::bounds_of(*document.find_solid(before.id)).mins.x ==
          doctest::Approx(64.0));
    REQUIRE(document.undo());
    CHECK(map::bounds_of(*document.find_solid(before.id)).mins.x ==
          doctest::Approx(0.0));
    CHECK(document.map().brush_count() == 1);
}

TEST_CASE("entities are added, replaced and removed the same way") {
    Document document;
    const map::Entity start = point_entity(document, "info_player_start", {64, 64, 8});
    const i32 id = start.id;

    document.apply(std::make_unique<AddEntity>(start, "Create entity"));
    REQUIRE(document.find_entity(id) != nullptr);

    map::Entity moved = start;
    moved.set("origin", "128 64 8");
    document.apply(std::make_unique<ReplaceEntity>(start, moved, "Move entity"));
    CHECK(document.find_entity(id)->origin()->x == doctest::Approx(128.0));

    REQUIRE(document.undo());
    CHECK(document.find_entity(id)->origin()->x == doctest::Approx(64.0));

    document.apply(std::make_unique<RemoveEntity>(id, "Delete entity"));
    CHECK(document.find_entity(id) == nullptr);
    REQUIRE(document.undo());
    CHECK(document.find_entity(id) != nullptr);
}

TEST_CASE("an edit after an undo discards the redo tail") {
    Document document;
    const i32 world = document.map().world.id;

    const map::Solid first = box(document, {0, 0, 0}, {64, 64, 64});
    const map::Solid second = box(document, {128, 0, 0}, {192, 64, 64});
    document.apply(std::make_unique<AddSolid>(world, first, "Create brush"));
    document.apply(std::make_unique<AddSolid>(world, second, "Create brush"));

    REQUIRE(document.undo());
    CHECK(document.can_redo());

    // The history is a line, not a tree.
    const map::Solid third = box(document, {256, 0, 0}, {320, 64, 64});
    document.apply(std::make_unique<AddSolid>(world, third, "Create brush"));

    CHECK_FALSE(document.can_redo());
    CHECK(document.map().brush_count() == 2);
    CHECK(document.find_solid(second.id) == nullptr);
    CHECK(document.find_solid(third.id) != nullptr);
}

TEST_CASE("undoing back to the saved point clears the dirty flag") {
    Document document;
    const i32 world = document.map().world.id;
    const std::filesystem::path path = scratch("dirty.kmap");

    std::string error;
    REQUIRE_MESSAGE(document.save_as(path.string(), error), error);
    CHECK_FALSE(document.dirty());

    document.apply(std::make_unique<AddSolid>(
        world, box(document, {0, 0, 0}, {64, 64, 64}), "Create brush"));
    CHECK(document.dirty());

    REQUIRE(document.undo());
    // Back to what is on disk, which is what a person means by "unchanged".
    CHECK_FALSE(document.dirty());

    REQUIRE(document.redo());
    CHECK(document.dirty());
}

TEST_CASE("saving marks the current point, wherever in the history it is") {
    Document document;
    const i32 world = document.map().world.id;
    const std::filesystem::path path = scratch("marked.kmap");
    std::string error;

    document.apply(std::make_unique<AddSolid>(
        world, box(document, {0, 0, 0}, {64, 64, 64}), "Create brush"));
    document.apply(std::make_unique<AddSolid>(
        world, box(document, {128, 0, 0}, {192, 64, 64}), "Create brush"));

    REQUIRE(document.save_as(path.string(), error));
    CHECK_FALSE(document.dirty());

    REQUIRE(document.undo());
    CHECK(document.dirty());
    REQUIRE(document.redo());
    CHECK_FALSE(document.dirty());
}

TEST_CASE("a saved point in a discarded tail cannot come back") {
    Document document;
    const i32 world = document.map().world.id;
    const std::filesystem::path path = scratch("tail.kmap");
    std::string error;

    document.apply(std::make_unique<AddSolid>(
        world, box(document, {0, 0, 0}, {64, 64, 64}), "Create brush"));
    REQUIRE(document.save_as(path.string(), error));

    REQUIRE(document.undo());
    CHECK(document.dirty());

    // The state matching the file is now unreachable by redo, so the document
    // must stay dirty however much undoing happens. Claiming otherwise would
    // let someone close a window over work that is not on disk.
    document.apply(std::make_unique<AddSolid>(
        world, box(document, {256, 0, 0}, {320, 64, 64}), "Create brush"));
    CHECK(document.dirty());
    REQUIRE(document.undo());
    CHECK(document.dirty());
}

TEST_CASE("undo restores what was selected to something that exists") {
    Document document;
    const i32 world = document.map().world.id;
    const map::Solid solid = box(document, {0, 0, 0}, {64, 64, 64});

    document.apply(std::make_unique<AddSolid>(world, solid, "Create brush"));
    document.selection().toggle_solid(solid.id);
    document.selection().face = 2;
    CHECK(document.selection().contains_solid(solid.id));

    REQUIRE(document.undo());

    // The brush is gone, so nothing may still claim to have it selected --
    // which is exactly the moment the user is watching.
    CHECK(document.selection().empty());
    CHECK_FALSE(document.selection().face.has_value());
}

TEST_CASE("ids are unique across everything the format numbers") {
    Document document;
    std::set<i32> seen{document.map().world.id};

    for (int i = 0; i < 8; ++i) {
        const map::Solid solid = box(document, {i * 64.0, 0, 0}, {i * 64.0 + 32, 32, 32});
        CHECK(seen.insert(solid.id).second);
        for (const map::Side& side : solid.sides) {
            CHECK(seen.insert(side.id).second);
        }
        document.apply(std::make_unique<AddSolid>(document.map().world.id, solid,
                                                  "Create brush"));
    }
}

TEST_CASE("opening a map continues its numbering rather than reusing it") {
    Document document;
    std::string error;
    REQUIRE_MESSAGE(document.open(sample_path(), error), error);

    // Every id already in the file, so a new brush cannot collide with one.
    std::set<i32> existing;
    for (const Document::SolidRef& ref : document.all_solids()) {
        existing.insert(ref.solid->id);
        for (const map::Side& side : ref.solid->sides) {
            existing.insert(side.id);
        }
    }
    for (const map::Entity& entity : document.map().entities) {
        existing.insert(entity.id);
    }

    for (int i = 0; i < 20; ++i) {
        CHECK(existing.count(document.allocate_id()) == 0);
    }
}

TEST_CASE("a load, an edit, an undo and a save reproduce the file exactly") {
    Document document;
    std::string error;
    REQUIRE_MESSAGE(document.open(sample_path(), error), error);

    CHECK(document.map().brush_count() == 23);
    CHECK(document.map().entities.size() == 7);

    // What the editor writes for an untouched map, which is the baseline the
    // round trip is measured against -- the committed file was written by a
    // generator, not by this.
    const std::filesystem::path baseline = scratch("baseline.kmap");
    REQUIRE(document.save_as(baseline.string(), error));
    const std::string before = read_file(baseline.string());

    document.apply(std::make_unique<AddSolid>(
        document.map().world.id, box(document, {0, 0, 0}, {64, 64, 64}), "Create brush"));
    document.apply(std::make_unique<RemoveEntity>(document.map().entities.front().id,
                                                  "Delete entity"));
    REQUIRE(document.undo());
    REQUIRE(document.undo());

    const std::filesystem::path after_path = scratch("roundtrip.kmap");
    REQUIRE(document.save_as(after_path.string(), error));

    // Byte for byte. An editor that quietly rewrites a map it did not change
    // makes every diff useless and every merge a fight.
    CHECK(read_file(after_path.string()) == before);
}

TEST_CASE("all_solids finds brushes on entities as well as the world") {
    Document document;
    std::string error;
    REQUIRE(document.open(sample_path(), error));

    const std::vector<Document::SolidRef> solids = document.all_solids();
    CHECK(solids.size() == document.map().brush_count());

    // The sample level has a func_detail pillar and a trigger volume, so not
    // every brush belongs to the world.
    bool found_entity_brush = false;
    for (const Document::SolidRef& ref : solids) {
        CHECK(ref.solid != nullptr);
        CHECK(document.owner_of(ref.solid->id) == ref.owner);
        if (ref.owner != document.map().world.id) {
            found_entity_brush = true;
        }
    }
    CHECK(found_entity_brush);
}
