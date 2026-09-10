// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "umbra/umbra.hpp"

#include "bsp/file.hpp"
#include "core/jobs.hpp"
#include "core/log.hpp"

#include <algorithm>
#include <bit>
#include <charconv>
#include <chrono>
#include <filesystem>
#include <format>
#include <fstream>
#include <numeric>
#include <sstream>

namespace kero::umbra {
namespace {

KERO_LOG_CATEGORY(log, "umbra");

using Tol = math::Tolerance<f64>;

/// A directed opening: standing in `from`, looking through it into `into`.
///
/// Each `.kprt` entry becomes two of these, one per direction, because
/// visibility is not symmetric during the flow even though the final answer is.
/// The plane is oriented so the destination is in front, which is what makes
/// every "can this portal see that one" test a plane-side test.
struct Portal {
    Planed plane;
    Windingd winding;
    i32 from = -1;
    i32 into = -1;

    /// Portals this one could conceivably see, from base vis. Used to prune the
    /// flow, which is the only reason the flow is affordable.
    std::vector<u8> might_see;
    /// Clusters this portal really can see, after the flow.
    std::vector<u8> can_see;
    bool flowed = false;
};

void set_bit(std::vector<u8>& bits, usize index) {
    bits[index >> 3] = static_cast<u8>(bits[index >> 3] | (1u << (index & 7u)));
}

[[nodiscard]] bool test_bit(const std::vector<u8>& bits, usize index) {
    return (bits[index >> 3] & (1u << (index & 7u))) != 0;
}

[[nodiscard]] usize count_bits(const std::vector<u8>& bits) {
    usize total = 0;
    for (u8 byte : bits) {
        total += static_cast<usize>(std::popcount(byte));
    }
    return total;
}

[[nodiscard]] usize bytes_for(usize bit_count) { return (bit_count + 7) / 8; }

/// Keeps the part of `winding` in front of `plane`, or nothing.
[[nodiscard]] std::optional<Windingd> chop(const Windingd& winding, const Planed& plane) {
    return winding.clipped(plane);
}

/// The plane a winding lies on, oriented by its own winding order.
[[nodiscard]] bool plane_of(const Windingd& winding, Planed& out) {
    return winding.plane(out);
}

// ---------------------------------------------------------------------------
// Base vis
// ---------------------------------------------------------------------------

/// Whether `target` could possibly be seen through `source`.
///
/// Two cheap necessary conditions: some part of the target must lie in front of
/// the source's plane (otherwise it is behind you), and some part of the source
/// must lie behind the target's plane (otherwise the target faces away). Neither
/// is sufficient, which is what the flow is for -- but together they discard the
/// overwhelming majority of pairs for the cost of a dot product each.
[[nodiscard]] bool might_see_portal(const Portal& source, const Portal& target) {
    bool any_in_front = false;
    for (const Vec3d& point : target.winding.points()) {
        if (source.plane.distance_to(point) > Tol::kPointOnPlane) {
            any_in_front = true;
            break;
        }
    }
    if (!any_in_front) {
        return false;
    }

    for (const Vec3d& point : source.winding.points()) {
        if (target.plane.distance_to(point) < -Tol::kPointOnPlane) {
            return true;
        }
    }
    return false;
}

void base_vis(std::vector<Portal>& portals) {
    const usize count = portals.size();
    const usize stride = bytes_for(count);

    jobs().parallel_for(0, count, [&](usize i) {
        Portal& source = portals[i];
        source.might_see.assign(stride, 0);
        for (usize j = 0; j < count; ++j) {
            if (i != j && might_see_portal(source, portals[j])) {
                set_bit(source.might_see, j);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Full vis: recursive portal flow
// ---------------------------------------------------------------------------

/// Clips `target` by the separating planes between `source` and `pass`.
///
/// This is the heart of exact visibility, and the part a conservative method
/// leaves out. For every edge of `source` and every vertex of `pass`, there is
/// a candidate plane containing that edge and that vertex. When such a plane
/// has all of `source` on one side and all of `pass` on the other, it separates
/// the two openings -- and nothing beyond it can be seen through both. Clipping
/// the next portal by all of them is what makes a corridor that bends actually
/// occlude.
[[nodiscard]] std::optional<Windingd> clip_to_separators(const Windingd& source,
                                                         const Windingd& pass,
                                                         const Windingd& target,
                                                         bool flip_clip) {
    std::optional<Windingd> clipped = target;
    const usize source_count = source.size();
    const usize pass_count = pass.size();

    for (usize i = 0; i < source_count && clipped; ++i) {
        const usize next = (i + 1) % source_count;
        const Vec3d edge = source[next] - source[i];

        for (usize j = 0; j < pass_count && clipped; ++j) {
            const Vec3d to_vertex = pass[j] - source[i];

            Planed plane;
            plane.normal = cross(edge, to_vertex);
            if (plane.normal.normalize() < Tol::kNormalLength) {
                continue;  // The edge and the vertex are collinear.
            }
            plane.distance = dot(pass[j], plane.normal);
            plane.type = Planed::classify_normal(plane.normal);

            // Which side of this candidate the source is on. A source that is
            // planar with the candidate says nothing, and is skipped.
            bool flip = false;
            bool decided = false;
            for (usize k = 0; k < source_count; ++k) {
                if (k == i || k == next) {
                    continue;
                }
                const f64 distance = plane.distance_to(source[k]);
                if (distance < -Tol::kPointOnPlane) {
                    flip = false;
                    decided = true;
                    break;
                }
                if (distance > Tol::kPointOnPlane) {
                    flip = true;
                    decided = true;
                    break;
                }
            }
            if (!decided) {
                continue;  // Planar with the source portal.
            }
            if (flip) {
                plane = plane.flipped();
            }

            // For this to separate, every vertex of `pass` must be on the
            // positive side, and at least one strictly so.
            bool separates = true;
            usize strictly_positive = 0;
            for (usize k = 0; k < pass_count; ++k) {
                if (k == j) {
                    continue;
                }
                const f64 distance = plane.distance_to(pass[k]);
                if (distance < -Tol::kPointOnPlane) {
                    separates = false;
                    break;
                }
                if (distance > Tol::kPointOnPlane) {
                    ++strictly_positive;
                }
            }
            if (!separates || strictly_positive == 0) {
                continue;
            }

            clipped = chop(*clipped, flip_clip ? plane.flipped() : plane);
        }
    }

    return clipped;
}

struct Frame {
    const Portal* portal = nullptr;
    Windingd source;
    Windingd pass;
    bool has_pass = false;
    Planed plane;
    std::vector<u8> might_see;
};

struct Flow {
    const std::vector<Portal>* portals = nullptr;
    const std::vector<std::vector<u32>>* cluster_portals = nullptr;
    std::vector<u8> visible;   ///< Clusters, for the portal being flowed.
    usize cluster_stride = 0;
    usize portal_stride = 0;
};

void flow_recursive(Flow& flow, i32 cluster, const Frame& previous) {
    set_bit(flow.visible, static_cast<usize>(cluster));

    for (u32 index : (*flow.cluster_portals)[static_cast<usize>(cluster)]) {
        const Portal& portal = (*flow.portals)[index];

        if (!test_bit(previous.might_see, index)) {
            continue;
        }

        // Prune: what could still be seen after passing through this portal.
        // Intersecting the two `might_see` sets is what turns an exponential
        // walk into a tractable one.
        std::vector<u8> might_see(flow.portal_stride, 0);
        bool anything = false;
        for (usize byte = 0; byte < flow.portal_stride; ++byte) {
            const u8 bits = static_cast<u8>(previous.might_see[byte] &
                                            portal.might_see[byte]);
            might_see[byte] = bits;
            if (bits != 0) {
                anything = true;
            }
        }

        // Nothing beyond is reachable *and* the cluster through this portal is
        // already known visible: there is nothing left to learn down here.
        //
        // The cluster must not be marked without the clipping below. Doing so
        // asserts that a neighbour is visible because it is adjacent, which is
        // not the same claim -- and since the pruning sets run out at different
        // depths in the two directions, it makes the answer asymmetric. Two
        // clusters either have a sight line between them or they do not; if A
        // sees B and B does not see A, the method is wrong.
        if (!anything && test_bit(flow.visible, static_cast<usize>(portal.into))) {
            continue;
        }

        // The opening we are looking through, clipped to what the previous
        // portal's plane admits.
        std::optional<Windingd> pass = chop(portal.winding, previous.plane);
        if (!pass) {
            continue;
        }

        Frame frame;
        frame.portal = &portal;
        frame.plane = portal.plane;
        frame.might_see = std::move(might_see);

        if (!previous.has_pass) {
            // The first step out of the source portal: nothing to separate
            // against yet.
            frame.source = previous.source;
            frame.pass = std::move(*pass);
            frame.has_pass = true;
            flow_recursive(flow, portal.into, frame);
            continue;
        }

        // Clip the new opening against the separators of the two behind it, and
        // then clip the source back against the new opening. Both directions
        // matter: the first says what can be seen through, the second narrows
        // where it can be seen from.
        pass = clip_to_separators(previous.source, previous.pass, *pass, false);
        if (!pass) {
            continue;
        }
        pass = clip_to_separators(previous.pass, previous.source, *pass, true);
        if (!pass) {
            continue;
        }

        std::optional<Windingd> source =
            clip_to_separators(*pass, previous.pass, previous.source, false);
        if (!source) {
            continue;
        }
        source = clip_to_separators(previous.pass, *pass, *source, true);
        if (!source) {
            continue;
        }

        frame.source = std::move(*source);
        frame.pass = std::move(*pass);
        frame.has_pass = true;
        flow_recursive(flow, portal.into, frame);
    }
}

void full_vis(std::vector<Portal>& portals,
              const std::vector<std::vector<u32>>& cluster_portals,
              usize cluster_count) {
    const usize cluster_stride = bytes_for(cluster_count);
    const usize portal_stride = bytes_for(portals.size());

    // One job per portal. They share nothing but read-only data, so this is the
    // pass that actually uses the machine -- and it is the pass that dominates
    // the run on any real level.
    jobs().parallel_for(0, portals.size(), [&](usize i) {
        Portal& portal = portals[i];

        Flow flow;
        flow.portals = &portals;
        flow.cluster_portals = &cluster_portals;
        flow.visible.assign(cluster_stride, 0);
        flow.cluster_stride = cluster_stride;
        flow.portal_stride = portal_stride;

        Frame frame;
        frame.portal = &portal;
        frame.source = portal.winding;
        frame.plane = portal.plane;
        frame.has_pass = false;
        frame.might_see = portal.might_see;

        flow_recursive(flow, portal.into, frame);

        portal.can_see = std::move(flow.visible);
        portal.flowed = true;
    });
}

}  // namespace

std::expected<PortalFile, std::string> parse_portals(std::string_view text,
                                                     std::string_view name) {
    std::istringstream stream{std::string(text)};

    std::string magic;
    if (!(stream >> magic) || magic != "KPRT1") {
        return std::unexpected(std::format(
            "{}: not a Kerosene portal file. Run cleave first", name));
    }

    PortalFile file;
    usize portal_count = 0;
    if (!(stream >> file.cluster_count) || !(stream >> portal_count)) {
        return std::unexpected(std::format("{}: truncated header", name));
    }

    file.entries.reserve(portal_count);
    for (usize i = 0; i < portal_count; ++i) {
        usize point_count = 0;
        PortalFile::Entry entry;
        if (!(stream >> point_count) || !(stream >> entry.front) || !(stream >> entry.back)) {
            return std::unexpected(std::format("{}: portal {} is truncated", name, i));
        }
        if (point_count < 3) {
            return std::unexpected(std::format(
                "{}: portal {} has {} points; a portal needs at least 3", name, i,
                point_count));
        }
        if (entry.front < 0 || entry.back < 0 ||
            static_cast<usize>(entry.front) >= file.cluster_count ||
            static_cast<usize>(entry.back) >= file.cluster_count) {
            return std::unexpected(std::format(
                "{}: portal {} names clusters {} and {}, but there are only {}", name, i,
                entry.front, entry.back, file.cluster_count));
        }

        std::vector<Vec3d> points;
        points.reserve(point_count);
        for (usize p = 0; p < point_count; ++p) {
            std::string token;
            Vec3d point;
            // Written as "(x y z)"; the parentheses are separators.
            for (usize axis = 0; axis < 3; ++axis) {
                if (!(stream >> token)) {
                    return std::unexpected(
                        std::format("{}: portal {} is truncated mid-point", name, i));
                }
                std::erase(token, '(');
                std::erase(token, ')');
                const auto [stop, code] =
                    std::from_chars(token.data(), token.data() + token.size(), point[axis]);
                if (code != std::errc{}) {
                    return std::unexpected(std::format(
                        "{}: portal {} has '{}' where a number should be", name, i, token));
                }
            }
            points.push_back(point);
        }

        entry.winding = Windingd(std::move(points));
        file.entries.push_back(std::move(entry));
    }

    return file;
}

std::expected<PortalFile, std::string> load_portals(const std::string& path) {
    std::ifstream stream(path);
    if (!stream) {
        return std::unexpected(std::format(
            "cannot open {}. Run cleave on the map first -- it writes the portal "
            "file that this stage reads", path));
    }
    std::ostringstream buffer;
    buffer << stream.rdbuf();
    return parse_portals(buffer.str(), path);
}

Visibility compute(const PortalFile& file, const Options& options, Stats& stats) {
    using Clock = std::chrono::steady_clock;
    const auto started = Clock::now();

    const usize cluster_count = file.cluster_count;
    const usize cluster_stride = bytes_for(cluster_count);

    // Each opening becomes two directed portals, one per direction: the plane
    // faces the destination, so every visibility question is a plane-side test.
    std::vector<Portal> portals;
    portals.reserve(file.entries.size() * 2);
    for (const PortalFile::Entry& entry : file.entries) {
        Planed plane;
        if (!plane_of(entry.winding, plane)) {
            KERO_WARN(log, "skipping a degenerate portal between clusters {} and {}",
                      entry.front, entry.back);
            continue;
        }

        Portal forward;
        forward.plane = plane;
        forward.winding = entry.winding;
        forward.from = entry.back;
        forward.into = entry.front;
        portals.push_back(std::move(forward));

        Portal backward;
        backward.plane = plane.flipped();
        backward.winding = entry.winding.reversed();
        backward.from = entry.front;
        backward.into = entry.back;
        portals.push_back(std::move(backward));
    }

    std::vector<std::vector<u32>> cluster_portals(cluster_count);
    for (usize i = 0; i < portals.size(); ++i) {
        cluster_portals[static_cast<usize>(portals[i].from)].push_back(
            static_cast<u32>(i));
    }

    stats.clusters = cluster_count;
    stats.portals = portals.size();

    base_vis(portals);

    // Base vis as clusters, for reporting -- and for `--fast`, where it is the
    // answer.
    Visibility visibility;
    visibility.cluster_count = cluster_count;
    visibility.pvs.assign(cluster_count, std::vector<u8>(cluster_stride, 0));

    {
        f64 total = 0.0;
        for (usize c = 0; c < cluster_count; ++c) {
            std::vector<u8>& row = visibility.pvs[c];
            set_bit(row, c);  // A cluster always sees itself.
            for (u32 index : cluster_portals[c]) {
                const Portal& portal = portals[index];
                set_bit(row, static_cast<usize>(portal.into));
                for (usize j = 0; j < portals.size(); ++j) {
                    if (test_bit(portal.might_see, j)) {
                        set_bit(row, static_cast<usize>(portals[j].into));
                    }
                }
            }
            total += static_cast<f64>(count_bits(row));
        }
        stats.average_base = cluster_count > 0 ? total / static_cast<f64>(cluster_count) : 0.0;
    }

    if (!options.fast) {
        full_vis(portals, cluster_portals, cluster_count);

        for (usize c = 0; c < cluster_count; ++c) {
            std::vector<u8> row(cluster_stride, 0);
            set_bit(row, c);
            for (u32 index : cluster_portals[c]) {
                const Portal& portal = portals[index];
                if (!portal.flowed) {
                    continue;
                }
                for (usize byte = 0; byte < cluster_stride; ++byte) {
                    row[byte] = static_cast<u8>(row[byte] | portal.can_see[byte]);
                }
            }
            visibility.pvs[c] = std::move(row);
        }
    }

    f64 total = 0.0;
    for (const std::vector<u8>& row : visibility.pvs) {
        total += static_cast<f64>(count_bits(row));
    }
    stats.average_visible =
        cluster_count > 0 ? total / static_cast<f64>(cluster_count) : 0.0;

    // The audible set: everything visible from anywhere visible. One portal
    // hop further than sight, which is the cheap approximation of "sound goes
    // round a corner" and is what Source's PHS is.
    visibility.pas.assign(cluster_count, std::vector<u8>(cluster_stride, 0));
    jobs().parallel_for(0, cluster_count, [&](usize c) {
        std::vector<u8>& row = visibility.pas[c];
        row = visibility.pvs[c];
        for (usize other = 0; other < cluster_count; ++other) {
            if (!test_bit(visibility.pvs[c], other)) {
                continue;
            }
            for (usize byte = 0; byte < cluster_stride; ++byte) {
                row[byte] = static_cast<u8>(row[byte] | visibility.pvs[other][byte]);
            }
        }
    });

    stats.seconds = std::chrono::duration<f64>(Clock::now() - started).count();
    return visibility;
}

namespace {

/// Run-length encodes runs of zero bytes.
///
/// A visibility row is mostly zeroes -- a cluster in a large level sees a small
/// fraction of it -- so the compression that matters is the one that costs
/// nothing to decode. A zero byte is followed by the length of its run; any
/// other byte stands for itself.
std::vector<u8> compress_row(const std::vector<u8>& row) {
    std::vector<u8> out;
    out.reserve(row.size() / 2);

    for (usize i = 0; i < row.size();) {
        if (row[i] != 0) {
            out.push_back(row[i]);
            ++i;
            continue;
        }
        usize run = 1;
        while (i + run < row.size() && row[i + run] == 0 && run < 255) {
            ++run;
        }
        out.push_back(0);
        out.push_back(static_cast<u8>(run));
        i += run;
    }
    return out;
}

}  // namespace

std::vector<std::byte> serialise(const Visibility& visibility) {
    const usize count = visibility.cluster_count;

    bsp::DiskVisHeader header{static_cast<u32>(count)};
    std::vector<u8> body;
    std::vector<u32> offsets(count * 2, 0);

    const usize table_bytes = sizeof(header) + offsets.size() * sizeof(u32);

    for (usize c = 0; c < count; ++c) {
        offsets[c * 2 + bsp::kVisPvs] = static_cast<u32>(table_bytes + body.size());
        const std::vector<u8> pvs = compress_row(visibility.pvs[c]);
        body.insert(body.end(), pvs.begin(), pvs.end());

        offsets[c * 2 + bsp::kVisPas] = static_cast<u32>(table_bytes + body.size());
        const std::vector<u8> pas = compress_row(visibility.pas[c]);
        body.insert(body.end(), pas.begin(), pas.end());
    }

    std::vector<std::byte> out;
    out.reserve(table_bytes + body.size());

    const auto* header_bytes = reinterpret_cast<const std::byte*>(&header);
    out.insert(out.end(), header_bytes, header_bytes + sizeof(header));
    const auto* offset_bytes = reinterpret_cast<const std::byte*>(offsets.data());
    out.insert(out.end(), offset_bytes, offset_bytes + offsets.size() * sizeof(u32));
    const auto* body_bytes = reinterpret_cast<const std::byte*>(body.data());
    out.insert(out.end(), body_bytes, body_bytes + body.size());

    return out;
}

std::expected<Stats, std::string> run(const std::string& bsp_path, const Options& options) {
    auto file = bsp::File::load(bsp_path);
    if (!file) {
        return std::unexpected(file.error());
    }

    std::filesystem::path portal_path(bsp_path);
    portal_path.replace_extension(".kprt");
    auto portals = load_portals(portal_path.string());
    if (!portals) {
        return std::unexpected(portals.error());
    }

    KERO_INFO(log, "{}: {} clusters, {} portals", portal_path.string(),
              portals->cluster_count, portals->entries.size());

    Stats stats;
    const Visibility visibility = compute(*portals, options, stats);

    std::vector<std::byte> lump = serialise(visibility);
    stats.visibility_bytes = lump.size();
    file->set_lump(bsp::LumpId::Visibility, std::move(lump));

    if (auto saved = file->save(bsp_path); !saved) {
        return std::unexpected(saved.error());
    }

    KERO_INFO(log,
              "{} clusters; {:.1f} visible on average (base vis said {:.1f}); "
              "{} bytes; {:.2f}s",
              stats.clusters, stats.average_visible, stats.average_base,
              stats.visibility_bytes, stats.seconds);
    KERO_INFO(log, "wrote {}", bsp_path);
    return stats;
}

}  // namespace kero::umbra
