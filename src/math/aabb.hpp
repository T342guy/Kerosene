// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "math/vec.hpp"

#include <algorithm>
#include <limits>

namespace kero::math {

/// An axis-aligned bounding box.
///
/// Empty is represented by mins > maxs on every axis rather than by a flag, so
/// an empty box grows correctly the first time a point is added to it and
/// intersects nothing until then.
template <Scalar T>
struct AabbT {
    Vec3T<T> mins{std::numeric_limits<T>::max()};
    Vec3T<T> maxs{std::numeric_limits<T>::lowest()};

    constexpr AabbT() = default;
    constexpr AabbT(const Vec3T<T>& mins_, const Vec3T<T>& maxs_) : mins(mins_), maxs(maxs_) {}

    [[nodiscard]] constexpr bool empty() const {
        return mins.x > maxs.x || mins.y > maxs.y || mins.z > maxs.z;
    }

    constexpr void add(const Vec3T<T>& point) {
        for (usize axis = 0; axis < 3; ++axis) {
            mins[axis] = std::min(mins[axis], point[axis]);
            maxs[axis] = std::max(maxs[axis], point[axis]);
        }
    }

    constexpr void add(const AabbT& other) {
        if (other.empty()) {
            return;
        }
        add(other.mins);
        add(other.maxs);
    }

    /// Grows by `amount` on every axis. Negative shrinks.
    constexpr void expand(T amount) {
        const Vec3T<T> delta(amount, amount, amount);
        mins -= delta;
        maxs += delta;
    }

    [[nodiscard]] constexpr Vec3T<T> size() const { return maxs - mins; }
    [[nodiscard]] constexpr Vec3T<T> centre() const { return (mins + maxs) * T{0.5}; }

    [[nodiscard]] constexpr bool contains(const Vec3T<T>& point) const {
        return point.x >= mins.x && point.x <= maxs.x &&
               point.y >= mins.y && point.y <= maxs.y &&
               point.z >= mins.z && point.z <= maxs.z;
    }

    /// Overlap test. Touching faces count as overlapping, which is what a
    /// broadphase wants: two brushes sharing a wall must be considered.
    [[nodiscard]] constexpr bool intersects(const AabbT& other) const {
        return mins.x <= other.maxs.x && maxs.x >= other.mins.x &&
               mins.y <= other.maxs.y && maxs.y >= other.mins.y &&
               mins.z <= other.maxs.z && maxs.z >= other.mins.z;
    }

    /// The corner furthest along `normal`. Used for plane-versus-box tests,
    /// where testing only this corner and its opposite settles the question.
    [[nodiscard]] constexpr Vec3T<T> support(const Vec3T<T>& normal) const {
        return Vec3T<T>(normal.x >= T{0} ? maxs.x : mins.x,
                        normal.y >= T{0} ? maxs.y : mins.y,
                        normal.z >= T{0} ? maxs.z : mins.z);
    }
};

using Aabb = AabbT<f32>;
using Aabbd = AabbT<f64>;

}  // namespace kero::math
