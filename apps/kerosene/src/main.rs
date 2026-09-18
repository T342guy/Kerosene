// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! `kerosene` -- the Kerosene runtime: the engine running the stock game.
//!
//! ```text
//! kerosene +map kero_start
//! kerosene +map kero_start +sv_gravity 200 +developer 1
//! kerosene --headless 600 +map kero_start     # simulate without a display
//! ```
//!
//! This is the whole binary. A game of your own is the same call with your
//! own [`Game`](kerosene_engine::Game); see `kerosene_engine::launch`.

use kerosene_engine::Game;
use kerosene_engine::launch::{LaunchOptions, launch};

/// The stock game: the classes in `kerosene-game`, nothing more.
struct Stock;

impl Game for Stock {
    fn classes(&self, registry: &mut kerosene_entity::ClassRegistry) {
        kerosene_game::register(registry);
    }
    fn schema(&self) -> &'static str {
        kerosene_game::schema::BUILTIN
    }
}

fn main() -> anyhow::Result<()> {
    launch(
        Stock,
        LaunchOptions {
            name: "Kerosene",
            version: env!("CARGO_PKG_VERSION"),
            ..Default::default()
        },
    )
}
