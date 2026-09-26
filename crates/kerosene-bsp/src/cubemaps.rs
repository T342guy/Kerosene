// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Cubemap probes: what the world looks like from a handful of points.
//!
//! A lightmap says how much light reaches a surface; it cannot say what a
//! shiny surface would *reflect*, because that depends on everything around
//! it. A probe records that surrounding once, at compile time: Radiance
//! stands at each `env_cubemap` entity, looks in every direction, and writes
//! down the lit colour of whatever it sees. The renderer then reflects the
//! nearest probe in every smooth or metal surface. It is Source's
//! `env_cubemap` and `buildcubemaps`, done by the compiler rather than by a
//! console command in a running game.
//!
//! The lump is [`lumps::CUBEMAPS`]. A map without probes -- or from before
//! they existed -- has it empty, and reads as `None`.
//!
//! [`lumps::CUBEMAPS`]: crate::lumps::CUBEMAPS

use kerosene_math::Vec3;

/// Lump magic, so a spare lump reused for something else fails loudly.
pub const MAGIC: [u8; 4] = *b"KCUB";
pub const VERSION: u32 = 1;

/// Faces per probe, in [`face_basis`] order.
pub const FACES: usize = 6;

/// Header: magic, version, face size, probe count.
const HEADER: usize = 16;

/// The probes of one map. Every probe has the same face size.
#[derive(Clone, Debug, PartialEq)]
pub struct Cubemaps {
    /// Texels along one edge of one face.
    pub face_size: u32,
    pub probes: Vec<Probe>,
}

/// One probe.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub origin: Vec3,
    /// `FACES * face_size * face_size` RGB9E5 texels, face by face, each face
    /// row by row. Linear light, on the same scale as the lightmap atlas:
    /// 1.0 is a white surface lit to full.
    pub texels: Vec<u32>,
}

impl Cubemaps {
    /// Texels in one probe.
    pub fn texels_per_probe(&self) -> usize {
        FACES * (self.face_size as usize).pow(2)
    }

    /// The probe nearest `point`, if there are any. Ties go to the lower
    /// index, so the answer is the same on every run.
    pub fn nearest(&self, point: Vec3) -> Option<usize> {
        self.probes
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.origin
                    .distance_squared(point)
                    .total_cmp(&b.origin.distance_squared(point))
            })
            .map(|(i, _)| i)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(HEADER + self.probes.len() * (12 + self.texels_per_probe() * 4));
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.face_size.to_le_bytes());
        out.extend_from_slice(&(self.probes.len() as u32).to_le_bytes());
        for probe in &self.probes {
            for c in probe.origin.to_array() {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        for probe in &self.probes {
            for t in &probe.texels {
                out.extend_from_slice(&t.to_le_bytes());
            }
        }
        out
    }

    /// Read the lump. Empty is `Ok(None)`: no probes, not an error.
    pub fn parse(bytes: &[u8]) -> Result<Option<Cubemaps>, String> {
        if bytes.is_empty() {
            return Ok(None);
        }
        if bytes.len() < HEADER || bytes[0..4] != MAGIC {
            return Err("cubemaps lump has no KCUB header".to_string());
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let version = word(4);
        if version != VERSION {
            return Err(format!(
                "cubemaps lump is version {version}; this build reads {VERSION}"
            ));
        }
        let face_size = word(8);
        let count = word(12) as usize;
        if face_size == 0 || face_size > 1024 {
            return Err(format!("cubemap face size {face_size} is out of range"));
        }
        let per_probe = FACES * (face_size as usize).pow(2);
        let needed = HEADER + count * 12 + count * per_probe * 4;
        if bytes.len() != needed {
            return Err(format!(
                "cubemaps lump is {} bytes; {count} probes of {face_size}x{face_size} need {needed}",
                bytes.len()
            ));
        }

        let mut probes = Vec::with_capacity(count);
        let texel_base = HEADER + count * 12;
        for i in 0..count {
            let o = HEADER + i * 12;
            let f = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            let start = texel_base + i * per_probe * 4;
            let texels = bytes[start..start + per_probe * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect();
            probes.push(Probe {
                origin: Vec3::new(f(o), f(o + 4), f(o + 8)),
                texels,
            });
        }
        Ok(Some(Cubemaps { face_size, probes }))
    }
}

/// The world axes of one face: the direction it looks, and the directions
/// its `s` (along a row) and `t` (down the rows) coordinates increase in.
///
/// Kerosene's own convention, not a GPU cube map's: the renderer stores
/// probes as a plain 2D array and picks the face itself, so the only thing
/// that has to agree with this table is `probe_uv` in the shaders -- which
/// carries a copy of it.
pub fn face_basis(face: usize) -> (Vec3, Vec3, Vec3) {
    match face {
        0 => (Vec3::X, Vec3::Y, Vec3::Z),
        1 => (-Vec3::X, -Vec3::Y, Vec3::Z),
        2 => (Vec3::Y, -Vec3::X, Vec3::Z),
        3 => (-Vec3::Y, Vec3::X, Vec3::Z),
        4 => (Vec3::Z, Vec3::X, Vec3::Y),
        _ => (-Vec3::Z, Vec3::X, -Vec3::Y),
    }
}

/// The direction through the centre of texel `(x, y)` of `face`.
pub fn texel_direction(face: usize, x: u32, y: u32, face_size: u32) -> Vec3 {
    let (major, s_axis, t_axis) = face_basis(face);
    let s = (x as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
    let t = (y as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
    (major + s_axis * s + t_axis * t).normalize()
}

/// Which face a direction lands on, and where on it, `s` and `t` in `0..1`.
pub fn direction_to_face(dir: Vec3) -> (usize, f32, f32) {
    let a = dir.abs();
    let face = if a.x >= a.y && a.x >= a.z {
        if dir.x >= 0.0 { 0 } else { 1 }
    } else if a.y >= a.z {
        if dir.y >= 0.0 { 2 } else { 3 }
    } else if dir.z >= 0.0 {
        4
    } else {
        5
    };
    let (major, s_axis, t_axis) = face_basis(face);
    let m = dir.dot(major).max(1e-12);
    (
        face,
        (dir.dot(s_axis) / m + 1.0) * 0.5,
        (dir.dot(t_axis) / m + 1.0) * 0.5,
    )
}

// ---- RGB9E5 -----------------------------------------------------------------
//
// The texel format of probes and of the renderer's lightmap atlas: three
// 9-bit mantissas sharing one 5-bit exponent, as `EXT_texture_shared_exponent`
// defines it. Linear HDR in four bytes, and filterable on every GPU.

const RGB9E5_MANTISSA_BITS: i32 = 9;
const RGB9E5_EXP_BIAS: i32 = 15;
const RGB9E5_MAX_EXP: i32 = 31;

/// The largest value RGB9E5 can hold, a little under 65536.
pub const RGB9E5_MAX: f32 = ((1 << RGB9E5_MANTISSA_BITS) - 1) as f32
    / (1 << RGB9E5_MANTISSA_BITS) as f32
    * (1u32 << (RGB9E5_MAX_EXP - RGB9E5_EXP_BIAS)) as f32;

/// Pack a linear colour into one RGB9E5 texel.
///
/// The three channels share the exponent of the brightest, so a dim channel
/// beside a bright one loses precision -- which is the right loss for light,
/// where the eye cannot see a small amount of blue under a lot of red anyway.
/// Negative and NaN inputs clamp to zero, which is what light below nothing
/// means.
pub fn encode_rgb9e5(color: Vec3) -> u32 {
    let clamp = |c: f32| {
        if c.is_nan() {
            0.0
        } else {
            c.clamp(0.0, RGB9E5_MAX)
        }
    };
    let (r, g, b) = (clamp(color.x), clamp(color.y), clamp(color.z));
    let max = r.max(g).max(b);
    if max == 0.0 {
        return 0;
    }

    let mut exp = (max.log2().floor() as i32).max(-RGB9E5_EXP_BIAS - 1) + 1 + RGB9E5_EXP_BIAS;
    let scale = |exp: i32| 2f32.powi(exp - RGB9E5_EXP_BIAS - RGB9E5_MANTISSA_BITS);
    // Rounding the largest channel up can carry it into a tenth bit, in
    // which case the exponent has to go up one to make room.
    if (max / scale(exp) + 0.5).floor() as u32 == 1 << RGB9E5_MANTISSA_BITS {
        exp += 1;
    }
    let s = scale(exp);
    let mantissa = |c: f32| ((c / s + 0.5).floor() as u32).min((1 << RGB9E5_MANTISSA_BITS) - 1);

    mantissa(r) | (mantissa(g) << 9) | (mantissa(b) << 18) | ((exp as u32) << 27)
}

/// Unpack one RGB9E5 texel, exactly as the GPU samples it.
pub fn decode_rgb9e5(texel: u32) -> Vec3 {
    let exp = (texel >> 27) as i32;
    let s = 2f32.powi(exp - RGB9E5_EXP_BIAS - RGB9E5_MANTISSA_BITS);
    let m = |shift: u32| ((texel >> shift) & 0x1ff) as f32 * s;
    Vec3::new(m(0), m(9), m(18))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Cubemaps {
        let face_size = 2;
        let per = FACES * 4;
        Cubemaps {
            face_size,
            probes: vec![
                Probe {
                    origin: Vec3::new(1.0, 2.0, 3.0),
                    texels: (0..per as u32).collect(),
                },
                Probe {
                    origin: Vec3::new(-64.0, 0.5, 128.0),
                    texels: (0..per as u32).map(|t| t * 7).collect(),
                },
            ],
        }
    }

    #[test]
    fn the_lump_round_trips() {
        let c = sample();
        assert_eq!(Cubemaps::parse(&c.encode()).unwrap(), Some(c));
    }

    #[test]
    fn an_empty_lump_is_no_probes_rather_than_an_error() {
        assert_eq!(Cubemaps::parse(&[]).unwrap(), None);
    }

    #[test]
    fn a_truncated_lump_is_an_error() {
        let bytes = sample().encode();
        assert!(Cubemaps::parse(&bytes[..bytes.len() - 4]).is_err());
        assert!(Cubemaps::parse(b"nope").is_err());
    }

    #[test]
    fn nearest_picks_the_closest_probe() {
        let c = sample();
        assert_eq!(c.nearest(Vec3::new(0.0, 0.0, 0.0)), Some(0));
        assert_eq!(c.nearest(Vec3::new(-60.0, 0.0, 120.0)), Some(1));
        let none = Cubemaps {
            face_size: 1,
            probes: vec![],
        };
        assert_eq!(none.nearest(Vec3::ZERO), None);
    }

    #[test]
    fn every_texel_direction_maps_back_to_its_own_texel() {
        // The shader's `probe_uv` inverts `texel_direction`; if this pair
        // disagree, reflections come from the wrong wall.
        let size = 8;
        for face in 0..FACES {
            for y in 0..size {
                for x in 0..size {
                    let d = texel_direction(face, x, y, size);
                    let (f, s, t) = direction_to_face(d);
                    assert_eq!(f, face, "texel ({x},{y}) of face {face}");
                    assert_eq!((s * size as f32) as u32, x);
                    assert_eq!((t * size as f32) as u32, y);
                }
            }
        }
    }

    #[test]
    fn each_face_looks_along_its_axis() {
        assert_eq!(direction_to_face(Vec3::X).0, 0);
        assert_eq!(direction_to_face(-Vec3::X).0, 1);
        assert_eq!(direction_to_face(Vec3::Y).0, 2);
        assert_eq!(direction_to_face(-Vec3::Y).0, 3);
        assert_eq!(direction_to_face(Vec3::Z).0, 4);
        assert_eq!(direction_to_face(-Vec3::Z).0, 5);
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        // Nine bits of mantissa: within half a step of the largest channel.
        let tolerance = a.max_element().max(b.max_element()) / 512.0 + 1e-9;
        (a - b).abs().max_element() <= tolerance
    }

    #[test]
    fn rgb9e5_round_trips_across_the_range_light_lives_in() {
        for c in [
            Vec3::new(0.001, 0.002, 0.003),
            Vec3::new(0.25, 0.5, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(3.7, 0.2, 12.5),
            Vec3::new(900.0, 450.0, 10.0),
        ] {
            let back = decode_rgb9e5(encode_rgb9e5(c));
            assert!(close(c, back), "{c} came back as {back}");
        }
    }

    #[test]
    fn rgb9e5_keeps_bright_light_bright_rather_than_compressing_it() {
        let two = decode_rgb9e5(encode_rgb9e5(Vec3::splat(2.0)));
        let twenty = decode_rgb9e5(encode_rgb9e5(Vec3::splat(20.0)));
        assert!((twenty.x / two.x - 10.0).abs() < 0.05);
    }

    #[test]
    fn rgb9e5_clamps_what_is_not_light() {
        assert_eq!(encode_rgb9e5(Vec3::ZERO), 0);
        assert_eq!(decode_rgb9e5(encode_rgb9e5(Vec3::splat(-4.0))), Vec3::ZERO);
        assert_eq!(
            decode_rgb9e5(encode_rgb9e5(Vec3::new(f32::NAN, 1.0, 0.0))).x,
            0.0
        );
        let huge = decode_rgb9e5(encode_rgb9e5(Vec3::splat(1e9)));
        assert!((huge.x - RGB9E5_MAX).abs() < 1.0, "{huge}");
    }

    #[test]
    fn rgb9e5_rounding_that_carries_bumps_the_exponent() {
        // Just under a power of two rounds up to it; the mantissa must not
        // wrap to zero.
        let back = decode_rgb9e5(encode_rgb9e5(Vec3::new(1.0 - 1e-4, 0.0, 0.0)));
        assert!((back.x - 1.0).abs() < 1e-3, "{back}");
    }
}
