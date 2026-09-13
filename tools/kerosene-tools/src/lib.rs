// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The Kerosene toolset, as a library.
//!
//! The whole toolset is one GUI application ([`toolset::Toolset`]): the world
//! editor, the sound editor, a build panel and an archive panel behind one
//! window. The same stages are also exposed as headless subcommands, so a
//! script or build server can drive them without a screen.
//!
//! The engine knows nothing about any of this. That boundary is what lets a
//! game ship as just the runtime and an archive; the toolset is a developer's
//! tool, never a player's.

pub mod init;
pub mod panels;
pub mod toolset;

pub use toolset::{Launch, Tab, Toolset, run_gui};

/// The headless subcommands, in the order help prints them.
pub const SUBCOMMANDS: &[(&str, &str)] = &[
    ("init", "start a project: a .keroproj and the tree beside it"),
    ("cleave", "compile a .keromap into a .kerobsp"),
    ("umbra", "compute the PVS for a compiled map"),
    ("radiance", "bake static lighting into a compiled map"),
    ("alchemy", "compile textures and author materials"),
    ("forge", "compile source meshes into .keromdl models"),
    ("timbre", "compile sounds into .keroaud"),
    ("kiln", "build a whole project's content"),
    ("vault", "pack and inspect content archives"),
];

/// Run one headless stage: `kerosene-tools <subcommand> <args...>`.
pub fn run_subcommand(name: &str, args: Vec<String>) -> anyhow::Result<()> {
    match name {
        "init" => init::run(args),
        "cleave" => cleave::run(args),
        "umbra" => umbra::run(args),
        "radiance" => radiance::run(args),
        "alchemy" => alchemy::run(args),
        "forge" => forge::run(args),
        "timbre" => timbre::run(args),
        "kiln" => kiln::run(args),
        "vault" => vault::run(args),
        other => {
            anyhow::bail!("unknown tool {other:?}; try `kerosene-tools` for the window");
        }
    }
}
