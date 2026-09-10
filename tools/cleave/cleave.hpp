// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "cleave/tree.hpp"

#include <expected>
#include <string>
#include <vector>

/// Cleave -- `.kmap` to `.kbsp`. The `vbsp` analogue.
///
/// Four passes, in order: build brushes from planes, chop away buried faces,
/// build the BSP tree, then portalise it and check that the level is sealed.
/// Each is separately testable and separately reportable, which matters because
/// the useful diagnostics all come from the seams between them.
namespace kero::cleave {

struct Options {
    /// Skip the leak check. For a layout that is still moving and is knowingly
    /// open on one side; a level that leaks still loads and plays.
    bool allow_leaks = false;

    /// Stop after building the tree, without portalising. Faster, and enough to
    /// see whether the geometry compiles at all.
    bool no_portals = false;

    /// Fewer, larger leaves: skips the balance term entirely. For quick
    /// iteration, not for a level being shipped.
    bool fast = false;

    SplitPolicy policy;

    /// Where to write. Empty means the input path with its extension replaced.
    std::string output;
};

struct Stats {
    usize brushes = 0;
    usize detail_brushes = 0;
    usize entities = 0;
    usize planes = 0;
    usize faces = 0;
    usize vertices = 0;
    TreeStats tree;

    /// True when the level was found to be open to the void.
    bool leaked = false;
    std::string leak_entity;
    std::string leak_path_file;

    std::vector<BrushProblem> problems;
};

struct Failure {
    std::string message;
};

/// Compiles `map` and writes the result. On a leak the `.kbsp` is still
/// written -- an unsealed level loads and plays, it just draws the void -- and
/// the leak is reported in the stats along with the `.kleak` path written
/// beside it.
[[nodiscard]] std::expected<Stats, Failure> run(const std::string& map_path,
                                                const Options& options);

/// The compile, without any file reading or writing. Exposed so the tests can
/// build a map in memory and check what came out.
[[nodiscard]] std::expected<Stats, Failure> compile(const map::Map& map,
                                                    const Options& options,
                                                    World& world, Tree& tree);

/// Serialises a compiled tree to `.kbsp` bytes.
[[nodiscard]] std::vector<std::byte> serialise(const map::Map& map, const World& world,
                                               Tree& tree, Stats& stats);

/// Writes the leak path as a `.kleak` file: a polyline a designer can load.
[[nodiscard]] bool write_leak_file(const std::string& path, const std::vector<Vec3d>& points);

}  // namespace kero::cleave
