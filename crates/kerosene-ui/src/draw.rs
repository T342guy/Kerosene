// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The display list: what the renderer is told to draw.
//!
//! Everything a UI draws is one primitive -- a quad with rounded corners, an
//! optional border, a colour or two-colour gradient, an optional texture and
//! an optional progress clip -- in physical pixels, back to front. Text is
//! quads sampling the glyph atlas; an image is a quad sampling an image. One
//! shape means one shader and one pipeline, and the renderer's job reduces to
//! batching runs of quads that share a texture and a clip.
//!
//! The list is plain data, so the whole of the UI -- cascade, layout, text,
//! bindings -- can be tested without a GPU by looking at what ends up here.

/// What a quad samples.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum TextureRef {
    /// Nothing: a solid shape.
    #[default]
    None,
    /// The shared glyph atlas; the texture is coverage, the colour is ink.
    Glyphs,
    /// An image, by the id [`crate::UiSystem::image_path`] resolves.
    Image(u32),
}

/// One rectangle to draw.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quad {
    /// `x, y, w, h`, physical pixels, before `transform`.
    pub rect: [f32; 4],
    /// `u0, v0, u1, v1`.
    pub uv: [f32; 4],
    pub texture: TextureRef,
    /// Straight-alpha sRGB, opacity already multiplied in.
    pub color: [f32; 4],
    /// The far end of a gradient; equal to `color` when there is none.
    pub color2: [f32; 4],
    /// 0 none, 1 top-to-bottom, 2 left-to-right.
    pub gradient: u32,
    /// Corner radius in physical pixels.
    pub radius: f32,
    pub border_width: f32,
    pub border_color: [f32; 4],
    /// `-kero-fill`: kind (0 none, 1 radial, 2 horizontal, 3 vertical) and
    /// amount.
    pub fill: (u32, f32),
    /// Soft edge width in physical pixels: a box shadow's blur. 0 is crisp.
    pub softness: f32,
    pub additive: bool,
    /// Row-major 2x3 affine applied to the corners: `[a, b, tx, c, d, ty]`,
    /// so `x' = a*x + b*y + tx`.
    pub transform: [f32; 6],
}

pub const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

impl Default for Quad {
    fn default() -> Self {
        Quad {
            rect: [0.0; 4],
            uv: [0.0, 0.0, 1.0, 1.0],
            texture: TextureRef::None,
            color: [1.0; 4],
            color2: [1.0; 4],
            gradient: 0,
            radius: 0.0,
            border_width: 0.0,
            border_color: [0.0; 4],
            fill: (0, 1.0),
            softness: 0.0,
            additive: false,
            transform: IDENTITY,
        }
    }
}

/// A scissor rectangle, physical pixels: `x, y, w, h`.
pub type ClipRect = [u32; 4];

#[derive(Clone, PartialEq, Debug)]
pub enum DrawItem {
    Quad(Quad),
    /// Clip what follows to this rectangle; `None` lifts the clip.
    Clip(Option<ClipRect>),
}

/// Everything one document, or one layer stack, draws in a frame.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct DisplayList {
    pub items: Vec<DrawItem>,
    /// Target size in physical pixels.
    pub size: (u32, u32),
}

impl DisplayList {
    pub fn quads(&self) -> impl Iterator<Item = &Quad> {
        self.items.iter().filter_map(|i| match i {
            DrawItem::Quad(q) => Some(q),
            DrawItem::Clip(_) => None,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn extend(&mut self, other: &DisplayList) {
        self.items.extend(other.items.iter().cloned());
        // Lift any clip explicitly, so one document's last clip can never
        // leak into the next.
        if other
            .items
            .iter()
            .any(|i| matches!(i, DrawItem::Clip(Some(_))))
        {
            self.items.push(DrawItem::Clip(None));
        }
    }
}

/// `a` then `b`: the affine that applies `b` first, then `a`.
pub fn compose(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}

/// Apply an affine to a point.
pub fn apply(m: [f32; 6], x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
}

/// Invert an affine; `None` when it is degenerate (a zero scale).
pub fn invert(m: [f32; 6]) -> Option<[f32; 6]> {
    let det = m[0] * m[4] - m[1] * m[3];
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let a = m[4] * inv;
    let b = -m[1] * inv;
    let c = -m[3] * inv;
    let d = m[0] * inv;
    Some([a, b, -(a * m[2] + b * m[5]), c, d, -(c * m[2] + d * m[5])])
}
