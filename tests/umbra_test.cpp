// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "bsp/file.hpp"
#include "umbra/umbra.hpp"

#include <bit>
#include <string>

using namespace kero;
using namespace kero::umbra;

namespace {

/// An axis-aligned quad, wound so its own plane normal points along +axis.
///
/// The winding order is what tells Umbra which cluster is in front, so it is
/// derived the same way the map generator and Cleave derive theirs rather than
/// guessed.
Windingd quad(usize axis, f64 at, f64 a1, f64 a2, f64 b1, f64 b2) {
    // For a plane on `axis`, the other two axes in order are (axis+1, axis+2)
    // for the winding to come out facing +axis.
    const usize u = (axis + 1) % 3;
    const usize v = (axis + 2) % 3;

    auto make = [&](f64 uu, f64 vv) {
        Vec3d point;
        point[axis] = at;
        point[u] = uu;
        point[v] = vv;
        return point;
    };

    // Chosen so cross(p0 - p1, p2 - p1) points along +axis.
    return Windingd({make(a2, b2), make(a2, b1), make(a1, b1), make(a1, b2)});
}

/// The corridor fixture: two rooms joined by a corridor that turns twice.
///
/// ```
///        room A            leg 1              leg 3         room B
///     x 0..100  --P0--> x 100..300  --P1--> x 300..500 --P3--> x 500..600
///     y 0..100          y 40..60      |     y 180..200        y 100..300
///                                     P2 up through leg 2
/// ```
///
/// The point of the shape: a straight line cannot pass through all four
/// openings. Room A can see the first two clusters and nothing beyond, which is
/// a fact about geometry that only an exact method can discover -- a
/// conservative one reports the whole level.
PortalFile corridor_fixture() {
    PortalFile file;
    file.cluster_count = 5;

    // 0 room A, 1 leg 1, 2 leg 2 (the vertical jog), 3 leg 3, 4 room B.
    // Each entry's winding faces its `front` cluster.
    file.entries.push_back({1, 0, quad(0, 100.0, 40.0, 60.0, 0.0, 64.0)});
    file.entries.push_back({2, 1, quad(1, 60.0, 0.0, 64.0, 280.0, 300.0)});
    file.entries.push_back({3, 2, quad(0, 300.0, 180.0, 200.0, 0.0, 64.0)});
    file.entries.push_back({4, 3, quad(0, 500.0, 180.0, 200.0, 0.0, 64.0)});
    return file;
}

bool sees(const Visibility& visibility, usize from, usize to) {
    return (visibility.pvs[from][to >> 3] & (1u << (to & 7u))) != 0;
}

bool hears(const Visibility& visibility, usize from, usize to) {
    return (visibility.pas[from][to >> 3] & (1u << (to & 7u))) != 0;
}

usize visible_count(const Visibility& visibility, usize from) {
    usize total = 0;
    for (u8 byte : visibility.pvs[from]) {
        total += static_cast<usize>(std::popcount(byte));
    }
    return total;
}

}  // namespace

TEST_CASE("the portal file round-trips through its text form") {
    const std::string text =
        "KPRT1\n"
        "3\n"
        "2\n"
        "4 1 0 (100 60 64) (100 60 0) (100 40 0) (100 40 64)\n"
        "4 2 1 (300 60 64) (300 60 0) (300 40 0) (300 40 64)\n";

    auto file = parse_portals(text, "test.kprt");
    if (!file) {
        FAIL("portal file did not parse: ", file.error());
    }

    CHECK(file->cluster_count == 3);
    REQUIRE(file->entries.size() == 2);
    CHECK(file->entries[0].front == 1);
    CHECK(file->entries[0].back == 0);
    CHECK(file->entries[0].winding.size() == 4);
    CHECK(file->entries[1].winding[0].x == doctest::Approx(300.0));
}

TEST_CASE("a portal file that is not one is rejected") {
    SUBCASE("wrong magic") {
        auto file = parse_portals("PRT1\n1\n0\n", "test.kprt");
        REQUIRE_FALSE(file.has_value());
        CHECK(file.error().find("not a Kerosene portal file") != std::string::npos);
    }
    SUBCASE("a cluster that does not exist") {
        auto file = parse_portals(
            "KPRT1\n2\n1\n4 9 0 (0 0 0) (0 0 1) (0 1 1) (0 1 0)\n", "test.kprt");
        REQUIRE_FALSE(file.has_value());
        CHECK(file.error().find("only 2") != std::string::npos);
    }
    SUBCASE("a portal with too few points") {
        auto file = parse_portals("KPRT1\n2\n1\n2 1 0 (0 0 0) (0 0 1)\n", "test.kprt");
        REQUIRE_FALSE(file.has_value());
        CHECK(file.error().find("at least 3") != std::string::npos);
    }
    SUBCASE("truncated") {
        auto file = parse_portals("KPRT1\n2\n1\n", "test.kprt");
        REQUIRE_FALSE(file.has_value());
    }
}

TEST_CASE("a cluster always sees itself") {
    Stats stats;
    const Visibility visibility = compute(corridor_fixture(), Options{}, stats);
    for (usize c = 0; c < visibility.cluster_count; ++c) {
        CAPTURE(c);
        CHECK(sees(visibility, c, c));
    }
}

TEST_CASE("visibility is mutual") {
    Stats stats;
    const Visibility visibility = compute(corridor_fixture(), Options{}, stats);

    // If A can see B then B can see A. Not an implementation detail -- it is
    // what "there is a line between them" means -- so an asymmetry is a bug.
    for (usize a = 0; a < visibility.cluster_count; ++a) {
        for (usize b = 0; b < visibility.cluster_count; ++b) {
            CAPTURE(a);
            CAPTURE(b);
            CHECK(sees(visibility, a, b) == sees(visibility, b, a));
        }
    }
}

TEST_CASE("neighbours through a portal always see each other") {
    Stats stats;
    const PortalFile file = corridor_fixture();
    const Visibility visibility = compute(file, Options{}, stats);

    for (const PortalFile::Entry& entry : file.entries) {
        CAPTURE(entry.front);
        CAPTURE(entry.back);
        CHECK(sees(visibility, static_cast<usize>(entry.front),
                   static_cast<usize>(entry.back)));
    }
}

TEST_CASE("a corridor that turns twice actually occludes") {
    Stats stats;
    const Visibility visibility = compute(corridor_fixture(), Options{}, stats);

    // Room A sees the first leg and, along it, the jog -- a shallow enough line
    // reaches both. It cannot see past the second turn.
    CHECK(sees(visibility, 0, 1));
    CHECK_FALSE(sees(visibility, 0, 3));
    CHECK_FALSE(sees(visibility, 0, 4));

    // And the far room cannot see back down the first leg.
    CHECK_FALSE(sees(visibility, 4, 1));
    CHECK_FALSE(sees(visibility, 4, 0));

    // Which is the whole claim: not everything is visible from everywhere.
    CHECK(visible_count(visibility, 0) < visibility.cluster_count);
}

TEST_CASE("the flow sees less than base vis, and base vis never sees less") {
    const PortalFile file = corridor_fixture();

    Stats fast_stats;
    Options fast;
    fast.fast = true;
    const Visibility base = compute(file, fast, fast_stats);

    Stats full_stats;
    const Visibility flowed = compute(file, Options{}, full_stats);

    // Base vis over-reports, which is the safe direction: too large a PVS costs
    // frame rate, too small a one puts holes in the world. So everything the
    // flow finds visible must also be in base vis.
    for (usize a = 0; a < file.cluster_count; ++a) {
        for (usize b = 0; b < file.cluster_count; ++b) {
            CAPTURE(a);
            CAPTURE(b);
            if (sees(flowed, a, b)) {
                CHECK(sees(base, a, b));
            }
        }
    }

    CHECK(full_stats.average_visible <= fast_stats.average_base);
    // And on this fixture the expensive pass buys something real.
    CHECK(full_stats.average_visible < full_stats.average_base);
}

TEST_CASE("the audible set is at least the visible set") {
    Stats stats;
    const Visibility visibility = compute(corridor_fixture(), Options{}, stats);

    for (usize a = 0; a < visibility.cluster_count; ++a) {
        for (usize b = 0; b < visibility.cluster_count; ++b) {
            CAPTURE(a);
            CAPTURE(b);
            if (sees(visibility, a, b)) {
                CHECK(hears(visibility, a, b));
            }
        }
    }

    // Sound goes round a corner: the room past the second turn cannot be seen
    // from room A, but it can be heard, because something in between can see
    // both.
    CHECK_FALSE(sees(visibility, 0, 3));
    CHECK(hears(visibility, 0, 3));
}

TEST_CASE("a level with no portals gives every cluster itself and nothing else") {
    PortalFile file;
    file.cluster_count = 3;

    Stats stats;
    const Visibility visibility = compute(file, Options{}, stats);
    for (usize c = 0; c < 3; ++c) {
        CHECK(visible_count(visibility, c) == 1);
        CHECK(sees(visibility, c, c));
    }
}

TEST_CASE("the visibility lump round-trips through its compressed form") {
    Stats stats;
    const Visibility visibility = compute(corridor_fixture(), Options{}, stats);
    const std::vector<std::byte> lump = serialise(visibility);

    CHECK(bsp::visibility_cluster_count(lump) == visibility.cluster_count);

    for (usize c = 0; c < visibility.cluster_count; ++c) {
        CAPTURE(c);
        std::vector<u8> row;
        REQUIRE(bsp::decode_visibility(lump, static_cast<i32>(c), bsp::kVisPvs,
                                       visibility.cluster_count, row));
        CHECK(row == visibility.pvs[c]);

        std::vector<u8> audible;
        REQUIRE(bsp::decode_visibility(lump, static_cast<i32>(c), bsp::kVisPas,
                                       visibility.cluster_count, audible));
        CHECK(audible == visibility.pas[c]);
    }
}

TEST_CASE("run-length encoding shrinks a sparse level") {
    // Many clusters, each seeing only itself: the case the encoding exists for.
    PortalFile file;
    file.cluster_count = 512;

    Stats stats;
    const Visibility visibility = compute(file, Options{}, stats);
    const std::vector<std::byte> lump = serialise(visibility);

    const usize uncompressed = 512 * ((512 + 7) / 8) * 2;
    CHECK(lump.size() < uncompressed / 4);
}

TEST_CASE("a missing or unreadable visibility lump means draw everything") {
    std::vector<u8> row;

    SUBCASE("an empty lump") {
        CHECK_FALSE(bsp::decode_visibility({}, 0, bsp::kVisPvs, 16, row));
        // All ones: an unvised level draws everything rather than nothing. A
        // visibility bug should look slow, not broken.
        for (u8 byte : row) {
            CHECK(byte == 0xFF);
        }
    }

    SUBCASE("a cluster outside the lump") {
        Stats stats;
        const Visibility visibility = compute(corridor_fixture(), Options{}, stats);
        const std::vector<std::byte> lump = serialise(visibility);
        CHECK_FALSE(bsp::decode_visibility(lump, 99, bsp::kVisPvs, 5, row));
        CHECK(row[0] == 0xFF);
    }

    SUBCASE("a leaf with no cluster, as a solid leaf has") {
        CHECK_FALSE(bsp::decode_visibility({}, -1, bsp::kVisPvs, 8, row));
    }
}
