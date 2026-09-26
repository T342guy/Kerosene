// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Fonts, text layout, and the glyph atlas.
//!
//! Text is the one thing in a HUD that cannot be a coloured rectangle, and the
//! one thing that goes wrong most visibly when it is scaled. So glyphs are
//! rasterised at the size they will be drawn -- a 24-pixel label at 4K is
//! rasterised at 48 -- rather than once and stretched, and packed into a
//! single 8-bit coverage atlas that every document and every world panel
//! shares. The atlas lives here on the CPU; the renderer only copies it up
//! when [`GlyphAtlas::dirty`] says something was added.
//!
//! Rasterising is [ab_glyph], the same crate egui draws with, so the engine
//! gains no new font code. The fallback face is egui's Ubuntu Light, already
//! in every build; a layout names others with `@font-face`.
//!
//! When the atlas fills it is cleared and starts again, and the generation
//! bumps so every document redraws against the new layout. That is a hitch,
//! and one that only a screen showing a few thousand distinct glyph sizes at
//! once can cause.

use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont, point};
use std::collections::HashMap;

/// Edge of the square glyph atlas, in texels.
pub const ATLAS_SIZE: u32 = 1024;

/// Texels of empty border around every glyph, so linear filtering never
/// samples a neighbour.
const PAD: u32 = 1;

/// Which loaded face.
pub type FaceId = usize;

struct Face {
    /// Lower-cased family name.
    family: String,
    bold: bool,
    font: FontArc,
}

/// Where a glyph sits in the atlas, and how to place it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AtlasGlyph {
    /// `u0, v0, u1, v1`, normalised.
    pub uv: [f32; 4],
    /// Offset of the bitmap's top-left from the pen position on the baseline,
    /// in physical pixels.
    pub offset: (f32, f32),
    /// Bitmap size in physical pixels.
    pub size: (f32, f32),
}

/// The shared coverage atlas: one byte per texel.
pub struct GlyphAtlas {
    pub pixels: Vec<u8>,
    /// Set when `pixels` changed; the renderer clears it after uploading.
    pub dirty: bool,
    /// Bumped whenever the atlas is wiped and every cached position is void.
    pub generation: u64,
    cursor: (u32, u32),
    row_height: u32,
    /// `None` for a glyph with no pixels (a space).
    slots: HashMap<(FaceId, u16, u32), Option<AtlasGlyph>>,
}

impl GlyphAtlas {
    fn new() -> GlyphAtlas {
        GlyphAtlas {
            pixels: vec![0; (ATLAS_SIZE * ATLAS_SIZE) as usize],
            dirty: true,
            generation: 0,
            cursor: (PAD, PAD),
            row_height: 0,
            slots: HashMap::new(),
        }
    }

    fn clear(&mut self) {
        self.pixels.fill(0);
        self.slots.clear();
        self.cursor = (PAD, PAD);
        self.row_height = 0;
        self.dirty = true;
        self.generation += 1;
    }

    /// Shelf packing: fill a row left to right, start a new row when it does
    /// not fit. Glyphs are similar heights, which is the case shelves are
    /// good at.
    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + 2 * PAD > ATLAS_SIZE || h + 2 * PAD > ATLAS_SIZE {
            return None;
        }
        if self.cursor.0 + w + PAD > ATLAS_SIZE {
            self.cursor = (PAD, self.cursor.1 + self.row_height + PAD);
            self.row_height = 0;
        }
        if self.cursor.1 + h + PAD > ATLAS_SIZE {
            return None;
        }
        let at = self.cursor;
        self.cursor.0 += w + PAD;
        self.row_height = self.row_height.max(h);
        Some(at)
    }
}

/// One positioned glyph of laid-out text, in UI pixels relative to the text
/// box's top-left, pen on the baseline.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlacedGlyph {
    pub id: u16,
    pub x: f32,
    pub baseline: f32,
}

/// Text broken into lines and positioned.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct TextLayout {
    pub face: FaceId,
    pub size: f32,
    pub glyphs: Vec<PlacedGlyph>,
    /// Width and height of the whole block, in UI pixels.
    pub width: f32,
    pub height: f32,
    /// Byte offset in the source text where each glyph starts, for placing a
    /// caret.
    pub offsets: Vec<usize>,
}

/// What text layout needs from a style.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TextParams<'a> {
    pub family: &'a str,
    pub bold: bool,
    pub size: f32,
    pub letter_spacing: f32,
    pub line_height: f32,
    pub wrap: bool,
    pub align: crate::style::TextAlign,
}

/// Every face the UI has, and the atlas they draw from.
pub struct Fonts {
    faces: Vec<Face>,
    pub atlas: GlyphAtlas,
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts::new()
    }
}

impl Fonts {
    pub fn new() -> Fonts {
        let fallback = FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)
            .expect("egui's bundled font parses");
        Fonts {
            faces: vec![Face {
                family: String::new(),
                bold: false,
                font: fallback,
            }],
            atlas: GlyphAtlas::new(),
        }
    }

    /// Add a face from TTF or OTF bytes. A second face for the same family
    /// and weight replaces the first, which is what a reload wants.
    pub fn add(&mut self, family: &str, bold: bool, bytes: Vec<u8>) -> Result<FaceId, String> {
        let font = FontArc::try_from_vec(bytes).map_err(|e| format!("{family}: {e}"))?;
        let family = family.to_lowercase();
        if let Some(i) = self
            .faces
            .iter()
            .position(|f| f.family == family && f.bold == bold)
        {
            self.faces[i].font = font;
            self.atlas.clear();
            return Ok(i);
        }
        self.faces.push(Face { family, bold, font });
        Ok(self.faces.len() - 1)
    }

    /// Whether a family has any face loaded.
    pub fn has_family(&self, family: &str) -> bool {
        let family = family.to_lowercase();
        self.faces.iter().any(|f| f.family == family)
    }

    /// Whether a face is a bold one.
    pub fn is_bold(&self, face: FaceId) -> bool {
        self.faces.get(face).is_some_and(|f| f.bold)
    }

    /// The best face for a family and weight: exact, then the family at any
    /// weight, then the fallback.
    pub fn face(&self, family: &str, bold: bool) -> FaceId {
        let family = family.to_lowercase();
        self.faces
            .iter()
            .position(|f| f.family == family && f.bold == bold)
            .or_else(|| self.faces.iter().position(|f| f.family == family))
            .unwrap_or(0)
    }

    /// Lay text out, wrapping at `max_width` UI pixels if given.
    pub fn layout(&self, text: &str, params: TextParams<'_>, max_width: Option<f32>) -> TextLayout {
        let face = self.face(params.family, params.bold);
        let font = self.faces[face]
            .font
            .as_scaled(PxScale::from(params.size.max(1.0)));
        let line_height = params.size * params.line_height;
        let ascent = font.ascent();
        // Where the first baseline sits: the ascent, plus half the extra
        // leading so a line-height above 1 spaces evenly above and below.
        let first_baseline = ascent + (line_height - (ascent - font.descent())) * 0.5;

        let wrap_at = if params.wrap { max_width } else { None };

        struct Line {
            start: usize,
            end: usize,
            width: f32,
        }
        let mut positioned: Vec<(u16, f32, usize)> = Vec::new();
        let mut lines: Vec<Line> = Vec::new();
        let mut line_start = 0;
        let mut x = 0.0f32;
        let mut prev: Option<GlyphId> = None;
        // Index in `positioned` and pen position just after the last space,
        // for breaking the line there.
        let mut last_break: Option<(usize, f32)> = None;

        for (offset, c) in text.char_indices() {
            if c == '\n' {
                lines.push(Line {
                    start: line_start,
                    end: positioned.len(),
                    width: x,
                });
                line_start = positioned.len();
                x = 0.0;
                prev = None;
                last_break = None;
                continue;
            }
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                x += font.kern(p, id);
            }
            let advance = font.h_advance(id) + params.letter_spacing;
            if let Some(limit) = wrap_at
                && x + advance > limit + 0.01
                && !c.is_whitespace()
                && positioned.len() > line_start
            {
                // Break at the last space if there was one on this line,
                // else mid-word: a word wider than its box has to go somewhere.
                let (break_at, break_x) = last_break.unwrap_or((positioned.len(), x));
                lines.push(Line {
                    start: line_start,
                    end: break_at,
                    width: break_x,
                });
                // Slide what came after the break to the start of the next
                // line, dropping the spaces the break consumed.
                let mut next = break_at;
                while next < positioned.len()
                    && text[positioned[next].2..].starts_with(char::is_whitespace)
                {
                    next += 1;
                }
                let shift = if next < positioned.len() {
                    positioned[next].1
                } else {
                    x
                };
                for g in &mut positioned[next..] {
                    g.1 -= shift;
                }
                positioned.drain(break_at..next);
                line_start = break_at;
                x -= shift;
                last_break = None;
            }
            positioned.push((id.0, x, offset));
            x += advance;
            if c.is_whitespace() {
                last_break = Some((positioned.len(), x - advance));
            }
            prev = Some(id);
        }
        lines.push(Line {
            start: line_start,
            end: positioned.len(),
            width: x,
        });

        // Trailing spaces do not count towards a line's width, so centred and
        // right-aligned text lines up on its ink.
        for line in &mut lines {
            let mut end = line.end;
            while end > line.start && text[positioned[end - 1].2..].starts_with(char::is_whitespace)
            {
                end -= 1;
            }
            if end < line.end {
                line.width = if end > line.start {
                    let (id, gx, _) = positioned[end - 1];
                    gx + font.h_advance(GlyphId(id))
                } else {
                    0.0
                };
            }
        }

        let block_width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
        let align_width = max_width
            .filter(|w| w.is_finite())
            .unwrap_or(block_width)
            .max(block_width);
        let mut out = TextLayout {
            face,
            size: params.size,
            width: block_width,
            height: line_height * lines.len() as f32,
            ..Default::default()
        };
        for (i, line) in lines.iter().enumerate() {
            let shift = match params.align {
                crate::style::TextAlign::Left => 0.0,
                crate::style::TextAlign::Center => (align_width - line.width) * 0.5,
                crate::style::TextAlign::Right => align_width - line.width,
            };
            let baseline = first_baseline + line_height * i as f32;
            for &(id, gx, offset) in &positioned[line.start..line.end] {
                out.glyphs.push(PlacedGlyph {
                    id,
                    x: gx + shift,
                    baseline,
                });
                out.offsets.push(offset);
            }
        }
        out
    }

    /// Width and height of text, for layout to size a label by.
    pub fn measure(
        &self,
        text: &str,
        params: TextParams<'_>,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let layout = self.layout(text, params, max_width);
        (layout.width, layout.height)
    }

    /// The atlas slot for a glyph at a physical pixel size, rasterising it on
    /// first use. `None` for glyphs with no ink, or when even an empty atlas
    /// cannot hold it.
    pub fn glyph(&mut self, face: FaceId, id: u16, px_size: f32) -> Option<AtlasGlyph> {
        let size_key = (px_size * 4.0).round().max(1.0) as u32;
        let key = (face, id, size_key);
        if let Some(slot) = self.atlas.slots.get(&key) {
            return *slot;
        }
        let px = size_key as f32 / 4.0;
        let font = &self.faces.get(face)?.font;
        let glyph = GlyphId(id).with_scale_and_position(PxScale::from(px), point(0.0, 0.0));
        let Some(outline) = font.outline_glyph(glyph) else {
            self.atlas.slots.insert(key, None);
            return None;
        };
        let bounds = outline.px_bounds();
        let w = bounds.width().ceil() as u32;
        let h = bounds.height().ceil() as u32;
        let at = match self.atlas.allocate(w, h) {
            Some(at) => at,
            None => {
                // Full: start again. Everything cached is void, which the
                // generation tells every document.
                self.atlas.clear();
                self.atlas.allocate(w, h)?
            }
        };
        let pixels = &mut self.atlas.pixels;
        outline.draw(|x, y, coverage| {
            let px = at.0 + x;
            let py = at.1 + y;
            if px < ATLAS_SIZE && py < ATLAS_SIZE {
                pixels[(py * ATLAS_SIZE + px) as usize] =
                    (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        self.atlas.dirty = true;
        let s = ATLAS_SIZE as f32;
        let slot = AtlasGlyph {
            uv: [
                at.0 as f32 / s,
                at.1 as f32 / s,
                (at.0 + w) as f32 / s,
                (at.1 + h) as f32 / s,
            ],
            offset: (bounds.min.x, bounds.min.y),
            size: (w as f32, h as f32),
        };
        self.atlas.slots.insert(key, Some(slot));
        Some(slot)
    }
}

#[cfg(test)]
mod tests;
