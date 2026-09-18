// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The window icon: the Kerosene mark, compiled into every binary that
//! opens a window.
//!
//! The picture lives with the rest of the project's artwork in
//! `.github/Images/` and is included from there, so there is one copy of it
//! and the README, the toolset and the game all show the same mark. It is
//! decoded once, at startup; a window without an icon is a grey square in a
//! taskbar, which is not a thing anyone should have to recognise.

/// The mark, as the repository keeps it. 256 pixels square: big enough for
/// a dock, and every platform scales down better than it scales up.
const PNG: &[u8] = include_bytes!("../../../.github/Images/kerosene-icon-256.png");

/// A decoded icon: RGBA pixels, row-major, and its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decode the mark. `None` only if the embedded picture is somehow not a
/// PNG the decoder understands, in which case the window simply has no
/// icon; a broken picture is not a reason to fail to open.
pub fn window_icon() -> Option<Icon> {
    match decode(PNG) {
        Ok(icon) => Some(icon),
        Err(e) => {
            log::warn!("the window icon could not be decoded: {e}");
            None
        }
    }
}

fn decode(bytes: &[u8]) -> Result<Icon, png::DecodingError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // Whatever the file holds -- palette, greyscale, 16-bit -- comes out as
    // 8-bit RGBA, which is the one form a window system takes.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer)?;
    buffer.truncate(info.buffer_size());

    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => {
            return Err(png::DecodingError::LimitsExceeded);
        }
    };
    Ok(Icon {
        rgba,
        width: info.width,
        height: info.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_decodes_to_a_square_of_rgba() {
        let icon = window_icon().expect("the embedded icon decodes");
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        // It is a picture, not a blank: some pixel is opaque.
        assert!(icon.rgba.chunks(4).any(|p| p[3] == 255));
    }
}
