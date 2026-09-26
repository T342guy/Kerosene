// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The Kerosene toolset, as a library.
//!
//! The whole toolset is one GUI application ([`toolset::Toolset`]): a project
//! page, the world editor, the sound editor, a build form and an archive form
//! behind one window, with one output panel for every job's log. The same stages are also exposed as headless subcommands, so a
//! script or build server can drive them without a screen.
//!
//! The engine knows nothing about any of this. That boundary is what lets a
//! game ship as just the runtime and an archive; the toolset is a developer's
//! tool, never a player's.

pub mod entry;
pub mod init;
pub mod new;
pub mod panels;
pub mod play;
pub mod project;
pub mod toolset;
pub mod workshop;

pub use entry::{Options, main_with};
pub use toolset::{Launch, Tab, Toolset, run_gui};

/// The headless subcommands, in the order help prints them.
pub const SUBCOMMANDS: &[(&str, &str)] = &[
    (
        "new",
        "start a game: a Cargo package, its project and a map",
    ),
    (
        "init",
        "start a project: a .keroproj and the tree beside it",
    ),
    ("cleave", "compile a .keromap into a .kerobsp"),
    ("umbra", "compute the PVS for a compiled map"),
    (
        "resonance",
        "work out what each part of a compiled map sounds like",
    ),
    ("radiance", "bake static lighting into a compiled map"),
    ("alchemy", "compile textures and author materials"),
    ("forge", "compile source meshes into .keromdl models"),
    ("timbre", "compile sounds into .keroaud"),
    ("kiln", "build a whole project's content"),
    ("play", "build what changed, then run the game"),
    ("vault", "pack and inspect content archives"),
    (
        "workshop",
        "upload a map to the Steam Workshop (a build with --features steam)",
    ),
];

/// Run one headless stage: `kerosene-tools <subcommand> <args...>`.
pub fn run_subcommand(name: &str, args: Vec<String>) -> anyhow::Result<()> {
    match name {
        "new" => new::run(args),
        "init" => init::run(args),
        "cleave" => cleave::run(args),
        "umbra" => umbra::run(args),
        "resonance" => resonance::run(args),
        "radiance" => radiance::run(args),
        "alchemy" => alchemy::run(args),
        "forge" => forge::run(args),
        "timbre" => timbre::run(args),
        "kiln" => kiln::run(args),
        "play" => play::run(args, None),
        "vault" => vault::run(args),
        "workshop" => workshop::run(args),
        other => {
            anyhow::bail!("unknown tool {other:?}; try `kerosene-tools` for the window");
        }
    }
}
