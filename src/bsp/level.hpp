// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "bsp/file.hpp"
#include "math/aabb.hpp"
#include "math/plane.hpp"
#include "math/vec.hpp"

#include <expected>
#include <optional>
#include <span>
#include <string>
#include <vector>

namespace kero::bsp {

using math::Aabb;
using math::Plane;
using math::Vec3;

/// The result of sweeping something through the level.
struct Trace {
    /// How far along the sweep it got, from 0 to 1. Exactly 1 means nothing
    /// was hit.
    f32 fraction = 1.0f;
    Vec3 end;

    /// The surface that stopped it, valid only when something was hit.
    Plane plane;
    Contents contents = Contents::Empty;
    u32 brush = Index::kNone;

    /// The sweep began inside something solid. Movement code has to handle
    /// this rather than assume it away: a player can be pushed into a wall by
    /// a door, and refusing to report it would strand them there.
    bool start_solid = false;
    /// It began *and* ended inside solid, so there is no direction out along
    /// this sweep.
    bool all_solid = false;

    [[nodiscard]] bool hit() const { return fraction < 1.0f; }
};

/// A compiled level, ready to be walked around in.
///
/// The queries the engine needs, and nothing else: which leaf a point is in,
/// what a leaf can see, what a leaf holds, and what a swept box collides with.
class Level {
public:
    [[nodiscard]] static std::expected<Level, std::string> load(const std::string& path);
    [[nodiscard]] static std::expected<Level, std::string> from_file(File file);

    Level() = default;

    // --- The compiled data ---------------------------------------------------

    [[nodiscard]] std::span<const DiskPlane> planes() const { return planes_; }
    [[nodiscard]] std::span<const DiskVertex> vertices() const { return vertices_; }
    [[nodiscard]] std::span<const u32> face_vertices() const { return face_vertices_; }
    [[nodiscard]] std::span<const DiskFace> faces() const { return faces_; }
    [[nodiscard]] std::span<const DiskNode> nodes() const { return nodes_; }
    [[nodiscard]] std::span<const DiskLeaf> leaves() const { return leaves_; }
    [[nodiscard]] std::span<const DiskTexInfo> texinfos() const { return texinfos_; }
    [[nodiscard]] std::span<const DiskModel> models() const { return models_; }
    [[nodiscard]] std::span<const DiskBrush> brushes() const { return brushes_; }

    /// Which entity owns the brush a trace hit, or nothing when the trace hit
    /// nothing.
    [[nodiscard]] std::optional<u32> owner_of_brush(u32 brush) const {
        return brush < brushes_.size() ? std::optional(brushes_[brush].entity)
                                       : std::nullopt;
    }

    /// The entity lump, as KeyValues text for the game code to parse.
    [[nodiscard]] std::string_view entities() const { return file_.entities(); }
    [[nodiscard]] std::string_view material_of(const DiskTexInfo& texinfo) const {
        return file_.material(texinfo.material_offset);
    }

    [[nodiscard]] usize cluster_count() const { return cluster_count_; }
    [[nodiscard]] bool has_visibility() const { return file_.has_visibility(); }
    [[nodiscard]] bool has_lighting() const { return file_.has_lighting(); }
    [[nodiscard]] const Aabb& bounds() const { return bounds_; }

    // --- Where am I ----------------------------------------------------------

    /// The index of the leaf containing `point`. Never fails: a point outside
    /// the level lands in the solid leaf that was filled in around it.
    [[nodiscard]] u32 leaf_at(const Vec3& point) const;

    /// The cluster containing `point`, or -1 for a solid leaf.
    [[nodiscard]] i32 cluster_at(const Vec3& point) const;

    [[nodiscard]] Contents contents_at(const Vec3& point) const;

    /// The faces to draw for a leaf.
    [[nodiscard]] std::span<const u32> leaf_faces(u32 leaf) const;
    [[nodiscard]] std::span<const u32> leaf_brushes(u32 leaf) const;

    // --- What can I see ------------------------------------------------------

    /// Fills `out` with the set of clusters visible from `cluster`, one bit
    /// each. `which` is kVisPvs or kVisPas.
    ///
    /// The caller owns the buffer because the natural place to keep it is the
    /// renderer, which needs it for exactly as long as it is drawing one frame,
    /// and because a cache inside a const object shared across threads is a
    /// data race waiting to be written.
    void visible_clusters(i32 cluster, usize which, std::vector<u8>& out) const;

    /// Whether `to` is in the set `out` came back holding.
    [[nodiscard]] static bool cluster_in_set(const std::vector<u8>& set, i32 cluster) {
        if (cluster < 0) {
            return false;
        }
        const auto index = static_cast<usize>(cluster);
        return index >> 3 < set.size() && (set[index >> 3] & (1u << (index & 7u))) != 0;
    }

    // --- What did I hit ------------------------------------------------------

    /// Sweeps `box` -- given relative to the moving point -- from `start` to
    /// `end`, stopping at anything in `mask`.
    ///
    /// An arbitrary box, not one of a few precomputed hull sizes. Quake and
    /// Source build collision hulls for a small set of dimensions and snap
    /// every entity to the nearest, which constrains what an entity can be for
    /// the rest of the engine's life. Here the brush planes carry bevels added
    /// at compile time, which are exactly the supporting planes of the
    /// Minkowski sum, so pushing each plane out by the box's extent along its
    /// normal gives an exact sweep for any box at all.
    [[nodiscard]] Trace trace(const Vec3& start, const Vec3& end, const Aabb& box,
                              Contents mask) const;

    /// A ray, for line of sight and for picking.
    [[nodiscard]] Trace trace_ray(const Vec3& start, const Vec3& end, Contents mask) const;

private:
    struct SweepContext;

    void descend(SweepContext& context, i32 node, f32 start_fraction, f32 end_fraction,
                 const Vec3& start, const Vec3& end) const;
    void clip_to_leaf(SweepContext& context, u32 leaf) const;
    void clip_to_brush(SweepContext& context, u32 brush_index) const;

    [[nodiscard]] Plane plane_of(u32 index, bool flipped) const;

    File file_;
    std::span<const DiskPlane> planes_;
    std::span<const DiskVertex> vertices_;
    std::span<const u32> face_vertices_;
    std::span<const DiskFace> faces_;
    std::span<const DiskNode> nodes_;
    std::span<const DiskLeaf> leaves_;
    std::span<const u32> leaf_faces_;
    std::span<const u32> leaf_brushes_;
    std::span<const DiskBrush> brushes_;
    std::span<const DiskBrushSide> brush_sides_;
    std::span<const DiskTexInfo> texinfos_;
    std::span<const DiskModel> models_;

    usize cluster_count_ = 0;
    Aabb bounds_;
};

}  // namespace kero::bsp
