// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/assert.hpp"
#include "math/scalar.hpp"

#include <cmath>
#include <format>
#include <string>

namespace kero::math {

/// A three-component vector, templated on precision.
///
/// Two instantiations exist and both are used constantly: `Vec3` (f32) at
/// runtime, `Vec3d` (f64) inside the compilers. Templating rather than
/// duplicating is what makes it possible to write one CSG routine and get the
/// precise version for free -- and what stops the two drifting apart, which is
/// the failure mode of every codebase that keeps a `vec_t` typedef and flips it.
///
/// Z is up. Positive X is east, positive Y is north.
template <Scalar T>
struct Vec3T {
    T x{};
    T y{};
    T z{};

    constexpr Vec3T() = default;
    constexpr Vec3T(T x_, T y_, T z_) : x(x_), y(y_), z(z_) {}
    explicit constexpr Vec3T(T all) : x(all), y(all), z(all) {}

    /// Conversion between precisions is explicit in the narrowing direction.
    /// A double-to-float that happens by accident, in the middle of a clip
    /// chain, is exactly the bug the two instantiations exist to prevent.
    template <Scalar U>
    explicit constexpr Vec3T(const Vec3T<U>& other)
        : x(static_cast<T>(other.x)), y(static_cast<T>(other.y)), z(static_cast<T>(other.z)) {}

    [[nodiscard]] constexpr T& operator[](usize index) {
        KERO_ASSERT(index < 3, "vector index out of range");
        return (&x)[index];
    }
    [[nodiscard]] constexpr const T& operator[](usize index) const {
        KERO_ASSERT(index < 3, "vector index out of range");
        return (&x)[index];
    }

    constexpr Vec3T& operator+=(const Vec3T& v) { x += v.x; y += v.y; z += v.z; return *this; }
    constexpr Vec3T& operator-=(const Vec3T& v) { x -= v.x; y -= v.y; z -= v.z; return *this; }
    constexpr Vec3T& operator*=(T s) { x *= s; y *= s; z *= s; return *this; }
    constexpr Vec3T& operator/=(T s) { x /= s; y /= s; z /= s; return *this; }

    [[nodiscard]] friend constexpr Vec3T operator+(Vec3T a, const Vec3T& b) { return a += b; }
    [[nodiscard]] friend constexpr Vec3T operator-(Vec3T a, const Vec3T& b) { return a -= b; }
    [[nodiscard]] friend constexpr Vec3T operator*(Vec3T v, T s) { return v *= s; }
    [[nodiscard]] friend constexpr Vec3T operator*(T s, Vec3T v) { return v *= s; }
    [[nodiscard]] friend constexpr Vec3T operator/(Vec3T v, T s) { return v /= s; }
    [[nodiscard]] constexpr Vec3T operator-() const { return Vec3T(-x, -y, -z); }

    /// Exact equality. Almost never what you want for geometry -- use
    /// nearly_equal() -- but needed for hashing and for tests that mean it.
    [[nodiscard]] friend constexpr bool operator==(const Vec3T&, const Vec3T&) = default;

    [[nodiscard]] constexpr T length_squared() const { return x * x + y * y + z * z; }
    [[nodiscard]] T length() const { return std::sqrt(length_squared()); }

    /// Returns the length before normalising, so a caller can reject a
    /// degenerate vector without computing the length twice.
    T normalize() {
        const T len = length();
        if (len < Tolerance<T>::kNormalLength) {
            return T{0};
        }
        *this /= len;
        return len;
    }

    [[nodiscard]] Vec3T normalized() const {
        Vec3T result = *this;
        result.normalize();
        return result;
    }

    [[nodiscard]] constexpr bool is_zero(T tolerance = Tolerance<T>::kNormalLength) const {
        return length_squared() <= tolerance * tolerance;
    }

    /// The axis with the largest magnitude: 0, 1 or 2.
    ///
    /// Used everywhere in BSP work -- picking a projection axis for a winding,
    /// choosing a texture axis, deciding which plane a face is most nearly
    /// parallel to. Projecting along the dominant axis is the one choice
    /// guaranteed not to collapse the polygon to a line.
    [[nodiscard]] constexpr usize major_axis() const {
        const T ax = x < T{0} ? -x : x;
        const T ay = y < T{0} ? -y : y;
        const T az = z < T{0} ? -z : z;
        if (ax >= ay && ax >= az) return 0;
        return ay >= az ? 1 : 2;
    }

    [[nodiscard]] static constexpr Vec3T zero() { return Vec3T{}; }
    [[nodiscard]] static constexpr Vec3T unit_x() { return Vec3T(T{1}, T{0}, T{0}); }
    [[nodiscard]] static constexpr Vec3T unit_y() { return Vec3T(T{0}, T{1}, T{0}); }
    [[nodiscard]] static constexpr Vec3T unit_z() { return Vec3T(T{0}, T{0}, T{1}); }
    /// Z is up, so this is it.
    [[nodiscard]] static constexpr Vec3T up() { return unit_z(); }
};

template <Scalar T>
[[nodiscard]] constexpr T dot(const Vec3T<T>& a, const Vec3T<T>& b) {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

template <Scalar T>
[[nodiscard]] constexpr Vec3T<T> cross(const Vec3T<T>& a, const Vec3T<T>& b) {
    return Vec3T<T>(a.y * b.z - a.z * b.y,
                    a.z * b.x - a.x * b.z,
                    a.x * b.y - a.y * b.x);
}

template <Scalar T>
[[nodiscard]] T distance(const Vec3T<T>& a, const Vec3T<T>& b) { return (a - b).length(); }

template <Scalar T>
[[nodiscard]] constexpr T distance_squared(const Vec3T<T>& a, const Vec3T<T>& b) {
    return (a - b).length_squared();
}

template <Scalar T>
[[nodiscard]] constexpr Vec3T<T> lerp(const Vec3T<T>& a, const Vec3T<T>& b, T t) {
    return a + (b - a) * t;
}

template <Scalar T>
[[nodiscard]] constexpr bool nearly_equal(const Vec3T<T>& a, const Vec3T<T>& b,
                                          T tolerance = Tolerance<T>::kPointOnPlane) {
    return nearly_equal(a.x, b.x, tolerance) &&
           nearly_equal(a.y, b.y, tolerance) &&
           nearly_equal(a.z, b.z, tolerance);
}

/// Snaps each component onto a whole number when it is within tolerance.
/// See snap_to_integer() for why this is worth doing.
template <Scalar T>
[[nodiscard]] inline Vec3T<T> snap_to_grid(const Vec3T<T>& v,
                                           T tolerance = Tolerance<T>::kPointOnPlane) {
    return Vec3T<T>(snap_to_integer(v.x, tolerance),
                    snap_to_integer(v.y, tolerance),
                    snap_to_integer(v.z, tolerance));
}

/// Any unit vector perpendicular to `v`.
///
/// Which one does not matter -- what matters is that it is never degenerate.
/// Crossing with a fixed axis fails when `v` happens to be that axis, so the
/// axis is chosen to be the one `v` is least aligned with.
template <Scalar T>
[[nodiscard]] Vec3T<T> any_perpendicular(const Vec3T<T>& v) {
    const usize major = v.major_axis();
    Vec3T<T> axis;
    axis[major == 0 ? 1 : 0] = T{1};
    return cross(v, axis).normalized();
}

template <Scalar T>
struct Vec2T {
    T x{};
    T y{};

    constexpr Vec2T() = default;
    constexpr Vec2T(T x_, T y_) : x(x_), y(y_) {}

    constexpr Vec2T& operator+=(const Vec2T& v) { x += v.x; y += v.y; return *this; }
    constexpr Vec2T& operator-=(const Vec2T& v) { x -= v.x; y -= v.y; return *this; }
    constexpr Vec2T& operator*=(T s) { x *= s; y *= s; return *this; }

    [[nodiscard]] friend constexpr Vec2T operator+(Vec2T a, const Vec2T& b) { return a += b; }
    [[nodiscard]] friend constexpr Vec2T operator-(Vec2T a, const Vec2T& b) { return a -= b; }
    [[nodiscard]] friend constexpr Vec2T operator*(Vec2T v, T s) { return v *= s; }
    [[nodiscard]] friend constexpr bool operator==(const Vec2T&, const Vec2T&) = default;

    [[nodiscard]] constexpr T length_squared() const { return x * x + y * y; }
    [[nodiscard]] T length() const { return std::sqrt(length_squared()); }
};

/// A four-component vector. Used for shader-facing data and homogeneous
/// coordinates, not for geometry -- geometry is Vec3 and a separate w means a
/// plane, which has its own type.
template <Scalar T>
struct Vec4T {
    T x{}, y{}, z{}, w{};

    constexpr Vec4T() = default;
    constexpr Vec4T(T x_, T y_, T z_, T w_) : x(x_), y(y_), z(z_), w(w_) {}
    constexpr Vec4T(const Vec3T<T>& v, T w_) : x(v.x), y(v.y), z(v.z), w(w_) {}

    [[nodiscard]] constexpr T& operator[](usize index) {
        KERO_ASSERT(index < 4, "vector index out of range");
        return (&x)[index];
    }
    [[nodiscard]] constexpr const T& operator[](usize index) const {
        KERO_ASSERT(index < 4, "vector index out of range");
        return (&x)[index];
    }

    [[nodiscard]] constexpr Vec3T<T> xyz() const { return Vec3T<T>(x, y, z); }
    [[nodiscard]] friend constexpr bool operator==(const Vec4T&, const Vec4T&) = default;
};

using Vec2 = Vec2T<f32>;
using Vec3 = Vec3T<f32>;
using Vec4 = Vec4T<f32>;

using Vec2d = Vec2T<f64>;
using Vec3d = Vec3T<f64>;
using Vec4d = Vec4T<f64>;

}  // namespace kero::math

/// "(128 -64 32)" -- the same shape the .kmap text format uses, so a logged
/// coordinate can be pasted straight into a map file.
template <kero::math::Scalar T>
struct std::formatter<kero::math::Vec3T<T>> : std::formatter<std::string> {
    auto format(const kero::math::Vec3T<T>& v, std::format_context& ctx) const {
        return std::formatter<std::string>::format(
            std::format("({:g} {:g} {:g})", static_cast<double>(v.x),
                        static_cast<double>(v.y), static_cast<double>(v.z)),
            ctx);
    }
};
