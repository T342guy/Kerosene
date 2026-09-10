// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "chisel/document.hpp"
#include "chisel/tools.hpp"
#include "chisel/viewport.hpp"
#include "math/units.hpp"

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
using kero::math::Vec2;
using kero::math::Vec3;

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

// ---------------------------------------------------------------------------
// Viewports and picking
// ---------------------------------------------------------------------------

TEST_CASE("an orthographic view maps screen pixels to world positions") {
    Viewport view;
    view.kind = ViewKind::Top;
    view.width = 800.0f;
    view.height = 600.0f;
    view.zoom = 2.0f;  // Two pixels per kerosene unit.
    view.centre = Vec3(100.0f, 200.0f, 0.0f);

    SUBCASE("the middle of the view is where it is centred") {
        const Vec3 middle = view.world_from_screen(Vec2(400.0f, 300.0f));
        CHECK(middle.x == doctest::Approx(100.0f));
        CHECK(middle.y == doctest::Approx(200.0f));
    }

    SUBCASE("right on screen is east; down on screen is south") {
        // The top view reads like a map: north is up.
        const Vec3 point = view.world_from_screen(Vec2(400.0f + 64.0f, 300.0f + 32.0f));
        CHECK(point.x == doctest::Approx(100.0f + 32.0f));
        CHECK(point.y == doctest::Approx(200.0f - 16.0f));
    }

    SUBCASE("screen and world round-trip") {
        for (const Vec2 pixel : {Vec2(0.0f, 0.0f), Vec2(799.0f, 599.0f),
                                 Vec2(123.0f, 456.0f)}) {
            const Vec2 again = view.screen_from_world(view.world_from_screen(pixel));
            CHECK(again.x == doctest::Approx(pixel.x));
            CHECK(again.y == doctest::Approx(pixel.y));
        }
    }
}

TEST_CASE("each orthographic view looks down a different axis") {
    CHECK(axes_of(ViewKind::Top).forward.z == doctest::Approx(-1.0f));
    CHECK(axes_of(ViewKind::Front).forward.y == doctest::Approx(1.0f));
    CHECK(axes_of(ViewKind::Side).forward.x == doctest::Approx(-1.0f));

    // In every orthographic view, the two screen axes and the view axis are
    // mutually perpendicular -- otherwise a drag in one pane would move a point
    // along an axis that pane cannot show.
    for (const ViewKind kind : {ViewKind::Top, ViewKind::Front, ViewKind::Side}) {
        const ViewAxes axes = axes_of(kind);
        CHECK(dot(axes.right, axes.up) == doctest::Approx(0.0f));
        CHECK(dot(axes.right, axes.forward) == doctest::Approx(0.0f));
        CHECK(dot(axes.up, axes.forward) == doctest::Approx(0.0f));
    }

    // Z is up in every view that can show it, which is what stops a level built
    // in the front view coming out on its side.
    CHECK(axes_of(ViewKind::Front).up.z == doctest::Approx(1.0f));
    CHECK(axes_of(ViewKind::Side).up.z == doctest::Approx(1.0f));
}

TEST_CASE("zooming keeps the world point under the cursor still") {
    Viewport view;
    view.kind = ViewKind::Top;
    view.width = 800.0f;
    view.height = 600.0f;
    view.zoom = 1.0f;

    const Vec2 cursor(600.0f, 150.0f);
    const Vec3 before = view.world_from_screen(cursor);

    view.zoom_at(cursor, 2.0f);
    const Vec3 after = view.world_from_screen(cursor);

    // Without this you spend the whole time zooming and then panning back.
    CHECK(after.x == doctest::Approx(before.x));
    CHECK(after.y == doctest::Approx(before.y));
    CHECK(view.zoom == doctest::Approx(2.0f));
}

TEST_CASE("framing a box fits it in the view") {
    Viewport view;
    view.kind = ViewKind::Top;
    view.width = 800.0f;
    view.height = 600.0f;

    const math::Aabb room(Vec3(0, 0, 0), Vec3(256, 256, 128));
    view.frame(room);

    CHECK(view.centre.x == doctest::Approx(128.0f));
    CHECK(view.centre.y == doctest::Approx(128.0f));

    // Every corner is on screen.
    for (const Vec3 corner : {Vec3(0, 0, 0), Vec3(256, 0, 0), Vec3(0, 256, 0),
                              Vec3(256, 256, 0)}) {
        const Vec2 pixel = view.screen_from_world(corner);
        CHECK(pixel.x >= 0.0f);
        CHECK(pixel.x <= view.width);
        CHECK(pixel.y >= 0.0f);
        CHECK(pixel.y <= view.height);
    }
}

TEST_CASE("a ray picks the brush it passes through") {
    Document document;
    const map::Solid solid = box(document, {0, 0, 0}, {128, 128, 128});
    document.apply(std::make_unique<AddSolid>(document.map().world.id, solid,
                                              "Create brush"));

    SUBCASE("straight down the middle, from above") {
        const Ray ray{Vec3d(64, 64, 512), Vec3d(0, 0, -1)};
        const Hit hit = pick_solid(solid, ray);

        REQUIRE(hit.valid);
        CHECK(hit.solid == solid.id);
        CHECK(hit.distance == doctest::Approx(384.0));
        CHECK(hit.point.z == doctest::Approx(128.0));
        // Entered through the top, so that is the face to highlight.
        CHECK(solid.sides[hit.face].plane.normal.z == doctest::Approx(1.0));
    }

    SUBCASE("from the side") {
        const Ray ray{Vec3d(-512, 64, 64), Vec3d(1, 0, 0)};
        const Hit hit = pick_solid(solid, ray);
        REQUIRE(hit.valid);
        CHECK(hit.point.x == doctest::Approx(0.0));
        CHECK(solid.sides[hit.face].plane.normal.x == doctest::Approx(-1.0));
    }

    SUBCASE("a ray that misses") {
        CHECK_FALSE(pick_solid(solid, Ray{Vec3d(512, 512, 512), Vec3d(0, 0, -1)}).valid);
    }

    SUBCASE("a ray pointing away from the brush") {
        CHECK_FALSE(pick_solid(solid, Ray{Vec3d(64, 64, 512), Vec3d(0, 0, 1)}).valid);
    }

    SUBCASE("a ray parallel to a face and outside it") {
        CHECK_FALSE(pick_solid(solid, Ray{Vec3d(64, 64, 256), Vec3d(1, 0, 0)}).valid);
    }
}

TEST_CASE("picking takes the nearest brush, not the first one found") {
    Document document;
    const i32 world = document.map().world.id;

    const map::Solid far_brush = box(document, {512, 0, 0}, {640, 128, 128});
    const map::Solid near_brush = box(document, {0, 0, 0}, {128, 128, 128});
    // Added far-first, so a search that took the first hit would get it wrong.
    document.apply(std::make_unique<AddSolid>(world, far_brush, "Create brush"));
    document.apply(std::make_unique<AddSolid>(world, near_brush, "Create brush"));

    const Hit hit = pick(document, Ray{Vec3d(-512, 64, 64), Vec3d(1, 0, 0)});
    REQUIRE(hit.valid);
    CHECK(hit.solid == near_brush.id);
}

TEST_CASE("a viewport's pick ray finds what is under the cursor") {
    Document document;
    const map::Solid solid = box(document, {0, 0, 0}, {128, 128, 128});
    document.apply(std::make_unique<AddSolid>(document.map().world.id, solid,
                                              "Create brush"));

    Viewport view;
    view.width = 800.0f;
    view.height = 600.0f;
    view.zoom = 2.0f;
    view.centre = Vec3(64.0f, 64.0f, 64.0f);

    // The same brush, from every orthographic direction. An orthographic ray
    // starts well outside the world so it enters the brush rather than
    // beginning inside it.
    for (const ViewKind kind : {ViewKind::Top, ViewKind::Front, ViewKind::Side}) {
        view.kind = kind;
        CAPTURE(name_of(kind));
        const Hit hit = pick(document, view.ray_from_screen(Vec2(400.0f, 300.0f)));
        REQUIRE(hit.valid);
        CHECK(hit.solid == solid.id);
    }

    SUBCASE("and misses when the cursor is off the brush") {
        view.kind = ViewKind::Top;
        // 300 pixels right of centre at 2 px/ku is 150 ku east, past the brush.
        CHECK_FALSE(pick(document, view.ray_from_screen(Vec2(700.0f, 300.0f))).valid);
    }
}

TEST_CASE("point entities are picked as spheres, because they have no geometry") {
    Document document;
    const map::Entity start = point_entity(document, "info_player_start", {64, 64, 32});
    document.apply(std::make_unique<AddEntity>(start, "Create entity"));

    const Ray through{Vec3d(-512, 64, 32), Vec3d(1, 0, 0)};
    CHECK(pick_entity(document, through, 16.0) == start.id);

    // Outside the radius.
    const Ray past{Vec3d(-512, 200, 32), Vec3d(1, 0, 0)};
    CHECK_FALSE(pick_entity(document, past, 16.0).has_value());

    // Behind the ray's origin.
    const Ray away{Vec3d(-512, 64, 32), Vec3d(-1, 0, 0)};
    CHECK_FALSE(pick_entity(document, away, 16.0).has_value());
}

TEST_CASE("a brush entity is picked by its brushes, not as a point") {
    Document document;
    std::string error;
    REQUIRE(document.open(sample_path(), error));

    // The sample level's func_detail and trigger both carry brushes; picking
    // them as points would put a target in the middle of nowhere.
    const Ray anywhere{Vec3d(-4096, 128, 64), Vec3d(1, 0, 0)};
    if (const std::optional<i32> id = pick_entity(document, anywhere, 4096.0)) {
        const map::Entity* entity = document.find_entity(*id);
        REQUIRE(entity != nullptr);
        CHECK_FALSE(entity->is_brush_entity());
    }
}

TEST_CASE("the grid snaps to whole steps and leaves other values alone") {
    CHECK(snap_to_grid(13.0, 4.0) == doctest::Approx(12.0));
    CHECK(snap_to_grid(14.0, 4.0) == doctest::Approx(16.0));
    CHECK(snap_to_grid(-13.0, 4.0) == doctest::Approx(-12.0));
    CHECK(snap_to_grid(128.0, 4.0) == doctest::Approx(128.0));

    // The default grid is 4 ku -- a stair riser at this unit scale.
    CHECK(snap_to_grid(units::kGridDefault, units::kGridDefault) ==
          doctest::Approx(units::kGridDefault));

    // Grid off leaves the value untouched, for the times you mean 13.
    CHECK(snap_to_grid(13.37, 0.0) == doctest::Approx(13.37));

    const Vec3d snapped = snap_to_grid(Vec3d(13.0, -13.0, 130.0), 4.0);
    CHECK(snapped.x == doctest::Approx(12.0));
    CHECK(snapped.y == doctest::Approx(-12.0));
    CHECK(snapped.z == doctest::Approx(132.0));
}

// ---------------------------------------------------------------------------
// The editing tools
// ---------------------------------------------------------------------------

TEST_CASE("a box brush has six outward-facing sides") {
    Document document;
    const math::Aabbd bounds(Vec3d(0, 0, 0), Vec3d(64, 128, 32));
    const map::Solid box = make_box(bounds, document, "dev/grid");

    REQUIRE(box.sides.size() == 6);
    CHECK(box.valid());
    CHECK(encloses_volume(box));

    // Every plane faces away from the middle. A brush with one plane the wrong
    // way round encloses nothing, and finds out about it three stages later.
    const Vec3d centre = bounds.centre();
    for (const map::Side& side : box.sides) {
        CHECK(side.plane.distance_to(centre) < 0.0);
    }

    const math::Aabbd measured = map::bounds_of(box);
    CHECK(measured.mins.x == doctest::Approx(0.0));
    CHECK(measured.maxs.y == doctest::Approx(128.0));
    CHECK(measured.maxs.z == doctest::Approx(32.0));

    // Ids come from the document's one pool: the brush and its six sides.
    std::set<i32> ids{box.id};
    for (const map::Side& side : box.sides) {
        ids.insert(side.id);
    }
    CHECK(ids.size() == 7);
}

TEST_CASE("a box with no volume is not a brush") {
    Document document;
    CHECK_FALSE(make_box(math::Aabbd(Vec3d(0, 0, 0), Vec3d(0, 64, 64)), document,
                         "dev/grid")
                    .valid());
    CHECK_FALSE(make_box(math::Aabbd(), document, "dev/grid").valid());
}

TEST_CASE("the base axes texture a wall upright and a floor north-up") {
    const TextureAxes floor = default_texture_axes(Vec3d(0, 0, 1));
    CHECK(floor.u.axis.x == doctest::Approx(1.0));
    CHECK(floor.v.axis.y == doctest::Approx(-1.0));

    const TextureAxes wall = default_texture_axes(Vec3d(1, 0, 0));
    CHECK(wall.u.axis.y == doctest::Approx(1.0));
    CHECK(wall.v.axis.z == doctest::Approx(-1.0));

    // A very slightly tilted face gets the same axes as the flat one it is
    // nearly parallel to, which is the point of a table.
    const TextureAxes tilted = default_texture_axes(Vec3d(0.02, 0, 0.99).normalized());
    CHECK(tilted.u.axis == floor.u.axis);
}

TEST_CASE("moving a brush moves its planes and keeps it a brush") {
    Document document;
    const map::Solid box =
        make_box(math::Aabbd(Vec3d(0, 0, 0), Vec3d(64, 64, 64)), document, "dev/grid");
    const map::Solid moved = translate(box, Vec3d(128, -32, 8));

    const math::Aabbd bounds = map::bounds_of(moved);
    CHECK(bounds.mins.x == doctest::Approx(128.0));
    CHECK(bounds.mins.y == doctest::Approx(-32.0));
    CHECK(bounds.maxs.z == doctest::Approx(72.0));
    CHECK(encloses_volume(moved));

    // Normals are unchanged by a translation; only the distances move.
    for (usize i = 0; i < box.sides.size(); ++i) {
        CHECK(moved.sides[i].plane.normal.x == doctest::Approx(box.sides[i].plane.normal.x));
        CHECK(moved.sides[i].plane.normal.z == doctest::Approx(box.sides[i].plane.normal.z));
    }
}

TEST_CASE("resizing maps one box onto another and leaves the texture alone") {
    Document document;
    const math::Aabbd from(Vec3d(0, 0, 0), Vec3d(64, 64, 64));
    const map::Solid box = make_box(from, document, "dev/grid");

    const math::Aabbd to(Vec3d(0, 0, 0), Vec3d(256, 64, 64));
    const map::Solid wider = resize(box, from, to);

    const math::Aabbd bounds = map::bounds_of(wider);
    CHECK(bounds.maxs.x == doctest::Approx(256.0));
    CHECK(bounds.maxs.y == doctest::Approx(64.0));
    CHECK(encloses_volume(wider));

    // The axes are world-space projections, so a wall that grows shows more
    // texture rather than the same texture stretched.
    for (usize i = 0; i < box.sides.size(); ++i) {
        CHECK(wider.sides[i].uaxis.scale == doctest::Approx(box.sides[i].uaxis.scale));
        CHECK(wider.sides[i].uaxis.axis == box.sides[i].uaxis.axis);
    }
}

TEST_CASE("a grip drags only the sides it holds") {
    const math::Aabbd bounds(Vec3d(0, 0, 0), Vec3d(64, 64, 64));
    const ViewAxes axes = axes_of(ViewKind::Top);  // X right, Y up.

    // The right edge grip moves +X and nothing else.
    const math::Aabbd east = drag_grip(bounds, axes, Grip{1, 0}, Vec3d(32, 32, 32));
    CHECK(east.maxs.x == doctest::Approx(96.0));
    CHECK(east.mins.x == doctest::Approx(0.0));
    CHECK(east.maxs.y == doctest::Approx(64.0));
    CHECK(east.maxs.z == doctest::Approx(64.0));

    // A corner grip moves two.
    const math::Aabbd corner = drag_grip(bounds, axes, Grip{-1, -1}, Vec3d(-16, -16, 0));
    CHECK(corner.mins.x == doctest::Approx(-16.0));
    CHECK(corner.mins.y == doctest::Approx(-16.0));
    CHECK(corner.maxs.x == doctest::Approx(64.0));

    CHECK(grips().size() == 8);
    for (const Grip& grip : grips()) {
        CHECK_FALSE(grip == Grip{0, 0});  // The middle is a move, not a resize.
    }
}

TEST_CASE("a grip cannot be dragged past the side opposite it") {
    const math::Aabbd bounds(Vec3d(0, 0, 0), Vec3d(64, 64, 64));
    const ViewAxes axes = axes_of(ViewKind::Top);

    const math::Aabbd crushed = drag_grip(bounds, axes, Grip{1, 0}, Vec3d(-4096, 0, 0));
    CHECK(crushed.maxs.x >= crushed.mins.x);
    CHECK(crushed.maxs.x == doctest::Approx(0.0));
}

TEST_CASE("grips sit on the corners and edges of the selection") {
    const math::Aabbd bounds(Vec3d(0, 0, 0), Vec3d(64, 128, 32));
    const ViewAxes axes = axes_of(ViewKind::Top);

    const Vec3d north_east = grip_position(bounds, axes, Grip{1, 1});
    CHECK(north_east.x == doctest::Approx(64.0));
    CHECK(north_east.y == doctest::Approx(128.0));

    const Vec3d west_edge = grip_position(bounds, axes, Grip{-1, 0});
    CHECK(west_edge.x == doctest::Approx(0.0));
    CHECK(west_edge.y == doctest::Approx(64.0));  // Halfway up the edge.
}

TEST_CASE("snapping a box never collapses it") {
    const math::Aabbd sliver(Vec3d(0.5, 0, 0), Vec3d(1.0, 64, 64));
    const math::Aabbd snapped = snap_bounds(sliver, 4.0);
    CHECK(snapped.maxs.x > snapped.mins.x);
    CHECK(snapped.maxs.x - snapped.mins.x == doctest::Approx(4.0));

    CHECK(snap_bounds(sliver, 0.0).mins.x == doctest::Approx(0.5));
}

TEST_CASE("the selection's bounds cover its brushes and entities") {
    Document document;
    std::string error;
    REQUIRE(document.open(sample_path(), error));

    CHECK(selection_bounds(document).empty());

    const std::vector<Document::SolidRef> solids = document.all_solids();
    REQUIRE_FALSE(solids.empty());
    document.selection().toggle_solid(solids.front().solid->id);

    const math::Aabbd one = selection_bounds(document);
    CHECK_FALSE(one.empty());

    const math::Aabbd expected = map::bounds_of(*solids.front().solid);
    CHECK(one.mins.x == doctest::Approx(expected.mins.x));
    CHECK(one.maxs.z == doctest::Approx(expected.maxs.z));
}

TEST_CASE("a drawn room compiles as a brush, moved or resized") {
    Document document;
    const math::Aabbd bounds(Vec3d(-128, -128, -16), Vec3d(128, 128, 0));
    const map::Solid floor = make_box(bounds, document, "dev/floor");
    REQUIRE(encloses_volume(floor));

    // Every stage a drag can leave a brush in still has to be a brush.
    CHECK(encloses_volume(translate(floor, Vec3d(1024, -2048, 96))));
    CHECK(encloses_volume(
        resize(floor, bounds, math::Aabbd(Vec3d(-128, -128, -16), Vec3d(4, 128, 0)))));
    CHECK(encloses_volume(resize(
        floor, bounds, snap_bounds(math::Aabbd(Vec3d(0, 0, 0), Vec3d(0.1, 4, 4)), 4.0))));
}
