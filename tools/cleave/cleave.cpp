// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "cleave/cleave.hpp"

#include "core/log.hpp"

#include <algorithm>
#include <chrono>
#include <filesystem>
#include <format>
#include <fstream>

namespace kero::cleave {
namespace {

KERO_LOG_CATEGORY(log, "cleave");

}  // namespace

/// The first line of a .kprt, so a file that is not one is rejected rather than
/// misread.
constexpr std::string_view kPortalMagic = "KPRT1";

namespace {

/// Assigns a cluster number to every open leaf.
///
/// A cluster is the unit the PVS is expressed in. One per open leaf for now,
/// which Umbra is free to merge: two leaves separated by a plane that no wall
/// lies on almost always see the same thing, and merging them shrinks the
/// visibility matrix quadratically. Solid leaves get -1 -- they see nothing,
/// and a player is never in one.
usize assign_clusters(Tree& tree) {
    i32 next = 0;
    for (Node* leaf : tree.leaves()) {
        leaf->cluster = leaf->solid() ? -1 : next++;
    }
    return static_cast<usize>(next);
}

std::string replace_extension(const std::string& path, std::string_view extension) {
    std::filesystem::path result(path);
    result.replace_extension(extension);
    return result.string();
}

}  // namespace

std::expected<Stats, Failure> compile(const map::Map& map, const Options& options,
                                      World& world, Tree& tree) {
    using Clock = std::chrono::steady_clock;
    const auto started = Clock::now();

    Stats stats;
    stats.entities = map.entities.size() + 1;

    world = build_world(map, stats.problems);
    for (const BrushProblem& problem : stats.problems) {
        // A warning rather than a failure: one bad brush in a thousand should
        // cost a hole in the level, not the whole compile.
        KERO_WARN(log, "brush {} in entity {}: {}", problem.map_id, problem.entity,
                  problem.message);
    }
    if (world.brushes.empty()) {
        return std::unexpected(Failure{"the map has no compilable brushes"});
    }

    for (const Brush& brush : world.brushes) {
        if (brush.detail) {
            ++stats.detail_brushes;
        }
    }
    stats.brushes = world.brushes.size();

    add_bevel_planes(world);
    chop_brushes(world);

    SplitPolicy policy = options.policy;
    if (options.fast) {
        // Balance is the expensive half of the heuristic and the half that only
        // pays off at runtime. Dropping it while a layout is still moving trades
        // a slower trace for a faster compile, which is the right way round
        // when you are about to move a wall again.
        policy.balance_cost = 0.0;
    }

    tree.build(world, policy);
    stats.tree = tree.stats();

    if (!options.no_portals) {
        tree.portalize(world);

        if (!tree.flood_entities(world, map)) {
            stats.leaked = true;
            stats.leak_entity = tree.leak_entity();
            if (!options.allow_leaks) {
                // Not a hard failure: an unsealed level still loads and plays,
                // and refusing to produce one would make the diagnostic harder
                // to act on rather than easier.
                KERO_ERROR(log,
                           "LEAK: the level is open to the void, reached from {}. "
                           "Everything after this point -- visibility and "
                           "lighting -- will be wrong until it is sealed",
                           stats.leak_entity);
            }
        } else {
            tree.fill_outside();
        }
        stats.tree = tree.stats();
    }

    stats.faces = tree.place_detail_and_faces(world);
    stats.planes = world.planes.size() / 2;
    const usize clusters = assign_clusters(tree);

    const auto elapsed = std::chrono::duration<f64>(Clock::now() - started).count();
    KERO_INFO(log, "{} clusters; compiled in {:.2f}s", clusters, elapsed);
    return stats;
}

std::string serialise_portals(const Tree& tree, usize cluster_count) {
    std::string out = std::format("{}\n{}\n", kPortalMagic, cluster_count);

    std::string body;
    usize count = 0;
    for (const std::unique_ptr<Portal>& portal : tree.portals()) {
        // Only portals between two open leaves matter: a portal onto solid is
        // not something you can see through.
        if (portal->front == nullptr || portal->back == nullptr) {
            continue;
        }
        if (portal->front->cluster < 0 || portal->back->cluster < 0) {
            continue;
        }

        body += std::format("{} {} {}", portal->winding.size(), portal->front->cluster,
                            portal->back->cluster);
        for (const Vec3d& point : portal->winding.points()) {
            body += std::format(" ({:.9g} {:.9g} {:.9g})", point.x, point.y, point.z);
        }
        body += '\n';
        ++count;
    }

    out += std::format("{}\n", count);
    out += body;
    return out;
}

bool write_leak_file(const std::string& path, const std::vector<Vec3d>& points) {
    std::ofstream file(path, std::ios::trunc);
    if (!file) {
        return false;
    }

    // A plain polyline, one point per line: the shortest path from the entity
    // that leaked out to the void. Simple on purpose -- the editor draws it,
    // and so can anything else.
    file << "// Kerosene leak path. Load it in Chisel, or read it as a polyline:\n"
         << "// it runs from the entity that leaked to the hole it escaped through.\n";
    for (const Vec3d& point : points) {
        file << std::format("{:g} {:g} {:g}\n", point.x, point.y, point.z);
    }
    return static_cast<bool>(file);
}

std::expected<Stats, Failure> run(const std::string& map_path, const Options& options) {
    auto map = map::load(map_path);
    if (!map) {
        return std::unexpected(Failure{map.error().format()});
    }

    KERO_INFO(log, "{}: {} brushes, {} sides, {} entities", map_path, map->brush_count(),
              map->side_count(), map->entities.size() + 1);

    World world;
    Tree tree;
    auto stats = compile(*map, options, world, tree);
    if (!stats) {
        return stats;
    }

    if (stats->leaked) {
        const std::string leak_path = replace_extension(map_path, ".kleak");
        if (write_leak_file(leak_path, tree.leak_path())) {
            stats->leak_path_file = leak_path;
            KERO_ERROR(log, "leak path written to {}", leak_path);
        }
    } else {
        // A stale .kleak beside a level that now compiles clean is worse than
        // none: it is a file that says the level is broken when it is not.
        std::error_code code;
        std::filesystem::remove(replace_extension(map_path, ".kleak"), code);
    }

    const std::vector<std::byte> bytes = serialise(*map, world, tree, *stats);
    const std::string output =
        options.output.empty() ? replace_extension(map_path, ".kbsp") : options.output;

    std::ofstream file(output, std::ios::binary | std::ios::trunc);
    if (!file) {
        return std::unexpected(Failure{std::format("cannot write {}", output)});
    }
    file.write(reinterpret_cast<const char*>(bytes.data()),
               static_cast<std::streamsize>(bytes.size()));
    if (!file) {
        return std::unexpected(Failure{std::format("writing {} failed", output)});
    }

    // The portal file, so Umbra can run as its own stage. Written beside the
    // .kbsp and read by exactly one thing, which is what makes that stage
    // replaceable.
    if (!options.no_portals) {
        usize clusters = 0;
        for (const std::unique_ptr<Portal>& portal : tree.portals()) {
            if (portal->front != nullptr) {
                clusters = std::max(clusters, static_cast<usize>(portal->front->cluster + 1));
            }
            if (portal->back != nullptr) {
                clusters = std::max(clusters, static_cast<usize>(portal->back->cluster + 1));
            }
        }

        const std::string portal_path = replace_extension(map_path, ".kprt");
        std::ofstream portal_file(portal_path, std::ios::trunc);
        const std::string text = serialise_portals(tree, clusters);
        portal_file << text;
        if (portal_file) {
            KERO_INFO(log, "wrote {}", portal_path);
        } else {
            KERO_WARN(log, "could not write {}; umbra will have nothing to read",
                      portal_path);
        }
    }

    KERO_INFO(log, "wrote {}", output);
    return stats;
}

}  // namespace kero::cleave
