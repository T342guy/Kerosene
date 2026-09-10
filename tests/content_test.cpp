// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "chisel/document.hpp"
#include "chisel/tools.hpp"
#include "cleave/cleave.hpp"
#include "umbra/umbra.hpp"

#include <algorithm>
#include <cstdio>
#include <filesystem>
#include <string>
#include <vector>

using namespace kero;

/// Compiles the committed sample content, and is the fixture every suite that
/// reads a `.kbsp` depends on.
///
/// This used to shell out to `kerosene-tools cleave`. It cannot any more --
/// the toolset is a GUI application now -- and calling the compiler libraries
/// directly turns out to be the better arrangement anyway:
///
///   * It works with no graphics headers and no GPU, so the whole suite still
///     runs on a CI machine and under the nogfx preset.
///   * It asserts the compile *succeeded*, rather than that a process exited
///     zero, and it can say which stage failed and why.
///   * A fresh clone runs `ctest` green with no arguments, instead of failing
///     on a missing file and leaving the reader to guess which step was
///     skipped.
namespace {

std::filesystem::path content_maps() {
    return std::filesystem::path(KEROSENE_SOURCE_DIR) / "content" / "maps";
}

std::vector<std::filesystem::path> sample_maps() {
    std::vector<std::filesystem::path> maps;
    std::error_code code;
    for (const std::filesystem::directory_entry& entry :
         std::filesystem::directory_iterator(content_maps(), code)) {
        if (entry.path().extension() == ".kmap") {
            maps.push_back(entry.path());
        }
    }
    std::ranges::sort(maps);
    return maps;
}

}  // namespace

TEST_CASE("the committed content compiles") {
    const std::vector<std::filesystem::path> maps = sample_maps();
    REQUIRE_MESSAGE(!maps.empty(), "no .kmap files under content/maps");

    for (const std::filesystem::path& map : maps) {
        CAPTURE(map.filename().string());

        const auto compiled = cleave::run(map.string(), cleave::Options{});
        if (!compiled) {
            FAIL("cleave failed on ", map.filename().string(), ": ",
                 compiled.error().message);
        }

        // A brush that will not compile is a hole in the level, and the sample
        // content is what every other suite measures against.
        CHECK(compiled->problems.empty());
        CHECK(compiled->faces > 0);
        CHECK(compiled->tree.empty_leaves > 0);

        // Sealed. Everything downstream -- visibility, and lighting when it
        // exists -- is wrong on a level that leaks, so this is the assertion
        // that keeps the rest of the suite meaningful.
        CHECK_MESSAGE(!compiled->leaked,
                      "the sample level leaks, reached from ", compiled->leak_entity);

        std::filesystem::path bsp = map;
        bsp.replace_extension(".kbsp");
        REQUIRE(std::filesystem::exists(bsp));

        const auto vis = umbra::run(bsp.string(), umbra::Options{});
        if (!vis) {
            FAIL("umbra failed on ", bsp.filename().string(), ": ", vis.error());
        }

        CHECK(vis->clusters > 0);
        CHECK(vis->visibility_bytes > 0);
        // Every cluster sees at least itself, so the average can never be below
        // one; a level where it were would have a broken PVS.
        CHECK(vis->average_visible >= 1.0);
    }
}

TEST_CASE("a room drawn the way the editor draws one compiles sealed") {
    // The editor's own primitives, end to end: six boxes make a sealed room,
    // a seventh is moved and resized into it, and the result goes through the
    // compiler. This is the loop Chisel exists for, minus the mouse.
    using namespace kero::chisel;
    using kero::math::Aabbd;
    using kero::map::Vec3d;

    chisel::Document document;

    const f64 inner = 256.0;
    const f64 thickness = 16.0;
    const Aabbd room(Vec3d(0, 0, 0), Vec3d(inner, inner, 128.0));

    const auto shell = [&](const Aabbd& bounds) {
        map::Solid solid = make_box(bounds, document, "dev/grid");
        REQUIRE(map::encloses_volume(solid));
        document.apply(std::make_unique<AddSolid>(document.map().world.id,
                                                  std::move(solid), "Draw block"));
    };

    shell(Aabbd(Vec3d(-thickness, -thickness, -thickness),
                Vec3d(inner + thickness, inner + thickness, 0)));               // Floor.
    shell(Aabbd(Vec3d(-thickness, -thickness, room.maxs.z),
                Vec3d(inner + thickness, inner + thickness,
                      room.maxs.z + thickness)));                                // Ceiling.
    shell(Aabbd(Vec3d(-thickness, -thickness, 0), Vec3d(0, inner + thickness,
                                                        room.maxs.z)));          // West.
    shell(Aabbd(Vec3d(inner, -thickness, 0),
                Vec3d(inner + thickness, inner + thickness, room.maxs.z)));      // East.
    shell(Aabbd(Vec3d(0, -thickness, 0), Vec3d(inner, 0, room.maxs.z)));         // South.
    shell(Aabbd(Vec3d(0, inner, 0), Vec3d(inner, inner + thickness, room.maxs.z)));

    // A pillar, drawn somewhere convenient and then dragged into place -- the
    // two operations every Select drag is made of.
    const Aabbd drawn(Vec3d(0, 0, 0), Vec3d(32, 32, 128));
    map::Solid pillar = make_box(drawn, document, "dev/grid");
    REQUIRE(pillar.valid());
    pillar = translate(pillar, Vec3d(96, 96, 0));
    pillar = resize(pillar, map::bounds_of(pillar),
                    Aabbd(Vec3d(96, 96, 0), Vec3d(160, 160, 128)));
    REQUIRE(map::encloses_volume(pillar));
    document.apply(std::make_unique<AddSolid>(document.map().world.id, std::move(pillar),
                                              "Draw block"));

    map::Entity start;
    start.id = document.allocate_id();
    start.classname = "info_player_start";
    start.set("classname", "info_player_start");
    // Clear of the pillar, which fills 96..160 on both horizontal axes.
    start.set("origin", "48 48 40");
    document.apply(std::make_unique<AddEntity>(std::move(start), "Place entity"));

    const std::filesystem::path path =
        std::filesystem::temp_directory_path() / "kerosene_chisel_room.kmap";
    std::string error;
    REQUIRE_MESSAGE(document.save_as(path.string(), error), error);

    const auto compiled = cleave::run(path.string(), cleave::Options{});
    if (!compiled) {
        FAIL("cleave failed on the drawn room: ", compiled.error().message);
    }
    CHECK(compiled->problems.empty());
    CHECK(compiled->faces > 0);
    CHECK_MESSAGE(!compiled->leaked, "the drawn room leaks from ",
                  compiled->leak_entity);

    std::filesystem::path bsp = path;
    bsp.replace_extension(".kbsp");
    // Left behind when it failed, because the map is the only useful evidence
    // and regenerating it by hand is not something anyone should have to do.
    std::error_code code;
    if (compiled->faces > 0 && !compiled->leaked) {
        std::filesystem::remove(path, code);
        std::filesystem::remove(bsp, code);
    }
}

TEST_CASE("an unsealed room leaks, and says where") {
    // The other half of the loop: a room with a wall missing does not compile,
    // and what Chisel draws in the viewports is the polyline Cleave leaves
    // behind. A leak you can only be told about is a leak you have to find.
    using namespace kero::chisel;
    using kero::math::Aabbd;
    using kero::map::Vec3d;

    chisel::Document document;

    const f64 inner = 256.0;
    const f64 thickness = 16.0;
    const f64 top = 128.0;

    const auto shell = [&](const Aabbd& bounds) {
        map::Solid solid = make_box(bounds, document, "dev/grid");
        REQUIRE(map::encloses_volume(solid));
        document.apply(std::make_unique<AddSolid>(document.map().world.id,
                                                  std::move(solid), "Draw block"));
    };

    shell(Aabbd(Vec3d(-thickness, -thickness, -thickness),
                Vec3d(inner + thickness, inner + thickness, 0)));
    shell(Aabbd(Vec3d(-thickness, -thickness, top),
                Vec3d(inner + thickness, inner + thickness, top + thickness)));
    shell(Aabbd(Vec3d(-thickness, -thickness, 0), Vec3d(0, inner + thickness, top)));
    shell(Aabbd(Vec3d(inner, -thickness, 0),
                Vec3d(inner + thickness, inner + thickness, top)));
    shell(Aabbd(Vec3d(0, -thickness, 0), Vec3d(inner, 0, top)));
    // The north wall is simply not drawn.

    map::Entity start;
    start.id = document.allocate_id();
    start.classname = "info_player_start";
    start.set("classname", "info_player_start");
    start.set("origin", "128 128 40");
    document.apply(std::make_unique<AddEntity>(std::move(start), "Place entity"));

    const std::filesystem::path path =
        std::filesystem::temp_directory_path() / "kerosene_chisel_leak.kmap";
    std::string error;
    REQUIRE_MESSAGE(document.save_as(path.string(), error), error);

    cleave::Options options;
    options.allow_leaks = true;  // As the editor runs it: report, do not refuse.
    const auto compiled = cleave::run(path.string(), options);
    if (!compiled) {
        FAIL("cleave failed on the unsealed room: ", compiled.error().message);
    }

    CHECK(compiled->leaked);
    CHECK(compiled->leak_entity.find("info_player_start") != std::string::npos);
    REQUIRE_FALSE(compiled->leak_path_file.empty());

    const std::vector<kero::math::Vec3d> leak =
        load_leak_path(compiled->leak_path_file);
    CHECK(leak.size() >= 2);

    // It runs from inside the room out through the missing wall, which is what
    // makes it a line worth following. The points are portal centres rather
    // than the entity's own position, so the first is inside rather than on it.
    CHECK(leak.front().y <= inner);
    CHECK(leak.front().z > 0.0);
    CHECK(leak.back().y > inner);

    // Nothing to draw once the level is sealed: Cleave removes a stale file.
    CHECK(load_leak_path((std::filesystem::temp_directory_path() / "nope.kleak").string())
              .empty());

    std::error_code code;
    std::filesystem::remove(path, code);
    std::filesystem::remove(compiled->leak_path_file, code);
    std::filesystem::path bsp = path;
    bsp.replace_extension(".kbsp");
    std::filesystem::remove(bsp, code);
}
