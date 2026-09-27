// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `kerosene` -- the Kerosene runtime: the engine running the stock game.
//!
//! ```text
//! kerosene +map kerosene_room
//! kerosene +map kerosene_room +sv_gravity 200 +developer 1
//! kerosene --headless 600 +map kerosene_room     # simulate without a display
//! ```
//!
//! This is the whole binary, and the shape of a game's own: one call with
//! its own [`Game`](kerosene::Game) in place of the stock one.

use kerosene::{LaunchOptions, launch};

fn main() -> anyhow::Result<()> {
    launch(
        kerosene::game::Stock::default(),
        LaunchOptions::new("Kerosene", env!("CARGO_PKG_VERSION")),
    )
}
