// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Kerosene, as one crate.
//!
//! A game depends on this and nothing else of the engine:
//!
//! ```toml
//! [dependencies]
//! kerosene = "1.0.0-a1"
//! ```
//!
//! -- or, quicker, `kerosene-tools new mygame` makes a game crate with the
//! editor, the compilers and `cargo play` set up, and a starter map.
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
//!     launch(MyGame, LaunchOptions::new("My Game", env!("CARGO_PKG_VERSION")))
//! }
//! ```
//!
//! # What is stable
//!
//! Kerosene's version is this crate's, and it follows [Semantic
//! Versioning](https://semver.org) on this crate's API. A release that
//! breaks a game which compiled against the one before is a new major
//! version. What that promise covers:
//!
//! * the items at the root -- [`launch`], [`LaunchOptions`], [`Game`],
//!   [`Engine`], [`EngineConfig`], [`VERSION`] -- and the [`prelude`];
//! * the modules [`engine`], [`entity`], [`math`], [`console`],
//!   [`physics`], [`script`], [`ui`], [`platform`], [`vfs`], [`game`] and,
//!   with the `tools` feature, `tools`;
//! * the third-party crates re-exported here -- [`egui`], [`glam`], [`rhai`],
//!   [`winit`], [`serde_json`], [`anyhow`], [`log`] -- whose own breaking
//!   releases arrive in Kerosene's major releases and nowhere else.
//!
//! [`internals`] is everything else: the formats, the renderer, the mixer,
//! the compilers' shared crates. It is public because tools and ambitious
//! games need it, and it may change in any minor release. The whole policy
//! is in the book, under *Versioning*.
//!
//! Every crate here is one version: the engine crates move together, so a
//! game's `glam::Vec3` and `egui::Context` are always the engine's own.

#![warn(missing_docs)]

/// Kerosene's version: this crate's, as `Cargo.toml` says it. A game's own
/// version is its own, and goes in [`LaunchOptions::new`].
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use kerosene_console as console;
pub use kerosene_engine as engine;
pub use kerosene_entity as entity;
pub use kerosene_math as math;
pub use kerosene_physics as physics;
pub use kerosene_platform as platform;
pub use kerosene_script as script;
pub use kerosene_ui as ui;
pub use kerosene_vfs as vfs;

/// The engine's inner crates, outside the SemVer promise: they may change in
/// any minor release. See [the crate docs](crate#what-is-stable).
pub mod internals {
    pub use kerosene_asset as asset;
    pub use kerosene_audio as audio;
    pub use kerosene_bsp as bsp;
    pub use kerosene_config as config;
    pub use kerosene_kv as kv;
    pub use kerosene_map as map;
    pub use kerosene_render as render;
    pub use kerosene_rigid as rigid;
    pub use kerosene_walk as walk;
}

/// What [`Game::save`] returns and
/// [`Game::load`] takes.
pub use serde_json;

pub use {anyhow, egui, glam, log, rhai, winit};

pub use kerosene_engine::launch::{LaunchOptions, launch};
pub use kerosene_engine::{Engine, EngineConfig, Game};

/// The stock game: the classes every Kerosene map may use, and the type
/// that runs them.
pub mod game {
    pub use kerosene_game::*;

    use crate::engine::input::InputState;
    use crate::{Engine, Game};
    use kerosene_game::weapons::{Arsenal, StateValue, WeaponEvent};

    /// The request kind the weapon commands leave for the game.
    const WEAPON: &str = "stock_weapon";

    /// How hard a dash throws the player, in units per second.
    pub const DASH_SPEED: f32 = 640.0;

    /// The stock classes, and the starter weapons and dash. What the
    /// `kerosene` binary runs, and what a game wraps when it wants doors and
    /// triggers as well as its own classes.
    ///
    /// The weapons are [`kerosene_game::weapons`]: a starting point that gives
    /// the HUD something to show, not a combat system. A game with its own
    /// replaces this type rather than extending it.
    #[derive(Default)]
    pub struct Stock {
        /// The weapons and the dash: what is carried, loaded and cooling.
        pub arsenal: Arsenal,
    }

    impl Stock {
        /// Carry out what the weapons did: trace shots, leave decals, and
        /// tell the UI.
        fn apply(&mut self, engine: &mut Engine, events: Vec<WeaponEvent>) {
            for event in events {
                match event {
                    WeaponEvent::Fired {
                        weapon,
                        directions,
                        range,
                        decal,
                        decal_size,
                        ..
                    } => {
                        for offset in directions {
                            if let Some((at, normal, _)) = engine.trace_view(offset, range) {
                                engine.place_decal(decal, at, normal, decal_size);
                            }
                        }
                        engine.ui_emit("weapon_fired", weapon);
                    }
                    WeaponEvent::Switched { from, to } => {
                        engine.ui_emit("weapon_changed", format!("{from} {to}"));
                    }
                    WeaponEvent::Empty => engine.ui_emit("weapon_empty", ""),
                    WeaponEvent::ReloadStarted => engine.ui_emit("weapon_reload", ""),
                    WeaponEvent::Reloaded => engine.ui_emit("weapon_reloaded", ""),
                    WeaponEvent::AbilityUsed(name) => {
                        if name == "dash" {
                            // Along the ground, the way the player faces:
                            // a dash, not a launch.
                            let mut forward = engine.player.view_angles.forward();
                            forward.z = 0.0;
                            let forward = forward.normalize_or_zero();
                            engine.player.movement.velocity += forward * DASH_SPEED;
                        }
                        engine.ui_emit("ability_used", name);
                    }
                    WeaponEvent::AbilityReady(name) => engine.ui_emit("ability_ready", name),
                }
            }
            self.publish(engine);
        }

        fn publish(&self, engine: &mut Engine) {
            for (key, value) in self.arsenal.state() {
                match value {
                    StateValue::Flag(b) => engine.ui_set(&key, b),
                    StateValue::Number(n) => engine.ui_set(&key, n),
                    StateValue::Text(t) => engine.ui_set(&key, t),
                }
            }
        }
    }

    impl Game for Stock {
        fn classes(&self, registry: &mut kerosene_entity::ClassRegistry) {
            register(registry);
        }

        fn schema(&self) -> &'static str {
            schema::BUILTIN
        }

        fn setup(&mut self, engine: &mut Engine) {
            use kerosene_console::ConVarFlags;
            let commands: [(&'static str, &'static str); 9] = [
                ("slot1", "Select the weapon in slot 1."),
                ("slot2", "Select the weapon in slot 2."),
                ("slot3", "Select the weapon in slot 3."),
                ("lastinv", "Switch back to the weapon held before this one."),
                ("invnext", "Select the next weapon."),
                ("invprev", "Select the previous weapon."),
                ("reload", "Reload the weapon in hand."),
                ("+ability1", "Use the first ability (dash)."),
                ("-ability1", "Release the first ability."),
            ];
            for (name, help) in commands {
                engine
                    .console
                    .register_command(name, ConVarFlags::NONE, help, move |con, _| {
                        con.request(WEAPON, name)
                    });
            }
            // Keys for them, on a first run. A saved config.cfg is exec'd
            // after this and has the last word.
            for (key, command) in [
                ("1", "slot1"),
                ("2", "slot2"),
                ("3", "slot3"),
                ("q", "lastinv"),
                ("r", "reload"),
                ("mouse2", "+ability1"),
            ] {
                engine.console.enqueue(format!("bind {key} {command}"));
            }
            self.publish(engine);
        }

        fn map_loaded(&mut self, engine: &mut Engine) {
            // A fresh map is a fresh loadout.
            self.arsenal = Arsenal::default();
            self.publish(engine);
        }

        fn save(&mut self, _: &mut Engine) -> serde_json::Value {
            serde_json::json!({ "arsenal": self.arsenal.save() })
        }

        fn load(&mut self, engine: &mut Engine, data: &serde_json::Value) {
            if let Some(arsenal) = data.get("arsenal") {
                self.arsenal.load(arsenal);
            }
            self.publish(engine);
        }

        fn pre_tick(&mut self, engine: &mut Engine, input: &InputState, dt: f32) {
            if !engine.has_level() {
                return;
            }
            // Attack also throws a carried prop and presses world panels;
            // it only fires when it is doing neither.
            let can_fire = engine.held_prop().is_none()
                && !engine.aiming_at_panel()
                && engine.player.health > 0.0;
            let events = self.arsenal.tick(dt, input.attack, can_fire);
            self.apply(engine, events);
        }

        fn console_request(&mut self, engine: &mut Engine, kind: &str, payload: &str) -> bool {
            if kind != WEAPON {
                return false;
            }
            let events = match payload {
                "slot1" => self.arsenal.select_slot(1),
                "slot2" => self.arsenal.select_slot(2),
                "slot3" => self.arsenal.select_slot(3),
                "lastinv" => self.arsenal.last(),
                "invnext" => self.arsenal.cycle(1),
                "invprev" => self.arsenal.cycle(-1),
                "reload" => self.arsenal.reload(),
                "+ability1" => self.arsenal.use_ability("dash").into_iter().collect(),
                _ => Vec::new(),
            };
            self.apply(engine, events);
            true
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
    fn the_stock_weapons_feed_the_hud() {
        use kerosene_engine::engine::{report_unhandled, take_console_requests};
        use kerosene_ui::Value;
        let mut engine =
            Engine::with_game(&EngineConfig::default(), Box::new(game::Stock::default()));
        let run = |engine: &mut Engine, line: &str| {
            engine.console.execute(line);
            let unclaimed = take_console_requests(engine);
            report_unhandled(engine, unclaimed);
        };
        assert_eq!(
            engine.ui.store.get("weapon.active"),
            Some(&Value::Str("pistol".into()))
        );
        run(&mut engine, "slot2");
        assert_eq!(
            engine.ui.store.get("weapon.active"),
            Some(&Value::Str("shotgun".into()))
        );
        assert_eq!(engine.ui.store.get("weapon.ammo"), Some(&Value::Float(6.0)));
        let changed = engine
            .ui
            .store
            .pending_events()
            .iter()
            .find(|e| e.name == "weapon_changed");
        assert_eq!(changed.map(|e| e.data.as_str()), Some("pistol shotgun"));
        run(&mut engine, "+ability1");
        assert_eq!(
            engine.ui.store.get("ability.dash.ready"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn stock_registers_exactly_the_stock_classes() {
        let engine = Engine::with_game(&EngineConfig::default(), Box::new(game::Stock::default()));
        let theirs = kerosene_game::registry();
        assert_eq!(engine.entities.registry.class_names(), theirs.class_names());
        assert!(!engine.game().unwrap().schema().is_empty());
    }
}
