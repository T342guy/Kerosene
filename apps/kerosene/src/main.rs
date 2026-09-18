// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! `kerosene` -- the Kerosene runtime: the engine running the stock game.
//!
//! ```text
//! kerosene +map kero_start
//! kerosene +map kero_start +sv_gravity 200 +developer 1
//! kerosene --headless 600 +map kero_start     # simulate without a display
//! ```
//!
//! This is the whole binary, and the shape of a game's own: one call with
//! its own [`Game`](kerosene::Game) in place of the stock one.

use kerosene::{LaunchOptions, launch};

fn main() -> anyhow::Result<()> {
    launch(
        kerosene::game::Stock,
        LaunchOptions {
            name: "Kerosene",
            version: env!("CARGO_PKG_VERSION"),
            ..Default::default()
        },
    )
}
