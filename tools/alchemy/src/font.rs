// SPDX-License-Identifier: LGPL-3.0-or-later
//! A 5x7 bitmap font, for labelling generated textures.
//!
//! Small on purpose. A tool texture is only useful if you can tell which one
//! it is from across a room, and a colour alone does not do that -- `clip` and
//! `playerclip` want to be told apart at a glance, and six shades of rose is
//! not a system anyone can hold in their head. So the name is written on it,
//! the way Source's tool textures are.
//!
//! Pulling in a font rasteriser for eight glyphs' worth of work would be a
//! dependency, a font licence, and a build step, to draw the word `nodraw`.

/// Column-major bits, five columns of seven rows, low bit at the top.
const fn glyph(c: char) -> [u8; 5] {
    match c {
        'A' => [0x7E, 0x11, 0x11, 0x11, 0x7E],
        'B' => [0x7F, 0x49, 0x49, 0x49, 0x36],
        'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
        'D' => [0x7F, 0x41, 0x41, 0x41, 0x3E],
        'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
        'F' => [0x7F, 0x09, 0x09, 0x09, 0x01],
        'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A],
        'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
        'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
        'J' => [0x20, 0x40, 0x41, 0x3F, 0x01],
        'K' => [0x7F, 0x08, 0x14, 0x22, 0x41],
        'L' => [0x7F, 0x40, 0x40, 0x40, 0x40],
        'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
        'N' => [0x7F, 0x04, 0x08, 0x10, 0x7F],
        'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
        'P' => [0x7F, 0x09, 0x09, 0x09, 0x06],
        'Q' => [0x3E, 0x41, 0x51, 0x21, 0x5E],
        'R' => [0x7F, 0x09, 0x19, 0x29, 0x46],
        'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
        'U' => [0x3F, 0x40, 0x40, 0x40, 0x3F],
        'V' => [0x1F, 0x20, 0x40, 0x20, 0x1F],
        'W' => [0x7F, 0x20, 0x18, 0x20, 0x7F],
        'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
        'Y' => [0x03, 0x04, 0x78, 0x04, 0x03],
        'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
        '0' => [0x3E, 0x51, 0x49, 0x45, 0x3E],
        '1' => [0x00, 0x42, 0x7F, 0x40, 0x00],
        '2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        '3' => [0x21, 0x41, 0x45, 0x4B, 0x31],
        '4' => [0x18, 0x14, 0x12, 0x7F, 0x10],
        '5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        '6' => [0x3C, 0x4A, 0x49, 0x49, 0x30],
        '7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        '8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        '9' => [0x06, 0x49, 0x49, 0x29, 0x1E],
        '/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        '-' => [0x08, 0x08, 0x08, 0x08, 0x08],
        '.' => [0x00, 0x40, 0x00, 0x00, 0x00],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00], // space, and anything unknown
    }
}

pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;
/// One blank column between letters.
pub const ADVANCE: usize = GLYPH_W + 1;

/// How wide a string is, in font pixels before scaling.
pub fn width(text: &str) -> usize {
    if text.is_empty() { return 0 }
    text.chars().count() * ADVANCE - 1
}

/// Call `plot(x, y)` for every lit pixel of `text`, scaled up by `scale`.
///
/// A callback rather than a buffer so the caller decides what a lit pixel
/// means -- these are drawn into textures that are already patterned, and
/// blending is the caller's business.
pub fn draw(text: &str, origin: (i32, i32), scale: usize, mut plot: impl FnMut(i32, i32)) {
    let scale = scale.max(1) as i32;
    for (index, c) in text.to_ascii_uppercase().chars().enumerate() {
        let bits = glyph(c);
        let left = origin.0 + (index * ADVANCE) as i32 * scale;
        for (column, byte) in bits.iter().enumerate() {
            for row in 0..GLYPH_H {
                if byte & (1 << row) == 0 { continue }
                let x0 = left + column as i32 * scale;
                let y0 = origin.1 + row as i32 * scale;
                for dy in 0..scale {
                    for dx in 0..scale {
                        plot(x0 + dx, y0 + dy);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_is_as_wide_as_its_letters_plus_the_gaps() {
        assert_eq!(width(""), 0);
        assert_eq!(width("A"), GLYPH_W);
        assert_eq!(width("AB"), GLYPH_W * 2 + 1);
    }

    #[test]
    fn every_letter_and_digit_has_a_shape() {
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
            assert_ne!(glyph(c), [0; 5], "`{c}` is blank");
        }
    }

    #[test]
    fn a_space_draws_nothing_and_an_unknown_character_does_not_panic() {
        let mut lit = 0;
        draw("  \u{1F600}", (0, 0), 1, |_, _| lit += 1);
        assert_eq!(lit, 0);
    }

    #[test]
    fn text_is_drawn_where_it_is_asked_for() {
        let mut min = (i32::MAX, i32::MAX);
        let mut max = (i32::MIN, i32::MIN);
        draw("HI", (10, 20), 2, |x, y| {
            min = (min.0.min(x), min.1.min(y));
            max = (max.0.max(x), max.1.max(y));
        });
        assert!(min.0 >= 10 && min.1 >= 20, "drew above or left of the origin: {min:?}");
        assert!(max.0 < 10 + (width("HI") * 2) as i32, "wider than it said: {max:?}");
        assert!(max.1 < 20 + (GLYPH_H * 2) as i32, "taller than it said: {max:?}");
    }

    #[test]
    fn scaling_up_lights_more_pixels() {
        let count = |scale| {
            let mut n = 0;
            draw("A", (0, 0), scale, |_, _| n += 1);
            n
        };
        assert_eq!(count(2), count(1) * 4, "a 2x glyph is four times the pixels");
    }
}
