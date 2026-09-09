// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "math/vec.hpp"

#include <cmath>
#include <numbers>

namespace kero::math {

inline constexpr f32 kPi = std::numbers::pi_v<f32>;

[[nodiscard]] inline constexpr f32 to_radians(f32 degrees) { return degrees * (kPi / 180.0f); }
[[nodiscard]] inline constexpr f32 to_degrees(f32 radians) { return radians * (180.0f / kPi); }

/// Euler angles in degrees, in Quake's order and Quake's sign convention.
///
/// **Pitch is positive downward.** Look at the floor and pitch is +90; look at
/// the sky and it is -90. This is backwards from every other right-handed
/// convention and it is kept on purpose. It is not a matter of taste: every
/// angle in every .kmap, every `angles` key on every entity, and every player's
/// muscle memory for mouse-look inversion is expressed in it. Correcting it
/// would silently mirror the vertical aim of every existing map, and the
/// "correct" version buys nothing -- an engine only has to be internally
/// consistent about which way is up.
struct Angles {
    f32 pitch = 0.0f;  ///< Rotation about Y. Positive looks *down*.
    f32 yaw = 0.0f;    ///< Rotation about Z. Positive turns left, 0 is +X.
    f32 roll = 0.0f;   ///< Rotation about X. Positive rolls right.

    constexpr Angles() = default;
    constexpr Angles(f32 pitch_, f32 yaw_, f32 roll_) : pitch(pitch_), yaw(yaw_), roll(roll_) {}

    [[nodiscard]] friend constexpr bool operator==(const Angles&, const Angles&) = default;
};

/// Wraps to [-180, 180). Applied to yaw before interpolating it, so turning
/// past due-east does not spin the long way round.
[[nodiscard]] inline f32 normalize_angle(f32 degrees) {
    degrees = std::fmod(degrees + 180.0f, 360.0f);
    if (degrees < 0.0f) {
        degrees += 360.0f;
    }
    return degrees - 180.0f;
}

/// The shortest signed difference from `from` to `to`.
[[nodiscard]] inline f32 angle_difference(f32 from, f32 to) {
    return normalize_angle(to - from);
}

/// The basis these angles describe. Any of the outputs may be null.
///
/// `forward` is where the entity is looking, `right` is to its right (not its
/// left -- this basis is left-handed in the same way Quake's was), and `up`
/// completes it.
void angle_vectors(const Angles& angles, Vec3* forward, Vec3* right, Vec3* up);

/// The angles that would produce `forward`. Roll cannot be recovered from a
/// direction alone and comes back as zero.
[[nodiscard]] Angles vector_to_angles(const Vec3& forward);

}  // namespace kero::math
