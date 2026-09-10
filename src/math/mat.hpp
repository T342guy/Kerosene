// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "math/angles.hpp"
#include "math/aabb.hpp"
#include "math/plane.hpp"
#include "math/vec.hpp"

#include <array>
#include <cmath>

namespace kero::math {

/// A 4x4 matrix, column-major, for the camera and nothing else.
///
/// Level geometry never needs one -- brushes are planes and stay planes -- so
/// this exists to get world coordinates into clip space and to pull a frustum
/// back out. Column-major because that is what every graphics API expects to be
/// handed, and converting at the boundary is one more place to get a transpose
/// wrong.
struct Mat4 {
    /// m[column][row].
    std::array<std::array<f32, 4>, 4> m{};

    [[nodiscard]] static constexpr Mat4 identity() {
        Mat4 result;
        for (usize i = 0; i < 4; ++i) {
            result.m[i][i] = 1.0f;
        }
        return result;
    }

    [[nodiscard]] const f32* data() const { return &m[0][0]; }

    [[nodiscard]] friend Mat4 operator*(const Mat4& a, const Mat4& b) {
        Mat4 result;
        for (usize column = 0; column < 4; ++column) {
            for (usize row = 0; row < 4; ++row) {
                f32 sum = 0.0f;
                for (usize k = 0; k < 4; ++k) {
                    sum += a.m[k][row] * b.m[column][k];
                }
                result.m[column][row] = sum;
            }
        }
        return result;
    }

    [[nodiscard]] Vec4 operator*(const Vec4& v) const {
        Vec4 result;
        for (usize row = 0; row < 4; ++row) {
            result[row] = m[0][row] * v.x + m[1][row] * v.y + m[2][row] * v.z +
                          m[3][row] * v.w;
        }
        return result;
    }

    /// A right-handed perspective projection with depth mapped to [0, 1].
    ///
    /// Zero to one rather than minus-one to one: it is what Vulkan, D3D12 and
    /// Metal all want, and combined with a reversed near and far plane it is
    /// also where floating-point depth has its precision. The reversal is not
    /// done here -- one thing at a time -- but the range is chosen so it can be.
    [[nodiscard]] static Mat4 perspective(f32 vertical_fov_degrees, f32 aspect, f32 near,
                                          f32 far) {
        const f32 focal = 1.0f / std::tan(to_radians(vertical_fov_degrees) * 0.5f);

        Mat4 result;
        result.m[0][0] = focal / aspect;
        result.m[1][1] = focal;
        result.m[2][2] = far / (near - far);
        result.m[2][3] = -1.0f;
        result.m[3][2] = (near * far) / (near - far);
        return result;
    }

    /// The view matrix for an eye at `eye` looking along Kerosene's angles.
    ///
    /// The world is Z-up with X east; clip space is the graphics convention of
    /// X right, Y up, -Z forward. The axis swap lives here, once, rather than
    /// being sprinkled through the renderer -- which is how a codebase ends up
    /// with two disagreeing ideas of which way is up.
    [[nodiscard]] static Mat4 view_from_angles(const Vec3& eye, const Angles& angles) {
        Vec3 forward;
        Vec3 right;
        Vec3 up;
        angle_vectors(angles, &forward, &right, &up);

        // `right` is used as it comes. It is tempting to negate it -- Quake's
        // basis is usually described as left-handed -- but the view basis
        // wanted here is (right, up, -forward), and that is right-handed
        // exactly when right is the vector angle_vectors already returns:
        // right x up = -forward. Negating it mirrors the whole image
        // left-to-right, which is a difficult thing to notice by eye in a
        // symmetrical room and an easy one to assert about.

        Mat4 result = identity();
        result.m[0][0] = right.x;
        result.m[1][0] = right.y;
        result.m[2][0] = right.z;
        result.m[0][1] = up.x;
        result.m[1][1] = up.y;
        result.m[2][1] = up.z;
        result.m[0][2] = -forward.x;
        result.m[1][2] = -forward.y;
        result.m[2][2] = -forward.z;
        result.m[3][0] = -dot(right, eye);
        result.m[3][1] = -dot(up, eye);
        result.m[3][2] = dot(forward, eye);
        return result;
    }
};

/// The six planes of a view volume, pointing inwards.
struct Frustum {
    std::array<Plane, 6> planes{};

    /// Extracted from a view-projection matrix by the standard Gribb-Hartmann
    /// method: each plane is a sum or difference of two rows.
    [[nodiscard]] static Frustum from_view_projection(const Mat4& vp) {
        Frustum frustum;
        const auto row = [&vp](usize r) {
            return Vec4(vp.m[0][r], vp.m[1][r], vp.m[2][r], vp.m[3][r]);
        };
        const Vec4 x = row(0);
        const Vec4 y = row(1);
        const Vec4 z = row(2);
        const Vec4 w = row(3);

        const Vec4 candidates[6] = {
            Vec4(w.x + x.x, w.y + x.y, w.z + x.z, w.w + x.w),  // left
            Vec4(w.x - x.x, w.y - x.y, w.z - x.z, w.w - x.w),  // right
            Vec4(w.x + y.x, w.y + y.y, w.z + y.z, w.w + y.w),  // bottom
            Vec4(w.x - y.x, w.y - y.y, w.z - y.z, w.w - y.w),  // top
            Vec4(z.x, z.y, z.z, z.w),                          // near (depth 0)
            Vec4(w.x - z.x, w.y - z.y, w.z - z.z, w.w - z.w),  // far
        };

        for (usize i = 0; i < 6; ++i) {
            Vec3 normal(candidates[i].x, candidates[i].y, candidates[i].z);
            const f32 length = normal.length();
            if (length > 1e-6f) {
                // Normalised so `distance_to` is a real distance, which the
                // box test relies on.
                frustum.planes[i] = Plane(normal / length, -candidates[i].w / length);
            }
        }
        return frustum;
    }

    /// Whether an axis-aligned box is at least partly inside.
    ///
    /// Conservative: a box that straddles two planes' outside regions without
    /// being outside either one is kept. Drawing something invisible costs a
    /// few microseconds; not drawing something visible is a hole in the world.
    [[nodiscard]] bool intersects(const AabbT<f32>& box) const {
        for (const Plane& plane : planes) {
            if (plane.normal.is_zero()) {
                continue;
            }
            // The corner furthest along the normal is the last to leave.
            if (plane.distance_to(box.support(plane.normal)) < 0.0f) {
                return false;
            }
        }
        return true;
    }
};

}  // namespace kero::math
