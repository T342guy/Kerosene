// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The units everything in Kerosene is measured in.
//!
//! Distances are **kerosene units** (`ku`). One kerosene unit is one inch, which fixes
//! the scale of everything else: a player is 72 ku tall, walks at 320 ku/s,
//! and a comfortable corridor is about 128 ku wide. The choice is inherited
//! from Quake and Source, and the reason to keep it is that it is a scale
//! brush geometry works at -- powers of two land on architectural sizes, so a
//! grid of 16 ku gives doorways and stair risers that are already right.
//!
//! Naming them matters more than it sounds. A number with no unit on it is a
//! number nobody can check: `128` could be inches, centimetres or something
//! the editor made up. Written as `128 ku (3.25 m)` it can be argued with.
//!
//! | Quantity | Unit | Symbol |
//! |---|---|---|
//! | Distance | kerosene unit | `ku` |
//! | Area | square kerosene unit | `vu²` |
//! | Volume | cubic kerosene unit | `vu³` |
//! | Speed | kerosene units per second | `ku/s` |
//! | Angle | degree | `°` |
//! | Time | second | `s` |

/// Kerosene units in one metre.
///
/// A kerosene unit is an inch, so this is the inches-per-metre conversion and not
/// a number anyone gets to choose.
pub const VU_PER_METRE: f32 = 39.3701;

/// Kerosene units in one foot.
pub const VU_PER_FOOT: f32 = 12.0;

/// The reference figure the scale is built around: a standing player.
///
/// Quoted here because it is the measurement a designer actually judges a
/// room against -- a ceiling is "three times the player" long before it is
/// "216 ku".
pub const PLAYER_HEIGHT: f32 = 72.0;
/// A player's width, corner to corner of the collision box.
pub const PLAYER_WIDTH: f32 = 32.0;
/// How fast a player runs on the flat.
pub const PLAYER_SPEED: f32 = 320.0;

pub fn metres(ku: f32) -> f32 { ku / VU_PER_METRE }
pub fn from_metres(m: f32) -> f32 { m * VU_PER_METRE }
pub fn feet(ku: f32) -> f32 { ku / VU_PER_FOOT }

/// Format a distance with its unit and a metric equivalent.
///
/// The metric half is what makes a number mean something to a person who has
/// not internalised the scale yet, which is everyone at first.
pub fn length(ku: f32) -> String {
    format!("{} ku ({:.2} m)", trim(ku), metres(ku))
}

/// Format a distance with just the unit, for places too narrow for both.
pub fn length_short(ku: f32) -> String {
    format!("{} ku", trim(ku))
}

/// Format a three-axis size, as a designer reads it off a brush.
pub fn size(x: f32, y: f32, z: f32) -> String {
    format!(
        "{} x {} x {} ku ({:.2} x {:.2} x {:.2} m)",
        trim(x),
        trim(y),
        trim(z),
        metres(x),
        metres(y),
        metres(z)
    )
}

pub fn area(vu2: f32) -> String {
    format!("{} ku\u{b2}", trim(vu2))
}

pub fn volume(vu3: f32) -> String {
    format!("{} ku\u{b3}", trim(vu3))
}

pub fn speed(vu_per_second: f32) -> String {
    format!("{} ku/s ({:.1} m/s)", trim(vu_per_second), metres(vu_per_second))
}

/// How tall something is in players, for judging a space.
pub fn in_players(ku: f32) -> String {
    format!("{:.1}x player", ku / PLAYER_HEIGHT)
}

/// Numbers without trailing zeroes: level geometry is nearly all integers, and
/// `128.00` is noise where `128` is a measurement.
fn trim(v: f32) -> String {
    crate::format_float(v)
}

#[cfg(test)]
mod format_tests {
    use crate::format_float;

    #[test]
    fn whole_numbers_lose_their_decimal_point() {
        assert_eq!(format_float(128.0), "128");
        assert_eq!(format_float(-0.0), "0");
        assert_eq!(format_float(0.25), "0.25");
        assert_eq!(format_float(1.5), "1.5");
        // Six places, then trimmed: enough for a plane distance, not enough
        // to write out the noise in the last bit of an f32.
        assert_eq!(format_float(1.0 / 3.0), "0.333333");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kerosene_unit_is_an_inch() {
        assert!((metres(VU_PER_METRE) - 1.0).abs() < 1e-4);
        assert!((from_metres(1.0) - 39.3701).abs() < 1e-3);
        assert!((feet(12.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_player_is_six_feet_tall() {
        assert!((feet(PLAYER_HEIGHT) - 6.0).abs() < 1e-6);
        assert!((metres(PLAYER_HEIGHT) - 1.83).abs() < 0.01);
    }

    #[test]
    fn distances_carry_their_unit_and_a_metric_equivalent() {
        assert_eq!(length(128.0), "128 ku (3.25 m)");
        assert_eq!(length_short(128.0), "128 ku");
        assert_eq!(length(0.5), "0.5 ku (0.01 m)");
    }

    #[test]
    fn sizes_read_the_way_a_brush_is_measured() {
        assert_eq!(size(64.0, 128.0, 16.0), "64 x 128 x 16 ku (1.63 x 3.25 x 0.41 m)");
    }

    #[test]
    fn speeds_and_areas_have_units_too() {
        assert_eq!(speed(320.0), "320 ku/s (8.1 m/s)");
        assert_eq!(area(1024.0), "1024 ku\u{b2}");
        assert_eq!(volume(4096.0), "4096 ku\u{b3}");
    }

    #[test]
    fn a_room_can_be_measured_in_players() {
        assert_eq!(in_players(144.0), "2.0x player");
        assert_eq!(in_players(PLAYER_HEIGHT), "1.0x player");
    }

    #[test]
    fn whole_numbers_do_not_grow_decimal_places() {
        assert_eq!(length_short(16.0), "16 ku");
        assert_eq!(length_short(-0.25), "-0.25 ku");
    }
}
