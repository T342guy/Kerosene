// SPDX-License-Identifier: LGPL-3.0-or-later
//! Chisel -- the Kerosene world editor.
//!
//! The Hammer analogue, and the flagship of the tool suite. A level is built
//! by drawing convex brushes, texturing their faces, placing entities and
//! wiring their outputs to each other's inputs -- then compiling the result
//! through Cleave, Umbra and Radiance and running it.
//!
//! The editor's *logic* lives in this library and is tested without a window:
//! the document and its undo history ([`document`]), the grid ([`grid`]), the
//! viewport projections and picking ([`viewport`]), the tools ([`tools`]) and
//! the compile pipeline ([`compile`]). The egui layer on top only draws it and
//! turns clicks into calls.
//!
//! That split is worth the small amount of extra structure: an editor that can
//! only be tested by a person clicking around is an editor whose bugs are
//! found by its users.

pub mod app;
pub mod browse;
pub mod brush;
pub mod classes;
pub mod compile;
pub mod draw;
pub mod document;
pub mod faces;
pub mod files;
pub mod grid;
pub mod icons;
pub mod inspector;
pub mod leak;
pub mod motion;
pub mod preview;
pub mod raster;
pub mod shapes;
pub mod textures;
pub mod tools;
pub mod viewport;
pub mod wiring;

pub use app::ChiselApp;
pub use document::{Document, Selection};
pub use grid::Grid;
pub use tools::{Tool, ToolKind};
pub use viewport::{Viewport, ViewportKind};
