// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Kerosene, as one crate.
//!
//! A game depends on this and nothing else of the engine:
//!
//! ```toml
//! [dependencies]
//! kerosene = { git = "https://github.com/t342guy/kerosene" }
//! ```
//!
//! and is, at its smallest, a type that implements [`Game`] and a `main`
//! that hands it to [`launch`]:
//!
//! ```no_run
//! use kerosene::prelude::*;
//!
//! struct MyGame;
//!
//! impl Game for MyGame {
//!     fn classes(&self, registry: &mut ClassRegistry) {
//!         kerosene::game::register(registry); // the stock classes, if wanted
//!         registry.register(ClassDef::new("item_pickup"));
//!     }
//! }
//!
//! fn main() -> anyhow::Result<()> {
//!     launch(MyGame, LaunchOptions { name: "My Game", ..Default::default() })
//! }
//! ```
//!
//! Every engine crate is here as a module -- [`engine`], [`entity`],
//! [`math`], [`console`], [`vfs`], [`map`], [`bsp`] and the rest -- and so
//! are the third-party crates a game names in its own signatures, so its
//! `egui::Context` and `glam::Vec3` are the engine's and never a second
//! version of the same thing. With the `tools` feature, [`tools`] is the
//! whole toolset as a library, for a game that ships an editor that knows
//! its classes.

pub use kerosene_asset as asset;
pub use kerosene_audio as audio;
pub use kerosene_bsp as bsp;
pub use kerosene_config as config;
pub use kerosene_console as console;
pub use kerosene_engine as engine;
pub use kerosene_entity as entity;
pub use kerosene_kv as kv;
pub use kerosene_map as map;
pub use kerosene_math as math;
pub use kerosene_physics as physics;
pub use kerosene_render as render;
pub use kerosene_rigid as rigid;
pub use kerosene_script as script;
pub use kerosene_vfs as vfs;
pub use kerosene_walk as walk;

pub use {anyhow, egui, glam, log, rhai, winit};

pub use kerosene_engine::launch::{LaunchOptions, launch};
pub use kerosene_engine::{Engine, EngineConfig, Game};

/// The stock game: the classes every Kerosene map may use, and the type
/// that runs them.
pub mod game {
    pub use kerosene_game::*;

    /// The stock classes and nothing more. What the `kerosene` binary runs,
    /// and what a game wraps when it wants doors and triggers as well as its
    /// own classes.
    pub struct Stock;

    impl crate::Game for Stock {
        fn classes(&self, registry: &mut kerosene_entity::ClassRegistry) {
            register(registry);
        }
        fn schema(&self) -> &'static str {
            schema::BUILTIN
        }
    }
}

/// The toolset as a library, behind the `tools` feature: a game's own
/// `mygame-tools` binary is `kerosene::tools::main_with(Options { .. })`.
#[cfg(feature = "tools")]
pub mod tools {
    pub use chisel;
    pub use kerosene_tools::*;
}

/// What most game code names.
pub mod prelude {
    pub use crate::LaunchOptions;
    pub use crate::engine::input::InputState;
    pub use crate::engine::{Engine, EngineConfig, Game};
    pub use crate::launch;
    pub use egui;
    pub use kerosene_console::{Args, ConVarFlags, Console};
    pub use kerosene_entity::{
        ClassDef, ClassRegistry, EntityId, EntityWorld, HostRequest, Value, host_requests,
    };
    pub use kerosene_math::{Aabb, Angles, Vec3};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_and_the_stock_game_agree_on_the_everywhere_flag() {
        assert_eq!(
            kerosene_engine::audio::SF_EVERYWHERE,
            kerosene_game::sound::SF_EVERYWHERE
        );
    }

    #[test]
    fn stock_registers_exactly_the_stock_classes() {
        let engine = Engine::with_game(&EngineConfig::default(), Box::new(game::Stock));
        let theirs = kerosene_game::registry();
        assert_eq!(engine.entities.registry.class_names(), theirs.class_names());
        assert!(!engine.game().unwrap().schema().is_empty());
    }
}
