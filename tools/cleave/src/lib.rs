// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Cleave -- the Kerosene BSP compiler, as a library.
//!
//! The command-line tool is a thin wrapper over [`pipeline::compile`]. Exposing
//! the compile as a library is what lets other crates -- Chisel, and the
//! engine's own integration tests -- build a map without shelling out to a
//! binary and without a temporary file in between.

pub mod brush;
mod cli;
pub mod csg;
pub mod emit;
pub mod material;
pub mod pipeline;
pub mod portal;
pub mod sections;
pub mod tree;
pub mod walk;

pub use cli::run;
pub use pipeline::{CompileError, CompileOptions, CompileOutput, Stats, compile, lint_materials};
