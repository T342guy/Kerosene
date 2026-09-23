// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Cubemap probes, ready to upload.
//!
//! Radiance bakes each probe face at one size. A rough surface should reflect
//! a blurred picture -- the rougher, the blurrier -- so the renderer needs the
//! whole mip chain, and the shader picks a level from the surface's
//! roughness. The chain is built here, on load, rather than stored in the
//! map: it is a few thousand texels a probe and takes no time to make.
//!
//! Each level is a 2x2 box filter of the one above. That is not the GGX
//! convolution a film renderer would do for each roughness, and it is the
//! same shortcut Source 1 took: the difference is visible on a mirror-smooth
//! metal ball and nowhere else a level designer will put one.
//!
//! Probes go to the GPU as one 2D texture array, six layers a probe, rather
//! than as a cube-map array. The shader picks the face itself (see
//! [`kerosene_bsp::cubemaps::direction_to_face`]), which works on every
//! backend wgpu has -- cube arrays are an optional feature the GL backend
//! lacks -- at the cost of a seam where two faces meet on the blurriest
//! levels, where there is no detail to see a seam in.

use kerosene_bsp::cubemaps::FACES;
use kerosene_bsp::{Cubemaps, decode_rgb9e5, encode_rgb9e5};
use kerosene_math::Vec3;

/// A probe set laid out for upload: one entry per mip level, each holding
/// every layer's texels, layer after layer.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeChain {
    /// Edge length of level 0.
    pub face_size: u32,
    /// Texture array layers: six per probe.
    pub layers: u32,
    /// RGB9E5 texels, `levels[l]` holding `layers * size_l * size_l`.
    pub levels: Vec<Vec<u32>>,
}

impl ProbeChain {
    /// The chain for a map's probes. A map with none gets one black probe,
    /// so there is always a texture to bind and no probe index ever points
    /// at it.
    pub fn build(cubemaps: Option<&Cubemaps>) -> ProbeChain {
        let Some(cubemaps) = cubemaps.filter(|c| !c.probes.is_empty()) else {
            return ProbeChain {
                face_size: 1,
                layers: FACES as u32,
                levels: vec![vec![0; FACES]],
            };
        };

        let size = cubemaps.face_size;
        let layers = (cubemaps.probes.len() * FACES) as u32;
        let mut level: Vec<u32> = Vec::with_capacity(layers as usize * (size * size) as usize);
        for probe in &cubemaps.probes {
            level.extend_from_slice(&probe.texels);
        }

        let mut levels = vec![level];
        let mut size_l = size;
        while size_l > 1 {
            let next = downsample(levels.last().unwrap(), layers, size_l);
            size_l = (size_l / 2).max(1);
            levels.push(next);
        }
        ProbeChain {
            face_size: size,
            layers,
            levels,
        }
    }

    /// Edge length of `level`.
    pub fn level_size(&self, level: usize) -> u32 {
        (self.face_size >> level).max(1)
    }
}

/// One level smaller: each texel the average of the 2x2 above it. Averaged
/// in linear light, as light adds.
fn downsample(texels: &[u32], layers: u32, size: u32) -> Vec<u32> {
    let half = (size / 2).max(1);
    let mut out = Vec::with_capacity((layers * half * half) as usize);
    let at = |layer: u32, x: u32, y: u32| {
        let (x, y) = (x.min(size - 1), y.min(size - 1));
        decode_rgb9e5(texels[(layer * size * size + y * size + x) as usize])
    };
    for layer in 0..layers {
        for y in 0..half {
            for x in 0..half {
                let sum: Vec3 = at(layer, 2 * x, 2 * y)
                    + at(layer, 2 * x + 1, 2 * y)
                    + at(layer, 2 * x, 2 * y + 1)
                    + at(layer, 2 * x + 1, 2 * y + 1);
                out.push(encode_rgb9e5(sum * 0.25));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_bsp::Probe;

    fn uniform(size: u32, probes: usize, value: Vec3) -> Cubemaps {
        Cubemaps {
            face_size: size,
            probes: (0..probes)
                .map(|i| Probe {
                    origin: Vec3::splat(i as f32),
                    texels: vec![encode_rgb9e5(value); FACES * (size * size) as usize],
                })
                .collect(),
        }
    }

    #[test]
    fn no_probes_is_one_black_placeholder() {
        let chain = ProbeChain::build(None);
        assert_eq!(chain.layers, 6);
        assert_eq!(chain.levels, vec![vec![0; 6]]);
    }

    #[test]
    fn the_chain_runs_down_to_one_texel_a_face() {
        let chain = ProbeChain::build(Some(&uniform(32, 2, Vec3::ONE)));
        assert_eq!(chain.layers, 12);
        assert_eq!(chain.levels.len(), 6, "32 16 8 4 2 1");
        for (l, level) in chain.levels.iter().enumerate() {
            let s = chain.level_size(l);
            assert_eq!(level.len(), (12 * s * s) as usize, "level {l}");
        }
    }

    #[test]
    fn averaging_keeps_the_brightness() {
        // A blurred reflection must be as bright on average as a sharp one,
        // or rough surfaces would read darker than smooth ones.
        let value = Vec3::new(0.5, 2.0, 7.0);
        let chain = ProbeChain::build(Some(&uniform(8, 1, value)));
        let last = decode_rgb9e5(chain.levels.last().unwrap()[0]);
        assert!((last - value).abs().max_element() < 0.05, "{last}");
    }

    #[test]
    fn a_bright_spot_spreads_into_the_blurrier_levels() {
        let mut c = uniform(4, 1, Vec3::ZERO);
        c.probes[0].texels[0] = encode_rgb9e5(Vec3::splat(16.0));
        let chain = ProbeChain::build(Some(&c));
        let level1 = decode_rgb9e5(chain.levels[1][0]);
        assert!((level1.x - 4.0).abs() < 0.05, "a quarter of it: {level1}");
        // And it stays on its own face: the second layer is untouched.
        assert_eq!(decode_rgb9e5(chain.levels[1][4]), Vec3::ZERO);
    }
}
