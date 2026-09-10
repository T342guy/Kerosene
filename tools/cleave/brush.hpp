// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "bsp/surface.hpp"
#include "map/map.hpp"
#include "math/aabb.hpp"
#include "math/plane.hpp"
#include "math/winding.hpp"

#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

/// Cleave's working representation of a level, and the CSG that turns
/// overlapping solids into a surface.
namespace kero::cleave {

using math::Aabbd;
using math::Planed;
using math::Vec3d;
using math::Windingd;

/// Every distinct plane in the level, deduplicated, stored in facing pairs.
///
/// Index `i` and index `i ^ 1` are the same plane facing opposite ways. That is
/// Quake's arrangement and it earns its keep constantly: flipping a plane is an
/// XOR rather than a search, two brushes sharing a wall reference the same
/// plane whichever way each faces it, and "have I already split on this plane?"
/// becomes an integer comparison instead of a geometric one.
///
/// Deduplication is the point. A level has tens of thousands of brush sides and
/// perhaps a few thousand distinct planes; without this, the BSP builder would
/// happily split on two floating-point copies of the same wall and produce a
/// leaf of zero thickness between them.
class PlaneSet {
public:
    /// Returns the index of `plane`, adding it if new. The returned index has
    /// the facing that was asked for.
    [[nodiscard]] u32 add(const Planed& plane);

    /// The index of `plane` if it is already known, or nothing.
    [[nodiscard]] std::optional<u32> find(const Planed& plane) const;

    [[nodiscard]] const Planed& operator[](u32 index) const { return planes_[index]; }
    [[nodiscard]] usize size() const { return planes_.size(); }
    [[nodiscard]] const std::vector<Planed>& planes() const { return planes_; }

    /// The same plane, facing the other way.
    [[nodiscard]] static constexpr u32 flip(u32 index) { return index ^ 1u; }

private:
    /// Buckets keyed on the rounded distance. Two planes that are equal within
    /// tolerance can round to adjacent buckets, so lookups check the
    /// neighbours; getting that wrong means silently duplicating a plane, which
    /// is the bug this class exists to prevent.
    [[nodiscard]] i64 bucket_of(const Planed& plane) const;

    std::vector<Planed> planes_;
    std::unordered_map<i64, std::vector<u32>> buckets_;
};

/// One face of a brush, during compilation.
struct Side {
    u32 plane = 0;             ///< Index into the PlaneSet.
    Windingd winding;          ///< The face, clipped by the brush's other sides.

    /// The parts of `winding` still visible after CSG. Empty when the face is
    /// entirely buried inside another brush.
    std::vector<Windingd> visible;

    std::string material;
    bsp::SurfaceKind kind;
    map::TextureAxis uaxis;
    map::TextureAxis vaxis;
    f32 lightmap_scale = 8.0f;
    i32 smoothing_groups = 0;
    i32 map_id = 0;

    /// A plane Cleave added so that box traces are exact, rather than one the
    /// designer drew. Bevels bound the volume but never produce a face.
    bool bevel = false;

    /// Cleared once this plane has been used as a split plane on the path down
    /// to the current node, so the tree never splits on it twice.
    bool used_as_splitter = false;
};

/// A convex solid.
struct Brush {
    i32 map_id = 0;
    usize entity = 0;            ///< 0 is the world.
    bsp::Contents contents = bsp::Contents::Solid;
    Aabbd bounds;
    std::vector<Side> sides;

    /// Detail brushes are solid but stay out of the visibility tree, so a
    /// cluttered room does not shred the PVS. They also cannot seal a level.
    bool detail = false;

    [[nodiscard]] bool seals() const;

    /// Whether `point` is inside, within tolerance.
    [[nodiscard]] bool contains(const Vec3d& point) const;

    /// The plane indices of every non-bevel side.
    [[nodiscard]] std::vector<u32> plane_indices() const;
};

/// A whole level, mid-compile.
struct World {
    PlaneSet planes;
    std::vector<Brush> brushes;

    /// Entity 0 is the world; the rest are the map's entities in file order.
    std::vector<const map::Entity*> entities;

    [[nodiscard]] Aabbd bounds() const;
};

/// Why a brush could not be compiled. Reported against the brush, so a designer
/// can find it.
struct BrushProblem {
    i32 map_id = 0;
    usize entity = 0;
    std::string message;
};

/// Turns a parsed map into brushes with windings.
///
/// Brushes that cannot be compiled are left out and reported in `problems`
/// rather than aborting the run: one bad brush in a thousand should cost you a
/// warning and a hole, not the whole compile.
[[nodiscard]] World build_world(const map::Map& map, std::vector<BrushProblem>& problems);

/// Adds the bevel planes that make box traces exact.
///
/// For an axis-aligned box swept against a convex brush, the supporting planes
/// of the Minkowski sum are the brush's own planes plus, for each axis, the
/// axial planes touching the brush's extent. A brush that is already a box
/// gains nothing; a wedge or a cylinder gains the axial planes it was missing,
/// and without them a box sweeping past a diagonal face clips the corner.
///
/// Quake and Source solve this by precomputing collision hulls at a few fixed
/// sizes and snapping every entity to the nearest. Adding the planes at compile
/// time instead costs a few bytes per brush and makes the trace exact for any
/// box, which is what lets an entity be whatever size it needs to be.
void add_bevel_planes(World& world);

/// Chops away the parts of each brush's faces that are inside another brush.
///
/// The brushes themselves are left whole -- they are what collision uses. What
/// CSG produces is the *surface*: the set of face fragments that are actually
/// exposed. Without it, every wall between two adjoining rooms would be drawn
/// twice, once from each side of the shared volume, and the lightmapper would
/// bake both.
///
/// Coplanar faces from two brushes are resolved by keeping the lower-indexed
/// brush's, so the answer does not depend on iteration order.
void chop_brushes(World& world);

}  // namespace kero::cleave
