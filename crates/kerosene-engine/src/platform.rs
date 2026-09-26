// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The engine's side of the store: Steam, or nothing.
//!
//! [`kerosene_platform`] owns the store; this is where the rest of the
//! engine meets it. Actions arrive from four places -- entity requests
//! (`logic_achievement` and friends), map scripts, UI scripts and the console
//! -- and all go through [`kerosene_platform::Platform::apply`]. Events come
//! back out through [`Engine::dispatch_platform_events`], which fires the
//! entity outputs wired to them, publishes the result to the UI store,
//! calls a map script's `on_platform_event`, and tells the game.
//!
//! Results fire on every entity of the class that names the same thing, not
//! only on the one that asked. An achievement a script awards still fires the
//! `logic_achievement` for it, and a leaderboard result that arrives a second
//! later from Steam's servers does not need to remember who was waiting.
//!
//! The store is pumped once a frame by the host ([`Engine::platform_frame`]),
//! not once a tick, because Steam's callbacks -- the overlay opening, above
//! all -- have to keep flowing while the game is paused.

use crate::engine::Engine;
use kerosene_entity::EntityId;
use kerosene_platform::{PlatformAction, PlatformEvent};

/// Console requests this module answers.
pub mod requests {
    pub const STATUS: &str = "platform_status";
    pub const ACTION: &str = "platform";
    pub const UNLOCK: &str = "achievement_unlock";
    pub const CLEAR: &str = "achievement_clear";
    pub const LIST: &str = "achievement_list";
    pub const STAT_SET: &str = "stat_set";
}

/// How many rounds of events one dispatch follows. An event's handler may
/// cause another -- a stat reaching a threshold awards an achievement -- and
/// this bounds a loop somebody wired by accident.
const MAX_ROUNDS: usize = 16;

impl Engine {
    pub fn platform(&self) -> &kerosene_platform::Platform {
        &self.platform
    }

    pub fn platform_mut(&mut self) -> &mut kerosene_platform::Platform {
        &mut self.platform
    }

    /// Ask the store for something, and act on what comes of it now.
    pub fn platform_apply(&mut self, action: &PlatformAction) -> Result<(), String> {
        let result = self.platform.apply(action);
        self.dispatch_platform_events();
        result
    }

    /// Apply for a caller with nowhere to report to but the console: a
    /// refusal is said there, once.
    pub(crate) fn platform_request(&mut self, action: &PlatformAction) {
        if let Some(e) = self.platform.apply_once(action) {
            self.console.warn(format!("platform: {e}"));
        }
    }

    /// Pump the store once a frame: its callbacks, batched stats, and the
    /// events that came of them.
    pub fn platform_frame(&mut self, dt: f32) {
        self.platform.frame(f64::from(dt));
        self.dispatch_platform_events();
    }

    /// Hand every pending store event to whoever listens: entities, the UI,
    /// the map's script and the game.
    pub fn dispatch_platform_events(&mut self) {
        for _ in 0..MAX_ROUNDS {
            let events = self.platform.take_events();
            if events.is_empty() {
                return;
            }
            for event in events {
                self.platform_event(&event);
            }
        }
        log::warn!("platform: events kept causing events; stopped after {MAX_ROUNDS} rounds");
    }

    fn platform_event(&mut self, event: &PlatformEvent) {
        match event {
            PlatformEvent::AchievementUnlocked(id) => {
                self.console.print(format!("achievement unlocked: {id}"));
                for e in self.with_key("logic_achievement", "achievement", id) {
                    self.entities.fire_output(e, "OnUnlocked", None, None);
                }
            }
            PlatformEvent::StatChanged { name, old, new } => {
                for e in self.with_key("logic_stat", "stat", name) {
                    let value = format_number(*new);
                    self.entities
                        .fire_output(e, "OnChanged", None, Some(&value));
                    let Some(entity) = self.entities.get(e) else {
                        continue;
                    };
                    let threshold = f64::from(entity.fields.f32("threshold", 0.0));
                    let achievement = field_text(&entity.fields, "achievement");
                    if threshold <= 0.0 {
                        continue;
                    }
                    let reached = *old < threshold && *new >= threshold;
                    if reached {
                        self.entities.fire_output(e, "OnThreshold", None, None);
                    }
                    if !achievement.is_empty() && *new > *old {
                        // Progress on the way, the award at the end:
                        // `Progress` turns into an unlock once it arrives.
                        self.platform_request(&PlatformAction::Progress {
                            id: achievement,
                            current: new.max(0.0) as u32,
                            max: threshold.ceil() as u32,
                        });
                    }
                }
            }
            PlatformEvent::ScoreSubmitted {
                board,
                score,
                rank,
                improved,
            } => {
                for e in self.with_key("logic_leaderboard", "leaderboard", board) {
                    self.entities
                        .fire_output(e, "OnSubmitted", None, Some(&score.to_string()));
                    if *improved {
                        self.entities.fire_output(
                            e,
                            "OnRankImproved",
                            None,
                            Some(&rank.to_string()),
                        );
                    }
                }
            }
            PlatformEvent::ScoreFailed { board } => {
                for e in self.with_key("logic_leaderboard", "leaderboard", board) {
                    self.entities.fire_output(e, "OnFailed", None, None);
                }
            }
            PlatformEvent::DlcChecked { appid, owned } => {
                let id = appid.to_string();
                let asking: Vec<EntityId> = self
                    .entities
                    .iter()
                    .filter(|e| e.classname.eq_ignore_ascii_case("logic_platform"))
                    .filter(|e| {
                        field_text(&e.fields, "dlc") == id
                            || e.fields.i32("__checking", 0) as u32 == *appid
                    })
                    .map(|e| e.id)
                    .collect();
                let output = if *owned {
                    "OnDlcOwned"
                } else {
                    "OnDlcNotOwned"
                };
                for e in asking {
                    if let Some(entity) = self.entities.get_mut(e) {
                        entity
                            .fields
                            .set("__checking", kerosene_entity::Value::Int(0));
                    }
                    self.entities.fire_output(e, output, None, Some(&id));
                }
            }
            PlatformEvent::OverlayChanged(open) => {
                let output = if *open {
                    "OnOverlayOpened"
                } else {
                    "OnOverlayClosed"
                };
                for e in self.of_class("logic_platform") {
                    self.entities.fire_output(e, output, None, None);
                }
                // Valve asks that a game pause under its overlay; the pause
                // menu is how this one pauses, and it stays open after the
                // overlay closes so the player comes back to a choice rather
                // than to whatever was shooting at them.
                if *open && self.level.is_some() && !self.ui_wants_input() {
                    self.toggle_pause_menu();
                }
            }
            PlatformEvent::AchievementCleared(_)
            | PlatformEvent::AchievementProgress { .. }
            | PlatformEvent::WorkshopMounted { .. } => {}
        }

        self.publish_platform_state();
        self.ui_emit(event.name(), event.data());
        if self
            .script
            .has_function(kerosene_script::hooks::PLATFORM_EVENT)
        {
            self.call_script_hook(
                kerosene_script::hooks::PLATFORM_EVENT,
                vec![
                    rhai::Dynamic::from(event.name().to_string()),
                    rhai::Dynamic::from(event.data()),
                ],
            );
        }
        self.with_game_mut(|game, engine| game.platform_event(engine, event));
    }

    fn of_class(&self, class: &str) -> Vec<EntityId> {
        self.entities
            .iter()
            .filter(|e| e.classname.eq_ignore_ascii_case(class))
            .map(|e| e.id)
            .collect()
    }

    /// Entities of `class` whose `key` is `value`.
    fn with_key(&self, class: &str, key: &str, value: &str) -> Vec<EntityId> {
        self.entities
            .iter()
            .filter(|e| e.classname.eq_ignore_ascii_case(class))
            .filter(|e| field_text(&e.fields, key) == value)
            .map(|e| e.id)
            .collect()
    }

    /// Publish what the UI binds to under `platform.`: see
    /// `kerosene_ui::script::platform_view` for the keys.
    pub(crate) fn publish_platform_state(&mut self) {
        let view = self.platform.view();
        let store = &mut self.ui.store;
        store.set("platform.available", view.available);
        store.set("platform.name", view.name);
        store.set("platform.user", view.user);
        store.set("platform.language", view.language);
        store.set("platform.overlay", view.overlay);
        for (id, on) in view.achievements {
            store.set(&format!("platform.achievements.{id}"), on);
        }
        for (id, name) in &self.platform.config().achievements {
            if !name.is_empty() {
                store.set(&format!("platform.names.{id}"), name.as_str());
            }
        }
        for (name, value) in view.stats {
            store.set(&format!("platform.stats.{name}"), value);
        }
        for (id, owned) in view.dlc {
            store.set(&format!("platform.dlc.{id}"), owned);
        }
    }

    /// An entity's store request. `false` if it is not one.
    pub(crate) fn platform_entity_request(
        &mut self,
        kind: &str,
        payload: &str,
        caller: EntityId,
    ) -> bool {
        use kerosene_entity::host_requests as hr;
        match kind {
            hr::PLATFORM => match PlatformAction::parse(payload) {
                Ok(action) => self.platform_request(&action),
                Err(e) => {
                    let class = self
                        .entities
                        .get(caller)
                        .map(|e| e.classname.clone())
                        .unwrap_or_default();
                    self.console.warn(format!("{class}: {e}"));
                }
            },
            hr::PLATFORM_STATUS => {
                let output = if self.platform.available() {
                    "OnAvailable"
                } else {
                    "OnUnavailable"
                };
                self.entities.fire_output(caller, output, None, None);
            }
            _ => return false,
        }
        true
    }

    /// A console command's request. `false` if it is not one.
    pub(crate) fn platform_console_request(&mut self, kind: &str, payload: &str) -> bool {
        let payload = payload.trim();
        let action = match kind {
            requests::STATUS => {
                let p = &self.platform;
                let mut lines = vec![format!(
                    "platform: {} ({})",
                    p.name(),
                    if p.available() {
                        "connected"
                    } else {
                        "not connected; achievements and stats are kept for this session"
                    }
                )];
                if let Some(appid) = p.config().steam_appid {
                    lines.push(format!("  steam app id {appid}"));
                }
                if !kerosene_platform::STEAM_BUILT_IN {
                    lines.push("  this build has no Steam support".to_string());
                }
                lines.push(format!("  user {}, language {}", p.user(), p.language()));
                lines.push(format!(
                    "  {} achievements, {} stats, {} DLC declared; cloud {}",
                    p.achievement_ids().len(),
                    p.config().stats.len(),
                    p.config().dlc.len(),
                    if p.cloud_enabled() { "on" } else { "off" }
                ));
                for line in lines {
                    self.console.print(line);
                }
                return true;
            }
            requests::LIST => {
                let view = self.platform.view();
                if view.achievements.is_empty() {
                    self.console.print("no achievements declared");
                }
                for (id, on) in view.achievements {
                    let name = self
                        .platform
                        .config()
                        .achievements
                        .iter()
                        .find(|(a, _)| *a == id)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    self.console
                        .print(format!("  [{}] {id} {name}", if on { "x" } else { " " }));
                }
                for (name, value) in view.stats {
                    self.console
                        .print(format!("  {name} = {}", format_number(value)));
                }
                return true;
            }
            requests::UNLOCK => Ok(PlatformAction::Unlock(payload.to_string())),
            requests::CLEAR => Ok(PlatformAction::Clear(payload.to_string())),
            requests::STAT_SET => PlatformAction::parse(&format!("set_stat {payload}")),
            requests::ACTION => PlatformAction::parse(payload),
            _ => return false,
        };
        match action {
            Ok(action) => {
                if let Err(e) = self.platform_apply(&action) {
                    self.console.warn(e);
                }
            }
            Err(e) => self.console.warn(e),
        }
        true
    }
}

/// A number as a designer wrote it: `3`, not `3.0`.
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

fn field_text(fields: &kerosene_entity::Fields, key: &str) -> String {
    fields
        .text(key)
        .map(|t| t.trim().to_string())
        .unwrap_or_default()
}

pub(crate) fn register(console: &mut kerosene_console::Console) {
    use kerosene_console::ConVarFlags;
    console.register_command(
        requests::STATUS,
        ConVarFlags::NONE,
        "Which store the game is connected to, and what it declares.",
        |con, args| con.request(requests::STATUS, args.rest.clone()),
    );
    console.register_command(
        requests::LIST,
        ConVarFlags::NONE,
        "Every declared achievement and whether it is unlocked, and every stat.",
        |con, args| con.request(requests::LIST, args.rest.clone()),
    );
    let cheats = [
        (
            requests::UNLOCK,
            "Award an achievement: achievement_unlock <id>",
        ),
        (
            requests::CLEAR,
            "Take an achievement back: achievement_clear <id>",
        ),
        (requests::STAT_SET, "Set a stat: stat_set <name> <value>"),
        (
            requests::ACTION,
            "Any store action: platform unlock <id> | add_stat <name> [n] | score <board> <n> [asc] | presence <key> <value> | overlay [dialog] | url <url> | store [appid] | dlc <appid>",
        ),
    ];
    for (name, help) in cheats {
        console.register_command(name, ConVarFlags::CHEAT, help, move |con, args| {
            con.request(name, args.rest.clone());
        });
    }
}
