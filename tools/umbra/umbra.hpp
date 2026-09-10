// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"
#include "math/plane.hpp"
#include "math/winding.hpp"

#include <expected>
#include <string>
#include <vector>

/// Umbra -- the potentially visible set. The `vvis` analogue.
///
/// The question it answers is "from anywhere in cluster A, can any part of
/// cluster B be seen?", for every pair. The answer is computed once, at build
/// time, and the engine then draws only what the answer allows. That is the
/// single largest thing a brush-based engine does for its own frame rate, and
/// it is why the compile is allowed to be slow.
///
/// The method is portal flow, and it is exact rather than conservative: sight
/// is traced through chains of portals, clipping the sight-lines against the
/// *separating planes* between each pair of openings. A corridor that bends
/// twice really does occlude, and a conservative method would not notice.
///
/// Two stages, because the second is expensive and the first is not:
///
///   * **Base vis** asks only whether two portals face each other at all. It is
///     quick, it over-reports, and it is what `--fast` stops at -- enough to
///     see the shape of the thing while a layout is still moving.
///   * **Full vis** does the recursive flow, using base vis to prune. Every
///     portal is an independent job, which is where the machine's cores go.
namespace kero::umbra {

using math::Planed;
using math::Vec3d;
using math::Windingd;

/// One entry of a `.kprt` file: an opening between two clusters.
struct PortalFile {
    usize cluster_count = 0;
    struct Entry {
        i32 front = -1;
        i32 back = -1;
        Windingd winding;
    };
    std::vector<Entry> entries;
};

[[nodiscard]] std::expected<PortalFile, std::string> parse_portals(std::string_view text,
                                                                   std::string_view name);
[[nodiscard]] std::expected<PortalFile, std::string> load_portals(const std::string& path);

struct Options {
    /// Stop after base vis. Over-reports what is visible, which is safe -- a
    /// PVS that is too large costs frame rate, one that is too small makes
    /// holes in the world.
    bool fast = false;
};

struct Stats {
    usize clusters = 0;
    usize portals = 0;
    /// Average clusters visible from a cluster, before and after the flow. The
    /// gap between the two is what the expensive pass bought.
    f64 average_base = 0.0;
    f64 average_visible = 0.0;
    usize visibility_bytes = 0;
    f64 seconds = 0.0;
};

/// The computed sets, one bitset per cluster.
struct Visibility {
    usize cluster_count = 0;
    /// Row `c` is the set of clusters visible from cluster `c`.
    std::vector<std::vector<u8>> pvs;
    /// The audible set: looser, and deliberately so. Sound goes round corners,
    /// so a source the player cannot see may still need mixing. Source keeps
    /// this distinction and it is why audio does not cut out when you step
    /// behind a pillar.
    std::vector<std::vector<u8>> pas;
};

[[nodiscard]] Visibility compute(const PortalFile& portals, const Options& options,
                                 Stats& stats);

/// Packs the sets into the Visibility lump's layout, run-length encoded.
[[nodiscard]] std::vector<std::byte> serialise(const Visibility& visibility);

/// Runs the stage over a `.kbsp`, reading the `.kprt` beside it and writing the
/// visibility lump back into the file.
[[nodiscard]] std::expected<Stats, std::string> run(const std::string& bsp_path,
                                                    const Options& options);

}  // namespace kero::umbra
