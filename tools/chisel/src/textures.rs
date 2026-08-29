// SPDX-License-Identifier: LGPL-3.0-or-later
//! Textures, for the 3D pane and the material browser.
//!
//! Chisel reads the *compiled* `.voidtex`, not the source PNG, and does it
//! through the same VFS the engine uses. Two reasons. A preview that decoded
//! the artist's file could show something the engine will never draw -- a
//! different size after mip generation, a different colour space -- and an
//! editor whose preview lies is worse than one with no preview. And going
//! through the VFS means a texture inside a `.vault` archive previews exactly
//! like a loose one, which is how content ships.
//!
//! The consequence is worth stating plainly: **the content has to be built.**
//! Nothing is drawn from a PNG that Alchemy has not compiled. A material with
//! no texture behind it falls back to a flat colour derived from its name, so
//! a missing texture is a wrong colour rather than a black hole.

use std::collections::HashMap;
use std::sync::Arc;
use void_vfs::Vfs;

/// One mip level: RGBA8, tightly packed.
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
}

impl Level {
    /// Sample at integer texel coordinates, wrapping.
    ///
    /// Wrapping rather than clamping because that is what a brush face does:
    /// a wall wider than its texture repeats it, and the whole point of a
    /// measurement texture is to count the repeats.
    #[inline]
    pub fn texel(&self, x: i32, y: i32) -> [u8; 4] {
        let x = x.rem_euclid(self.width.max(1) as i32) as u32;
        let y = y.rem_euclid(self.height.max(1) as i32) as u32;
        self.pixels[(y * self.width + x) as usize]
    }
}

/// A loaded texture and its mip chain.
pub struct Texture {
    pub mips: Vec<Level>,
    /// The mean colour, for the 2D views and for anywhere too small to draw
    /// the texture itself.
    pub average: [u8; 3],
}

impl Texture {
    pub fn width(&self) -> u32 { self.mips.first().map_or(1, |m| m.width) }
    pub fn height(&self) -> u32 { self.mips.first().map_or(1, |m| m.height) }

    /// The level to read, clamped to what exists.
    pub fn level(&self, mip: usize) -> &Level {
        &self.mips[mip.min(self.mips.len().saturating_sub(1))]
    }

    /// The smallest mip that is still at least `size` pixels across.
    ///
    /// For drawing a texture at a known size -- a thumbnail, a swatch -- where
    /// the right level is the one just big enough. Picking by position in the
    /// chain instead is how the material picker came to draw every swatch
    /// from a 2x2 image: the second-to-last mip of a 256-pixel texture is
    /// two pixels wide, so every checkerboard in the browser was a flat
    /// smudge and no two materials could be told apart.
    pub fn level_for_size(&self, size: u32) -> &Level {
        let index = self
            .mips
            .iter()
            .position(|m| m.width < size.max(1))
            .map(|first_too_small| first_too_small.saturating_sub(1))
            .unwrap_or(self.mips.len().saturating_sub(1));
        self.level(index)
    }

    /// Sample in normalised coordinates, at a chosen mip.
    pub fn sample(&self, u: f32, v: f32, mip: usize) -> [u8; 4] {
        let level = self.level(mip);
        let x = (u * level.width as f32).floor() as i32;
        let y = (v * level.height as f32).floor() as i32;
        level.texel(x, y)
    }
}

/// Textures, loaded once and kept.
///
/// A failed load is remembered as a failure. Retrying every frame would mean
/// a mistyped material name costs a file-system miss sixty times a second,
/// and would fill the log with the same line.
pub struct TextureCache {
    entries: HashMap<String, Option<Arc<Texture>>>,
    /// Names that failed, and why, for the material browser to show.
    problems: HashMap<String, String>,
}

impl Default for TextureCache {
    fn default() -> Self { TextureCache::new() }
}

impl TextureCache {
    pub fn new() -> TextureCache {
        TextureCache { entries: HashMap::new(), problems: HashMap::new() }
    }

    pub fn len(&self) -> usize { self.entries.values().filter(|e| e.is_some()).count() }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn problem(&self, material: &str) -> Option<&str> {
        self.problems.get(&key(material)).map(String::as_str)
    }
    pub fn problem_count(&self) -> usize { self.problems.len() }

    /// Forget everything, so a rebuild of the content shows up.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.problems.clear();
    }

    /// The texture for a material, loading it the first time.
    pub fn get(&mut self, vfs: &Vfs, material: &str) -> Option<Arc<Texture>> {
        let key = key(material);
        if let Some(entry) = self.entries.get(&key) { return entry.clone() }

        let loaded = match load(vfs, material) {
            Ok(texture) => Some(Arc::new(texture)),
            Err(e) => {
                self.problems.insert(key.clone(), e);
                None
            }
        };
        self.entries.insert(key, loaded.clone());
        loaded
    }

    /// A colour to stand in for a material with no texture behind it.
    ///
    /// Derived from the name, so two materials are two colours and the same
    /// material is the same colour every time -- which is enough to tell one
    /// wall from another while the content is still building.
    pub fn fallback_colour(material: &str) -> [u8; 3] {
        let mut hash: u32 = 2_166_136_261;
        for byte in material.as_bytes() {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(16_777_619);
        }
        // Kept mid-tone: a fallback that is nearly black or nearly white reads
        // as a lighting bug rather than as a missing texture.
        [
            96 + (hash & 0x3F) as u8,
            96 + ((hash >> 8) & 0x3F) as u8,
            96 + ((hash >> 16) & 0x3F) as u8,
        ]
    }

    /// The average colour of a material, loading it if need be.
    pub fn average(&mut self, vfs: &Vfs, material: &str) -> [u8; 3] {
        match self.get(vfs, material) {
            Some(texture) => texture.average,
            None => TextureCache::fallback_colour(material),
        }
    }
}

fn key(material: &str) -> String {
    material.trim_start_matches('/').to_ascii_lowercase()
}

/// Read a material and the texture it names.
fn load(vfs: &Vfs, material: &str) -> Result<Texture, String> {
    let material_path = void_asset::material_path(material);
    let text = vfs
        .read_string(&material_path)
        .map_err(|e| format!("{material_path}: {e}"))?;
    let parsed = void_asset::Material::parse(&text).map_err(|e| format!("{material_path}: {e}"))?;

    // A material with no `$basetexture` is legitimate -- a sky shader, a
    // colour-only surface -- so it is not an error, just nothing to draw.
    let base = parsed
        .base_texture()
        .ok_or_else(|| format!("{material_path} names no $basetexture"))?;

    let texture_path = void_asset::texture_path(base);
    let bytes = vfs
        .read(&texture_path)
        .map_err(|e| format!("{texture_path}: {e}. Has the content been built?"))?;
    let texture =
        void_asset::Texture::from_bytes(&bytes).map_err(|e| format!("{texture_path}: {e}"))?;

    let mut mips = Vec::with_capacity(texture.mip_count());
    for level in 0..texture.mip_count() {
        let Some(rgba) = texture.mip_as_rgba8(level) else { continue };
        let mip = &texture.mips[level];
        let pixels: Vec<[u8; 4]> = rgba
            .chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect();
        if pixels.len() != (mip.width * mip.height) as usize { continue }
        mips.push(Level { width: mip.width, height: mip.height, pixels });
    }
    if mips.is_empty() {
        return Err(format!("{texture_path} has no readable mip levels"));
    }

    // The smallest mip is the cheapest honest average, and it is already
    // filtered rather than point-sampled.
    let smallest = mips.last().expect("checked");
    let mut total = [0u64; 3];
    for pixel in &smallest.pixels {
        for c in 0..3 { total[c] += pixel[c] as u64; }
    }
    let count = smallest.pixels.len().max(1) as u64;
    let average = [
        (total[0] / count) as u8,
        (total[1] / count) as u8,
        (total[2] / count) as u8,
    ];

    Ok(Texture { mips, average })
}

#[cfg(test)]
mod tests;
