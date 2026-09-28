// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The developer's commands: cheats, moving about, poking entities, and the
//! session.
//!
//! `god`, `buddha`, `notarget`, `kill`, `give`; `getpos`, `setpos`,
//! `setang`; `ent_fire`, `ent_create`, `ent_remove`, `ent_info`; `restart`,
//! `maps`, `host_writeconfig`. What a level designer reaches for in the
//! first minute of testing a map, named as Source names them, so the muscle
//! memory carries over.
//!
//! The commands leave requests, like every engine command; the engine
//! carries them out in [`Engine::debug_console_request`], where the world
//! is. Anything acting on "the entity I am looking at" traces from the eye,
//! as the use key does.

use super::*;
use kerosene_entity::Target;

/// Console request kinds.
mod requests {
    pub const KILL: &str = "kill";
    pub const GIVE: &str = "give";
    pub const GETPOS: &str = "getpos";
    pub const SETPOS: &str = "setpos";
    pub const SETANG: &str = "setang";
    pub const ENT_FIRE: &str = "ent_fire";
    pub const ENT_CREATE: &str = "ent_create";
    pub const ENT_REMOVE: &str = "ent_remove";
    pub const ENT_INFO: &str = "ent_info";
    pub const RESTART: &str = "restart";
    pub const MAPS: &str = "maps";
    pub const WRITE_CONFIG: &str = "host_writeconfig";
}

/// Where the console's history is kept between sessions, beside
/// `config.cfg` in the player's own directory.
const HISTORY_FILE: &str = "cfg/console_history.txt";

/// How far `ent_create`, `ent_remove` and `ent_info` look for what the
/// player is aiming at.
const AIM_RANGE: f32 = 8192.0;

/// Register the convars and commands.
pub(super) fn register(console: &mut Console) {
    for (cvar, command, help) in [
        ("sv_god", "god", "Take no damage."),
        (
            "sv_buddha",
            "buddha",
            "Take damage, but never below 1 health.",
        ),
        (
            "sv_notarget",
            "notarget",
            "Go unnoticed: what a game's AI reads before it looks for the player.",
        ),
    ] {
        console.register_cvar(cvar, "0", ConVarFlags::CHEAT, help);
        console.register_command(command, ConVarFlags::CHEAT, help, move |con, _| {
            let on = !con.bool(cvar);
            con.set_bool(cvar, on);
            con.print(format!("{command} {}", if on { "on" } else { "off" }));
        });
    }
    console.register_cvar_ranged(
        "host_timescale",
        "1",
        Some(0.01),
        Some(10.0),
        ConVarFlags::CHEAT,
        "Run the simulation this many times faster than real time.",
    );

    // (name, flags, help, request, whether it needs arguments)
    let commands: [(&str, ConVarFlags, &str, &str, bool); 12] = [
        ("kill", ConVarFlags::NONE, "Die.", requests::KILL, false),
        (
            "give",
            ConVarFlags::CHEAT,
            "Spawn an entity where you stand: give <class> [key value]...",
            requests::GIVE,
            true,
        ),
        (
            "getpos",
            ConVarFlags::NONE,
            "Print where you are and which way you face, as a setpos and setang.",
            requests::GETPOS,
            false,
        ),
        (
            "setpos",
            ConVarFlags::CHEAT,
            "Move to a point: setpos <x> <y> <z>",
            requests::SETPOS,
            true,
        ),
        (
            "setang",
            ConVarFlags::CHEAT,
            "Face a direction: setang <pitch> <yaw> [roll]",
            requests::SETANG,
            true,
        ),
        (
            "ent_fire",
            ConVarFlags::CHEAT,
            "Fire an input: ent_fire <name, class or !picker> <input> [parameter] [delay]",
            requests::ENT_FIRE,
            true,
        ),
        (
            "ent_create",
            ConVarFlags::CHEAT,
            "Spawn an entity where you are looking: ent_create <class> [key value]...",
            requests::ENT_CREATE,
            true,
        ),
        (
            "ent_remove",
            ConVarFlags::CHEAT,
            "Remove what you are looking at, or every entity with a name: ent_remove [name]",
            requests::ENT_REMOVE,
            false,
        ),
        (
            "ent_info",
            ConVarFlags::NONE,
            "Print an entity's class, name, place and keys: what you are looking at, or ent_info <name>",
            requests::ENT_INFO,
            false,
        ),
        (
            "restart",
            ConVarFlags::NONE,
            "Load the current map again from the start.",
            requests::RESTART,
            false,
        ),
        (
            "maps",
            ConVarFlags::NONE,
            "List the maps there are: maps [filter]",
            requests::MAPS,
            false,
        ),
        (
            "host_writeconfig",
            ConVarFlags::NONE,
            "Write config.cfg now, rather than waiting for the game to quit.",
            requests::WRITE_CONFIG,
            false,
        ),
    ];
    for (name, flags, help, request, needs_args) in commands {
        let usage = help.split_once(": ").map_or(name, |(_, usage)| usage);
        let usage = usage.to_string();
        console.register_command(name, flags, help, move |con, args| {
            if needs_args && args.count() < 2 {
                con.warn(format!("usage: {usage}"));
                return;
            }
            let rest = args.rest.clone();
            con.request(request, rest);
        });
    }
}

/// Give commands that take a map, a save, a sound or a class something to
/// complete. Called once the game has registered its classes.
pub(super) fn register_completers(engine: &mut Engine) {
    fn names(vfs: &Vfs, dir: &str, extension: &str) -> Vec<String> {
        vfs.list(dir, Some(extension))
            .into_iter()
            .filter_map(|path| {
                let name = path.strip_prefix(&format!("{dir}/"))?;
                Some(name.strip_suffix(&format!(".{extension}"))?.to_string())
            })
            .collect()
    }
    let vfs = engine.vfs.clone();
    let maps = move |_: &Console, _: &str| names(&vfs, "maps", kerosene_vfs::ext::BSP);
    let maps = std::sync::Arc::new(maps);
    for command in ["map", "maps", "changelevel"] {
        let maps = maps.clone();
        engine
            .console
            .register_completer(command, move |c, a| maps(c, a));
    }
    let vfs = engine.vfs.clone();
    let saves = std::sync::Arc::new(move |_: &Console, _: &str| {
        names(&vfs, crate::save::SAVE_DIR, kerosene_vfs::ext::SAVE)
    });
    for command in ["load", "save"] {
        let saves = saves.clone();
        engine
            .console
            .register_completer(command, move |c, a| saves(c, a));
    }
    let vfs = engine.vfs.clone();
    engine.console.register_completer("play", move |_, _| {
        names(&vfs, "sound", kerosene_vfs::ext::AUDIO)
    });
    let registry = engine.registry.clone();
    let classes = std::sync::Arc::new(move |_: &Console, _: &str| {
        registry
            .class_names()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    });
    for command in ["give", "ent_create"] {
        let classes = classes.clone();
        engine
            .console
            .register_completer(command, move |c, a| classes(c, a));
    }
}

/// Words as the console splits them: quotes keep a phrase together.
fn words(payload: &str) -> Vec<String> {
    kerosene_console::tokenize(payload)
}

/// `key value` pairs after a class name.
fn keyvalues(words: &[String]) -> Vec<(&str, &str)> {
    words
        .chunks(2)
        .filter_map(|pair| match pair {
            [key, value] => Some((key.as_str(), value.as_str())),
            _ => None,
        })
        .collect()
}

fn floats(words: &[String]) -> Option<Vec<f32>> {
    words
        .iter()
        .map(|w| w.parse::<f32>().ok().filter(|f| f.is_finite()))
        .collect()
}

impl Engine {
    /// Whether god mode is on.
    pub fn god(&self) -> bool {
        self.console.bool("sv_god")
    }

    /// Whether buddha mode is on: damage is taken, death is not.
    pub fn buddha(&self) -> bool {
        self.console.bool("sv_buddha")
    }

    /// Whether the player should go unnoticed. The engine has no AI of its
    /// own; this is for a game's to read before it looks for the player.
    pub fn notarget(&self) -> bool {
        self.console.bool("sv_notarget")
    }

    /// The entity the player is looking at, if any.
    pub fn aimed_entity(&self) -> Option<EntityId> {
        self.trace_view((0.0, 0.0), AIM_RANGE)?.entity
    }

    /// Write `cfg/config.cfg` now: archived convars and bindings. The host
    /// does it on exit; `host_writeconfig` does it on demand.
    pub fn write_config(&mut self) -> Result<PathBuf, VfsError> {
        let text = self.config_text(&self.input.to_config());
        self.vfs.write("cfg/config.cfg", text.as_bytes())
    }

    /// Keep what was typed into the console for the next session: the
    /// host does it on exit, and [`Engine::with_game`] reads it back.
    pub fn save_console_history(&self) -> Result<PathBuf, VfsError> {
        let mut text = self.console.history().join("\n");
        text.push('\n');
        self.vfs.write(HISTORY_FILE, text.as_bytes())
    }

    /// Read back the last session's console history, if it kept one.
    pub(super) fn load_console_history(&mut self) {
        if let Ok(Some(bytes)) = self.vfs.read_optional(HISTORY_FILE) {
            let lines = String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            self.console.set_history(lines);
        }
    }

    /// Carry out a request one of these commands left. `false` for a kind
    /// that is not one of them.
    pub(crate) fn debug_console_request(&mut self, kind: &str, payload: &str) -> bool {
        let words = words(payload);
        match kind {
            requests::KILL => {
                if self.level.is_some() && self.player_alive() {
                    self.kill_player("suicide");
                }
            }
            requests::GIVE => self.give(&words),
            requests::GETPOS => {
                let o = self.player.movement.origin;
                let a = self.player.view_angles;
                self.console.print(format!(
                    "setpos {:.2} {:.2} {:.2}; setang {:.2} {:.2} {:.2}",
                    o.x, o.y, o.z, a.pitch, a.yaw, a.roll
                ));
            }
            requests::SETPOS => match floats(&words).as_deref() {
                Some([x, y, z, ..]) => self.teleport_player(Vec3::new(*x, *y, *z), None),
                _ => self.console.warn("usage: setpos <x> <y> <z>"),
            },
            requests::SETANG => match floats(&words).as_deref() {
                Some([pitch, yaw]) => self.set_view_angles(Angles::new(*pitch, *yaw, 0.0)),
                Some([pitch, yaw, roll, ..]) => {
                    self.set_view_angles(Angles::new(*pitch, *yaw, *roll))
                }
                _ => self.console.warn("usage: setang <pitch> <yaw> [roll]"),
            },
            requests::ENT_FIRE => self.ent_fire(&words),
            requests::ENT_CREATE => self.ent_create(&words),
            requests::ENT_REMOVE => {
                let targets = self.named_or_aimed(words.first().map(String::as_str));
                for &id in &targets {
                    self.entities.remove(id);
                }
                self.console
                    .print(format!("removed {} entities", targets.len()));
            }
            requests::ENT_INFO => {
                for id in self.named_or_aimed(words.first().map(String::as_str)) {
                    self.print_entity(id);
                }
            }
            requests::RESTART => match self.map_name().map(str::to_string) {
                Some(map) => self.pending_map = Some(map),
                None => self.console.warn("restart: no map is loaded"),
            },
            requests::MAPS => {
                let filter = words.first().map(|w| w.to_lowercase()).unwrap_or_default();
                let maps: Vec<String> = self
                    .vfs
                    .list("maps", Some(kerosene_vfs::ext::BSP))
                    .into_iter()
                    .filter_map(|p| {
                        let name = p.strip_prefix("maps/")?;
                        Some(name.strip_suffix(".kbsp")?.to_string())
                    })
                    .filter(|m| m.to_lowercase().contains(&filter))
                    .collect();
                for map in &maps {
                    self.console.print(format!("  {map}"));
                }
                self.console.print(format!("{} maps", maps.len()));
            }
            requests::WRITE_CONFIG => match self.write_config() {
                Ok(path) => self.console.print(format!("wrote {}", path.display())),
                Err(e) => self.console.error(format!("host_writeconfig: {e}")),
            },
            _ => return false,
        }
        true
    }

    fn give(&mut self, words: &[String]) {
        let Some(class) = words.first() else { return };
        if self.level.is_none() {
            self.console.warn("give: no map is loaded");
            return;
        }
        let origin = self.player.movement.origin;
        let origin = format!("{} {} {}", origin.x, origin.y, origin.z);
        let mut keys = keyvalues(&words[1..]);
        keys.push(("origin", &origin));
        let id = self.spawn_entity(class, &keys);
        self.console.print(format!("gave {class} ({id:?})"));
    }

    fn ent_create(&mut self, words: &[String]) {
        let Some(class) = words.first() else { return };
        if self.level.is_none() {
            self.console.warn("ent_create: no map is loaded");
            return;
        }
        // On the surface looked at, lifted off it a little so a box spawned
        // against a wall is not half inside it.
        let at = match self.trace_view((0.0, 0.0), AIM_RANGE) {
            Some(hit) => hit.pos + hit.normal * 16.0,
            None => {
                let eye = self.player.movement.eye_position();
                eye + self.player.view_angles.forward() * 128.0
            }
        };
        let origin = format!("{} {} {}", at.x, at.y, at.z);
        let mut keys = keyvalues(&words[1..]);
        keys.push(("origin", &origin));
        let id = self.spawn_entity(class, &keys);
        self.console
            .print(format!("created {class} ({id:?}) at {at:.1}"));
    }

    fn ent_fire(&mut self, words: &[String]) {
        let [target, input, rest @ ..] = words else {
            self.console
                .warn("usage: ent_fire <name, class or !picker> <input> [parameter] [delay]");
            return;
        };
        let parameter = rest.first().map(String::as_str).unwrap_or("");
        let delay = rest
            .get(1)
            .and_then(|d| d.parse::<f32>().ok())
            .filter(|d| d.is_finite())
            .unwrap_or(0.0)
            .max(0.0);
        let targets = if target.eq_ignore_ascii_case("!picker") {
            self.aimed_entity().into_iter().collect()
        } else {
            let named = self.entities.find_by_name(target);
            if named.is_empty() {
                self.entities.find_by_class(target)
            } else {
                named
            }
        };
        if targets.is_empty() {
            self.console
                .warn(format!("ent_fire: nothing named or of class `{target}`"));
            return;
        }
        let player = self.player.entity;
        for &id in &targets {
            self.entities
                .queue_input(Target::Handle(id), input, parameter, delay, player, player);
        }
        self.console
            .developer(format!("ent_fire: {input} to {} entities", targets.len()));
    }

    /// Every entity with this name, or with none given, the one aimed at.
    fn named_or_aimed(&mut self, name: Option<&str>) -> Vec<EntityId> {
        match name {
            Some(name) => {
                let found = self.entities.find_by_name(name);
                if found.is_empty() {
                    self.console.warn(format!("nothing is named `{name}`"));
                }
                found
            }
            None => {
                let aimed: Vec<EntityId> = self.aimed_entity().into_iter().collect();
                if aimed.is_empty() {
                    self.console.warn("not looking at an entity");
                }
                aimed
            }
        }
    }

    fn print_entity(&mut self, id: EntityId) {
        let Some(e) = self.entities.get(id) else {
            return;
        };
        let mut lines = vec![format!(
            "{} `{}` ({id:?}) at {:.1}, facing {:.1} {:.1} {:.1}",
            e.classname,
            e.targetname().unwrap_or(""),
            e.origin,
            e.angles.pitch,
            e.angles.yaw,
            e.angles.roll
        )];
        let mut fields: Vec<(String, String)> = e
            .fields
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        fields.sort();
        lines.extend(fields.into_iter().map(|(k, v)| format!("  {k} = {v}")));
        lines.extend(e.connections.iter().map(|c| format!("  {c:?}")));
        for line in lines {
            self.console.print(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::{Engine, EngineConfig};
    use kerosene_math::Vec3;

    /// The stock classes, so inputs like `Kill` mean something.
    struct Stock;
    impl crate::Game for Stock {
        fn classes(&self, registry: &mut kerosene_entity::ClassRegistry) {
            kerosene_game::register(registry);
        }
    }

    fn engine() -> Engine {
        let mut engine = Engine::with_game(&EngineConfig::default(), Box::new(Stock));
        engine.load_map(crate::base::FALLBACK_MAP).unwrap();
        engine.console.execute("sv_cheats 1");
        engine
    }

    fn run(engine: &mut Engine, line: &str) {
        engine.console.execute(line);
        let unclaimed = crate::engine::take_console_requests(engine);
        assert!(unclaimed.is_empty(), "{unclaimed:?}");
    }

    #[test]
    fn setpos_and_getpos_agree() {
        let mut engine = engine();
        run(&mut engine, "setpos 10 20 30; setang 5 90");
        assert_eq!(engine.player.movement.origin, Vec3::new(10.0, 20.0, 30.0));
        assert_eq!(engine.player.view_angles.yaw, 90.0);
        run(&mut engine, "getpos");
        assert!(engine.console.log().any(|l| {
            l.text
                .starts_with("setpos 10.00 20.00 30.00; setang 5.00 90.00")
        }));
    }

    #[test]
    fn god_takes_no_damage_and_buddha_does_not_die() {
        let mut engine = engine();
        run(&mut engine, "god");
        engine.hurt_player(50.0, "test");
        assert_eq!(engine.player.health, 100.0);
        run(&mut engine, "god; buddha");
        engine.hurt_player(500.0, "test");
        assert_eq!(engine.player.health, 1.0);
        // Cheats off, and a hit is a hit again.
        engine.console.execute("sv_cheats 0");
        assert!(!engine.god() && !engine.buddha());
    }

    #[test]
    fn kill_kills_even_a_god() {
        let mut engine = engine();
        let start = engine.player.movement.origin;
        run(&mut engine, "god; setpos 0 0 500; kill");
        // The stock death respawns at the start, alive.
        assert_eq!(engine.player.movement.origin, start);
        assert_eq!(engine.player.health, engine.player_max_health());
    }

    #[test]
    fn ent_create_ent_fire_and_ent_remove() {
        let mut engine = engine();
        run(
            &mut engine,
            "ent_create logic_relay targetname made; setpos 0 0 0",
        );
        let made = engine.entities.find_by_name("made");
        assert_eq!(made.len(), 1);
        run(&mut engine, "ent_fire made Kill");
        engine.tick(engine.tick_interval(), &Default::default());
        assert!(engine.entities.find_by_name("made").is_empty());

        run(&mut engine, "give logic_relay targetname given");
        assert_eq!(engine.entities.find_by_name("given").len(), 1);
        run(&mut engine, "ent_remove given");
        engine.tick(engine.tick_interval(), &Default::default());
        assert!(engine.entities.find_by_name("given").is_empty());
    }

    #[test]
    fn maps_restart_and_completion() {
        let mut engine = engine();
        run(&mut engine, "maps room");
        assert!(
            engine
                .console
                .log()
                .any(|l| l.text.trim() == crate::base::FALLBACK_MAP)
        );
        assert_eq!(
            engine.console.complete_line("map kerosene_r"),
            [format!("map {}", crate::base::FALLBACK_MAP)]
        );
        let generation = engine.load_generation();
        run(&mut engine, "restart");
        engine.load_pending_map();
        assert_ne!(engine.load_generation(), generation);
    }

    #[test]
    fn history_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("kerosene-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = EngineConfig::default().with_user_dir(Some(dir.clone()));
        let mut first = Engine::new(&config);
        first.console.execute_user("echo one");
        first.console.execute_user("echo two");
        first.save_console_history().unwrap();
        let second = Engine::new(&config);
        assert_eq!(second.console.history(), ["echo one", "echo two"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cheats_need_sv_cheats() {
        let mut engine = engine();
        engine.console.execute("sv_cheats 0");
        run(&mut engine, "setpos 1 2 3; god");
        assert_ne!(engine.player.movement.origin, Vec3::new(1.0, 2.0, 3.0));
        assert!(!engine.god());
    }
}
