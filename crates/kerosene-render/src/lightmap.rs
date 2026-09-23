// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Packing per-face lightmaps into one atlas.
//!
//! Every lit face carries its own small grid of luxels. Uploading them as
//! separate textures would mean a texture bind per face and thousands of draw
//! calls; packing them into one atlas means the whole world can be drawn with
//! a handful.
//!
//! The packer is a shelf packer: sort by height, lay pages out in rows. It is
//! not optimal, but lightmap patches are small and similar in size, which is
//! exactly the case shelf packing handles well -- and being deterministic
//! matters more than the last few percent of occupancy, because a map that
//! packs differently between runs invalidates every cached atlas.

use kerosene_bsp::{Bsp, ColorRgbExp32, encode_rgb9e5};

/// Atlas edge length. 2048 holds a substantial map at 16-unit luxels and is
/// within every GPU's limits.
pub const ATLAS_SIZE: u32 = 2048;

/// Blank border kept around each patch.
///
/// Without it, bilinear filtering at a patch's edge samples its neighbour and
/// every surface picks up a thin bleed of whatever was packed next to it.
pub const PADDING: u32 = 1;

/// Where one face's lightmap ended up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl AtlasRect {
    /// Convert a luxel coordinate within the patch into an atlas UV.
    ///
    /// The half-texel inset makes a sample at luxel 0 land at the centre of
    /// the first texel rather than on the boundary between it and the padding.
    pub fn to_uv(&self, u: f32, v: f32) -> [f32; 2] {
        let size = ATLAS_SIZE as f32;
        [
            (self.x as f32 + u.clamp(0.0, self.width as f32 - 1.0) + 0.5) / size,
            (self.y as f32 + v.clamp(0.0, self.height as f32 - 1.0) + 0.5) / size,
        ]
    }
}

/// The atlas's texel format.
///
/// Shared-exponent float: three 9-bit mantissas and one 5-bit exponent in 32
/// bits. Light is HDR by nature -- a lamp against a wall bakes to many times
/// what a lit floor does -- and the atlas used to squeeze that into 8-bit
/// unorm through a Reinhard curve, which meant the frame's own tone-mapper
/// then compressed it a second time and nothing downstream could tell a
/// bright surface from a very bright one. This keeps it linear, in the same
/// four bytes a texel cost before, and every GPU wgpu runs on can filter it.
pub const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgb9e5Ufloat;

/// A packed lightmap atlas, ready to upload.
pub struct LightmapAtlas {
    /// Linear light, one little-endian [`ATLAS_FORMAT`] texel per four bytes,
    /// `ATLAS_SIZE` square. 1.0 is a surface lit to what Radiance calls full.
    pub pixels: Vec<u8>,
    /// Where each face landed, indexed by face. `None` for unlit faces.
    pub rects: Vec<Option<AtlasRect>>,
    pub used_pixels: u32,
    /// Faces that did not fit.
    pub overflowed: usize,
}

impl LightmapAtlas {
    pub fn occupancy(&self) -> f32 {
        self.used_pixels as f32 / (ATLAS_SIZE * ATLAS_SIZE) as f32
    }

    /// Pack every lit face of a map into one atlas.
    ///
    /// `exposure` scales the samples as they are stored. The engine passes
    /// 1.0 and leaves exposure to the tone-map pass, where it is live; the
    /// parameter is for a tool that wants a brighter or darker atlas baked in.
    /// Values over 1.0 are kept as they are, not compressed: that is what the
    /// atlas format is for.
    pub fn build(bsp: &Bsp, exposure: f32) -> LightmapAtlas {
        Self::build_for(bsp, exposure, |_| true)
    }

    /// Pack the lit faces `keep` accepts -- one streamed section's, so each
    /// section carries its own atlas and can be dropped with it.
    pub fn build_for(bsp: &Bsp, exposure: f32, keep: impl Fn(usize) -> bool) -> LightmapAtlas {
        let mut atlas = LightmapAtlas {
            pixels: vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize],
            rects: vec![None; bsp.faces.len()],
            used_pixels: 0,
            overflowed: 0,
        };

        // Tallest first, so shelves are filled by pieces of similar height and
        // little vertical space is wasted.
        let mut order: Vec<usize> = (0..bsp.faces.len())
            .filter(|&i| keep(i))
            .filter(|&i| bsp.faces[i].lightmap_offset >= 0)
            .filter(|&i| bsp.faces[i].lightmap_size[0] > 0 && bsp.faces[i].lightmap_size[1] > 0)
            .collect();
        order.sort_by_key(|&i| {
            // Face index breaks ties so the layout is identical between runs.
            (std::cmp::Reverse(bsp.faces[i].lightmap_size[1]), i)
        });

        let mut cursor_x = PADDING;
        let mut cursor_y = PADDING;
        let mut shelf_height = 0u32;

        for face_index in order {
            let face = &bsp.faces[face_index];
            let (w, h) = (face.lightmap_size[0], face.lightmap_size[1]);
            if w > ATLAS_SIZE - PADDING * 2 || h > ATLAS_SIZE - PADDING * 2 {
                atlas.overflowed += 1;
                continue;
            }

            if cursor_x + w + PADDING > ATLAS_SIZE {
                // Start a new shelf.
                cursor_x = PADDING;
                cursor_y += shelf_height + PADDING;
                shelf_height = 0;
            }
            if cursor_y + h + PADDING > ATLAS_SIZE {
                atlas.overflowed += 1;
                continue;
            }

            let rect = AtlasRect {
                x: cursor_x,
                y: cursor_y,
                width: w,
                height: h,
            };
            atlas.blit(bsp, face_index, rect, exposure);
            atlas.rects[face_index] = Some(rect);
            atlas.used_pixels += w * h;

            cursor_x += w + PADDING;
            shelf_height = shelf_height.max(h);
        }

        atlas
    }

    fn blit(&mut self, bsp: &Bsp, face_index: usize, rect: AtlasRect, exposure: f32) {
        let Some(samples) = bsp.face_lightmap(face_index) else {
            return;
        };
        for y in 0..rect.height {
            for x in 0..rect.width {
                let sample = samples
                    .get((y * rect.width + x) as usize)
                    .copied()
                    .unwrap_or(ColorRgbExp32::default());
                let texel = encode_rgb9e5(sample.to_linear() * (exposure / 255.0));
                let dst = (((rect.y + y) * ATLAS_SIZE + (rect.x + x)) * 4) as usize;
                self.pixels[dst..dst + 4].copy_from_slice(&texel.to_le_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_bsp::{Face, TexData, TexInfo};
    use kerosene_math::Vec3;

    /// A map with `count` faces, each claiming a `w` by `h` lightmap.
    fn map_with_faces(sizes: &[(u32, u32)]) -> Bsp {
        let mut bsp = Bsp::new();
        let name = bsp.intern_texdata_string("dev/grid");
        bsp.texdata.push(TexData {
            name_offset: name,
            ..Default::default()
        });
        bsp.texinfo.push(TexInfo::default());

        let mut offset = 0i32;
        for &(w, h) in sizes {
            bsp.faces.push(Face {
                lightmap_offset: offset,
                lightmap_size: [w, h],
                texinfo: 0,
                dispinfo: -1,
                ..Default::default()
            });
            let count = (w * h) as usize;
            bsp.lighting.extend(std::iter::repeat_n(
                ColorRgbExp32 {
                    r: 128,
                    g: 128,
                    b: 128,
                    exponent: 0,
                },
                count,
            ));
            offset += count as i32;
        }
        bsp
    }

    #[test]
    fn every_face_gets_a_rect() {
        let bsp = map_with_faces(&[(16, 16), (32, 8), (4, 4)]);
        let atlas = LightmapAtlas::build(&bsp, 1.0);
        assert_eq!(atlas.overflowed, 0);
        assert!(atlas.rects.iter().all(|r| r.is_some()));
        assert_eq!(atlas.used_pixels, 16 * 16 + 32 * 8 + 4 * 4);
    }

    #[test]
    fn rects_do_not_overlap() {
        let sizes: Vec<(u32, u32)> = (1..80).map(|i| ((i % 40) + 1, (i % 17) + 1)).collect();
        let bsp = map_with_faces(&sizes);
        let atlas = LightmapAtlas::build(&bsp, 1.0);
        let rects: Vec<AtlasRect> = atlas.rects.iter().flatten().copied().collect();
        assert_eq!(rects.len(), sizes.len());

        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let separate = a.x + a.width <= b.x
                    || b.x + b.width <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y;
                assert!(separate, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn patches_are_padded_apart() {
        // Without padding, bilinear filtering bleeds one patch into the next.
        let bsp = map_with_faces(&[(8, 8), (8, 8)]);
        let atlas = LightmapAtlas::build(&bsp, 1.0);
        let a = atlas.rects[0].unwrap();
        let b = atlas.rects[1].unwrap();
        let gap = b.x.saturating_sub(a.x + a.width);
        assert!(gap >= PADDING, "patches are touching: {a:?} then {b:?}");
    }

    #[test]
    fn unlit_faces_get_no_rect() {
        let mut bsp = map_with_faces(&[(16, 16), (8, 8)]);
        bsp.faces[1].lightmap_offset = -1;
        let atlas = LightmapAtlas::build(&bsp, 1.0);
        assert!(atlas.rects[0].is_some());
        assert!(atlas.rects[1].is_none());
    }

    #[test]
    fn packing_is_identical_between_runs() {
        // A layout that shuffles invalidates every cached atlas.
        let sizes: Vec<(u32, u32)> = (1..50).map(|i| ((i % 23) + 1, (i % 11) + 1)).collect();
        let bsp = map_with_faces(&sizes);
        let a = LightmapAtlas::build(&bsp, 1.0);
        let b = LightmapAtlas::build(&bsp, 1.0);
        assert_eq!(a.rects, b.rects);
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn an_oversized_patch_is_reported_rather_than_corrupting_the_atlas() {
        let bsp = map_with_faces(&[(ATLAS_SIZE + 10, 4)]);
        let atlas = LightmapAtlas::build(&bsp, 1.0);
        assert_eq!(atlas.overflowed, 1);
        assert!(atlas.rects[0].is_none());
    }

    #[test]
    fn uv_conversion_lands_inside_the_patch() {
        let rect = AtlasRect {
            x: 100,
            y: 200,
            width: 16,
            height: 8,
        };
        let [u, v] = rect.to_uv(0.0, 0.0);
        let size = ATLAS_SIZE as f32;
        // Half a texel in, not on the boundary with the padding.
        assert!((u * size - 100.5).abs() < 1e-3, "{}", u * size);
        assert!((v * size - 200.5).abs() < 1e-3);

        // Out-of-range luxels clamp rather than sampling a neighbour.
        let [u, _] = rect.to_uv(1000.0, 0.0);
        assert!(
            u * size < 116.0,
            "should clamp inside the patch, got {}",
            u * size
        );
    }

    #[test]
    fn exposure_scales_the_stored_light() {
        let mut bsp = map_with_faces(&[(1, 1)]);
        bsp.lighting = vec![ColorRgbExp32::from_linear(Vec3::splat(255.0))];
        let texel = |exposure: f32| {
            let atlas = LightmapAtlas::build(&bsp, exposure);
            let rect = atlas.rects[0].unwrap();
            let at = ((rect.y * ATLAS_SIZE + rect.x) * 4) as usize;
            kerosene_bsp::decode_rgb9e5(u32::from_le_bytes(
                atlas.pixels[at..at + 4].try_into().unwrap(),
            ))
        };
        let one = texel(1.0);
        assert!((one.x - 1.0).abs() < 0.02, "255 is full light: {one}");
        assert!((texel(2.0).x / one.x - 2.0).abs() < 0.02);
    }

    #[test]
    fn an_unlit_map_produces_an_empty_atlas() {
        let atlas = LightmapAtlas::build(&Bsp::new(), 1.0);
        assert_eq!(atlas.used_pixels, 0);
        assert_eq!(atlas.occupancy(), 0.0);
    }
}
