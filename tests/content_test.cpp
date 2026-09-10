// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "cleave/cleave.hpp"
#include "umbra/umbra.hpp"

#include <algorithm>
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
