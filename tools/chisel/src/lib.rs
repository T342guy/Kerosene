//! Chisel -- the VoidEngine world editor.
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
pub mod compile;
pub mod draw;
pub mod document;
pub mod grid;
pub mod tools;
pub mod viewport;

pub use app::ChiselApp;
pub use document::{Document, Selection};
pub use grid::Grid;
pub use tools::{Tool, ToolKind};
pub use viewport::{Viewport, ViewportKind};
