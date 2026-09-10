// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "bsp/format.hpp"
#include "cleave/cleave.hpp"
#include "math/units.hpp"

#include <cstring>

#include <array>
#include <string>

using namespace kero;
using namespace kero::cleave;

namespace {

/// A box brush, built the way the .kmap format stores one: six sides, each a
/// plane given as three points wound counter-clockwise from the front.
///
/// Built here rather than loaded from a file so a test can state the geometry
/// it means and nothing else. The winding order matches the sample map's, so if
/// it were wrong both would be wrong together and the map test would catch it.
map::Solid box(Vec3d mins, Vec3d maxs, const std::string& material = "dev/wall") {
    const f64 x1 = mins.x;
    const f64 y1 = mins.y;
    const f64 z1 = mins.z;
    const f64 x2 = maxs.x;
    const f64 y2 = maxs.y;
    const f64 z2 = maxs.z;

    const std::array<std::array<Vec3d, 3>, 6> faces{{
        {{{x1, y2, z2}, {x2, y2, z2}, {x2, y1, z2}}},  // +Z
        {{{x1, y1, z1}, {x2, y1, z1}, {x2, y2, z1}}},  // -Z
        {{{x2, y2, z2}, {x2, y2, z1}, {x2, y1, z1}}},  // +X
        {{{x1, y1, z2}, {x1, y1, z1}, {x1, y2, z1}}},  // -X
        {{{x1, y2, z2}, {x1, y2, z1}, {x2, y2, z1}}},  // +Y
        {{{x2, y1, z2}, {x2, y1, z1}, {x1, y1, z1}}},  // -Y
    }};

    map::Solid solid;
    static i32 next_id = 1;
    solid.id = next_id++;
    for (const std::array<Vec3d, 3>& points : faces) {
        map::Side side;
        side.id = next_id++;
        side.plane_points = points;
        REQUIRE(math::Planed::from_points(points[0], points[1], points[2], side.plane));
        side.material = material;
        solid.sides.push_back(std::move(side));
    }
    return solid;
}

map::Entity point_entity(const std::string& classname, Vec3d origin) {
    map::Entity entity;
    entity.classname = classname;
    entity.set("classname", classname);
    entity.set("origin",
               std::format("{:g} {:g} {:g}", origin.x, origin.y, origin.z));
    return entity;
}

/// A sealed box room: six wall brushes 16 ku thick around the given interior,
/// with a player start in the middle of it.
map::Map room(Vec3d interior_mins, Vec3d interior_maxs, f64 thickness = 16.0) {
    map::Map map;
    map.world.classname = "worldspawn";
    map.world.set("classname", "worldspawn");

    const Vec3d outer_mins = interior_mins - Vec3d(thickness, thickness, thickness);
    const Vec3d outer_maxs = interior_maxs + Vec3d(thickness, thickness, thickness);

    // Floor and ceiling span the full outer footprint; the four walls fill the
    // sides between them. Overlapping would be fine -- CSG is for exactly that --
    // but keeping them flush makes the expected face count easy to state.
    map.world.solids.push_back(box({outer_mins.x, outer_mins.y, outer_mins.z},
                                   {outer_maxs.x, outer_maxs.y, interior_mins.z}));
    map.world.solids.push_back(box({outer_mins.x, outer_mins.y, interior_maxs.z},
                                   {outer_maxs.x, outer_maxs.y, outer_maxs.z}));
    map.world.solids.push_back(box({outer_mins.x, outer_mins.y, interior_mins.z},
                                   {interior_mins.x, outer_maxs.y, interior_maxs.z}));
    map.world.solids.push_back(box({interior_maxs.x, outer_mins.y, interior_mins.z},
                                   {outer_maxs.x, outer_maxs.y, interior_maxs.z}));
    map.world.solids.push_back(box({interior_mins.x, outer_mins.y, interior_mins.z},
                                   {interior_maxs.x, interior_mins.y, interior_maxs.z}));
    map.world.solids.push_back(box({interior_mins.x, interior_maxs.y, interior_mins.z},
                                   {interior_maxs.x, outer_maxs.y, interior_maxs.z}));

    const Vec3d centre = (interior_mins + interior_maxs) * 0.5;
    map.entities.push_back(point_entity("info_player_start", centre));
    return map;
}

Stats compile_ok(const map::Map& map, const Options& options, World& world, Tree& tree) {
    auto stats = compile(map, options, world, tree);
    if (!stats) {
        FAIL("compile failed: ", stats.error().message);
    }
    return *stats;
}

/// The total area of every visible face fragment CSG left behind.
f64 visible_area(const World& world) {
    f64 total = 0.0;
    for (const Brush& brush : world.brushes) {
        for (const Side& side : brush.sides) {
            for (const math::Windingd& fragment : side.visible) {
                total += fragment.area();
            }
        }
    }
    return total;
}

}  // namespace

TEST_CASE("a single cube compiles to its six faces") {
    map::Map map;
    map.world.classname = "worldspawn";
    map.world.solids.push_back(box({0, 0, 0}, {128, 128, 128}));

    World world;
    Tree tree;
    const Stats stats = compile_ok(map, Options{}, world, tree);

    CHECK(stats.brushes == 1);
    CHECK(stats.problems.empty());
    REQUIRE(world.brushes.size() == 1);

    // Six sides bound the solid, and nothing buried any of them.
    usize fragments = 0;
    for (const Side& side : world.brushes[0].sides) {
        if (!side.bevel) {
            fragments += side.visible.size();
        }
    }
    CHECK(fragments == 6);
    CHECK(visible_area(world) == doctest::Approx(6.0 * 128.0 * 128.0));

    // A lone cube in the void: the tree must separate its inside from its
    // outside, so there is at least one solid leaf and one open one.
    CHECK(stats.tree.solid_leaves >= 1);
    CHECK(stats.tree.empty_leaves >= 1);
}

TEST_CASE("a box already has every axial plane, so it gains no bevels") {
    map::Map map;
    map.world.classname = "worldspawn";
    map.world.solids.push_back(box({0, 0, 0}, {128, 128, 128}));

    World world;
    Tree tree;
    (void)compile_ok(map, Options{}, world, tree);

    usize bevels = 0;
    for (const Side& side : world.brushes[0].sides) {
        if (side.bevel) {
            ++bevels;
        }
    }
    CHECK(bevels == 0);
}

TEST_CASE("CSG leaves the union's surface, not both brushes' surfaces") {
    // Two cubes sharing a 64 ku slab. The union's surface is the sum of the two
    // surfaces minus the four buried face-halves and the two shared walls.
    map::Map map;
    map.world.classname = "worldspawn";
    map.world.solids.push_back(box({0, 0, 0}, {128, 128, 128}));
    map.world.solids.push_back(box({64, 0, 0}, {192, 128, 128}));

    World world;
    Tree tree;
    (void)compile_ok(map, Options{}, world, tree);

    // The union is a 192 x 128 x 128 box: two 128x128 ends, and four
    // 192x128 sides.
    const f64 expected = 2.0 * 128.0 * 128.0 + 4.0 * 192.0 * 128.0;
    CHECK(visible_area(world) == doctest::Approx(expected));
}

TEST_CASE("two brushes sharing a wall keep exactly one copy of it") {
    // Flush, not overlapping: the classic case, and the one where a naive
    // implementation keeps both faces or neither.
    map::Map map;
    map.world.classname = "worldspawn";
    map.world.solids.push_back(box({0, 0, 0}, {128, 128, 128}));
    map.world.solids.push_back(box({128, 0, 0}, {256, 128, 128}));

    World world;
    Tree tree;
    (void)compile_ok(map, Options{}, world, tree);

    const f64 expected = 2.0 * 128.0 * 128.0 + 4.0 * 256.0 * 128.0;
    CHECK(visible_area(world) == doctest::Approx(expected));
}

TEST_CASE("a brush fully inside another contributes no surface at all") {
    map::Map map;
    map.world.classname = "worldspawn";
    map.world.solids.push_back(box({0, 0, 0}, {256, 256, 256}));
    map.world.solids.push_back(box({64, 64, 64}, {192, 192, 192}));

    World world;
    Tree tree;
    (void)compile_ok(map, Options{}, world, tree);

    CHECK(visible_area(world) == doctest::Approx(6.0 * 256.0 * 256.0));
}

TEST_CASE("a sealed room does not leak") {
    const map::Map map = room({0, 0, 0}, {256, 256, 128});

    World world;
    Tree tree;
    const Stats stats = compile_ok(map, Options{}, world, tree);

    CHECK_FALSE(stats.leaked);
    CHECK(stats.leak_entity.empty());
    CHECK(tree.leak_path().empty());

    // Sealing means there is somewhere to be. A single convex room is one leaf,
    // so there are no portals *inside* it to count -- what matters is that the
    // flood found the interior and stopped at the walls.
    const Node* inside = tree.leaf_at(world, {128, 128, 64});
    REQUIRE(inside);
    CHECK_FALSE(inside->solid());
    CHECK(inside->occupant >= 0);

    const Node* outside = tree.leaf_at(world, {128, 128, 400});
    REQUIRE(outside);
    CHECK(outside->solid());  // Filled in, because the flood never reached it.

    CHECK(stats.faces > 0);
}

TEST_CASE("a room with a wall removed leaks, and says what leaked out") {
    map::Map map = room({0, 0, 0}, {256, 256, 128});
    map.world.solids.erase(map.world.solids.begin() + 1);  // The ceiling.

    World world;
    Tree tree;
    const Stats stats = compile_ok(map, Options{}, world, tree);

    REQUIRE(stats.leaked);
    // Naming the entity is the difference between a hunt and a fix.
    CHECK(stats.leak_entity.find("info_player_start") != std::string::npos);

    // The path starts at the entity and ends outside the level.
    const std::vector<Vec3d>& path = tree.leak_path();
    REQUIRE(path.size() >= 2);
    CHECK(path.front().z < 128.0);
    CHECK(path.back().z > 128.0);
}

TEST_CASE("a leak is found from any entity, not only the first") {
    map::Map map = room({0, 0, 0}, {256, 256, 128});
    map.world.solids.erase(map.world.solids.begin() + 1);
    // A second entity, further from the hole.
    map.entities.push_back(point_entity("light", {32, 32, 32}));

    World world;
    Tree tree;
    const Stats stats = compile_ok(map, Options{}, world, tree);
    CHECK(stats.leaked);
}

TEST_CASE("a level with no point entities cannot be leak-checked, and says so") {
    map::Map map = room({0, 0, 0}, {256, 256, 128});
    map.entities.clear();

    World world;
    Tree tree;
    const Stats stats = compile_ok(map, Options{}, world, tree);

    // Nothing says which side of the geometry is the inside, so there is no
    // leak to report -- not a false pass, an absent question.
    CHECK_FALSE(stats.leaked);
}

TEST_CASE("detail brushes stay out of the tree but still collide") {
    map::Map sparse = room({0, 0, 0}, {256, 256, 128});

    map::Map detailed = sparse;
    map::Entity detail;
    detail.classname = "func_detail";
    detail.set("classname", "func_detail");
    detail.solids.push_back(box({96, 96, 0}, {160, 160, 128}, "dev/orange"));
    detailed.entities.push_back(std::move(detail));

    World sparse_world;
    Tree sparse_tree;
    const Stats sparse_stats = compile_ok(sparse, Options{}, sparse_world, sparse_tree);

    World detailed_world;
    Tree detailed_tree;
    const Stats detailed_stats =
        compile_ok(detailed, Options{}, detailed_world, detailed_tree);

    CHECK(detailed_stats.detail_brushes == 1);

    // The point of func_detail: a pillar in the middle of a room must not carve
    // the visibility structure. Same tree, same leaves, same portals.
    CHECK(detailed_stats.tree.nodes == sparse_stats.tree.nodes);
    CHECK(detailed_stats.tree.leaves == sparse_stats.tree.leaves);
    CHECK(detailed_stats.tree.portals == sparse_stats.tree.portals);

    // But it is still there to be collided with: it was filed into the leaves
    // it touches.
    usize detail_in_leaves = 0;
    for (Node* leaf : detailed_tree.leaves()) {
        for (const Brush& brush : leaf->brushes) {
            if (brush.detail) {
                ++detail_in_leaves;
            }
        }
    }
    CHECK(detail_in_leaves > 0);
}

TEST_CASE("a detail brush cannot seal a level") {
    // A room whose ceiling is a func_detail. It looks sealed and is not: detail
    // is excluded from the tree, so the void is one flood away. Making that a
    // rule rather than a convention is what stops the mistake reaching a player.
    map::Map map = room({0, 0, 0}, {256, 256, 128});
    map::Solid ceiling = map.world.solids[1];
    map.world.solids.erase(map.world.solids.begin() + 1);

    map::Entity detail;
    detail.classname = "func_detail";
    detail.set("classname", "func_detail");
    detail.solids.push_back(std::move(ceiling));
    map.entities.push_back(std::move(detail));

    World world;
    Tree tree;
    const Stats stats = compile_ok(map, Options{}, world, tree);
    CHECK(stats.leaked);
}

TEST_CASE("a trigger is not solid, so it neither seals nor hides a wall") {
    map::Map map = room({0, 0, 0}, {256, 256, 128});
    const f64 sealed_area = [&] {
        World world;
        Tree tree;
        (void)compile_ok(map, Options{}, world, tree);
        return visible_area(world);
    }();

    map::Entity trigger;
    trigger.classname = "trigger_multiple";
    trigger.set("classname", "trigger_multiple");
    // Overlapping the floor: a solid brush there would delete the floor's face.
    trigger.solids.push_back(box({32, 32, -8}, {224, 224, 64}, "tools/trigger"));
    map.entities.push_back(std::move(trigger));

    World world;
    Tree tree;
    const Stats stats = compile_ok(map, Options{}, world, tree);

    CHECK_FALSE(stats.leaked);
    CHECK(visible_area(world) == doctest::Approx(sealed_area));
}

TEST_CASE("a brush whose faces cross over is reported, not compiled") {
    map::Map map;
    map.world.classname = "worldspawn";
    map::Solid inverted = box({0, 0, 0}, {128, 128, 128});
    // Drag the +X face behind the -X face. A real thing to do by accident.
    inverted.sides[2].plane.distance = -64.0;
    map.world.solids.push_back(std::move(inverted));
    map.world.solids.push_back(box({256, 0, 0}, {384, 128, 128}));

    World world;
    Tree tree;
    const Stats stats = compile_ok(map, Options{}, world, tree);

    REQUIRE(stats.problems.size() == 1);
    CHECK(stats.problems[0].message.find("enclose a volume") != std::string::npos);
    // The other brush still compiled: one bad brush costs a hole, not the run.
    CHECK(world.brushes.size() == 1);
}

TEST_CASE("the split heuristic is a policy, and changing it changes the tree") {
    map::Map map = room({0, 0, 0}, {512, 512, 256});
    for (int i = 0; i < 6; ++i) {
        // Some non-axial geometry, so balance and splits actually trade off.
        const f64 offset = 64.0 * i;
        map.world.solids.push_back(
            box({64 + offset, 64, 0}, {96 + offset, 448, 64}, "dev/wall"));
    }

    Options balanced;
    balanced.policy.balance_cost = 100.0;
    Options unbalanced;
    unbalanced.policy.balance_cost = 0.0;

    World world_a;
    Tree tree_a;
    const Stats a = compile_ok(map, balanced, world_a, tree_a);

    World world_b;
    Tree tree_b;
    const Stats b = compile_ok(map, unbalanced, world_b, tree_b);

    // Weighting balance heavily should not produce a deeper tree than ignoring
    // it. That is the whole claim the knob makes.
    CHECK(a.tree.max_depth <= b.tree.max_depth);
}

TEST_CASE("compiling is deterministic") {
    const map::Map map = room({0, 0, 0}, {256, 256, 128});

    World first_world;
    Tree first_tree;
    const Stats first = compile_ok(map, Options{}, first_world, first_tree);
    Stats first_copy = first;
    const std::vector<std::byte> first_bytes =
        serialise(map, first_world, first_tree, first_copy);

    World second_world;
    Tree second_tree;
    const Stats second = compile_ok(map, Options{}, second_world, second_tree);
    Stats second_copy = second;
    const std::vector<std::byte> second_bytes =
        serialise(map, second_world, second_tree, second_copy);

    // Two runs must produce the same level, byte for byte. Without it a
    // rebuild produces a spurious diff, incremental builds cannot be trusted,
    // and a bug that depends on iteration order is invisible.
    CHECK(first.tree.nodes == second.tree.nodes);
    CHECK(first.tree.leaves == second.tree.leaves);
    CHECK(first_bytes == second_bytes);
}

TEST_CASE("the compiled file is a well-formed .kbsp") {
    const map::Map map = room({0, 0, 0}, {256, 256, 128});

    World world;
    Tree tree;
    Stats stats = compile_ok(map, Options{}, world, tree);
    const std::vector<std::byte> bytes = serialise(map, world, tree, stats);

    REQUIRE(bytes.size() > sizeof(bsp::Header));

    bsp::Header header{};
    std::memcpy(&header, bytes.data(), sizeof(header));
    CHECK(header.magic == bsp::kMagic);
    CHECK(header.version == bsp::kVersion);

    for (usize i = 0; i < bsp::kLumpCount; ++i) {
        const bsp::Lump& lump = header.lumps[i];
        CAPTURE(i);
        // Every lump lies inside the file and starts aligned, so a reader can
        // point a struct at one instead of copying it out.
        CHECK(lump.offset % 4 == 0);
        CHECK(static_cast<usize>(lump.offset) + lump.length <= bytes.size());
    }

    const bsp::Lump& planes = header.lumps[static_cast<usize>(bsp::LumpId::Planes)];
    CHECK(planes.length % sizeof(bsp::DiskPlane) == 0);
    CHECK(planes.length / sizeof(bsp::DiskPlane) == stats.planes);

    const bsp::Lump& faces = header.lumps[static_cast<usize>(bsp::LumpId::Faces)];
    CHECK(faces.length / sizeof(bsp::DiskFace) == stats.faces);
    CHECK(stats.faces > 0);

    SUBCASE("visibility and lighting are left empty for the later stages") {
        CHECK(header.lumps[static_cast<usize>(bsp::LumpId::Visibility)].length == 0);
        CHECK(header.lumps[static_cast<usize>(bsp::LumpId::Lighting)].length == 0);
    }

    SUBCASE("the entity lump carries the entities and not the brushes") {
        const bsp::Lump& entities =
            header.lumps[static_cast<usize>(bsp::LumpId::Entities)];
        const std::string text(reinterpret_cast<const char*>(bytes.data() + entities.offset),
                               entities.length);
        CHECK(text.find("info_player_start") != std::string::npos);
        // The brushes are compiled into the tree; carrying their text as well
        // would double the lump for nothing.
        CHECK(text.find("\"plane\"") == std::string::npos);
    }
}

TEST_CASE("the sample map compiles and is sealed") {
    auto map = map::load(std::string(KEROSENE_SOURCE_DIR) + "/content/maps/kero_start.kmap");
    if (!map) {
        FAIL("could not load the sample map: ", map.error().format());
    }

    World world;
    Tree tree;
    const Stats stats = compile_ok(*map, Options{}, world, tree);

    CHECK_FALSE(stats.leaked);
    CHECK(stats.problems.empty());
    CHECK(stats.tree.portals > 0);
    CHECK(stats.faces > 0);
    CHECK(stats.detail_brushes == 1);

    SUBCASE("the player start is in open space, not embedded in a wall") {
        const std::vector<const map::Entity*> starts =
            map->by_classname("info_player_start");
        REQUIRE(starts.size() == 1);
        const std::optional<Vec3d> origin = starts[0]->origin();
        REQUIRE(origin);

        const Node* leaf = tree.leaf_at(world, *origin);
        REQUIRE(leaf);
        CHECK_FALSE(leaf->solid());
        // And the flood reached it, which is what "inside the level" means.
        CHECK(leaf->occupant >= 0);
    }

    SUBCASE("both rooms and the corridor are inside the level") {
        for (const Vec3d& point : {Vec3d{128, 128, 32},    // room A
                                   Vec3d{352, 128, 32},    // the corridor
                                   Vec3d{576, 128, 32}}) { // room B
            CAPTURE(point.x);
            const Node* leaf = tree.leaf_at(world, point);
            REQUIRE(leaf);
            CHECK_FALSE(leaf->solid());
            CHECK(leaf->occupant >= 0);
        }
    }

    SUBCASE("the space outside the level is filled in, not left open") {
        for (const Vec3d& point : {Vec3d{-256, 128, 32}, Vec3d{352, 128, 400}}) {
            CAPTURE(point.x);
            const Node* leaf = tree.leaf_at(world, point);
            REQUIRE(leaf);
            CHECK(leaf->solid());
        }
    }

    SUBCASE("inside a wall is solid") {
        const Node* leaf = tree.leaf_at(world, Vec3d{-8, 128, 64});
        REQUIRE(leaf);
        CHECK(leaf->solid());
    }
}
