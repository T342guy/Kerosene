// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "asset/devtexture.hpp"

#include <set>
#include <string>

using namespace kero;
using namespace kero::asset;

TEST_CASE("a developer texture is the size it says it is") {
    const std::vector<u8> pixels = dev_texture("dev/wall");
    CHECK(pixels.size() == static_cast<usize>(kDevTextureSize) * kDevTextureSize * 4);

    // Fully opaque throughout: a stand-in texture that was accidentally
    // transparent would look like a hole in the level.
    for (usize i = 3; i < pixels.size(); i += 4) {
        REQUIRE(pixels[i] == 255);
    }
}

TEST_CASE("the colour is stable, so a material looks the same every run") {
    // Not just deterministic within a run -- specified. std::hash is neither,
    // and a level whose materials changed colour between builds would be
    // unrecognisable from one day to the next.
    CHECK(dev_colour("dev/wall").r == dev_colour("dev/wall").r);
    CHECK(dev_colour("dev/wall").g == dev_colour("dev/wall").g);

    const Colour wall = dev_colour("dev/wall");
    const Colour floor = dev_colour("dev/floor");
    CHECK((wall.r != floor.r || wall.g != floor.g || wall.b != floor.b));
}

TEST_CASE("different materials get visibly different colours") {
    // The point of the tint: telling one surface from another at a glance.
    const std::vector<std::string> materials{
        "dev/wall", "dev/floor", "dev/ceiling", "dev/grid",
        "dev/grey", "dev/orange", "dev/measure", "tools/trigger"};

    std::set<std::tuple<u8, u8, u8>> seen;
    for (const std::string& material : materials) {
        const Colour colour = dev_colour(material);
        seen.emplace(colour.r, colour.g, colour.b);
    }
    CHECK(seen.size() == materials.size());
}

TEST_CASE("nothing comes out too dark to read a grid line against") {
    for (const char* material : {"dev/wall", "dev/floor", "a", "", "tools/nodraw"}) {
        const Colour colour = dev_colour(material);
        CAPTURE(material);
        CHECK(colour.r >= 96);
        CHECK(colour.g >= 96);
        CHECK(colour.b >= 96);
    }
}

TEST_CASE("the grid lines are actually there") {
    const std::vector<u8> pixels = dev_texture("dev/grid");

    // A pixel on a grid line is darker than one in the middle of a cell. This
    // is what gives a surface its sense of scale.
    const auto luminance = [&pixels](u32 x, u32 y) {
        const usize offset = (static_cast<usize>(y) * kDevTextureSize + x) * 4;
        return static_cast<u32>(pixels[offset]) + pixels[offset + 1] + pixels[offset + 2];
    };

    CHECK(luminance(0, 8) < luminance(8, 8));
    CHECK(luminance(8, 0) < luminance(8, 8));
    // And the checkerboard alternates between cells.
    CHECK(luminance(8, 8) != luminance(24, 8));
}
