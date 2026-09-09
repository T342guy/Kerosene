// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "math/plane.hpp"

#include <optional>
#include <vector>

namespace kero::math {

/// A convex polygon in 3D, wound counter-clockwise about its own plane.
///
/// This is the single busiest type in the map compilers. A brush side becomes a
/// winding, CSG clips windings against planes, the BSP builder splits them at
/// every node, portals are windings, and lightmaps are parameterised over them.
/// Nearly every geometric bug in a brush-based engine is a winding operation
/// that lost precision or dropped a nearly-degenerate result.
///
/// Two properties are maintained rather than assumed:
///
///   * **Convexity is structural.** Every winding here comes either from
///     clipping a convex polygon by a half-space -- which cannot make it
///     concave -- or from `from_plane()`. Nothing constructs one from an
///     arbitrary point list, so there is no concave case to handle.
///
///   * **Degenerate results are absent, not tiny.** A clip that leaves a sliver
///     thinner than the tolerance returns nothing at all. Keeping such a thing
///     is how a compiler ends up emitting faces with no area for the renderer
///     and the lightmapper to trip over separately.
template <Scalar T>
class WindingT {
public:
    using Vec = Vec3T<T>;
    using Plane = PlaneT<T>;

    WindingT() = default;
    explicit WindingT(std::vector<Vec> points) : points_(std::move(points)) {}

    /// The largest quad lying on `plane` that still fits in the world.
    ///
    /// Every brush face starts life as one of these and is then clipped down by
    /// the brush's other sides. Starting from something certainly large enough
    /// and cutting away is far more robust than trying to construct the final
    /// polygon directly -- there is no ordering to get wrong and no intersection
    /// to compute that might be near-parallel.
    [[nodiscard]] static WindingT from_plane(const Plane& plane, T extent);

    /// A default-sized base winding, using the world extent plus headroom.
    [[nodiscard]] static WindingT from_plane(const Plane& plane);

    [[nodiscard]] const std::vector<Vec>& points() const { return points_; }
    [[nodiscard]] std::vector<Vec>& points() { return points_; }
    [[nodiscard]] usize size() const { return points_.size(); }
    [[nodiscard]] bool empty() const { return points_.size() < 3; }
    [[nodiscard]] const Vec& operator[](usize index) const { return points_[index]; }

    /// Keeps only the part on or in front of `plane`.
    ///
    /// Returns nothing if the result would be degenerate -- either because the
    /// winding is entirely behind the plane, or because what survives is too
    /// thin to be a surface.
    [[nodiscard]] std::optional<WindingT> clipped(const Plane& plane) const;

    /// Splits into the parts in front of and behind `plane`.
    ///
    /// Either half may come back empty. Both halves are produced in one pass,
    /// which matters: computing them as two independent clips can round the
    /// shared edge two different ways and open a crack between them.
    void split(const Plane& plane, std::optional<WindingT>& front,
               std::optional<WindingT>& back) const;

    /// Which side of `plane` the winding as a whole is on.
    [[nodiscard]] Side classify(const Plane& plane) const;

    /// The plane this winding lies on, derived from its own points. Fails for a
    /// degenerate winding.
    [[nodiscard]] bool plane(Plane& out) const;

    [[nodiscard]] T area() const;
    [[nodiscard]] Vec centre() const;
    void bounds(Vec& mins, Vec& maxs) const;

    /// The winding facing the other way.
    [[nodiscard]] WindingT reversed() const;

    /// Drops points that lie on the line between their neighbours.
    ///
    /// Clipping produces these constantly, and each one is a vertex the
    /// renderer, the lightmapper and the collision code all carry for nothing.
    /// More than cost: a collinear point is where a T-junction crack starts.
    void remove_collinear();

    /// Snaps points onto whole numbers where they are within tolerance, then
    /// removes any point the snapping made collinear or coincident.
    void snap();

    /// Whether the winding is a usable surface: enough points, enough area, no
    /// zero-length edges.
    [[nodiscard]] bool valid() const;

private:
    std::vector<Vec> points_;
};

using Winding = WindingT<f32>;
using Windingd = WindingT<f64>;

extern template class WindingT<f32>;
extern template class WindingT<f64>;

}  // namespace kero::math
