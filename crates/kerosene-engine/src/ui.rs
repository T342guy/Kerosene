// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The game UI, wired to the engine.
//!
//! `kerosene-ui` is a UI with no game in it; this is where it meets one. The
//! engine owns a [`GameUi`] -- the store every layout binds to, the layers and
//! world panels, and the decals placed so far -- and does four things with it:
//!
//! * **Publishes** what it knows each tick: `player.health`, `player.alive`,
//!   `player.speed`, `map.name`, `time`, and `cvar.<name>` for every convar a
//!   layout reads. A game publishes its own with [`Engine::ui_set`].
//! * **Runs** the documents once a frame ([`Engine::ui_frame`]) and carries
//!   out what they ask: console commands, convar changes (through the console,
//!   so cheat flags hold), sounds, and events -- which reach the game through
//!   [`Game::ui_event`](crate::Game::ui_event) and, from a world panel, fire
//!   that panel entity's outputs.
//! * **Places world panels** from `point_worldpanel` entities, and turns the
//!   player aiming at one and pressing use into clicks on it.
//! * **Keeps the decal list**: requests from weapons, `infodecal` and the
//!   `decal` command, capped by `r_decals`. Cutting them out of the world's
//!   geometry needs the meshes, which only the host has; the engine keeps the
//!   list so a headless run agrees with a windowed one about what was placed.
//!
//! All of it runs headless. Only [`Engine::ui_frame`] needs a viewport, and a
//! test can hand it any size it likes.

use crate::engine::Engine;
use kerosene_bsp::contents;
use kerosene_entity::EntityId;
use kerosene_math::Vec3;
use kerosene_ui::{LogLevel, UiAction, UiInput, UiStore, UiSystem, Value};
use std::collections::VecDeque;

/// Console request kinds the UI's commands leave.
pub mod requests {
    pub const RELOAD: &str = "ui_reload";
    pub const SHOW: &str = "ui_show";
    pub const HIDE: &str = "ui_hide";
    pub const TOGGLE: &str = "ui_toggle";
    pub const SET: &str = "ui_set";
    pub const EMIT: &str = "ui_emit";
    pub const DUMP: &str = "ui_dump";
    pub const DECAL: &str = "decal";
    pub const CLEAR_DECALS: &str = "r_cleardecals";
}

/// The layer the pause menu goes on.
pub const MENU_LAYER: &str = "menu";
/// The layer the HUD goes on.
pub const HUD_LAYER: &str = "hud";

/// A decal somebody asked for.
#[derive(Clone, PartialEq, Debug)]
pub struct DecalRequest {
    /// Unique for the engine's lifetime, so the host can remember which it
    /// has already cut out of the world.
    pub id: u64,
    pub material: String,
    pub origin: Vec3,
    pub normal: Vec3,
    pub size: f32,
    pub rotation: f32,
}

/// Every decal placed, oldest first, up to `r_decals`.
#[derive(Default, Debug)]
pub struct Decals {
    pub list: VecDeque<DecalRequest>,
    /// Bumped on every change, so the host re-uploads only when something
    /// moved.
    pub revision: u64,
    /// Total placed since the map loaded, including ones since dropped: the
    /// host uses it to know which it has already cut.
    pub placed: u64,
}

impl Decals {
    pub fn push(&mut self, mut decal: DecalRequest, cap: usize) {
        decal.id = self.placed;
        self.list.push_back(decal);
        self.placed += 1;
        while self.list.len() > cap {
            self.list.pop_front();
        }
        self.revision += 1;
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.revision += 1;
    }
}

/// A world panel as the level placed it.
#[derive(Clone, PartialEq, Debug)]
pub struct WorldPanel {
    pub entity: EntityId,
    /// The name the UI knows it by.
    pub name: String,
    pub layout: String,
    /// Texture size in pixels.
    pub pixels: (u32, u32),
    /// Top-left, top-right, bottom-left, bottom-right.
    pub corners: [Vec3; 4],
    pub normal: Vec3,
    pub brightness: f32,
    pub interactive: bool,
}

impl WorldPanel {
    /// Where a ray meets the panel, in the panel's pixels.
    pub fn hit(&self, start: Vec3, dir: Vec3) -> Option<(f32, (f32, f32))> {
        let denom = dir.dot(self.normal);
        if denom.abs() < 1e-5 {
            return None;
        }
        let t = (self.corners[0] - start).dot(self.normal) / denom;
        if t < 0.0 {
            return None;
        }
        let p = start + dir * t;
        let across = self.corners[1] - self.corners[0];
        let down = self.corners[2] - self.corners[0];
        let u = (p - self.corners[0]).dot(across) / across.length_squared();
        let v = (p - self.corners[0]).dot(down) / down.length_squared();
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return None;
        }
        Some((t, (u * self.pixels.0 as f32, v * self.pixels.1 as f32)))
    }
}

/// The engine's UI state.
pub struct GameUi {
    pub store: UiStore,
    pub system: UiSystem,
    pub decals: Decals,
    pub panels: Vec<WorldPanel>,
    /// The panel the player is aiming at, if any.
    pub aimed_panel: Option<String>,
    /// Anything a document asked for that the host must do: the engine has
    /// no window to release the mouse from.
    pub host_actions: Vec<UiAction>,
    /// Set by `ui_reload`: the host should load its images again too.
    pub images_stale: bool,
    /// World panels whose layout would not load, as `name layout`.
    failed_panels: std::collections::BTreeSet<String>,
}

impl Default for GameUi {
    fn default() -> Self {
        GameUi {
            store: UiStore::new(),
            system: UiSystem::new(),
            decals: Decals::default(),
            panels: Vec::new(),
            aimed_panel: None,
            host_actions: Vec::new(),
            images_stale: false,
            failed_panels: Default::default(),
        }
    }
}

impl Engine {
    /// Publish a value for the UI to bind to.
    pub fn ui_set(&mut self, key: &str, value: impl Into<Value>) {
        self.ui.store.set(key, value);
    }

    /// Send every UI document an event.
    pub fn ui_emit(&mut self, name: &str, data: impl Into<String>) {
        self.ui.store.emit(name, data);
    }

    /// Show a layout on a layer, saying so if it would not load.
    pub fn ui_show(&mut self, layer: &str, path: &str) -> bool {
        let vfs = self.vfs.clone();
        match self.ui.system.show(layer, path, vfs.as_ref()) {
            Ok(()) => true,
            Err(e) => {
                self.console.error(format!("ui: {e}"));
                false
            }
        }
    }

    pub fn ui_hide(&mut self, layer: &str) {
        self.ui.system.hide(layer);
    }

    /// Whether a UI layer wants the mouse and keyboard: a menu is open.
    pub fn ui_wants_input(&self) -> bool {
        self.ui.system.wants_input()
    }

    /// Screen input for the UI. `true` if a layer took it.
    pub fn ui_input(&mut self, input: UiInput) -> bool {
        self.ui.system.input(input)
    }

    /// Open the pause menu, or close it if it is open. `false` when there is
    /// no menu to open (no map, or the file is missing).
    pub fn toggle_pause_menu(&mut self) -> bool {
        if self.ui.system.is_visible(MENU_LAYER) {
            self.ui.system.hide(MENU_LAYER);
            return true;
        }
        if self.level.is_none() {
            return false;
        }
        let path = self.console.string("ui_pausemenu").to_string();
        if path.is_empty() {
            return false;
        }
        self.ui_show(MENU_LAYER, &path)
    }

    /// Place a decal. `origin` on the surface, `normal` out of it.
    pub fn place_decal(&mut self, material: &str, origin: Vec3, normal: Vec3, size: f32) {
        let cap = self.console.int("r_decals").max(0) as usize;
        if cap == 0 || size <= 0.0 {
            return;
        }
        // Turned a little each time, so a spray of bullet holes does not look
        // stamped.
        let rotation = (self.ui.decals.placed as f32 * 137.5) % 360.0;
        self.ui.decals.push(
            DecalRequest {
                id: 0,
                material: material.to_string(),
                origin,
                normal,
                size,
                rotation,
            },
            cap,
        );
    }

    /// Project a decal onto whatever surface is nearest `origin`, within
    /// `reach`: what `infodecal` does.
    pub fn place_decal_near(
        &mut self,
        material: &str,
        origin: Vec3,
        size: f32,
        reach: f32,
    ) -> bool {
        let Some(level) = &self.level else {
            return false;
        };
        let world = crate::LevelCollision::new(&level.bsp, &self.entities);
        let mut best: Option<(f32, Vec3, Vec3)> = None;
        for dir in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
            let t = world.trace(
                origin,
                origin + dir * reach,
                Vec3::ZERO,
                Vec3::ZERO,
                contents::MASK_SOLID,
            );
            if t.fraction < 1.0
                && best.is_none_or(|b| t.fraction < b.0)
                && let Some(plane) = t.plane
            {
                best = Some((t.fraction, t.endpos, plane.normal));
            }
        }
        match best {
            Some((_, at, normal)) => {
                self.place_decal(material, at, normal, size);
                true
            }
            None => false,
        }
    }

    /// Trace from the eye along the view, offset by `(yaw, pitch)` degrees:
    /// what a shot or a `decal` command hits.
    pub fn trace_view(&self, offset: (f32, f32), range: f32) -> Option<(Vec3, Vec3, f32)> {
        let level = self.level.as_ref()?;
        let mut angles = self.player.view_angles;
        angles.yaw += offset.0;
        angles.pitch += offset.1;
        let eye = self.player.movement.eye_position();
        let end = eye + angles.forward() * range;
        let world = crate::LevelCollision::new(&level.bsp, &self.entities);
        let t = world.trace(eye, end, Vec3::ZERO, Vec3::ZERO, contents::MASK_SOLID);
        if t.fraction >= 1.0 {
            return None;
        }
        let normal = t.plane.map_or(-angles.forward(), |p| p.normal);
        Some((t.endpos, normal, t.fraction * range))
    }

    /// Publish what the engine knows. Every tick, and again each frame.
    pub(crate) fn publish_ui_state(&mut self) {
        let alive = self.player.health > 0.0 && self.player.entity.is_some();
        let speed = {
            let v = self.player.movement.velocity;
            (v.x * v.x + v.y * v.y).sqrt()
        };
        let origin = self.player.movement.origin;
        let map = self
            .level
            .as_ref()
            .map(|l| l.name.clone())
            .unwrap_or_default();
        let store = &mut self.ui.store;
        store.set("player.health", self.player.health.max(0.0).round());
        store.set("player.alive", alive);
        store.set("player.speed", speed.round());
        store.set("player.x", origin.x.round());
        store.set("player.y", origin.y.round());
        store.set("player.z", origin.z.round());
        store.set("map.name", map);
        store.set("ui.menu_open", self.ui.system.is_visible(MENU_LAYER));

        // Only the convars something reads: publishing all two hundred would
        // wake every binding on `cvar` whenever any one of them changed.
        for key in self.ui.system.dependencies() {
            let Some(name) = key.strip_prefix("cvar.") else {
                continue;
            };
            if let Some(var) = self.console.cvar(name) {
                let value = Value::parse(var.string());
                self.ui.store.set(&key, value);
            }
        }
    }

    /// Run the UI for one frame at `viewport` physical pixels.
    pub fn ui_frame(&mut self, dt: f32, viewport: (u32, u32)) {
        self.ui.system.debug = self.console.bool("ui_debug");
        self.ui.system.hot_reload = self.console.bool("ui_hotreload");
        if self.level.is_some() {
            self.ensure_hud();
        } else {
            self.ui.system.hide(HUD_LAYER);
            self.ui.system.hide(MENU_LAYER);
        }
        self.sync_world_panels();
        self.publish_ui_state();

        let vfs = self.vfs.clone();
        let actions = self
            .ui
            .system
            .update(dt, viewport, &mut self.ui.store, vfs.as_ref());
        for (level, message) in self.ui.system.take_messages() {
            match level {
                LogLevel::Info => self.console.developer(format!("ui: {message}")),
                LogLevel::Warn => self.console.warn(format!("ui: {message}")),
                LogLevel::Error => self.console.error(format!("ui: {message}")),
            }
        }
        for action in actions {
            self.apply_ui_action(action);
        }
    }

    fn apply_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::Command(text) => self.console.enqueue(text),
            // Through the console, as typed, so a cheat convar stays a cheat.
            UiAction::SetCvar { name, value } => {
                self.console.execute(&format!("{name} \"{value}\""))
            }
            UiAction::PlaySound(name) => {
                let vfs = self.vfs.clone();
                if self.audio.play(&vfs, &name, None, 1.0).is_none() {
                    self.console
                        .developer(format!("ui: could not play `{name}`"));
                }
            }
            UiAction::Emit { name, data, source } => {
                if let Some(panel) = source.strip_prefix("panel:")
                    && let Some(id) = self
                        .ui
                        .panels
                        .iter()
                        .find(|p| p.name == panel)
                        .map(|p| p.entity)
                {
                    self.entities
                        .fire_output(id, "OnPanelEvent", None, Some(&name));
                    if name.starts_with("On") {
                        self.entities.fire_output(id, &name, None, Some(&data));
                    }
                }
                self.with_game_mut(|game, engine| game.ui_event(engine, &name, &data, &source));
            }
            UiAction::Log(_, text) => self.console.print(text),
            other => self.ui.host_actions.push(other),
        }
    }

    /// Bring the UI's world panels in line with the level's
    /// `point_worldpanel` entities.
    fn sync_world_panels(&mut self) {
        let mut panels = Vec::new();
        for id in self.entities.find_by_class("point_worldpanel") {
            let Some(e) = self.entities.get(id) else {
                continue;
            };
            if e.fields
                .bool("disabled", e.fields.bool("startdisabled", false))
            {
                continue;
            }
            let layout = e
                .fields
                .text("layout")
                .map(|t| t.into_owned())
                .unwrap_or_default();
            if layout.is_empty() {
                continue;
            }
            let width = e.fields.f32("width", 32.0).max(1.0);
            let height = e.fields.f32("height", 32.0).max(1.0);
            let tall = e.fields.i32("resolution", 512).clamp(16, 2048) as u32;
            let wide = ((tall as f32 * width / height).round() as u32).clamp(16, 4096);
            let basis = e.angles.vectors();
            // The panel faces along its forward vector; its right is the
            // viewer's right when looking at it.
            let right = -basis.right;
            let right = if right.length_squared() > 0.0 {
                right
            } else {
                Vec3::Y
            };
            let up = basis.up;
            let (hw, hh) = (right * width * 0.5, up * height * 0.5);
            let o = e.origin;
            panels.push(WorldPanel {
                entity: id,
                name: format!("{}_{}", e.targetname().unwrap_or("panel"), id.index),
                layout,
                pixels: (wide, tall),
                corners: [o - hw + hh, o + hw + hh, o - hw - hh, o + hw - hh],
                normal: basis.forward,
                brightness: e.fields.f32("brightness", 1.0).max(0.0),
                interactive: e.fields.bool("interactive", false),
            });
        }
        let vfs = self.vfs.clone();
        for p in &panels {
            // A layout that would not load is tried once, not every frame:
            // `ui_reload` or the next map tries it again.
            let key = format!("{} {}", p.name, p.layout);
            if self.ui.failed_panels.contains(&key) {
                continue;
            }
            if let Err(e) = self
                .ui
                .system
                .set_panel(&p.name, &p.layout, p.pixels, vfs.as_ref())
            {
                self.console.error(format!("point_worldpanel: {e}"));
                self.ui.failed_panels.insert(key);
            }
        }
        let gone: Vec<String> = self
            .ui
            .system
            .panels()
            .filter(|name| !panels.iter().any(|p| p.name == *name))
            .map(str::to_string)
            .collect();
        for name in gone {
            self.ui.system.remove_panel(&name);
        }
        self.ui.panels = panels;
    }

    /// Point the player's view at interactive world panels; a press of use
    /// or attack on one is a click. Returns whether the press was taken, so
    /// the tick does not also use or fire at what is behind it.
    pub(crate) fn world_panel_input(
        &mut self,
        use_down: bool,
        attack_down: bool,
        use_edge: bool,
        attack_edge: bool,
    ) -> bool {
        let eye = self.player.movement.eye_position();
        let dir = self.player.view_angles.forward();
        let range = self.console.float("sv_use_range").max(1.0) * 1.5;
        let mut best: Option<(f32, String, (f32, f32))> = None;
        for p in self.ui.panels.iter().filter(|p| p.interactive) {
            if let Some((t, px)) = p.hit(eye, dir)
                && t <= range
                && best.as_ref().is_none_or(|b| t < b.0)
            {
                best = Some((t, p.name.clone(), px));
            }
        }
        // A wall between the eye and the panel hides it.
        if let (Some((t, _, _)), Some(level)) = (&best, &self.level) {
            let world = crate::LevelCollision::new(&level.bsp, &self.entities);
            let trace = world.trace(
                eye,
                eye + dir * *t,
                Vec3::ZERO,
                Vec3::ZERO,
                contents::MASK_SOLID,
            );
            if trace.fraction < 0.98 {
                best = None;
            }
        }
        let aimed = best.as_ref().map(|b| b.1.clone());
        if aimed != self.ui.aimed_panel
            && let Some(old) = self.ui.aimed_panel.take()
        {
            self.ui.system.panel_leave(&old);
        }
        self.ui.aimed_panel = aimed;
        let Some((_, name, (x, y))) = best else {
            return false;
        };
        self.ui
            .system
            .panel_input(&name, UiInput::PointerMove { x, y });
        let down = use_down || attack_down;
        if use_edge || attack_edge {
            self.ui
                .system
                .panel_input(&name, UiInput::PointerButton { down: true });
            return true;
        }
        if !down {
            self.ui
                .system
                .panel_input(&name, UiInput::PointerButton { down: false });
        }
        // Holding the button on a panel is still the panel's.
        down
    }

    /// Whether the player was aiming at an interactive world panel last tick,
    /// so a game does not fire a weapon at a keypad being pressed.
    pub fn aiming_at_panel(&self) -> bool {
        self.ui.aimed_panel.is_some()
    }

    /// Show the HUD the `ui_hud` convar names, if it is not up.
    fn ensure_hud(&mut self) {
        let path = self.console.string("ui_hud").to_string();
        if path.is_empty() {
            self.ui.system.hide(HUD_LAYER);
            return;
        }
        let current = self
            .ui
            .system
            .layers()
            .find(|(n, _, _)| *n == HUD_LAYER)
            .map(|(_, p, v)| (p.to_string(), v));
        if current.as_ref().is_some_and(|(p, v)| *p == path && *v) {
            return;
        }
        // Once per path: a HUD that will not load should say so once, not
        // every frame.
        if current.as_ref().is_some_and(|(p, _)| *p == path) {
            return;
        }
        if self.vfs.exists(&path) {
            self.ui_show(HUD_LAYER, &path);
        }
    }

    /// A new map: its decals and panels were the old map's.
    pub(crate) fn ui_map_loaded(&mut self) {
        self.ui.decals.clear();
        self.ui.aimed_panel = None;
        self.ui.system.clear_panels();
        self.ui.failed_panels.clear();
        self.ui.system.hide(MENU_LAYER);
        self.ui_emit(
            "map_loaded",
            self.level
                .as_ref()
                .map(|l| l.name.clone())
                .unwrap_or_default(),
        );
    }

    /// Handle one of the UI's console requests. `false` if it is not one.
    pub(crate) fn ui_console_request(&mut self, kind: &str, payload: &str) -> bool {
        let mut words = payload.split_whitespace();
        match kind {
            requests::RELOAD => {
                let vfs = self.vfs.clone();
                self.ui.system.reload_all(vfs.as_ref());
                self.ui.images_stale = true;
                self.ui.failed_panels.clear();
                self.console.print("ui: reloaded");
            }
            requests::SHOW => match (words.next(), words.next()) {
                (Some(layer), Some(file)) => {
                    self.ui_show(layer, file);
                }
                _ => self.console.warn("usage: ui_show <layer> <file>"),
            },
            requests::HIDE => self.ui_hide(payload.trim()),
            requests::TOGGLE => match (words.next(), words.next()) {
                (Some(layer), file) => {
                    if self.ui.system.is_visible(layer) {
                        self.ui_hide(layer);
                    } else if let Some(file) = file {
                        self.ui_show(layer, file);
                    } else if layer == MENU_LAYER {
                        self.toggle_pause_menu();
                    }
                }
                _ => self.console.warn("usage: ui_toggle <layer> [file]"),
            },
            requests::SET => match payload.trim().split_once(char::is_whitespace) {
                Some((key, value)) => self.ui_set(key, Value::parse(value.trim())),
                None => self.console.warn("usage: ui_set <key> <value>"),
            },
            requests::EMIT => {
                let (name, data) = payload
                    .trim()
                    .split_once(char::is_whitespace)
                    .unwrap_or((payload.trim(), ""));
                if name.is_empty() {
                    self.console.warn("usage: ui_emit <event> [data]");
                } else {
                    self.ui_emit(name, data.trim());
                }
            }
            requests::DUMP => {
                let lines: Vec<String> = self
                    .ui
                    .store
                    .iter()
                    .map(|(k, v)| format!("  {k} = {v}"))
                    .collect();
                self.console
                    .print(format!("ui store ({} values):", lines.len()));
                for line in lines {
                    self.console.print(line);
                }
                let layers: Vec<String> = self
                    .ui
                    .system
                    .layers()
                    .map(|(n, p, v)| format!("  {n}: {p}{}", if v { "" } else { " (hidden)" }))
                    .collect();
                for line in layers {
                    self.console.print(line);
                }
                if let Some(under) = self.ui.system.describe_pointer() {
                    self.console.print(format!("  under the pointer: {under}"));
                }
            }
            requests::DECAL => {
                let material = words.next().unwrap_or("decals/bullet").to_string();
                let size = words.next().and_then(|s| s.parse().ok()).unwrap_or(16.0);
                match self.trace_view((0.0, 0.0), 4096.0) {
                    Some((at, normal, _)) => self.place_decal(&material, at, normal, size),
                    None => self.console.warn("decal: nothing in front of you"),
                }
            }
            requests::CLEAR_DECALS => self.ui.decals.clear(),
            _ => return false,
        }
        true
    }

    /// Handle an entity's UI request. `false` if it is not one.
    pub(crate) fn ui_entity_request(
        &mut self,
        kind: &str,
        payload: &str,
        caller: EntityId,
    ) -> bool {
        use kerosene_entity::host_requests as hr;
        match kind {
            hr::UI_EMIT => {
                // Every document hears it, the HUD and every world panel
                // alike; a layout listens only for the names it cares about.
                let (name, data) = payload
                    .trim()
                    .split_once(char::is_whitespace)
                    .unwrap_or((payload.trim(), ""));
                let _ = caller;
                self.ui_emit(name, data.trim());
            }
            hr::UI_SET => match payload.trim().split_once(char::is_whitespace) {
                Some((key, value)) => self.ui_set(key, Value::parse(value.trim())),
                None => self.console.warn("logic_ui: SetValue needs <key> <value>"),
            },
            hr::UI_SHOW => match payload.split_whitespace().collect::<Vec<_>>().as_slice() {
                [layer, file] => {
                    self.ui_show(layer, file);
                }
                _ => self
                    .console
                    .warn("logic_ui: ShowLayer needs <layer> <file>"),
            },
            hr::UI_HIDE => self.ui_hide(payload.trim()),
            hr::PLACE_DECAL => {
                let mut words = payload.split_whitespace();
                let material = words.next().unwrap_or_default().to_string();
                let size = words.next().and_then(|s| s.parse().ok()).unwrap_or(32.0);
                let origin = self
                    .entities
                    .get(caller)
                    .map(|e| e.origin)
                    .unwrap_or_default();
                if !self.place_decal_near(&material, origin, size, 64.0) {
                    self.console.warn(format!(
                        "infodecal at {origin:.0}: no surface within 64 units"
                    ));
                }
            }
            _ => return false,
        }
        true
    }
}

pub(crate) fn register(console: &mut kerosene_console::Console) {
    use kerosene_console::ConVarFlags;
    console.register_cvar(
        "ui_hud",
        "ui/hud.keroui",
        ConVarFlags::NONE,
        "The HUD layout shown while a map is running. Empty for none.",
    );
    console.register_cvar(
        "ui_pausemenu",
        "ui/menus/pause.keroui",
        ConVarFlags::NONE,
        "The layout Escape opens over a running map. Empty to have Escape only free the mouse.",
    );
    console.register_cvar(
        "ui_debug",
        "0",
        ConVarFlags::NONE,
        "Outline every UI panel; ui_dump then names the one under the pointer.",
    );
    console.register_cvar(
        "ui_hotreload",
        "1",
        ConVarFlags::NONE,
        "Reload UI layouts, styles and scripts a second after their files change.",
    );
    console.register_cvar_ranged(
        "r_decals",
        "256",
        Some(0.0),
        Some(4096.0),
        ConVarFlags::ARCHIVE,
        "How many decals may exist at once; the oldest go first.",
    );
    let simple = [
        (
            requests::RELOAD,
            "Load every UI layout, style and script again from disk.",
        ),
        (
            requests::SHOW,
            "Show a layout on a layer: ui_show <layer> <file>",
        ),
        (requests::HIDE, "Hide a UI layer: ui_hide <layer>"),
        (
            requests::TOGGLE,
            "Show or hide a layer: ui_toggle <layer> [file]. ui_toggle menu opens the pause menu.",
        ),
        (
            requests::SET,
            "Publish a value to the UI: ui_set <key> <value>",
        ),
        (
            requests::EMIT,
            "Send the UI an event: ui_emit <event> [data]",
        ),
        (
            requests::DUMP,
            "Print every value the UI can bind to, and the layers.",
        ),
        (requests::CLEAR_DECALS, "Remove every decal."),
    ];
    for (name, help) in simple {
        console.register_command(name, ConVarFlags::NONE, help, move |con, args| {
            con.request(name, args.rest.clone());
        });
    }
    console.register_command(
        requests::DECAL,
        ConVarFlags::CHEAT,
        "Put a decal where you are looking: decal [material] [size]",
        |con, args| con.request(requests::DECAL, args.rest.clone()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decal_list_keeps_the_newest() {
        let mut d = Decals::default();
        for i in 0..5 {
            d.push(
                DecalRequest {
                    id: 0,
                    material: format!("d{i}"),
                    origin: Vec3::ZERO,
                    normal: Vec3::Z,
                    size: 4.0,
                    rotation: 0.0,
                },
                3,
            );
        }
        assert_eq!(d.list.len(), 3);
        assert_eq!(d.list[0].material, "d2");
        assert_eq!(d.placed, 5);
        assert_eq!(d.list[2].id, 4);
    }

    #[test]
    fn a_ray_meets_a_panel_in_its_pixels() {
        let p = WorldPanel {
            entity: EntityId {
                index: 0,
                generation: 0,
            },
            name: "p".into(),
            layout: String::new(),
            pixels: (200, 100),
            // A 20x10 panel in the x = 0 plane, facing -x.
            corners: [
                Vec3::new(0.0, 10.0, 10.0),
                Vec3::new(0.0, -10.0, 10.0),
                Vec3::new(0.0, 10.0, 0.0),
                Vec3::new(0.0, -10.0, 0.0),
            ],
            normal: -Vec3::X,
            brightness: 1.0,
            interactive: true,
        };
        let (t, (x, y)) = p.hit(Vec3::new(-50.0, 0.0, 5.0), Vec3::X).unwrap();
        assert!((t - 50.0).abs() < 1e-4);
        assert!((x - 100.0).abs() < 1e-3 && (y - 50.0).abs() < 1e-3);
        assert!(p.hit(Vec3::new(-50.0, 30.0, 5.0), Vec3::X).is_none());
    }
}
