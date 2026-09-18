// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! `kerosene-tools` -- the Kerosene toolset.
//!
//! Run with no subcommand it opens the one window that holds every tool: a
//! project page, the world editor, the sound editor, a build form and an
//! archive form, with an activity bar of icons down the left edge to switch
//! between them and an output panel along the bottom that every job logs
//! into. That is the developer's door into the engine.
//!
//! The stages also run headless, as subcommands, for scripts and build
//! servers:
//!
//! ```text
//! kerosene-tools                          the toolset window
//! kerosene-tools cleave map.keromap       the compilers, one at a time
//! kerosene-tools kiln [...]               a whole project build
//! kerosene-tools vault <cmd>              content archives
//! ```
//!
//! The engine runtime is `kerosene`, its own binary, and not part of this
//! set: a game ships the runtime and an archive, never the tools. A game
//! that wants a toolset of its own makes the same call as this file with
//! its own `Options` -- see `kerosene_tools::entry`.

fn main() -> anyhow::Result<()> {
    kerosene_tools::main_with(kerosene_tools::Options {
        name: "kerosene-tools",
        version: env!("CARGO_PKG_VERSION"),
        ..Default::default()
    })
}
