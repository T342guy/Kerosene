// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The front end: what a player sees when no map is running.
//!
//! A game started from its shortcut opens on the main menu (`ui_mainmenu`)
//! rather than dropping straight into a map: New Game loads the project's
//! start map, Load lists the saves, and `game_end` or `disconnect` come
//! back here. A `+map` on the command line, `-nomenu`, or a headless run go
//! straight in, as they always did.
//!
//! Between asking for a map and having it, the host draws one frame of
//! `ui_loading` -- the map's name over the logo -- so the window says what
//! it is doing instead of freezing on the last picture while the level
//! loads. A map that will not load leaves its reason on the main menu
//! (`error.message` in the UI store), not only in the console.

use crate::engine::Engine;
use crate::ui::{HUD_LAYER, MENU_LAYER};
use kerosene_console::{ConVarFlags, Console};

/// The layer the loading screen is drawn on, above everything but the
/// console.
pub const LOADING_LAYER: &str = "loading";

/// The layer the opening splash is drawn on, above the main menu.
pub const SPLASH_LAYER: &str = "splash";

/// Console request kinds.
mod requests {
    pub const NEW_GAME: &str = "newgame";
    pub const DISCONNECT: &str = "disconnect";
}

pub(crate) fn register(console: &mut Console) {
    console.register_cvar(
        "ui_mainmenu",
        "ui/menus/main.kui",
        ConVarFlags::NONE,
        "The layout shown when no map is running. Empty for none.",
    );
    console.register_cvar(
        "ui_loading",
        "ui/menus/loading.kui",
        ConVarFlags::NONE,
        "The layout shown while a map loads. Empty for none.",
    );
    console.register_cvar(
        "ui_splash",
        "ui/menus/splash.kui",
        ConVarFlags::NONE,
        "The layout shown for a moment as the game opens on its main menu. Empty for none; -nosplash empties it.",
    );
    console.register_command(
        "newgame",
        ConVarFlags::NONE,
        "Start a new game on the start map.",
        |con, _| con.request(requests::NEW_GAME, ""),
    );
    console.register_command(
        "disconnect",
        ConVarFlags::NONE,
        "Leave the map for the main menu.",
        |con, _| con.request(requests::DISCONNECT, ""),
    );
}

impl Engine {
    /// The map New Game starts on: the project's start map, or the base
    /// content's room.
    pub fn new_game_map(&self) -> &str {
        &self.new_game_map
    }

    /// Start a new game on [`new_game_map`](Engine::new_game_map), at the
    /// start of the next frame.
    pub fn new_game(&mut self) {
        let map = self.new_game_map.clone();
        self.request_map(&map);
    }

    /// Leave the running map for nothing: no level, no entities, no sounds.
    /// The main menu comes up by itself on the next UI frame. The game hears
    /// of it through [`Game::map_unloading`](crate::Game::map_unloading).
    pub fn unload_map(&mut self) {
        if self.level.is_none() {
            return;
        }
        self.with_game_mut(|game, engine| game.map_unloading(engine));
        self.level = None;
        self.load_generation += 1;
        self.entities = kerosene_entity::EntityWorld::new(self.registry.clone());
        self.physics = crate::physics::PhysicsProps::new();
        self.animations = crate::animation::Animations::new();
        self.previous_brush_poses.clear();
        self.view = Default::default();
        self.entity_voices.clear();
        self.held_prop_clear();
        self.pending_save = None;
        self.audio.stop_all();
        self.player = Default::default();
        self.ui_map_loaded();
        self.ui.system.hide(HUD_LAYER);
    }

    /// End the game, back to the main menu: what `game_end` and
    /// `disconnect` do.
    pub fn end_game(&mut self) {
        self.pending_map = None;
        self.pending_change = None;
        self.unload_map();
        self.ui_emit("game_ended", "");
    }

    /// Put the opening splash up, if `ui_splash` names one: what a window
    /// does as it opens on the main menu. It takes itself down.
    pub fn show_splash(&mut self) {
        // The command line's `-nosplash` and a config's `ui_splash ""` have
        // not run yet: they are buffered for the first frame.
        self.console.run_buffered();
        let path = self.console.string("ui_splash").to_string();
        if !path.is_empty() && self.vfs.exists(&path) {
            self.ui_show(SPLASH_LAYER, &path);
        }
    }

    /// Whether the host should draw a loading screen for one frame before
    /// the next load: a window does, a headless run has nothing to draw.
    pub fn set_loading_screen(&mut self, on: bool) {
        self.loading_screen = on;
    }

    /// Before a load, once: put the loading screen up and say to wait a
    /// frame for it to be drawn. `true` when the load should wait.
    pub(crate) fn hold_for_loading_screen(&mut self) -> bool {
        if !self.loading_screen || !self.has_pending_map() {
            self.loading_shown = false;
            return false;
        }
        if self.loading_shown {
            return false;
        }
        let path = self.console.string("ui_loading").to_string();
        if path.is_empty() || !self.vfs.exists(&path) {
            return false;
        }
        let map = match (&self.pending_map, &self.pending_change) {
            (Some(map), _) => map.clone(),
            (None, Some(crate::save::PendingChange::Level { map, .. })) => map.clone(),
            (None, Some(crate::save::PendingChange::Load(save))) => save.clone(),
            (None, None) => String::new(),
        };
        self.ui_set("loading.map", map);
        self.loading_shown = self.ui_show(LOADING_LAYER, &path);
        self.loading_shown
    }

    /// After a load, or a failed one: take the loading screen down.
    pub(crate) fn loading_done(&mut self) {
        if std::mem::take(&mut self.loading_shown) {
            self.ui.system.hide(LOADING_LAYER);
        }
    }

    /// Say why a map would not load where the player can see it.
    pub(crate) fn load_failed(&mut self, message: String) {
        self.console.error(&message);
        self.ui_set("error.message", message.clone());
        self.ui_emit("load_failed", message);
    }

    /// The main menu, while no map runs: shown once per path, like the HUD.
    pub(crate) fn ensure_main_menu(&mut self) {
        let path = self.console.string("ui_mainmenu").to_string();
        if path.is_empty() || self.ui.system.is_visible(MENU_LAYER) {
            return;
        }
        if self.ui.main_menu_failed.as_deref() == Some(path.as_str()) {
            return;
        }
        self.publish_saves();
        self.ui_set("game.title", self.title.clone());
        if !self.vfs.exists(&path) || !self.ui_show(MENU_LAYER, &path) {
            self.ui.main_menu_failed = Some(path);
            // With no menu to show, a window would sit empty: start the
            // game instead, the once, so a map that will not load does not
            // make this a loop.
            if !std::mem::replace(&mut self.started_without_menu, true) {
                self.new_game();
            }
        }
    }

    /// Handle one of the front end's console requests. `false` if it is
    /// not one.
    pub(crate) fn frontend_console_request(&mut self, kind: &str) -> bool {
        match kind {
            requests::NEW_GAME => {
                self.ui_set("error.message", "");
                self.new_game();
            }
            requests::DISCONNECT => self.end_game(),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::{Engine, EngineConfig};
    use crate::ui::MENU_LAYER;

    fn frame(engine: &mut Engine) {
        engine.frame(1.0 / 60.0, &Default::default());
        let unclaimed = crate::engine::take_console_requests(engine);
        assert!(unclaimed.is_empty(), "{unclaimed:?}");
        engine.ui_frame(1.0 / 60.0, (1280, 720));
    }

    #[test]
    fn no_map_is_the_main_menu_and_new_game_leaves_it() {
        let mut engine = Engine::new(&EngineConfig::default());
        frame(&mut engine);
        assert!(!engine.has_level());
        assert!(
            engine.ui.system.is_visible(MENU_LAYER),
            "the main menu is up"
        );

        engine.console.execute("newgame");
        frame(&mut engine);
        frame(&mut engine);
        assert_eq!(engine.map_name(), Some(crate::base::FALLBACK_MAP));
        assert!(
            !engine.ui.system.is_visible(MENU_LAYER),
            "and gone with a map"
        );

        engine.console.execute("disconnect");
        frame(&mut engine);
        assert!(!engine.has_level());
        assert!(engine.entities.is_empty());
        assert!(engine.ui.system.is_visible(MENU_LAYER), "back again");
    }

    #[test]
    fn a_window_draws_the_loading_screen_for_a_frame_first() {
        let mut engine = Engine::new(&EngineConfig::default());
        engine.set_loading_screen(true);
        engine
            .console
            .execute(&format!("map {}", crate::base::FALLBACK_MAP));
        // The request is taken after the frame it was typed in.
        frame(&mut engine);
        frame(&mut engine);
        assert!(
            !engine.has_level(),
            "not yet: the loading screen is drawn first"
        );
        assert!(engine.ui.system.is_visible(super::LOADING_LAYER));
        frame(&mut engine);
        assert!(engine.has_level());
        assert!(!engine.ui.system.is_visible(super::LOADING_LAYER));
    }

    #[test]
    fn a_map_that_will_not_load_says_so_on_the_menu() {
        let mut engine = Engine::new(&EngineConfig::default());
        engine.console.execute("map no_such_map");
        frame(&mut engine);
        frame(&mut engine);
        assert!(!engine.has_level());
        let message = engine.ui.store.get("error.message").map(|v| v.to_string());
        assert!(
            message
                .as_deref()
                .is_some_and(|m| m.contains("no_such_map")),
            "{message:?}"
        );
    }

    /// Every page of both menus, with saves to list: nothing the layouts
    /// bind to or call may be an error.
    #[test]
    fn the_menus_load_every_page_without_complaint() {
        let dir = std::env::temp_dir().join(format!("kerosene-menus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = EngineConfig::default().with_user_dir(Some(dir.clone()));
        let mut engine = Engine::new(&config);
        engine.load_map(crate::base::FALLBACK_MAP).unwrap();
        engine.save_game("first").unwrap();
        engine.save_game("second").unwrap();

        let complaints = |engine: &Engine| -> Vec<String> {
            engine
                .console
                .log()
                .filter(|l| {
                    matches!(
                        l.level,
                        kerosene_console::LogLevel::Warning | kerosene_console::LogLevel::Error
                    ) && l.text.starts_with("ui:")
                })
                .map(|l| l.text.clone())
                .collect()
        };
        let pages = |engine: &mut Engine, pages: &[&str]| {
            for page in pages {
                engine.ui.store.set("ui.page", *page);
                engine.ui_frame(1.0 / 60.0, (1280, 720));
                engine.ui_frame(1.0 / 60.0, (1280, 720));
            }
        };

        assert!(engine.toggle_pause_menu());
        // Ending on the save page, whose list is the first in the layout.
        pages(&mut engine, &["main", "load", "options", "save"]);
        let menu = engine.ui.system.document(MENU_LAYER).unwrap();
        // Newest first, by the name each row's label is bound to.
        let save = menu
            .find_by_attr("text", "second")
            .expect("the saves are listed");
        assert!(menu.is_drawn(save));

        engine.console.execute("disconnect");
        frame(&mut engine);
        assert!(engine.ui.system.is_visible(MENU_LAYER));
        pages(&mut engine, &["main", "load", "options"]);
        assert!(
            engine
                .ui
                .system
                .document(MENU_LAYER)
                .and_then(|d| d.find("new-game"))
                .is_some(),
            "the main menu, not the pause menu"
        );
        assert_eq!(complaints(&engine), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
