// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "math/plane.hpp"

namespace kero::math {

template <Scalar T>
bool PlaneT<T>::from_points(const Vec3T<T>& a, const Vec3T<T>& b, const Vec3T<T>& c, PlaneT& out) {
    // (a - b) x (c - b) rather than the more obvious (b - a) x (c - a): this is
    // the winding order .kmap uses, and getting it backwards turns every brush
    // inside out in a way that only shows up several stages later.
    Vec3T<T> normal = cross(a - b, c - b);
    const T length = normal.normalize();
    if (length < Tolerance<T>::kNormalLength) {
        return false;  // Collinear or coincident: not a plane.
    }

    out.normal = normal;
    out.distance = dot(normal, a);
    out.snap();
    return true;
}

template <Scalar T>
void PlaneT<T>::snap() {
    // An exactly-axial normal makes distance_to() cheap and, more importantly,
    // makes two faces of the same wall agree to the bit.
    for (usize axis = 0; axis < 3; ++axis) {
        if (normal[axis] > T{0} && normal[axis] >= Tolerance<T>::kPlaneNormal) {
            normal = Vec3T<T>{};
            normal[axis] = T{1};
            break;
        }
        if (normal[axis] < T{0} && -normal[axis] >= Tolerance<T>::kPlaneNormal) {
            normal = Vec3T<T>{};
            normal[axis] = T{-1};
            break;
        }
    }

    distance = snap_to_integer(distance, Tolerance<T>::kPlaneDistance);
    type = classify_normal(normal);
}

template struct PlaneT<f32>;
template struct PlaneT<f64>;

}  // namespace kero::math
