// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The game: what the engine runs, and the one thing it does not own.
//!
//! Source splits the engine from the game DLL. The engine moves the player,
//! routes entity I/O, draws the world; the DLL says what a `func_door` is,
//! what an inventory holds, what the HUD shows. Kerosene keeps the same
//! split, as a trait: a game is a type that implements [`Game`], handed to
//! [`Engine::with_game`] once, and the engine calls it at the moments a
//! game needs to be asked.
//!
//! Every hook takes `&mut Engine`, so a game can do anything the engine can
//! -- register a console command, read the player, spawn a prop, play a
//! sound. The trade for that power is one rule: the engine holds the game
//! outside itself while a hook runs, so a hook that makes the engine call
//! *another* hook on the same game finds nobody home and the inner call is
//! skipped. Concretely, `tick` must not call `engine.load_map` (use
//! [`Engine::request_map`], which loads at the start of the next frame and
//! then calls `map_loaded` properly). Every other engine method is fine.
//!
//! Handlers on entity classes get `&mut EntityWorld` and nothing more, as
//! before; when one of them needs the rest of the engine it leaves a
//! [`HostRequest`] with a kind of the game's own and [`Game::entity_request`]
//! picks it up at the end of the tick. That keeps the entity world a plain
//! data structure that can be tested without an engine.
//!
//! `()` implements `Game` and does nothing, which is what `Engine::new`
//! uses: an engine with no classes, for tests and for a server that only
//! wants the world.

use crate::engine::Engine;
use crate::input::InputState;
use kerosene_entity::{ClassRegistry, HostRequest};

/// A game, as the engine sees it. Every method has a do-nothing default.
#[allow(unused_variables)]
pub trait Game: 'static {
    /// The entity classes this game provides.
    ///
    /// Called once, when the engine is made; the registry it fills is shared
    /// by every map the engine loads after that. A class registered here
    /// spawns, thinks and answers inputs the same way the stock ones do.
    fn classes(&self, registry: &mut ClassRegistry) {}

    /// The `.kerodef` text describing [`classes`](Game::classes), for the
    /// tools: what Chisel shows in its property inspector. Empty means the
    /// game has no classes of its own to describe.
    fn schema(&self) -> &'static str {
        ""
    }

    /// The engine exists: its console, filesystem and audio are up, and no
    /// map is loaded. Called before `config.cfg`, `autoexec.cfg` and the
    /// command line run, so a command or convar registered here can be used
    /// from any of them.
    fn setup(&mut self, engine: &mut Engine) {}

    /// A map has loaded: its entities are spawned, the player is placed, the
    /// map script has run its `on_map_start`.
    fn map_loaded(&mut self, engine: &mut Engine) {}

    /// The start of a tick, before the player moves. The place to change
    /// how they move -- speed, gravity, whether they may jump -- since the
    /// movement reads the convars right after this.
    fn pre_tick(&mut self, engine: &mut Engine, input: &InputState, dt: f32) {}

    /// The middle of a tick: the player has moved, triggers have fired,
    /// entities have thought and their requests are answered. Physics props
    /// have not yet stepped. The place for the game's own rules.
    fn tick(&mut self, engine: &mut Engine, input: &InputState, dt: f32) {}

    /// An entity left a request whose kind the engine does not know. Return
    /// `true` when it was one of this game's; anything left is reported as
    /// unknown on the console.
    fn entity_request(&mut self, engine: &mut Engine, request: &HostRequest) -> bool {
        false
    }

    /// A console command left a request neither the engine nor the host
    /// claimed. Return `true` when this game handled it.
    fn console_request(&mut self, engine: &mut Engine, kind: &str, payload: &str) -> bool {
        false
    }

    /// Whether [`ui`](Game::ui) should be given a frame.
    ///
    /// Read every frame. When it is false and the console is closed, the
    /// host does not run the UI layer at all, so a game with no HUD pays
    /// nothing for one.
    fn wants_ui(&self) -> bool {
        false
    }

    /// Draw the game's own UI -- a HUD, a menu -- over the world and under
    /// the console. While the mouse is captured the pointer never reaches
    /// this layer, so a HUD is display-only during play and a menu drawn
    /// after the mouse is released is clickable.
    fn ui(&mut self, engine: &mut Engine, ctx: &egui::Context) {}

    /// An event a UI document emitted: a menu button, a keypad code, a
    /// choice in a dialogue. `source` is the layer (`hud`, `menu`) or world
    /// panel (`panel:<name>`) it came from.
    fn ui_event(&mut self, engine: &mut Engine, name: &str, data: &str, source: &str) {}

    /// The store reported something: an achievement unlocked, a stat
    /// changed, a score posted, the overlay opened. Entities wired to it and
    /// the map's `on_platform_event` have already heard.
    fn platform_event(&mut self, engine: &mut Engine, event: &kerosene_platform::PlatformEvent) {}
}

/// No game: no classes, nothing on any hook.
impl Game for () {}

impl Engine {
    /// The game this engine runs, when no hook is in progress.
    pub fn game(&self) -> Option<&dyn Game> {
        self.game.as_deref()
    }

    /// Run one hook with the game held outside the engine.
    ///
    /// `None` when a hook is already running -- the case the module doc
    /// describes -- and logged, because a game author who hits it will
    /// otherwise see a hook silently not happen.
    pub(crate) fn with_game_mut<R>(
        &mut self,
        f: impl FnOnce(&mut dyn Game, &mut Engine) -> R,
    ) -> Option<R> {
        let Some(mut game) = self.game.take() else {
            log::debug!("a game hook was skipped because another one is running");
            return None;
        };
        let out = f(game.as_mut(), self);
        self.game = Some(game);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineConfig;
    use kerosene_entity::{ClassDef, EntityWorld, Value};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A game that writes down what happened to it, in order.
    struct Diary(Rc<RefCell<Vec<String>>>);

    impl Diary {
        fn note(&self, what: impl Into<String>) {
            self.0.borrow_mut().push(what.into());
        }
    }

    fn spawn_widget(world: &mut EntityWorld, id: kerosene_entity::EntityId) {
        if let Some(e) = world.get_mut(id) {
            e.fields.set("spawned", Value::Bool(true));
        }
        world.request("diary.spawned", "widget", id, None);
    }

    impl Game for Diary {
        fn classes(&self, registry: &mut ClassRegistry) {
            registry.register(ClassDef::new("widget").on_spawn(spawn_widget));
            self.note("classes");
        }
        fn setup(&mut self, engine: &mut Engine) {
            engine.console.register_command(
                "diary_ping",
                kerosene_console::ConVarFlags::NONE,
                "a command the game added",
                |con, _| con.request("diary.ping", "pong"),
            );
            self.note("setup");
        }
        fn map_loaded(&mut self, engine: &mut Engine) {
            self.note(format!("map_loaded {}", engine.entities.len()));
            // The rule: this must not reach a nested hook. It must also not
            // hang or panic.
            engine.with_game_mut(|g, _| g.wants_ui());
        }
        fn pre_tick(&mut self, _: &mut Engine, _: &InputState, _: f32) {
            self.note("pre_tick");
        }
        fn tick(&mut self, engine: &mut Engine, _: &InputState, _: f32) {
            self.note(format!("tick {}", engine.tick_count));
        }
        fn entity_request(&mut self, _: &mut Engine, request: &HostRequest) -> bool {
            if request.kind == "diary.spawned" {
                self.note(format!("request {}", request.payload));
                return true;
            }
            false
        }
        fn console_request(&mut self, _: &mut Engine, kind: &str, payload: &str) -> bool {
            if kind == "diary.ping" {
                self.note(format!("console {payload}"));
                return true;
            }
            false
        }
    }

    fn engine_with_diary() -> (Engine, Rc<RefCell<Vec<String>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let engine = Engine::with_game(&EngineConfig::default(), Box::new(Diary(log.clone())));
        (engine, log)
    }

    #[test]
    fn the_game_is_asked_for_classes_and_set_up_before_anything_runs() {
        let (engine, log) = engine_with_diary();
        assert_eq!(log.borrow().as_slice(), ["classes", "setup"]);
        assert!(engine.entities.registry.is_registered("widget"));
        assert!(engine.game().is_some());
    }

    #[test]
    fn a_custom_class_spawns_and_its_request_reaches_the_game() {
        let (mut engine, log) = engine_with_diary();
        let kv = kerosene_kv::KeyValues::parse(r#"entity { "classname" "widget" }"#).unwrap();
        engine.entities.load_from_kv(&kv).unwrap();
        let id = engine.entities.find_by_class("widget")[0];
        assert!(
            engine
                .entities
                .get(id)
                .unwrap()
                .fields
                .bool("spawned", false)
        );
        engine.take_entity_requests();
        assert!(log.borrow().iter().any(|l| l == "request widget"));
    }

    #[test]
    fn ticks_call_pre_tick_then_tick_in_order() {
        let (mut engine, log) = engine_with_diary();
        log.borrow_mut().clear();
        engine.tick(1.0 / 64.0, &InputState::default());
        assert_eq!(log.borrow().as_slice(), ["pre_tick", "tick 1"]);
    }

    #[test]
    fn a_console_request_the_engine_does_not_know_is_offered_to_the_game() {
        let (mut engine, log) = engine_with_diary();
        engine.console.execute_user("diary_ping");
        let unclaimed = crate::engine::take_console_requests(&mut engine);
        crate::engine::report_unhandled(&mut engine, unclaimed);
        assert!(log.borrow().iter().any(|l| l == "console pong"));
        assert!(
            !engine
                .console
                .log()
                .any(|l| l.text.contains("unknown host request")),
            "the game claimed it"
        );
    }

    #[test]
    fn no_game_means_no_classes_and_nothing_breaks() {
        let mut engine = Engine::new(&EngineConfig::default());
        assert!(engine.game().is_some(), "the unit game is still a game");
        assert!(!engine.entities.registry.is_registered("widget"));
        engine.tick(1.0 / 64.0, &InputState::default());
    }

    #[test]
    fn a_nested_hook_is_skipped_rather_than_looped() {
        let (mut engine, log) = engine_with_diary();
        // `map_loaded` tries to re-enter; the outer call still completes.
        engine.with_game_mut(|g, e| g.map_loaded(e));
        assert!(log.borrow().iter().any(|l| l.starts_with("map_loaded")));
        assert!(engine.game().is_some(), "the game was put back");
    }
}
