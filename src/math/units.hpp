// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <string>

/// The scale everything in Kerosene is measured in.
///
/// **One kerosene unit is two inches**, about five centimetres. Z is up.
///
/// The scale matters more than it looks like it should, because it decides
/// whether the numbers a level designer types are round. Quake picked the inch
/// and Source inherited it; the useful property was never the inch itself but
/// that powers of two landed on architectural sizes -- a grid step gave a stair
/// riser, a doubling gave a doorway, another gave a room. Two inches keeps that
/// property exactly and halves every number in the process.
///
/// Halving also buys real precision. A brush at the edge of a map sits at 8192
/// here rather than 16384, which is one more bit of float mantissa for every
/// intersection computed out there -- and the far corners of a level are
/// precisely where CSG produces the microscopic slivers that turn into leaks.
namespace kero::units {

/// Kerosene units in one metre. A ku is two inches, so this is a conversion,
/// not a number anyone gets to choose.
inline constexpr f32 kPerMetre = 19.685039f;
/// Kerosene units in one foot. Exactly six, which is why foot-scale
/// architecture lands on the grid without anyone trying.
inline constexpr f32 kPerFoot = 6.0f;
/// Kerosene units in one inch.
inline constexpr f32 kPerInch = 0.5f;

/// The reference figure the scale is built around: a standing player.
///
/// Quoted because it is what a designer actually judges a room against. A
/// ceiling is "three times the player" long before it is "108 ku".
inline constexpr f32 kPlayerHeight = 36.0f;
/// A ducking player.
inline constexpr f32 kPlayerDuckHeight = 18.0f;
/// Corner to corner of the player's collision box.
inline constexpr f32 kPlayerWidth = 16.0f;
/// Eye height, measured from the feet.
inline constexpr f32 kPlayerEyeHeight = 32.0f;
/// How fast a player runs on the flat.
inline constexpr f32 kPlayerSpeed = 160.0f;
/// The tallest ledge a player walks up without jumping.
inline constexpr f32 kStepHeight = 9.0f;

/// The default editing grid, and the size of a stair riser.
inline constexpr f32 kGridDefault = 4.0f;
/// Half the extent of the world along each axis. Everything beyond is void.
inline constexpr f32 kWorldExtent = 8192.0f;

[[nodiscard]] inline constexpr f32 to_metres(f32 ku) { return ku / kPerMetre; }
[[nodiscard]] inline constexpr f32 from_metres(f32 metres) { return metres * kPerMetre; }

/// "128 ku (6.50 m)" -- a distance with a metric equivalent beside it.
///
/// The metric half is what makes a number mean something to someone who has not
/// internalised the scale yet, which is everyone at first.
[[nodiscard]] std::string format_distance(f32 ku);

/// "128 ku", for places too narrow for both.
[[nodiscard]] std::string format_distance_short(f32 ku);

/// "512 x 384 x 128", as a designer reads a brush's size off the screen.
[[nodiscard]] std::string format_size(f32 x, f32 y, f32 z);

/// How tall something is in players -- the unit rooms are actually judged in.
[[nodiscard]] std::string format_in_players(f32 ku);

}  // namespace kero::units
