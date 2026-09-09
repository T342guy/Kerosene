// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "math/angles.hpp"

namespace kero::math {

void angle_vectors(const Angles& angles, Vec3* forward, Vec3* right, Vec3* up) {
    const f32 sp = std::sin(to_radians(angles.pitch));
    const f32 cp = std::cos(to_radians(angles.pitch));
    const f32 sy = std::sin(to_radians(angles.yaw));
    const f32 cy = std::cos(to_radians(angles.yaw));
    const f32 sr = std::sin(to_radians(angles.roll));
    const f32 cr = std::cos(to_radians(angles.roll));

    if (forward != nullptr) {
        // -sp on Z is where "pitch is positive downward" lives.
        *forward = Vec3(cp * cy, cp * sy, -sp);
    }
    if (right != nullptr) {
        *right = Vec3(-sr * sp * cy + cr * sy,
                      -sr * sp * sy - cr * cy,
                      -sr * cp);
    }
    if (up != nullptr) {
        *up = Vec3(cr * sp * cy + sr * sy,
                   cr * sp * sy - sr * cy,
                   cr * cp);
    }
}

Angles vector_to_angles(const Vec3& forward) {
    if (forward.x == 0.0f && forward.y == 0.0f) {
        // Straight up or straight down: yaw is unconstrained, so it is reported
        // as zero rather than as whatever atan2(0, 0) happens to give.
        return Angles(forward.z > 0.0f ? -90.0f : 90.0f, 0.0f, 0.0f);
    }

    const f32 yaw = to_degrees(std::atan2(forward.y, forward.x));
    const f32 horizontal = std::sqrt(forward.x * forward.x + forward.y * forward.y);
    const f32 pitch = to_degrees(std::atan2(-forward.z, horizontal));
    return Angles(pitch, yaw, 0.0f);
}

}  // namespace kero::math
