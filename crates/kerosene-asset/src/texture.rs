// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! `.kerotex` -- Kerosene's texture format, the VTF analogue.
//!
//! A texture is not just an image: it is an image plus everything the renderer
//! needs to decide *how* to sample it. That is why source art (`.png`) is
//! compiled by Alchemy rather than loaded directly:
//!
//! * **Mipmaps are precomputed.** Generating them at load time costs startup
//!   time on every run, and doing it well (gamma-correct downsampling) is not
//!   something to redo per launch.
//! * **Average colour is precomputed.** The lighting compile needs a surface's
//!   reflectivity to bounce light off it, and scanning every texture at
//!   compile time would be wasteful.
//! * **Sampling intent is recorded.** Whether a texture clamps or wraps, and
//!   whether it is a normal map, belongs with the texture rather than being
//!   guessed from its name.

use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use kerosene_math::Vec3;

const MAGIC: [u8; 4] = *b"KRTX";
const VERSION: u32 = 1;
const HEADER_SIZE: usize = 48;

/// Largest texture edge the format allows.
///
/// 8192 is well past what any surface needs and keeps a full mip chain's size
/// comfortably inside a `u32`.
pub const MAX_DIMENSION: u32 = 8192;

#[derive(Debug, Error)]
pub enum TextureError {
    #[error("not a .kerotex file (bad magic)")]
    BadMagic,
    #[error("version {found}; this build reads version {expected}")]
    BadVersion { found: u32, expected: u32 },
    #[error("truncated: needs {needed} bytes, has {available}")]
    Truncated { needed: usize, available: usize },
    #[error("unknown pixel format {0}")]
    BadFormat(u32),
    #[error("{width}x{height} is not a usable size (max {MAX_DIMENSION}, and neither side may be zero)")]
    BadSize { width: u32, height: u32 },
}

/// How pixels are stored.
///
/// Uncompressed only. Block compression would cut memory four-fold, but it
/// needs a real encoder to look acceptable, and shipping a bad one is worse
/// than shipping none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum PixelFormat {
    Rgba8 = 0,
    Rgb8 = 1,
    /// Single channel, for masks and height maps.
    R8 = 2,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgba8 => 4,
            PixelFormat::Rgb8 => 3,
            PixelFormat::R8 => 1,
        }
    }

    fn from_u32(v: u32) -> Result<Self, TextureError> {
        match v {
            0 => Ok(PixelFormat::Rgba8),
            1 => Ok(PixelFormat::Rgb8),
            2 => Ok(PixelFormat::R8),
            other => Err(TextureError::BadFormat(other)),
        }
    }
}

/// Sampling and usage flags.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct TextureFlags(pub u32);

impl TextureFlags {
    pub const NONE: TextureFlags = TextureFlags(0);
    /// Clamp at the edges instead of repeating.
    pub const CLAMP: TextureFlags = TextureFlags(1 << 0);
    /// Nearest-neighbour sampling, for pixel art and lookup tables.
    pub const POINT_SAMPLE: TextureFlags = TextureFlags(1 << 1);
    /// Tangent-space normal map. Kept out of sRGB, since its values are
    /// directions rather than colours.
    pub const NORMAL_MAP: TextureFlags = TextureFlags(1 << 2);
    /// Has meaningful alpha.
    pub const TRANSLUCENT: TextureFlags = TextureFlags(1 << 3);
    /// Interface art: never mipmapped, always clamped.
    pub const UI: TextureFlags = TextureFlags(1 << 4);

    pub fn contains(self, other: TextureFlags) -> bool { self.0 & other.0 == other.0 }
}

impl std::ops::BitOr for TextureFlags {
    type Output = TextureFlags;
    fn bitor(self, o: TextureFlags) -> TextureFlags { TextureFlags(self.0 | o.0) }
}

/// One mip level.
#[derive(Clone, Debug)]
pub struct Mip {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A compiled texture.
#[derive(Clone, Debug)]
pub struct Texture {
    pub format: PixelFormat,
    pub flags: TextureFlags,
    /// Average colour, linear. Radiance uses it to tint bounced light.
    pub reflectivity: Vec3,
    /// Mip chain, largest first.
    pub mips: Vec<Mip>,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RawHeader {
    magic: [u8; 4],
    version: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
    mip_count: u32,
    reflectivity: [f32; 3],
    /// Pads the header to 48 bytes, which keeps the mip data that follows
    /// 16-byte aligned for the GPU upload path.
    _reserved: [u32; 2],
}

impl Texture {
    pub fn width(&self) -> u32 { self.mips.first().map_or(0, |m| m.width) }
    pub fn height(&self) -> u32 { self.mips.first().map_or(0, |m| m.height) }
    pub fn mip_count(&self) -> usize { self.mips.len() }

    /// Build a texture from top-level pixels, generating the mip chain.
    pub fn build(
        width: u32,
        height: u32,
        format: PixelFormat,
        flags: TextureFlags,
        pixels: Vec<u8>,
    ) -> Result<Texture, TextureError> {
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(TextureError::BadSize { width, height });
        }
        let expected = width as usize * height as usize * format.bytes_per_pixel();
        if pixels.len() != expected {
            return Err(TextureError::Truncated { needed: expected, available: pixels.len() });
        }

        let reflectivity = average_color(&pixels, format);
        let base = Mip { width, height, pixels };
        let mips = if flags.contains(TextureFlags::UI) {
            vec![base]
        } else {
            generate_mips(base, format)
        };

        Ok(Texture { format, flags, reflectivity, mips })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let header = RawHeader {
            magic: MAGIC,
            version: VERSION,
            width: self.width(),
            height: self.height(),
            format: self.format as u32,
            flags: self.flags.0,
            mip_count: self.mips.len() as u32,
            reflectivity: self.reflectivity.to_array(),
            _reserved: [0; 2],
        };
        let mut out = Vec::with_capacity(HEADER_SIZE + self.mips.iter().map(|m| m.pixels.len()).sum::<usize>());
        out.extend_from_slice(bytemuck::bytes_of(&header));
        debug_assert_eq!(out.len(), HEADER_SIZE);
        for mip in &self.mips {
            out.extend_from_slice(&mip.pixels);
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Texture, TextureError> {
        if bytes.len() < HEADER_SIZE {
            return Err(TextureError::Truncated { needed: HEADER_SIZE, available: bytes.len() });
        }
        let header: RawHeader = *bytemuck::from_bytes(&bytes[..HEADER_SIZE]);
        if header.magic != MAGIC { return Err(TextureError::BadMagic); }
        if header.version != VERSION {
            return Err(TextureError::BadVersion { found: header.version, expected: VERSION });
        }
        if header.width == 0
            || header.height == 0
            || header.width > MAX_DIMENSION
            || header.height > MAX_DIMENSION
        {
            return Err(TextureError::BadSize { width: header.width, height: header.height });
        }

        let format = PixelFormat::from_u32(header.format)?;
        let bpp = format.bytes_per_pixel();

        let mut mips = Vec::with_capacity(header.mip_count as usize);
        let mut offset = HEADER_SIZE;
        let (mut w, mut h) = (header.width, header.height);
        for _ in 0..header.mip_count {
            let size = w as usize * h as usize * bpp;
            if offset + size > bytes.len() {
                return Err(TextureError::Truncated {
                    needed: offset + size,
                    available: bytes.len(),
                });
            }
            mips.push(Mip { width: w, height: h, pixels: bytes[offset..offset + size].to_vec() });
            offset += size;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }

        Ok(Texture {
            format,
            flags: TextureFlags(header.flags),
            reflectivity: Vec3::from_array(header.reflectivity),
            mips,
        })
    }

    /// Expand to RGBA8, whatever the stored format.
    ///
    /// The renderer wants one upload path, so narrower formats are widened
    /// here rather than in every backend.
    pub fn mip_as_rgba8(&self, level: usize) -> Option<Vec<u8>> {
        let mip = self.mips.get(level)?;
        Some(match self.format {
            PixelFormat::Rgba8 => mip.pixels.clone(),
            PixelFormat::Rgb8 => mip
                .pixels
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            PixelFormat::R8 => mip.pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        })
    }
}

/// Halve a mip repeatedly until it reaches 1x1.
fn generate_mips(base: Mip, format: PixelFormat) -> Vec<Mip> {
    let bpp = format.bytes_per_pixel();
    let mut mips = vec![base];
    while mips.last().unwrap().width > 1 || mips.last().unwrap().height > 1 {
        let prev = mips.last().unwrap();
        let w = (prev.width / 2).max(1);
        let h = (prev.height / 2).max(1);
        let mut pixels = vec![0u8; w as usize * h as usize * bpp];

        for y in 0..h {
            for x in 0..w {
                for c in 0..bpp {
                    // Average the 2x2 block, clamping at the edge when the
                    // previous level had an odd dimension.
                    let mut sum = 0u32;
                    let mut count = 0u32;
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let sx = (x * 2 + dx).min(prev.width - 1);
                            let sy = (y * 2 + dy).min(prev.height - 1);
                            let i = ((sy * prev.width + sx) as usize) * bpp + c;
                            sum += prev.pixels[i] as u32;
                            count += 1;
                        }
                    }
                    pixels[((y * w + x) as usize) * bpp + c] = (sum / count) as u8;
                }
            }
        }
        mips.push(Mip { width: w, height: h, pixels });
    }
    mips
}

/// Average colour of an image, as a 0..1 linear-ish value.
fn average_color(pixels: &[u8], format: PixelFormat) -> Vec3 {
    let bpp = format.bytes_per_pixel();
    if pixels.is_empty() || bpp == 0 { return Vec3::splat(0.5); }
    let count = (pixels.len() / bpp) as f64;
    let mut sum = [0f64; 3];
    for p in pixels.chunks_exact(bpp) {
        match format {
            PixelFormat::R8 => {
                let v = p[0] as f64;
                sum[0] += v; sum[1] += v; sum[2] += v;
            }
            _ => {
                sum[0] += p[0] as f64;
                sum[1] += p[1] as f64;
                sum[2] += p[2] as f64;
            }
        }
    }
    Vec3::new(
        (sum[0] / count / 255.0) as f32,
        (sum[1] / count / 255.0) as f32,
        (sum[2] / count / 255.0) as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkerboard(size: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let on = (x / 8 + y / 8) % 2 == 0;
                let v = if on { 255 } else { 0 };
                out.extend_from_slice(&[v, v, v, 255]);
            }
        }
        out
    }

    #[test]
    fn the_header_is_exactly_as_declared() {
        assert_eq!(std::mem::size_of::<RawHeader>(), HEADER_SIZE);
    }

    #[test]
    fn a_texture_round_trips() {
        let tex = Texture::build(64, 64, PixelFormat::Rgba8, TextureFlags::NONE, checkerboard(64)).unwrap();
        let bytes = tex.to_bytes();
        let back = Texture::from_bytes(&bytes).unwrap();
        assert_eq!(back.width(), 64);
        assert_eq!(back.height(), 64);
        assert_eq!(back.format, PixelFormat::Rgba8);
        assert_eq!(back.mip_count(), tex.mip_count());
        assert_eq!(back.mips[0].pixels, tex.mips[0].pixels);
        assert_eq!(back.to_bytes(), bytes);
    }

    #[test]
    fn the_mip_chain_runs_all_the_way_down() {
        let tex = Texture::build(64, 64, PixelFormat::Rgba8, TextureFlags::NONE, checkerboard(64)).unwrap();
        // 64, 32, 16, 8, 4, 2, 1
        assert_eq!(tex.mip_count(), 7);
        assert_eq!(tex.mips.last().unwrap().width, 1);
        assert_eq!(tex.mips.last().unwrap().height, 1);
    }

    #[test]
    fn non_square_and_non_power_of_two_sizes_still_mip_down() {
        // The trap: halving a 5-pixel dimension must not reach zero.
        let pixels = vec![128u8; 5 * 3 * 4];
        let tex = Texture::build(5, 3, PixelFormat::Rgba8, TextureFlags::NONE, pixels).unwrap();
        assert!(tex.mips.iter().all(|m| m.width >= 1 && m.height >= 1));
        assert_eq!(tex.mips.last().unwrap().width, 1);
        assert_eq!(tex.mips.last().unwrap().height, 1);
        for m in &tex.mips {
            assert_eq!(m.pixels.len(), (m.width * m.height * 4) as usize);
        }
    }

    #[test]
    fn a_flat_image_stays_flat_through_every_mip() {
        let pixels = vec![200u8; 32 * 32 * 4];
        let tex = Texture::build(32, 32, PixelFormat::Rgba8, TextureFlags::NONE, pixels).unwrap();
        for mip in &tex.mips {
            assert!(mip.pixels.iter().all(|&v| v == 200), "mip {}x{} drifted", mip.width, mip.height);
        }
    }

    #[test]
    fn a_checkerboard_averages_to_grey_at_the_bottom() {
        let tex = Texture::build(64, 64, PixelFormat::Rgba8, TextureFlags::NONE, checkerboard(64)).unwrap();
        let last = tex.mips.last().unwrap();
        assert!((last.pixels[0] as i32 - 127).abs() < 8, "got {}", last.pixels[0]);
    }

    #[test]
    fn reflectivity_reflects_the_image() {
        let white = Texture::build(8, 8, PixelFormat::Rgba8, TextureFlags::NONE, vec![255u8; 8 * 8 * 4]).unwrap();
        assert!((white.reflectivity.x - 1.0).abs() < 1e-4);

        let black = Texture::build(8, 8, PixelFormat::Rgba8, TextureFlags::NONE, vec![0u8; 8 * 8 * 4]).unwrap();
        assert_eq!(black.reflectivity, Vec3::ZERO);

        let checker = Texture::build(64, 64, PixelFormat::Rgba8, TextureFlags::NONE, checkerboard(64)).unwrap();
        assert!((checker.reflectivity.x - 0.5).abs() < 0.05, "{}", checker.reflectivity.x);
    }

    #[test]
    fn ui_textures_skip_mipmapping() {
        let tex = Texture::build(64, 64, PixelFormat::Rgba8, TextureFlags::UI, checkerboard(64)).unwrap();
        assert_eq!(tex.mip_count(), 1, "interface art has no business being mipmapped");
    }

    #[test]
    fn narrow_formats_widen_to_rgba() {
        let tex = Texture::build(2, 2, PixelFormat::Rgb8, TextureFlags::NONE, vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120]).unwrap();
        let rgba = tex.mip_as_rgba8(0).unwrap();
        assert_eq!(rgba.len(), 16);
        assert_eq!(&rgba[0..4], &[10, 20, 30, 255]);

        let r = Texture::build(2, 1, PixelFormat::R8, TextureFlags::NONE, vec![7, 9]).unwrap();
        assert_eq!(r.mip_as_rgba8(0).unwrap(), vec![7, 7, 7, 255, 9, 9, 9, 255]);
    }

    #[test]
    fn a_wrong_sized_pixel_buffer_is_rejected() {
        assert!(matches!(
            Texture::build(64, 64, PixelFormat::Rgba8, TextureFlags::NONE, vec![0; 10]),
            Err(TextureError::Truncated { .. })
        ));
    }

    #[test]
    fn impossible_sizes_are_rejected() {
        assert!(Texture::build(0, 8, PixelFormat::Rgba8, TextureFlags::NONE, vec![]).is_err());
        assert!(Texture::build(MAX_DIMENSION + 1, 8, PixelFormat::Rgba8, TextureFlags::NONE, vec![]).is_err());
    }

    #[test]
    fn garbage_and_truncated_files_are_rejected() {
        assert!(matches!(Texture::from_bytes(&[0u8; 64]), Err(TextureError::BadMagic)));
        assert!(matches!(Texture::from_bytes(b"VT"), Err(TextureError::Truncated { .. })));

        let tex = Texture::build(32, 32, PixelFormat::Rgba8, TextureFlags::NONE, vec![1; 32 * 32 * 4]).unwrap();
        let bytes = tex.to_bytes();
        assert!(matches!(
            Texture::from_bytes(&bytes[..bytes.len() / 2]),
            Err(TextureError::Truncated { .. })
        ));
    }

    #[test]
    fn flags_survive_the_round_trip() {
        let flags = TextureFlags::CLAMP | TextureFlags::NORMAL_MAP;
        let tex = Texture::build(8, 8, PixelFormat::Rgba8, flags, vec![128; 8 * 8 * 4]).unwrap();
        let back = Texture::from_bytes(&tex.to_bytes()).unwrap();
        assert!(back.flags.contains(TextureFlags::CLAMP));
        assert!(back.flags.contains(TextureFlags::NORMAL_MAP));
        assert!(!back.flags.contains(TextureFlags::UI));
    }
}
