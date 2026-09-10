// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "bsp/level.hpp"

#include "core/assert.hpp"
#include "core/log.hpp"

#include <algorithm>
#include <cmath>
#include <format>

namespace kero::bsp {
namespace {

KERO_LOG_CATEGORY(log, "bsp");

/// How far short of a surface a sweep stops.
///
/// Without it, a sweep that ends exactly on a plane leaves the mover touching
/// the wall, and the next tick's start-solid test can go either way depending
/// on the last bit of the float. Stopping a hair short means the mover is
/// always cleanly outside, which is worth far more than the gap is visible --
/// at 1 ku = 2 inches, this is a hundredth of a millimetre.
constexpr f32 kSurfaceGap = 0.0625f;

/// Encodes a BSP child: non-negative is a node, negative is the leaf
/// `-(child + 1)`. Quake's encoding, kept because it makes a node's two
/// children one array with no discriminant.
[[nodiscard]] bool is_leaf(i32 child) { return child < 0; }
[[nodiscard]] u32 leaf_of(i32 child) { return static_cast<u32>(-(child + 1)); }

}  // namespace

struct Level::SweepContext {
    Vec3 start;        ///< Already shifted so the box is centred on it.
    Vec3 end;
    Vec3 half;         ///< Half the box's size.
    Contents mask = Contents::SolidMask;
    bool is_point = false;
    Trace trace;
};

std::expected<Level, std::string> Level::from_file(File file) {
    Level level;
    level.file_ = std::move(file);

    level.planes_ = level.file_.lump_as<DiskPlane>(LumpId::Planes);
    level.vertices_ = level.file_.lump_as<DiskVertex>(LumpId::Vertices);
    level.face_vertices_ = level.file_.lump_as<u32>(LumpId::FaceVertices);
    level.faces_ = level.file_.lump_as<DiskFace>(LumpId::Faces);
    level.nodes_ = level.file_.lump_as<DiskNode>(LumpId::Nodes);
    level.leaves_ = level.file_.lump_as<DiskLeaf>(LumpId::Leaves);
    level.leaf_faces_ = level.file_.lump_as<u32>(LumpId::LeafFaces);
    level.leaf_brushes_ = level.file_.lump_as<u32>(LumpId::LeafBrushes);
    level.brushes_ = level.file_.lump_as<DiskBrush>(LumpId::Brushes);
    level.brush_sides_ = level.file_.lump_as<DiskBrushSide>(LumpId::BrushSides);
    level.texinfos_ = level.file_.lump_as<DiskTexInfo>(LumpId::TexInfo);
    level.models_ = level.file_.lump_as<DiskModel>(LumpId::Models);

    if (level.leaves_.empty()) {
        return std::unexpected("the level has no leaves; it did not compile");
    }
    if (level.models_.empty()) {
        return std::unexpected("the level has no models; it did not compile");
    }

    // Every index in the file is validated once, here, rather than at each use.
    // A level is loaded once and traced a million times a second; and a bad
    // index found at load time is a message, where the same index found during
    // a trace is a crash.
    for (const DiskNode& node : level.nodes_) {
        if (node.plane >= level.planes_.size()) {
            return std::unexpected("a node names a plane that does not exist");
        }
        for (i32 child : node.children) {
            if (is_leaf(child)) {
                if (leaf_of(child) >= level.leaves_.size()) {
                    return std::unexpected("a node names a leaf that does not exist");
                }
            } else if (static_cast<usize>(child) >= level.nodes_.size()) {
                return std::unexpected("a node names a child node that does not exist");
            }
        }
    }
    for (const DiskFace& face : level.faces_) {
        if (static_cast<usize>(face.first_vertex) + face.vertex_count >
            level.face_vertices_.size()) {
            return std::unexpected("a face's vertices run past the end of the lump");
        }
        if (face.plane >= level.planes_.size()) {
            return std::unexpected("a face names a plane that does not exist");
        }
        if (!level.texinfos_.empty() && face.texinfo >= level.texinfos_.size()) {
            return std::unexpected("a face names a texinfo that does not exist");
        }
    }
    for (u32 index : level.face_vertices_) {
        if (index >= level.vertices_.size()) {
            return std::unexpected("a face names a vertex that does not exist");
        }
    }
    for (const DiskLeaf& leaf : level.leaves_) {
        if (static_cast<usize>(leaf.first_leaf_face) + leaf.leaf_face_count >
            level.leaf_faces_.size()) {
            return std::unexpected("a leaf's faces run past the end of the lump");
        }
        if (static_cast<usize>(leaf.first_leaf_brush) + leaf.leaf_brush_count >
            level.leaf_brushes_.size()) {
            return std::unexpected("a leaf's brushes run past the end of the lump");
        }
    }
    for (u32 index : level.leaf_faces_) {
        if (index >= level.faces_.size()) {
            return std::unexpected("a leaf names a face that does not exist");
        }
    }
    for (u32 index : level.leaf_brushes_) {
        if (index >= level.brushes_.size()) {
            return std::unexpected("a leaf names a brush that does not exist");
        }
    }
    for (const DiskBrush& brush : level.brushes_) {
        if (static_cast<usize>(brush.first_side) + brush.side_count >
            level.brush_sides_.size()) {
            return std::unexpected("a brush's sides run past the end of the lump");
        }
    }
    for (const DiskBrushSide& side : level.brush_sides_) {
        if ((side.plane >> 1) >= level.planes_.size()) {
            return std::unexpected("a brush side names a plane that does not exist");
        }
    }

    level.cluster_count_ = visibility_cluster_count(level.file_.lump(LumpId::Visibility));
    if (level.cluster_count_ == 0) {
        // No visibility lump: count the clusters the leaves claim, so the
        // renderer still has a coherent number to work with.
        i32 highest = -1;
        for (const DiskLeaf& leaf : level.leaves_) {
            highest = std::max(highest, leaf.cluster);
        }
        level.cluster_count_ = static_cast<usize>(highest + 1);
    }

    const DiskModel& world = level.models_.front();
    level.bounds_ = Aabb(Vec3(world.mins[0], world.mins[1], world.mins[2]),
                         Vec3(world.maxs[0], world.maxs[1], world.maxs[2]));

    KERO_INFO(log,
              "{} nodes, {} leaves, {} faces, {} brushes, {} clusters; "
              "visibility {}, lighting {}",
              level.nodes_.size(), level.leaves_.size(), level.faces_.size(),
              level.brushes_.size(), level.cluster_count_,
              level.has_visibility() ? "baked" : "not built",
              level.has_lighting() ? "baked" : "not built");
    if (!level.has_visibility()) {
        KERO_WARN(log,
                  "no visibility: everything will be drawn. Run "
                  "`kerosene-tools umbra` on the map");
    }

    return level;
}

std::expected<Level, std::string> Level::load(const std::string& path) {
    auto file = File::load(path);
    if (!file) {
        return std::unexpected(file.error());
    }
    return from_file(std::move(*file));
}

Plane Level::plane_of(u32 index, bool flipped) const {
    const DiskPlane& disk = planes_[index];
    Plane plane(Vec3(disk.normal[0], disk.normal[1], disk.normal[2]), disk.distance);
    return flipped ? plane.flipped() : plane;
}

u32 Level::leaf_at(const Vec3& point) const {
    if (nodes_.empty()) {
        return 0;
    }
    i32 node = models_.front().head_node;
    while (!is_leaf(node)) {
        const DiskNode& current = nodes_[static_cast<usize>(node)];
        const DiskPlane& plane = planes_[current.plane];
        const f32 distance = plane.normal[0] * point.x + plane.normal[1] * point.y +
                             plane.normal[2] * point.z - plane.distance;
        node = current.children[distance >= 0.0f ? 0 : 1];
    }
    return leaf_of(node);
}

i32 Level::cluster_at(const Vec3& point) const {
    return leaves_[leaf_at(point)].cluster;
}

Contents Level::contents_at(const Vec3& point) const {
    return static_cast<Contents>(leaves_[leaf_at(point)].contents);
}

std::span<const u32> Level::leaf_faces(u32 leaf) const {
    const DiskLeaf& current = leaves_[leaf];
    return leaf_faces_.subspan(current.first_leaf_face, current.leaf_face_count);
}

std::span<const u32> Level::leaf_brushes(u32 leaf) const {
    const DiskLeaf& current = leaves_[leaf];
    return leaf_brushes_.subspan(current.first_leaf_brush, current.leaf_brush_count);
}

void Level::visible_clusters(i32 cluster, usize which, std::vector<u8>& out) const {
    (void)decode_visibility(file_.lump(LumpId::Visibility), cluster, which,
                            cluster_count_, out);
}

// ---------------------------------------------------------------------------
// Tracing
// ---------------------------------------------------------------------------

void Level::clip_to_brush(SweepContext& context, u32 brush_index) const {
    const DiskBrush& brush = brushes_[brush_index];
    if (brush.side_count == 0) {
        return;
    }
    if (!any(static_cast<Contents>(brush.contents) & context.mask)) {
        return;
    }

    f32 enter = -1.0f;
    f32 leave = 1.0f;
    bool started_outside = false;
    bool ended_outside = false;
    Plane hit_plane;

    for (u32 i = 0; i < brush.side_count; ++i) {
        const DiskBrushSide& side = brush_sides_[brush.first_side + i];
        const Plane plane = plane_of(side.plane >> 1, (side.plane & 1u) != 0);

        // Push the plane out by the box's extent along its normal. For a
        // centred box that is just the dot of the absolute normal with the
        // half-size -- and doing it here, rather than growing the geometry, is
        // what makes a sweep of any box size exact rather than approximate.
        f32 distance = plane.distance;
        if (!context.is_point) {
            distance += std::abs(plane.normal.x) * context.half.x +
                        std::abs(plane.normal.y) * context.half.y +
                        std::abs(plane.normal.z) * context.half.z;
        }

        const f32 from = dot(plane.normal, context.start) - distance;
        const f32 to = dot(plane.normal, context.end) - distance;

        if (to > 0.0f) {
            ended_outside = true;
        }
        if (from > 0.0f) {
            started_outside = true;
        }

        // Entirely in front of this plane: outside the brush, so it cannot be
        // hit at all.
        if (from > 0.0f && to >= from) {
            return;
        }
        // Entirely behind: this plane does not bound the crossing.
        if (from <= 0.0f && to <= 0.0f) {
            continue;
        }

        if (from > to) {
            const f32 fraction = (from - kSurfaceGap) / (from - to);
            if (fraction > enter) {
                enter = fraction;
                hit_plane = plane;
            }
        } else {
            const f32 fraction = (from + kSurfaceGap) / (from - to);
            leave = std::min(leave, fraction);
        }
    }

    if (!started_outside) {
        // The sweep began inside this brush. Reported rather than ignored: a
        // player pushed into a wall by a door has to be able to get out, and
        // movement code cannot solve a problem it is not told about.
        context.trace.start_solid = true;
        if (!ended_outside) {
            context.trace.all_solid = true;
        }
        context.trace.contents = static_cast<Contents>(brush.contents);
        context.trace.brush = brush_index;
        return;
    }

    if (enter < leave && enter > -1.0f && enter < context.trace.fraction) {
        context.trace.fraction = std::max(enter, 0.0f);
        context.trace.plane = hit_plane;
        context.trace.contents = static_cast<Contents>(brush.contents);
        context.trace.brush = brush_index;
    }
}

void Level::clip_to_leaf(SweepContext& context, u32 leaf) const {
    for (u32 brush : leaf_brushes(leaf)) {
        clip_to_brush(context, brush);
        if (context.trace.fraction <= 0.0f) {
            return;
        }
    }
}

void Level::descend(SweepContext& context, i32 node, f32 start_fraction, f32 end_fraction,
                    const Vec3& start, const Vec3& end) const {
    if (context.trace.fraction <= start_fraction) {
        return;  // Something nearer has already stopped the sweep.
    }

    if (is_leaf(node)) {
        clip_to_leaf(context, leaf_of(node));
        return;
    }

    const DiskNode& current = nodes_[static_cast<usize>(node)];
    const DiskPlane& disk = planes_[current.plane];
    const Vec3 normal(disk.normal[0], disk.normal[1], disk.normal[2]);

    const f32 from = dot(normal, start) - disk.distance;
    const f32 to = dot(normal, end) - disk.distance;

    // The box's reach along this plane's normal. Conservative by construction,
    // so the descent visits every leaf the box can touch; the exact answer
    // comes from the brush planes at the leaves.
    const f32 offset =
        context.is_point ? 0.0f
                         : std::abs(normal.x) * context.half.x +
                               std::abs(normal.y) * context.half.y +
                               std::abs(normal.z) * context.half.z;

    if (from >= offset && to >= offset) {
        descend(context, current.children[0], start_fraction, end_fraction, start, end);
        return;
    }
    if (from < -offset && to < -offset) {
        descend(context, current.children[1], start_fraction, end_fraction, start, end);
        return;
    }

    // Straddles: split the sweep at the plane and take the near side first, so
    // the nearest hit is found before the far side is even visited.
    i32 near_child = 0;
    i32 far_child = 1;
    f32 near_fraction = 0.0f;
    f32 far_fraction = 0.0f;

    // The box straddles the plane while its centre is within `offset` of it, so
    // each child has to be searched over the sub-range where that is true --
    // and the two ranges overlap. The near child's range runs from the start
    // until the box has fully left its side; the far child's begins as soon as
    // the box first reaches into it.
    //
    // The two formulas swap when the sweep runs back-to-front. Using the same
    // one in both directions is a subtle and expensive mistake: the far child
    // gets an empty range, `descend` returns immediately on its start-fraction
    // guard, and a whole subtree is silently skipped. It shows up as short
    // sweeps missing geometry that long sweeps over the same ground catch,
    // which reads as a physics bug rather than a trace bug.
    if (from < to) {
        // Starting behind the plane: the near child is the back one.
        const f32 inverse = 1.0f / (from - to);
        near_child = 1;
        far_child = 0;
        near_fraction = (from - offset - kSurfaceGap) * inverse;
        far_fraction = (from + offset + kSurfaceGap) * inverse;
    } else if (from > to) {
        // Starting in front: the near child is the front one.
        const f32 inverse = 1.0f / (from - to);
        near_fraction = (from + offset + kSurfaceGap) * inverse;
        far_fraction = (from - offset - kSurfaceGap) * inverse;
    } else {
        // Parallel to the plane and within the box's reach of it: both sides
        // have to be checked.
        near_child = 0;
        far_child = 1;
        near_fraction = 1.0f;
        far_fraction = 0.0f;
    }

    near_fraction = std::clamp(near_fraction, 0.0f, 1.0f);
    far_fraction = std::clamp(far_fraction, 0.0f, 1.0f);

    const f32 near_end = start_fraction + (end_fraction - start_fraction) * near_fraction;
    const Vec3 near_point = lerp(start, end, near_fraction);
    descend(context, current.children[near_child], start_fraction, near_end, start,
            near_point);

    const f32 far_start = start_fraction + (end_fraction - start_fraction) * far_fraction;
    const Vec3 far_point = lerp(start, end, far_fraction);
    descend(context, current.children[far_child], far_start, end_fraction, far_point, end);
}

Trace Level::trace(const Vec3& start, const Vec3& end, const Aabb& box,
                   Contents mask) const {
    SweepContext context;
    context.mask = mask;

    // The box is centred on the sweep by shifting the sweep instead. Every
    // plane offset below is then a symmetric one, which keeps the arithmetic
    // exact for an asymmetric box -- a player's box runs from its feet to its
    // head, and treating that as symmetric would make it collide with the floor
    // above its own head.
    const Vec3 centre = box.empty() ? Vec3{} : box.centre();
    context.half = box.empty() ? Vec3{} : box.size() * 0.5f;
    context.is_point = context.half.is_zero(1e-4f);
    context.start = start + centre;
    context.end = end + centre;

    context.trace.fraction = 1.0f;
    context.trace.end = end;

    if (nodes_.empty()) {
        // A level with a single leaf and no tree: still answer coherently.
        clip_to_leaf(context, 0);
    } else {
        descend(context, models_.front().head_node, 0.0f, 1.0f, context.start,
                context.end);
    }

    context.trace.fraction = std::clamp(context.trace.fraction, 0.0f, 1.0f);
    context.trace.end = lerp(start, end, context.trace.fraction);
    return context.trace;
}

Trace Level::trace_ray(const Vec3& start, const Vec3& end, Contents mask) const {
    return trace(start, end, Aabb(Vec3{}, Vec3{}), mask);
}

}  // namespace kero::bsp
