// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The stock game, for tests that need a map's entities to do something.

#![allow(dead_code)]

use kerosene_engine::Game;
use kerosene_engine::engine::{Engine, EngineConfig};
use kerosene_entity::ClassRegistry;

/// The classes in `kerosene-game` and nothing else.
pub struct Stock;

impl Game for Stock {
    fn classes(&self, registry: &mut ClassRegistry) {
        kerosene_game::register(registry);
    }
}

/// An engine running the stock game.
pub fn stock(config: &EngineConfig) -> Engine {
    Engine::with_game(config, Box::new(Stock))
}
