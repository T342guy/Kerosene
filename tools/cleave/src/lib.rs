//! Cleave -- the VoidEngine BSP compiler, as a library.
//!
//! The command-line tool is a thin wrapper over [`pipeline::compile`]. Exposing
//! the compile as a library is what lets other crates -- Chisel, and the
//! engine's own integration tests -- build a map without shelling out to a
//! binary and without a temporary file in between.

pub mod brush;
pub mod csg;
pub mod emit;
pub mod material;
pub mod pipeline;
pub mod portal;
pub mod tree;

pub use pipeline::{CompileError, CompileOptions, CompileOutput, Stats, compile, lint_materials};
