// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What the renderer is asked to draw.
//!
//! The scene is the contract between the things that decide what is on
//! screen and the renderer that puts it there. Producers -- the game UI
//! today -- fill it with plain data; `kerosene-render` reads it and never
//! reaches back into them. That is what lets the UI and the renderer be
//! separate subsystems that do not depend on each other, with the engine
//! passing the data across.
//!
//! It holds both halves. The 2D half is the UI's [`DisplayList`] of quads, the
//! [`Images`] those quads name, and the size of the glyph atlas they sample.
//! The 3D half is the CPU-side data for a frame: the [`camera`], the world
//! [`mesh`] and its visibility culling, the [`lightmap`] atlas, dynamic
//! [`lights`], reflection [`probes`], [`decals`] and the [`brdf`] tables. None
//! of it touches the graphics API, so all of it is tested without a GPU.

pub mod brdf;
pub mod camera;
pub mod decals;
pub mod draw;
mod images;
pub mod lightmap;
pub mod lights;
pub mod mesh;
pub mod probes;

pub use draw::{ClipRect, DisplayList, DrawItem, Quad, TextureRef};
pub use images::Images;

/// Width and height of the glyph atlas texture, in pixels. One byte per
/// pixel (coverage), so the atlas is `ATLAS_SIZE * ATLAS_SIZE` bytes.
pub const ATLAS_SIZE: u32 = 1024;
