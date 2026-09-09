// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "math/vec.hpp"

namespace kero::math {

/// How a plane is oriented, which decides how cheaply it can be worked with.
///
/// The overwhelming majority of planes in a brush-built level are axial: walls,
/// floors and ceilings drawn on a grid. Knowing that lets distance tests skip
/// two multiplies, and lets the BSP builder prefer axial splits, which produce
/// far fewer fragments than an arbitrary plane through the same space.
enum class PlaneType : u8 {
    X,        ///< Normal is +X or -X.
    Y,
    Z,
    AnyX,     ///< Not axial; X is the dominant component.
    AnyY,
    AnyZ,
};

[[nodiscard]] constexpr bool is_axial(PlaneType type) { return type <= PlaneType::Z; }

/// An oriented plane: the set of points p where dot(normal, p) == distance.
///
/// "Front" is the side the normal points into, and a brush is the intersection
/// of the *back* half-spaces of its sides -- the normals face outward, away from
/// the solid. That convention is inherited from Quake, and it is worth keeping
/// because it makes a brush's sides and its rendered faces the same objects
/// pointing the same way.
template <Scalar T>
struct PlaneT {
    Vec3T<T> normal{};
    T distance{};
    PlaneType type = PlaneType::AnyZ;

    constexpr PlaneT() = default;
    PlaneT(const Vec3T<T>& normal_, T distance_)
        : normal(normal_), distance(distance_), type(classify_normal(normal_)) {}

    /// The plane through three points, wound counter-clockwise when seen from
    /// the front. This is how .kmap stores a brush side, so it is the entry
    /// point for every plane the compiler ever sees.
    ///
    /// Returns false without touching `out` if the points are collinear or
    /// coincident -- a real condition in hand-edited maps, and one the caller
    /// has to report against the brush that caused it rather than assert on.
    [[nodiscard]] static bool from_points(const Vec3T<T>& a, const Vec3T<T>& b,
                                          const Vec3T<T>& c, PlaneT& out);

    [[nodiscard]] constexpr T distance_to(const Vec3T<T>& point) const {
        // The axial cases are not an optimisation the compiler could not do --
        // it cannot, because it does not know the two other components are
        // exactly zero -- and axial planes are most of a level.
        switch (type) {
            case PlaneType::X: return normal.x * point.x - distance;
            case PlaneType::Y: return normal.y * point.y - distance;
            case PlaneType::Z: return normal.z * point.z - distance;
            default: return dot(normal, point) - distance;
        }
    }

    [[nodiscard]] constexpr Side classify(const Vec3T<T>& point) const {
        return classify_distance<T>(distance_to(point));
    }

    /// The same plane facing the other way.
    [[nodiscard]] constexpr PlaneT flipped() const {
        PlaneT result;
        result.normal = -normal;
        result.distance = -distance;
        result.type = type;
        return result;
    }

    /// Nudges a nearly-axial normal onto the axis, and a nearly-integral
    /// distance onto the integer.
    ///
    /// A wall drawn on the grid *is* axial; it only fails to be after the
    /// arithmetic that produced its plane from three points. Left alone, a
    /// normal of (0.9999999, 0.0000004, 0) makes the BSP builder treat a
    /// perfectly ordinary wall as an arbitrary plane, and makes two faces of
    /// the same wall disagree about where it is. Putting it back costs a
    /// comparison and removes an entire class of sliver.
    void snap();

    /// Whether two planes are the same plane, facing the same way.
    [[nodiscard]] bool equivalent(const PlaneT& other) const {
        return dot(normal, other.normal) >= Tolerance<T>::kPlaneNormal &&
               nearly_equal(distance, other.distance, Tolerance<T>::kPlaneDistance);
    }

    /// Whether two planes are the same plane, ignoring which way they face.
    [[nodiscard]] bool coplanar(const PlaneT& other) const {
        return equivalent(other) || equivalent(other.flipped());
    }

    [[nodiscard]] static constexpr PlaneType classify_normal(const Vec3T<T>& normal_) {
        for (usize axis = 0; axis < 3; ++axis) {
            if (normal_[axis] == T{1} || normal_[axis] == T{-1}) {
                return static_cast<PlaneType>(axis);
            }
        }
        switch (normal_.major_axis()) {
            case 0:  return PlaneType::AnyX;
            case 1:  return PlaneType::AnyY;
            default: return PlaneType::AnyZ;
        }
    }
};

using Plane = PlaneT<f32>;
using Planed = PlaneT<f64>;

extern template struct PlaneT<f32>;
extern template struct PlaneT<f64>;

}  // namespace kero::math
