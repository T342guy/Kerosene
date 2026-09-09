// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "math/winding.hpp"

#include "math/units.hpp"

#include <algorithm>
#include <limits>

namespace kero::math {

template <Scalar T>
WindingT<T> WindingT<T>::from_plane(const PlaneT<T>& plane, T extent) {
    // Two axes spanning the plane. `any_perpendicular` picks the one least
    // aligned with the normal, so neither axis can collapse.
    const Vec up = any_perpendicular(plane.normal);
    const Vec right = cross(up, plane.normal);

    const Vec origin = plane.normal * plane.distance;
    const Vec u = up * extent;
    const Vec r = right * extent;

    // Counter-clockwise seen from the front, matching the convention every
    // other winding in the compiler is built with.
    return WindingT<T>(std::vector<Vec>{
        origin - r + u,
        origin + r + u,
        origin + r - u,
        origin - r - u,
    });
}

template <Scalar T>
WindingT<T> WindingT<T>::from_plane(const PlaneT<T>& plane) {
    // Comfortably outside the world, so a base winding always survives being
    // clipped by the brush sides that actually bound it. Cheaper to start too
    // large than to discover halfway through CSG that it was too small.
    constexpr T kExtent = static_cast<T>(units::kWorldExtent) * T{2};
    return from_plane(plane, kExtent);
}

template <Scalar T>
Side WindingT<T>::classify(const PlaneT<T>& plane) const {
    bool front = false;
    bool back = false;
    for (const Vec& point : points_) {
        switch (plane.classify(point)) {
            case Side::Front: front = true; break;
            case Side::Back:  back = true; break;
            default: break;
        }
        if (front && back) {
            return Side::Crossing;
        }
    }
    if (front) return Side::Front;
    if (back) return Side::Back;
    return Side::On;
}

template <Scalar T>
void WindingT<T>::split(const PlaneT<T>& plane,
                        std::optional<WindingT<T>>& front,
                        std::optional<WindingT<T>>& back) const {
    front.reset();
    back.reset();

    const usize count = points_.size();
    if (count < 3) {
        return;
    }

    // Classified once up front, and reused. Recomputing a point's side while
    // walking the edges can classify the same point two different ways when it
    // sits exactly on the tolerance boundary, which produces a winding with a
    // duplicated or missing vertex -- a crack, one pixel wide, in a wall.
    std::vector<Side> sides(count + 1);
    std::vector<T> distances(count + 1);
    usize counts[3] = {0, 0, 0};

    for (usize i = 0; i < count; ++i) {
        distances[i] = plane.distance_to(points_[i]);
        sides[i] = classify_distance<T>(distances[i]);
        ++counts[static_cast<usize>(sides[i])];
    }
    // The wrap-around entry, so the edge loop needs no modulo.
    sides[count] = sides[0];
    distances[count] = distances[0];

    if (counts[static_cast<usize>(Side::Back)] == 0 &&
        counts[static_cast<usize>(Side::Front)] == 0) {
        return;  // Entirely on the plane: belongs to neither side.
    }
    if (counts[static_cast<usize>(Side::Back)] == 0) {
        front = *this;
        return;
    }
    if (counts[static_cast<usize>(Side::Front)] == 0) {
        back = *this;
        return;
    }

    std::vector<Vec> front_points;
    std::vector<Vec> back_points;
    front_points.reserve(count + 4);
    back_points.reserve(count + 4);

    for (usize i = 0; i < count; ++i) {
        const Vec& current = points_[i];

        if (sides[i] == Side::On) {
            // A point on the plane belongs to both halves, and is not an
            // intersection to be computed -- computing it would move it.
            front_points.push_back(current);
            back_points.push_back(current);
            continue;
        }
        if (sides[i] == Side::Front) {
            front_points.push_back(current);
        } else {
            back_points.push_back(current);
        }

        if (sides[i + 1] == Side::On || sides[i + 1] == sides[i]) {
            continue;  // This edge does not cross.
        }

        const Vec& next = points_[(i + 1) % count];
        const T t = distances[i] / (distances[i] - distances[i + 1]);

        Vec mid;
        for (usize axis = 0; axis < 3; ++axis) {
            // An axial plane pins one coordinate exactly. Interpolating it
            // instead would leave the split vertex a rounding error off the
            // wall it is supposed to be welded to.
            if (plane.type == static_cast<PlaneType>(axis)) {
                mid[axis] = plane.normal[axis] > T{0} ? plane.distance : -plane.distance;
            } else {
                mid[axis] = current[axis] + t * (next[axis] - current[axis]);
            }
        }

        // The same point object goes into both halves, so the shared edge is
        // shared exactly rather than twice-computed.
        front_points.push_back(mid);
        back_points.push_back(mid);
    }

    WindingT<T> front_winding(std::move(front_points));
    WindingT<T> back_winding(std::move(back_points));

    if (front_winding.valid()) {
        front = std::move(front_winding);
    }
    if (back_winding.valid()) {
        back = std::move(back_winding);
    }
}

template <Scalar T>
std::optional<WindingT<T>> WindingT<T>::clipped(const PlaneT<T>& plane) const {
    std::optional<WindingT<T>> front;
    std::optional<WindingT<T>> back;
    split(plane, front, back);
    return front;
}

template <Scalar T>
bool WindingT<T>::plane(PlaneT<T>& out) const {
    if (points_.size() < 3) {
        return false;
    }
    // The first three points can be nearly collinear even when the polygon is
    // perfectly good, so the widest triple is used rather than the first.
    usize best_a = 0;
    usize best_b = 1;
    usize best_c = 2;
    T best = T{0};
    const usize count = points_.size();
    for (usize i = 0; i < count; ++i) {
        for (usize j = i + 1; j < count; ++j) {
            for (usize k = j + 1; k < count; ++k) {
                const T magnitude = cross(points_[i] - points_[j], points_[k] - points_[j])
                                        .length_squared();
                if (magnitude > best) {
                    best = magnitude;
                    best_a = i;
                    best_b = j;
                    best_c = k;
                }
            }
        }
        // Quadratic in the point count, and windings past a handful of points
        // are already well-conditioned; the exhaustive search only pays for
        // itself on the small, nearly-degenerate ones.
        if (count > 8) {
            break;
        }
    }
    return PlaneT<T>::from_points(points_[best_a], points_[best_b], points_[best_c], out);
}

template <Scalar T>
T WindingT<T>::area() const {
    if (points_.size() < 3) {
        return T{0};
    }
    // Fan triangulation from the first point. Valid for any convex polygon, and
    // this type is only ever convex.
    T total{0};
    for (usize i = 2; i < points_.size(); ++i) {
        total += cross(points_[i - 1] - points_[0], points_[i] - points_[0]).length();
    }
    return total * T{0.5};
}

template <Scalar T>
Vec3T<T> WindingT<T>::centre() const {
    if (points_.empty()) {
        return Vec{};
    }
    Vec sum{};
    for (const Vec& point : points_) {
        sum += point;
    }
    return sum / static_cast<T>(points_.size());
}

template <Scalar T>
void WindingT<T>::bounds(Vec& mins, Vec& maxs) const {
    constexpr T kBig = std::numeric_limits<T>::max();
    mins = Vec(kBig, kBig, kBig);
    maxs = Vec(-kBig, -kBig, -kBig);
    for (const Vec& point : points_) {
        for (usize axis = 0; axis < 3; ++axis) {
            mins[axis] = std::min(mins[axis], point[axis]);
            maxs[axis] = std::max(maxs[axis], point[axis]);
        }
    }
}

template <Scalar T>
WindingT<T> WindingT<T>::reversed() const {
    std::vector<Vec> flipped(points_.rbegin(), points_.rend());
    return WindingT<T>(std::move(flipped));
}

template <Scalar T>
void WindingT<T>::remove_collinear() {
    if (points_.size() < 3) {
        return;
    }

    std::vector<Vec> kept;
    kept.reserve(points_.size());

    const usize count = points_.size();
    for (usize i = 0; i < count; ++i) {
        const Vec& previous = points_[(i + count - 1) % count];
        const Vec& current = points_[i];
        const Vec& next = points_[(i + 1) % count];

        Vec incoming = current - previous;
        Vec outgoing = next - current;
        const T incoming_length = incoming.normalize();
        const T outgoing_length = outgoing.normalize();

        // A coincident point has no direction to compare, and is dropped for
        // the same reason a collinear one is.
        if (incoming_length < Tolerance<T>::kDegenerateEdge ||
            outgoing_length < Tolerance<T>::kDegenerateEdge) {
            continue;
        }
        if (dot(incoming, outgoing) >= Tolerance<T>::kPlaneNormal) {
            continue;  // Straight through: the vertex carries no shape.
        }
        kept.push_back(current);
    }

    points_ = std::move(kept);
}

template <Scalar T>
void WindingT<T>::snap() {
    for (Vec& point : points_) {
        point = snap_to_grid(point);
    }
    remove_collinear();
}

template <Scalar T>
bool WindingT<T>::valid() const {
    if (points_.size() < 3) {
        return false;
    }
    if (area() < Tolerance<T>::kDegenerateArea) {
        return false;
    }
    // Area alone passes a long thin triangle with one collapsed edge, which is
    // exactly the shape that survives a near-tangent clip.
    const usize count = points_.size();
    for (usize i = 0; i < count; ++i) {
        const T edge = (points_[(i + 1) % count] - points_[i]).length();
        if (edge < Tolerance<T>::kDegenerateEdge) {
            return false;
        }
        for (usize axis = 0; axis < 3; ++axis) {
            if (!(points_[i][axis] > -static_cast<T>(units::kWorldExtent) * T{4} &&
                  points_[i][axis] < static_cast<T>(units::kWorldExtent) * T{4})) {
                return false;  // Escaped the world, or is not a number at all.
            }
        }
    }
    return true;
}

template class WindingT<f32>;
template class WindingT<f64>;

}  // namespace kero::math
