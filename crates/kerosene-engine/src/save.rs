// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Saved games, and what survives a level change.
//!
//! A save is one JSON file, `save/<name>.kerosave`, holding everything a
//! level is between two ticks:
//!
//! | Part | What |
//! |---|---|
//! | `world` | Every entity's fields, wires and pose, and the I/O queue |
//! | `player` | Where they stand, how they move, their health |
//! | `props` | Each physics prop's velocity (its pose is in `world`) |
//! | `script` | Which script files were loaded, and their plain variables |
//! | `ui` | The UI store, the layers showing, the decals placed |
//! | `game` | Whatever [`Game::save`](crate::Game::save) returned |
//!
//! Loading builds the level exactly as a fresh load does -- geometry,
//! physics, streaming -- then puts the save's entities in place of the map's
//! instead of spawning them, so a `logic_auto` does not fire again and a
//! counter keeps its count. See [`kerosene_entity::WorldSnapshot`].
//!
//! JSON so a save can be read without a tool: by a player, a modder, a bug
//! report. It is also mirrored to the store's cloud when there is one, and
//! whichever copy is newer wins on load, which is how a save follows a
//! player to another machine.
//!
//! A level change (`changelevel`, `trigger_changelevel`) is a smaller
//! version of the same thing: the player's health, and the game's own state,
//! carried into a fresh map, placed relative to a shared `info_landmark` so
//! a corridor that crosses the seam is walked straight through.

use crate::engine::Engine;
use crate::ui::{DecalRequest, MENU_LAYER};
use kerosene_entity::{EntityId, WorldSnapshot};
use kerosene_math::{Angles, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// Where saves go, under the first writable search path.
pub const SAVE_DIR: &str = "save";
/// A save file's extension.
pub const EXTENSION: &str = "kerosave";
/// The layout this engine writes. A save from a newer engine is refused
/// rather than half-read.
pub const FORMAT: u32 = 1;
/// The name `quicksave` and `quickload` use.
pub const QUICK: &str = "quick";
/// The name a level change saves under, when `sv_autosave` is on.
pub const AUTO: &str = "auto";

/// Console requests this module answers.
pub mod requests {
    pub const SAVE: &str = "save";
    pub const LOAD: &str = "load";
    pub const QUICKSAVE: &str = "quicksave";
    pub const QUICKLOAD: &str = "quickload";
    pub const LIST: &str = "saves";
    pub const CHANGELEVEL: &str = "changelevel";
}

/// A saved game.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveGame {
    pub format: u32,
    pub map: String,
    /// A fingerprint of the map's entities as compiled, so loading a save
    /// into a map that has since been rebuilt can say so.
    #[serde(default)]
    pub map_hash: String,
    /// Seconds since the Unix epoch.
    #[serde(default)]
    pub saved_at: u64,
    pub time: f32,
    pub tick: u64,
    pub player: SavedPlayer,
    pub world: WorldSnapshot,
    #[serde(default)]
    pub props: Vec<SavedMotion>,
    #[serde(default)]
    pub script: SavedScripts,
    #[serde(default)]
    pub ui: SavedUi,
    #[serde(default)]
    pub game: Json,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedPlayer {
    pub origin: [f32; 3],
    pub velocity: [f32; 3],
    /// Pitch, yaw, roll.
    pub view: [f32; 3],
    pub health: f32,
    #[serde(default)]
    pub on_ground: bool,
    #[serde(default)]
    pub ducked: bool,
}

/// A physics prop's motion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedMotion {
    pub id: [u32; 2],
    pub linear: [f32; 3],
    pub angular: [f32; 3],
    pub awake: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedScripts {
    /// In the order they were loaded.
    pub files: Vec<String>,
    pub vars: serde_json::Map<String, Json>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedUi {
    pub store: BTreeMap<String, Json>,
    /// `[layer, layout]` for every layer showing but the menu.
    pub layers: Vec<[String; 2]>,
    pub decals: Vec<SavedDecal>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedDecal {
    pub material: String,
    pub origin: [f32; 3],
    pub normal: [f32; 3],
    pub size: f32,
    #[serde(default)]
    pub rotation: f32,
}

/// One save on disk, for a list.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveInfo {
    pub name: String,
    pub map: String,
    pub saved_at: u64,
}

/// Something to do at the start of the next frame, when nothing is
/// standing on the level.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingChange {
    Load(String),
    Level {
        map: String,
        landmark: Option<String>,
    },
}

/// What a level change carries across.
struct Carry {
    health: f32,
    game: Json,
    view: Angles,
    /// Where the player stood relative to the landmark, and how they moved.
    landmark: Option<(String, Vec3, Vec3, bool)>,
}

/// Whether a name is fit to be a file name everywhere a game runs.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !name.contains("..")
}

/// Where a save of this name lives in the file system.
pub fn save_path(name: &str) -> String {
    format!("{SAVE_DIR}/{name}.{EXTENSION}")
}

fn cloud_name(name: &str) -> String {
    format!("{name}.{EXTENSION}")
}

fn v3(v: Vec3) -> [f32; 3] {
    let f = |x: f32| if x.is_finite() { x } else { 0.0 };
    [f(v.x), f(v.y), f(v.z)]
}

fn handle([index, generation]: [u32; 2]) -> EntityId {
    EntityId { index, generation }
}

/// A short, stable fingerprint of some text (FNV-1a).
fn fingerprint(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn to_json(value: &kerosene_ui::Value) -> Json {
    use kerosene_ui::Value as V;
    match value {
        V::Bool(b) => Json::Bool(*b),
        V::Int(i) => Json::from(*i),
        V::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        V::Str(s) => Json::String(s.clone()),
    }
}

fn from_json(value: &Json) -> Option<kerosene_ui::Value> {
    use kerosene_ui::Value as V;
    Some(match value {
        Json::Bool(b) => V::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => V::Int(i),
            None => V::Float(n.as_f64()?),
        },
        Json::String(s) => V::Str(s.clone()),
        _ => return None,
    })
}

impl Engine {
    /// Write the running game to `save/<name>.kerosave`, and to the store's
    /// cloud if it has one. Returns where it went.
    pub fn save_game(&mut self, name: &str) -> anyhow::Result<std::path::PathBuf> {
        if !valid_name(name) {
            anyhow::bail!(
                "`{name}` is not a save name: letters, digits, `_`, `-` and `.`, up to 64"
            );
        }
        let Some(level) = &self.level else {
            anyhow::bail!("nothing to save: no map is loaded");
        };
        if self.player.health <= 0.0 {
            anyhow::bail!("cannot save while dead");
        }
        let map = level.name.clone();
        let map_hash = fingerprint(&level.bsp.entities);

        let game = self
            .with_game_mut(|game, engine| game.save(engine))
            .unwrap_or(Json::Null);
        let save = SaveGame {
            format: FORMAT,
            map,
            map_hash,
            saved_at: now(),
            time: self.time,
            tick: self.tick_count,
            player: SavedPlayer {
                origin: v3(self.player.movement.origin),
                velocity: v3(self.player.movement.velocity),
                view: {
                    let a = self.player.view_angles;
                    v3(Vec3::new(a.pitch, a.yaw, a.roll))
                },
                health: self.player.health,
                on_ground: self.player.movement.on_ground,
                ducked: self.player.movement.ducked,
            },
            world: self.entities.snapshot(),
            props: {
                let mut props: Vec<SavedMotion> = self
                    .physics
                    .prop_motion()
                    .into_iter()
                    .map(|(id, linear, angular, awake)| SavedMotion {
                        id: [id.index, id.generation],
                        linear: v3(linear),
                        angular: v3(angular),
                        awake,
                    })
                    .collect();
                props.sort_by_key(|p| p.id);
                props
            },
            script: SavedScripts {
                files: self.script.loaded().to_vec(),
                vars: self.script.variables(),
            },
            ui: SavedUi {
                store: self
                    .ui
                    .store
                    .iter()
                    // The store's own state is published afresh every frame.
                    .filter(|(k, _)| !k.starts_with(kerosene_ui::script::PLATFORM_PREFIX))
                    .map(|(k, v)| (k.to_string(), to_json(v)))
                    .collect(),
                layers: self
                    .ui
                    .system
                    .layers()
                    .filter(|(layer, _, visible)| *visible && *layer != MENU_LAYER)
                    .map(|(layer, path, _)| [layer.to_string(), path.to_string()])
                    .collect(),
                decals: self
                    .ui
                    .decals
                    .list
                    .iter()
                    .map(|d| SavedDecal {
                        material: d.material.clone(),
                        origin: v3(d.origin),
                        normal: v3(d.normal),
                        size: d.size,
                        rotation: d.rotation,
                    })
                    .collect(),
            },
            game,
        };

        let bytes = serde_json::to_vec_pretty(&save)?;
        let written = self.vfs.write(&save_path(name), &bytes)?;
        if self.platform.cloud_enabled()
            && let Err(e) = self.platform.cloud_write(&cloud_name(name), &bytes)
        {
            self.console
                .warn(format!("save: saved here but not to the cloud: {e}"));
        }
        self.ui_set("save.last", name);
        self.ui_emit("game_saved", name);
        self.console.print(format!("saved {name}"));
        Ok(written)
    }

    /// Read a save without loading it: the local file or the cloud's copy,
    /// whichever is newer.
    pub fn read_save(&self, name: &str) -> anyhow::Result<SaveGame> {
        if !valid_name(name) {
            anyhow::bail!("`{name}` is not a save name");
        }
        let parse = |bytes: &[u8], from: &str| -> anyhow::Result<SaveGame> {
            serde_json::from_slice(bytes)
                .map_err(|e| anyhow::anyhow!("save `{name}` ({from}) is damaged: {e}"))
        };
        let local = self
            .vfs
            .read_optional(&save_path(name))?
            .map(|b| parse(&b, "on disk"))
            .transpose()?;
        let cloud = if self.platform.cloud_enabled() {
            self.platform
                .cloud_read(&cloud_name(name))
                .and_then(|b| parse(&b, "in the cloud").ok())
        } else {
            None
        };
        let save = match (local, cloud) {
            (Some(l), Some(c)) => {
                if c.saved_at > l.saved_at {
                    c
                } else {
                    l
                }
            }
            (Some(s), None) | (None, Some(s)) => s,
            (None, None) => anyhow::bail!("no saved game named `{name}`"),
        };
        if save.format > FORMAT {
            anyhow::bail!(
                "save `{name}` was made by a newer version of the game (format {}, this reads {FORMAT})",
                save.format
            );
        }
        Ok(save)
    }

    /// Load a saved game now. The running level is left alone if it will
    /// not load.
    ///
    /// Not from inside a game hook -- the same rule as `load_map`; use
    /// [`request_load`](Engine::request_load) there.
    pub fn load_game(&mut self, name: &str) -> anyhow::Result<()> {
        let save = self.read_save(name)?;
        if let Ok(bytes) = self.vfs.read(&crate::engine::map_path(&save.map))
            && let Ok(bsp) = kerosene_bsp::Bsp::from_bytes(&bytes, &save.map)
            && !save.map_hash.is_empty()
            && fingerprint(&bsp.entities) != save.map_hash
        {
            self.console.warn(format!(
                "load: {} has been rebuilt since `{name}` was saved; things may be out of place",
                save.map
            ));
        }
        self.load_level(&save.map, Some(&save))?;
        self.ui_set("save.last", name);
        self.ui_emit("game_loaded", name);
        self.console.print(format!("loaded {name}"));
        Ok(())
    }

    /// Load a saved game at the start of the next frame.
    pub fn request_load(&mut self, name: &str) {
        self.pending_change = Some(PendingChange::Load(name.to_string()));
    }

    /// Move to another map at the start of the next frame, carrying the
    /// player's health and the game's state, and placing the player
    /// relative to the `info_landmark` both maps share, if one is named.
    pub fn change_level(&mut self, map: &str, landmark: Option<&str>) {
        self.pending_change = Some(PendingChange::Level {
            map: map.to_string(),
            landmark: landmark.filter(|l| !l.is_empty()).map(str::to_string),
        });
    }

    /// Every save on disk, newest first.
    pub fn list_saves(&self) -> Vec<SaveInfo> {
        let mut out: Vec<SaveInfo> = self
            .vfs
            .list(SAVE_DIR, Some(EXTENSION))
            .into_iter()
            .filter_map(|path| {
                let name = path
                    .strip_prefix(&format!("{SAVE_DIR}/"))?
                    .strip_suffix(&format!(".{EXTENSION}"))?
                    .to_string();
                let save: SaveGame = serde_json::from_slice(&self.vfs.read(&path).ok()?).ok()?;
                Some(SaveInfo {
                    name,
                    map: save.map,
                    saved_at: save.saved_at,
                })
            })
            .collect();
        out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then(a.name.cmp(&b.name)));
        out.dedup_by(|a, b| a.name == b.name);
        out
    }

    pub(crate) fn make_change(&mut self, change: PendingChange) {
        match change {
            PendingChange::Load(name) => {
                if let Err(e) = self.load_game(&name) {
                    self.console.error(format!("load: {e}"));
                }
            }
            PendingChange::Level { map, landmark } => self.change_level_now(&map, landmark),
        }
    }

    fn change_level_now(&mut self, map: &str, landmark: Option<String>) {
        let carry = Carry {
            health: self.player.health,
            game: self
                .with_game_mut(|game, engine| game.save(engine))
                .unwrap_or(Json::Null),
            view: self.player.view_angles,
            landmark: landmark.and_then(|name| {
                let Some(mark) = self.landmark(&name) else {
                    self.console
                        .warn(format!("changelevel: no info_landmark named `{name}` here"));
                    return None;
                };
                Some((
                    name,
                    self.player.movement.origin - mark,
                    self.player.movement.velocity,
                    self.player.movement.ducked,
                ))
            }),
        };
        if let Err(e) = self.load_map(map) {
            self.console.error(format!("changelevel: {e}"));
            return;
        }
        self.player.health = carry.health;
        if let Some((name, offset, velocity, ducked)) = carry.landmark {
            match self.landmark(&name) {
                Some(mark) => {
                    let origin = mark + offset;
                    self.player.movement.origin = origin;
                    self.player.movement.velocity = velocity;
                    self.player.movement.ducked = ducked;
                    self.player.previous_origin = origin;
                    self.player.view_angles = carry.view;
                    if let Some(e) = self.player.entity.and_then(|id| self.entities.get_mut(id)) {
                        e.origin = origin;
                    }
                }
                None => self.console.warn(format!(
                    "changelevel: {map} has no info_landmark named `{name}`; \
                     the player starts at the spawn point"
                )),
            }
        }
        if !carry.game.is_null() {
            let data = carry.game;
            self.with_game_mut(|game, engine| game.load(engine, &data));
        }
        self.ui_emit("level_changed", map);
        if self.console.bool("sv_autosave")
            && let Err(e) = self.save_game(AUTO)
        {
            self.console.warn(format!("autosave: {e}"));
        }
    }

    /// Where the `info_landmark` of this name stands.
    fn landmark(&self, name: &str) -> Option<Vec3> {
        self.entities
            .find_by_name(name)
            .into_iter()
            .filter_map(|id| self.entities.get(id))
            .find(|e| e.classname.eq_ignore_ascii_case("info_landmark"))
            .map(|e| e.origin)
    }

    /// Everything after the entities: the player, the scripts, the props'
    /// motion, the UI, the game. Called by `load_level` for a save.
    pub(crate) fn restore_from(&mut self, save: &SaveGame) {
        // The player.
        self.held_prop_clear();
        let player = match self.entities.player {
            Some(id) => id,
            None => {
                let id = self.entities.spawn("player");
                self.entities.player = Some(id);
                id
            }
        };
        let p = &save.player;
        let origin = Vec3::from_array(p.origin);
        let movement = kerosene_physics::MoveState {
            origin,
            velocity: Vec3::from_array(p.velocity),
            on_ground: p.on_ground,
            ducked: p.ducked,
            ..Default::default()
        };
        if let Some(e) = self.entities.get_mut(player) {
            e.origin = origin;
        }
        self.player = crate::engine::PlayerState {
            entity: Some(player),
            movement,
            view_angles: Angles::new(p.view[0], p.view[1], p.view[2]).clamped_view(),
            previous_origin: origin,
            health: p.health,
            use_held: self.player.use_held,
            attack_held: self.player.attack_held,
            step_distance: 0.0,
            step_index: self.player.step_index,
        };

        // The scripts: the same files, run for their functions, with what
        // their top level asked for dropped -- it asked the first time --
        // and their variables put back over whatever it set.
        self.script.clear();
        for file in &save.script.files {
            match self.vfs.read_string(file) {
                Ok(source) => {
                    if let Err(e) = self.script.load(file, &source) {
                        self.console.error(format!("load: script: {e}"));
                    }
                    self.script.take_actions();
                }
                Err(e) => self.console.warn(format!("load: script {file}: {e}")),
            }
        }
        self.script.set_variables(&save.script.vars);

        // The props, where their entities say, moving as they were.
        let vfs = self.vfs.clone();
        self.physics.adopt_props(&self.entities, &vfs);
        for m in &save.props {
            self.physics.set_prop_motion(
                handle(m.id),
                Vec3::from_array(m.linear),
                Vec3::from_array(m.angular),
                m.awake,
            );
        }

        // The UI.
        for (key, value) in &save.ui.store {
            if let Some(v) = from_json(value) {
                self.ui.store.set(key, v);
            }
        }
        for [layer, path] in &save.ui.layers {
            if !self.ui.system.is_visible(layer) {
                self.ui_show(layer, path);
            }
        }
        let cap = self.console.int("r_decals").max(0) as usize;
        for d in &save.ui.decals {
            self.ui.decals.push(
                DecalRequest {
                    id: 0,
                    material: d.material.clone(),
                    origin: Vec3::from_array(d.origin),
                    normal: Vec3::from_array(d.normal),
                    size: d.size,
                    rotation: d.rotation,
                },
                cap,
            );
        }

        // What restore handlers asked for: a looping sound, mostly.
        self.take_entity_requests();
        self.with_game_mut(|game, engine| game.map_loaded(engine));
        if !save.game.is_null() {
            self.with_game_mut(|game, engine| game.load(engine, &save.game));
        }
    }

    /// Handle one of this module's console requests. `false` if it is not
    /// one.
    pub(crate) fn save_console_request(&mut self, kind: &str, payload: &str) -> bool {
        let name = payload.trim();
        match kind {
            requests::SAVE | requests::QUICKSAVE => {
                let name = if kind == requests::QUICKSAVE {
                    QUICK
                } else {
                    name
                };
                if name.is_empty() {
                    self.console.warn("usage: save <name>");
                } else if let Err(e) = self.save_game(name) {
                    self.console.error(format!("save: {e}"));
                }
            }
            requests::LOAD | requests::QUICKLOAD => {
                let name = if kind == requests::QUICKLOAD {
                    QUICK
                } else {
                    name
                };
                if name.is_empty() {
                    self.console.warn("usage: load <name>");
                } else {
                    self.request_load(name);
                }
            }
            requests::LIST => {
                let saves = self.list_saves();
                if saves.is_empty() {
                    self.console.print("no saved games");
                }
                let now = now();
                for s in saves {
                    self.console.print(format!(
                        "  {:<20} {:<20} {}",
                        s.name,
                        s.map,
                        age(now.saturating_sub(s.saved_at))
                    ));
                }
            }
            requests::CHANGELEVEL => {
                let mut words = name.split_whitespace();
                match words.next() {
                    Some(map) => self.change_level(map, words.next()),
                    None => self.console.warn("usage: changelevel <map> [landmark]"),
                }
            }
            _ => return false,
        }
        true
    }
}

/// How long ago, roughly, in words.
fn age(seconds: u64) -> String {
    match seconds {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

pub(crate) fn register(console: &mut kerosene_console::Console) {
    use kerosene_console::ConVarFlags;
    for (name, help) in [
        (requests::SAVE, "Save the game: save <name>"),
        (requests::LOAD, "Load a saved game: load <name>"),
        (requests::QUICKSAVE, "Save the game as `quick`."),
        (requests::QUICKLOAD, "Load the game saved as `quick`."),
        (requests::LIST, "List saved games, newest first."),
        (
            requests::CHANGELEVEL,
            "Move to another map, keeping the player's health and the game's state: \
             changelevel <map> [landmark]",
        ),
    ] {
        console.register_command(name, ConVarFlags::NONE, help, move |con, args| {
            con.request(name, args.rest.clone())
        });
    }
    console.register_cvar(
        "sv_autosave",
        "1",
        ConVarFlags::ARCHIVE,
        "Save as `auto` after every level change.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_names_cannot_leave_the_save_directory() {
        for good in ["quick", "auto", "chapter_2", "slot-1", "a.b"] {
            assert!(valid_name(good), "{good}");
        }
        for bad in ["", "../x", "a/b", "a\\b", ".hidden", "a..b", "with space"] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(!valid_name(&"x".repeat(65)));
    }

    #[test]
    fn ages_read_as_words() {
        assert_eq!(age(5), "just now");
        assert_eq!(age(125), "2 min ago");
        assert_eq!(age(7200), "2 h ago");
        assert_eq!(age(200_000), "2 days ago");
    }

    #[test]
    fn ui_values_survive_json() {
        use kerosene_ui::Value as V;
        for v in [
            V::Bool(true),
            V::Int(-3),
            V::Float(2.5),
            V::Str("hello".into()),
        ] {
            let text = serde_json::to_string(&to_json(&v)).unwrap();
            let back: Json = serde_json::from_str(&text).unwrap();
            assert_eq!(from_json(&back), Some(v));
        }
    }
}
