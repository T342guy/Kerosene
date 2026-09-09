// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <cmath>
#include <concepts>
#include <string_view>

/// The epsilon policy, in one place.
///
/// `vbsp` carries a handful of tolerances -- ON_EPSILON, a distance epsilon, an
/// area threshold -- spread across the files that use them, several of them
/// spelled `0.1` inline. The values are not wrong so much as unattributed: it is
/// impossible to tell, at a given comparison, which tolerance is meant, whether
/// it is the same one two files over, or what would break if it moved. That is
/// how a compiler ends up producing microscopic slivers in one stage and
/// dropping real faces in the next.
///
/// So: every tolerance in Kerosene is named for what it means, defined here,
/// expressed in kerosene units, and chosen per scalar type. The compile stages
/// instantiate the geometry on `f64` and get tight tolerances; the runtime
/// instantiates on `f32` and gets loose ones. That split is deliberate. Source
/// runs CSG in single precision and pays for it in phantom leaks; doubles cost
/// nothing at build time, where the whole point is to be slow and right.
namespace kero::math {

template <typename T>
concept Scalar = std::floating_point<T>;

template <Scalar T>
struct Tolerance;

template <>
struct Tolerance<f64> {
    /// A point closer than this to a plane is treated as on it.
    ///
    /// 0.005 ku is a quarter of a millimetre. Tight enough that no visible
    /// geometry is quantised away, loose enough to absorb the rounding of a few
    /// hundred chained clip operations in double precision.
    static constexpr f64 kPointOnPlane = 0.005;

    /// A winding with less area than this is not a surface; it is the residue
    /// of clipping a face down to nothing, and keeping it produces a degenerate
    /// face the renderer and the lightmapper both have to special-case.
    static constexpr f64 kDegenerateArea = 0.05;

    /// Edges shorter than this collapse. Slivers this thin cannot be textured,
    /// lit, or collided with meaningfully.
    static constexpr f64 kDegenerateEdge = 0.02;

    /// Two planes within this distance, and pointing the same way, are one
    /// plane. Deduplicating planes is what keeps the BSP tree from splitting on
    /// two copies of the same wall.
    static constexpr f64 kPlaneDistance = 0.005;

    /// Two normals whose dot product exceeds this are the same direction.
    /// Corresponds to about a quarter of a degree.
    static constexpr f64 kPlaneNormal = 0.99999;

    /// Below this, a normal is not a direction and the plane is degenerate.
    static constexpr f64 kNormalLength = 1e-6;
};

template <>
struct Tolerance<f32> {
    /// Ten times looser than the compile-time value: a float carries about seven
    /// significant digits, and a coordinate at the world edge (8192 ku) has
    /// roughly 0.001 ku of representable resolution left. Asking for a quarter
    /// of a millimetre out there would be asking for noise.
    static constexpr f32 kPointOnPlane = 0.05f;
    static constexpr f32 kDegenerateArea = 0.1f;
    static constexpr f32 kDegenerateEdge = 0.05f;
    static constexpr f32 kPlaneDistance = 0.01f;
    static constexpr f32 kPlaneNormal = 0.9999f;
    static constexpr f32 kNormalLength = 1e-5f;
};

/// Where a point sits relative to a plane, within kPointOnPlane.
enum class Side : u8 {
    Front,
    Back,
    On,
    /// Only ever the answer for a set of points, never for one.
    Crossing,
};

[[nodiscard]] constexpr std::string_view to_string(Side side) {
    switch (side) {
        case Side::Front:    return "front";
        case Side::Back:     return "back";
        case Side::On:       return "on";
        case Side::Crossing: return "crossing";
    }
    return "?";
}

template <Scalar T>
[[nodiscard]] constexpr Side classify_distance(T distance) {
    if (distance > Tolerance<T>::kPointOnPlane) {
        return Side::Front;
    }
    if (distance < -Tolerance<T>::kPointOnPlane) {
        return Side::Back;
    }
    return Side::On;
}

template <Scalar T>
[[nodiscard]] constexpr bool nearly_equal(T a, T b, T tolerance = Tolerance<T>::kPointOnPlane) {
    const T difference = a > b ? a - b : b - a;
    return difference <= tolerance;
}

/// Snaps a value that is within `tolerance` of a whole number onto it.
///
/// Level geometry is authored on a grid, so a coordinate that comes out of a
/// chain of clips as 127.99999998 was 128 before anyone did arithmetic to it.
/// Putting it back is not cosmetic: it stops the same corner being two
/// different points in two different faces, which is what T-junctions and
/// pinhole seams are made of.
template <Scalar T>
[[nodiscard]] inline T snap_to_integer(T value, T tolerance = Tolerance<T>::kPointOnPlane) {
    const T rounded = std::round(value);
    return (std::abs(value - rounded) <= tolerance) ? rounded : value;
}

}  // namespace kero::math
