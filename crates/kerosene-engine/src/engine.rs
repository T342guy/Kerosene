// SPDX-License-Identifier: LGPL-3.0-or-later
//! The engine core: everything except the window.
//!
//! Deliberately separated from [`crate::host`] so the whole simulation can run
//! without a display. That is not only for testing: a dedicated server runs
//! exactly this, and being unable to start one without a GPU would be a
//! serious design mistake in an engine meant to host multiplayer games.
//!
//! The tick is fixed-rate, as Source's is. Physics and entity I/O advance in
//! equal steps whatever the frame rate, so a fast machine and a slow one
//! simulate identically -- and rendering interpolates between the last two
//! states rather than dragging simulation along with it.

use crate::input::InputState;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use kerosene_bsp::{Bsp, contents};
use kerosene_console::{ConVarFlags, Console, requests};
use kerosene_entity::{EntityId, EntityWorld};
use kerosene_math::{Aabb, Angles, Pose, Vec3};
use crate::collision::LevelCollision;
use kerosene_physics::{MoveInput, MoveParams, MoveState};
use kerosene_vfs::{Vfs, VfsError};

/// Server tick rate. 64 is Source's modern default: fine enough that
/// movement feels continuous, coarse enough to be affordable.
pub const DEFAULT_TICKRATE: f32 = 64.0;

/// How the engine was asked to start.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Directories to mount, searched in order.
    pub content_paths: Vec<PathBuf>,
    /// Archives to mount after them.
    pub archives: Vec<PathBuf>,
    /// Map to load on start.
    pub map: Option<String>,
    /// Console commands to run once everything is up.
    pub startup_commands: Vec<String>,
    /// Whether to open an audio device.
    ///
    /// Off in tests and headless runs: opening a sound card is slow, and a
    /// hundred `Engine`s in one test binary would each try.
    pub audio: bool,
    /// The global log relay, if one was installed.
    ///
    /// Handed in rather than installed here because installing a logger is a
    /// process-wide act and an `Engine` is constructed in a hundred tests.
    /// Without it the console still works; it just cannot show anything the
    /// rest of the engine logged.
    pub log: Option<std::sync::Arc<kerosene_console::LogRelay>>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            audio: false,
            log: None,
            content_paths: vec![PathBuf::from("content")],
            archives: Vec::new(),
            map: None,
            startup_commands: Vec::new(),
        }
    }
}

/// Say why a map would not load, in terms someone can act on.
///
/// "not found in any search path" is true and useless. The overwhelmingly
/// common reason a map is missing is that it has never been compiled -- the
/// `.keromap` is right there, and nothing turned it into a `.kerobsp`. The
/// next most common is that the content tree being searched is not the one
/// the map lives in. Both are worth saying outright, along with what was
/// searched, because the alternative is reading the source to find out.
pub fn explain_missing_map(vfs: &Vfs, name: &str, why: &VfsError) -> String {
    let mut said = format!("could not load the map '{name}'\n");

    // Anything other than "it is not there" is its own problem -- a permission,
    // a truncated archive -- and guessing "you forgot to compile it" over the
    // top of it would send someone the wrong way.
    if !matches!(why, VfsError::NotFound(_)) {
        return format!("{said}  {why}").trim_end().to_string();
    }

    if vfs.exists(&format!("maps/{name}.keromap")) {
        said.push_str(&format!("  maps/{name}.keromap is there, but has not been compiled.\n"));
        said.push_str("  build it with:  scripts/build-content.sh\n");
        said.push_str(&format!("  or on its own:  cleave maps/{name}.keromap\n"));
    } else {
        let mut maps: Vec<String> = vfs
            .list("maps", Some("kerobsp"))
            .iter()
            .filter_map(|p| p.strip_prefix("maps/").map(|n| n.trim_end_matches(".kerobsp").to_string()))
            .collect();
        maps.sort();
        maps.dedup();
        if maps.is_empty() {
            said.push_str("  no compiled maps in any search path. Run scripts/build-content.sh\n");
        } else {
            said.push_str(&format!("  compiled maps here: {}\n", maps.join(", ")));
        }
    }

    said.push_str("  searched:\n");
    for layer in vfs.describe() {
        said.push_str(&format!("    {layer}\n"));
    }
    if vfs.path_count() == 0 {
        said.push_str("    (nothing mounted)\n");
    }
    said.trim_end().to_string()
}

/// A loaded level.
pub struct Level {
    pub name: String,
    pub bsp: Bsp,
    /// What the sky is tinted, from the map's `light_environment`.
    ///
    /// Kept on the level rather than read per frame: it cannot change while a
    /// map is loaded, and the entity that names it is inert at runtime, so
    /// this is the only moment anything asks.
    pub sky_color: Vec3,
}

/// How a brush model is placed, given where its entity has got to.
///
/// The pivot is the model's own centre, taken from its compiled bounds rather
/// than from a keyvalue. A brush model is built in world coordinates, so there
/// is no other point that means "spin where you stand", and asking a designer
/// to place an origin brush -- Source's answer -- is asking them to state
/// something the geometry already knows.
pub fn brush_pose(bsp: Option<&Bsp>, model: usize, origin: Vec3, angles: Angles) -> Pose {
    if angles == Angles::ZERO {
        // The common case by a wide margin, and it needs no pivot at all.
        return Pose::new(origin, angles);
    }
    let pivot = bsp
        .and_then(|b| b.models.get(model))
        .map(|m| m.bounds().center())
        .unwrap_or(Vec3::ZERO);
    Pose::about(origin, angles, pivot)
}

/// The engine.
pub struct Engine {
    pub console: Console,
    /// The global log relay, drained into the console once a frame.
    pub log: Option<std::sync::Arc<kerosene_console::LogRelay>>,
    /// The script VM. Empty until a map with a script loads.
    pub script: kerosene_script::ScriptHost,
    /// Sound. The mixer runs whether or not a device opened.
    pub audio: crate::audio::AudioSystem,
    pub vfs: Arc<Vfs>,
    pub level: Option<Level>,
    pub entities: EntityWorld,
    pub player: PlayerState,
    /// Accumulated real time not yet simulated.
    accumulator: f32,
    /// Total simulated time.
    pub time: f32,
    pub tick_count: u64,
    /// Set when the console asks for a different map.
    pending_map: Option<String>,
    /// Set by the `quit` command.
    pub should_quit: bool,
}

/// The local player.
pub struct PlayerState {
    pub entity: Option<EntityId>,
    pub movement: MoveState,
    pub view_angles: Angles,
    /// Simulation state at the start and end of the current tick, so rendering
    /// can interpolate between them instead of stuttering at the tick rate.
    pub previous_origin: Vec3,
    pub health: f32,
    /// Whether the use key was down last tick.
    ///
    /// Using is an edge, not a state: holding the key against a door should
    /// open it once, not toggle it sixty-four times a second.
    pub use_held: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        PlayerState {
            entity: None,
            movement: MoveState::default(),
            view_angles: Angles::ZERO,
            previous_origin: Vec3::ZERO,
            health: 100.0,
            use_held: false,
        }
    }
}

impl Engine {
    pub fn new(config: &EngineConfig) -> Engine {
        let mut vfs = Vfs::new();
        for dir in &config.content_paths {
            vfs.add_directory(dir, "GAME");
        }
        for archive in &config.archives {
            match vfs.mount_archive(archive, "GAME").map(|_| ()) {
                // Said out loud. Which archives are mounted decides which
                // version of every asset the game is running, and working
                // that out from the outside means guessing.
                Ok(()) => log::info!("mounted {}", archive.display()),
                Err(e) => log::warn!("could not mount {}: {e}", archive.display()),
            }
        }
        // Loose files win over packed ones, so a developer can drop a file
        // beside a shipped archive and see it immediately.
        let vfs = Arc::new(vfs);

        let mut console = Console::new();
        register_cvars(&mut console);
        register_commands(&mut console);

        // Wire `exec` to the filesystem.
        let exec_vfs = vfs.clone();
        console.set_exec_handler(move |name| {
            let path = if name.contains('/') { name.to_string() } else { format!("cfg/{name}") };
            exec_vfs.read_string(&path).ok()
        });

        // Logging convars, wired to the relay if there is one. Both are
        // no-ops without it, which is the headless and test case.
        console.register_cvar("con_logfile", "", ConVarFlags::NONE, "Write every log line to this file. Empty closes it.");
        if let Some(relay) = config.log.clone() {
            console.on_change("con_logfile", move |con, _, value| {
                let value = value.trim();
                if value.is_empty() {
                    relay.close_file();
                    con.print("log file closed");
                    return;
                }
                match relay.open_file(std::path::Path::new(value)) {
                    Ok(()) => con.print(format!("logging to {value}")),
                    Err(e) => con.error(format!("could not open {value}: {e}")),
                }
            });
        }

        let entities = EntityWorld::new(kerosene_game::registry());

        let mut engine = Engine {
            console,
            log: config.log.clone(),
            script: kerosene_script::ScriptHost::new(),
            audio: if config.audio {
                crate::audio::AudioSystem::open()
            } else {
                crate::audio::AudioSystem::silent()
            },
            vfs,
            level: None,
            entities,
            player: PlayerState::default(),
            accumulator: 0.0,
            time: 0.0,
            tick_count: 0,
            pending_map: config.map.clone(),
            should_quit: false,
        };

        let vfs = engine.vfs.clone();
        engine.audio.load_scripts(&vfs);

        for command in &config.startup_commands {
            engine.console.enqueue(command.clone());
        }
        engine
    }

    pub fn tick_rate(&self) -> f32 {
        self.console.float("sv_tickrate").max(1.0)
    }

    pub fn tick_interval(&self) -> f32 { 1.0 / self.tick_rate() }

    /// Where each brush entity's model has moved to, as `(model, offset)`.
    ///
    /// The renderer and the collision system both need this, and they must
    /// agree: a door drawn where it is not is worse than a door that does not
    /// move at all, because the second is obvious. Model 0 is the static
    /// world and never appears here.
    pub fn brush_model_poses(&self) -> Vec<(usize, Pose)> {
        let level = self.level.as_ref();
        self.entities
            .iter()
            .filter_map(|e| {
                let model = e.brush_model?;
                if model == 0 { return None }
                Some((model, brush_pose(level.map(|l| &l.bsp), model, e.origin, e.angles)))
            })
            .collect()
    }

    /// Load a map by name, e.g. `kero_start`.
    pub fn load_map(&mut self, name: &str) -> anyhow::Result<()> {
        let path = format!("maps/{name}.kerobsp");
        let bytes = match self.vfs.read(&path) {
            Ok(bytes) => bytes,
            Err(e) => anyhow::bail!("{}", explain_missing_map(&self.vfs, name, &e)),
        };
        let bsp = Bsp::from_bytes(&bytes, &path)?;

        self.console.print(format!("loading {path}"));
        for (name, count) in bsp.stats() {
            self.console.developer(format!("  {name}: {count}"));
        }
        if bsp.visibility.is_empty() {
            self.console.warn("this map has no visibility data; run Umbra on it");
        }
        if bsp.lighting.is_empty() {
            self.console.warn("this map has no lighting; run Radiance on it");
        }

        // A fresh entity world per map: nothing from the last one should
        // survive, and a stale handle must not resolve.
        self.entities = EntityWorld::new(kerosene_game::registry());
        self.entities.set_trace(self.console.int("developer") >= 2);
        let count = self.entities.load_from_bsp(&bsp)?;
        self.console.print(format!("{count} entities"));

        let sky_color = self.sky_color_from_map();
        self.level = Some(Level { name: name.to_string(), bsp, sky_color });
        self.spawn_player();
        self.time = 0.0;
        self.tick_count = 0;
        self.accumulator = 0.0;

        // Whatever the last level was playing is not playing any more.
        self.audio.stop_all();

        // A map's script loads after every entity exists, so `on_map_start`
        // can find them. A map without one is the normal case and is silent.
        self.load_map_script(name);
        // ...and any `logic_script` that named a file of its own got its
        // request in during spawn.
        self.take_entity_requests();
        self.call_script_hook(kerosene_script::hooks::MAP_START, vec![]);
        Ok(())
    }

    /// The sun's colour, which is also what the sky is tinted.
    ///
    /// Radiance reads the same `_light` value to bake the lighting, so a map
    /// lit by a warm sun gets a warm sky without anyone stating it twice.
    /// White when a map has no `light_environment`, since a tint of nothing
    /// is the same as no tint.
    fn sky_color_from_map(&self) -> Vec3 {
        let Some(id) = self.entities.find_by_class("light_environment").first().copied() else {
            return Vec3::ONE;
        };
        let Some(entity) = self.entities.get(id) else { return Vec3::ONE };
        // "r g b brightness"; the brightness belongs to the lighting compile
        // and would blow the sky out if it were applied here as well.
        let raw = entity.fields.text("_light").unwrap_or_default();
        let numbers: Vec<f32> = raw
            .split_whitespace()
            .take(3)
            .filter_map(|n| n.parse::<f32>().ok())
            .collect();
        if numbers.len() < 3 { return Vec3::ONE }
        Vec3::new(numbers[0], numbers[1], numbers[2]) / 255.0
    }

    /// Put the player at an `info_player_start`, or somewhere sane if there is
    /// none.
    fn spawn_player(&mut self) {
        let spawn = self
            .entities
            .find_by_class("info_player_start")
            .first()
            .copied()
            .and_then(|id| self.entities.get(id).map(|e| (e.origin, e.angles)));

        let (origin, angles) = match spawn {
            Some(found) => found,
            None => {
                self.console.warn("no info_player_start; spawning at the world origin");
                (Vec3::ZERO, Angles::ZERO)
            }
        };

        let player = self.entities.spawn("player");
        self.entities.player = Some(player);
        if let Some(e) = self.entities.get_mut(player) { e.origin = origin; }

        self.player = PlayerState {
            entity: Some(player),
            // Lifted slightly so the first ground trace has somewhere to land
            // rather than starting flush with the floor.
            movement: MoveState { origin: origin + Vec3::Z, ..Default::default() },
            view_angles: angles.clamped_view(),
            previous_origin: origin,
            health: 100.0,
            // Carried across a respawn rather than cleared: a player who died
            // with the use key held should have to let go and press again,
            // not immediately use whatever they spawn facing.
            use_held: self.player.use_held,
        };
    }

    /// Advance by real elapsed time, running as many fixed ticks as it covers.
    ///
    /// Returns how many ticks ran. The accumulator is capped so that a long
    /// stall -- a breakpoint, a window drag -- does not produce a burst of
    /// hundreds of catch-up ticks, which would look like the world
    /// fast-forwarding and could take longer to simulate than it did to stall.
    pub fn frame(&mut self, real_dt: f32, input: &InputState) -> usize {
        // Anything the rest of the engine logged since the last frame becomes
        // console scrollback, so the console is a view of the whole engine
        // rather than only of what was printed through it.
        if let Some(relay) = &self.log {
            let relay = std::sync::Arc::clone(relay);
            self.console.drain_log_relay(&relay);
        }
        self.console.run_buffered();

        if let Some(map) = self.pending_map.take() {
            if let Err(e) = self.load_map(&map) {
                self.console.error(format!("{e}"));
            }
        }

        let interval = self.tick_interval();
        self.accumulator = (self.accumulator + real_dt).min(interval * 8.0);

        let mut ticks = 0;
        while self.accumulator >= interval {
            self.accumulator -= interval;
            self.tick(interval, input);
            ticks += 1;
        }
        ticks
    }

    /// One fixed simulation step.
    pub fn tick(&mut self, dt: f32, input: &InputState) {
        self.time += dt;
        self.tick_count += 1;
        self.player.previous_origin = self.player.movement.origin;

        self.player.view_angles = input.view_angles.clamped_view();

        if let Some(level) = &self.level {
            // Rebuilt each tick: a door that moved since the last one has to
            // block where it is now, not where it was.
            let world = LevelCollision::new(&level.bsp, &self.entities);
            let params = self.movement_params();
            let move_input = MoveInput {
                forward: input.forward,
                side: input.side,
                up: input.up,
                jump: input.jump,
                duck: input.duck,
                view_angles: self.player.view_angles,
            };

            self.player.movement.noclip = self.console.bool("sv_noclip");
            let result =
                kerosene_physics::player_move(&mut self.player.movement, &move_input, &params, &world, dt);

            if let Some(speed) = result.landed_at_speed {
                self.apply_fall_damage(speed);
            }
        }

        // Keep the player's entity in step, so `!player` targets and trigger
        // tests both see where they actually are.
        if let Some(id) = self.player.entity {
            let origin = self.player.movement.origin;
            if let Some(e) = self.entities.get_mut(id) { e.origin = origin; }
        }

        // The ears follow the player. Done here rather than in the renderer
        // so that a headless run mixes the same audio a windowed one does.
        self.audio.set_listener(self.player.movement.eye_position(), self.player.view_angles.vectors());
        self.audio.set_volume(self.console.float("volume"));

        if input.use_key && !self.player.use_held {
            self.use_what_is_in_front();
        }
        self.player.use_held = input.use_key;

        self.update_triggers(dt);
        self.entities.run(dt);
        self.take_entity_requests();

        // Only when a script asked for it: the snapshot a hook reads is
        // O(entities) to build, and most maps define no tick hook at all.
        if self.script.has_function(kerosene_script::hooks::TICK) {
            self.call_script_hook(kerosene_script::hooks::TICK, vec![rhai::Dynamic::from(dt as f64)]);
        }
    }

    fn movement_params(&self) -> MoveParams {
        let gravity = self.console.float("sv_gravity");
        let mut params = MoveParams {
            gravity,
            max_speed: self.console.float("sv_maxspeed"),
            accelerate: self.console.float("sv_accelerate"),
            air_accelerate: self.console.float("sv_airaccelerate"),
            friction: self.console.float("sv_friction"),
            stop_speed: self.console.float("sv_stopspeed"),
            step_size: self.console.float("sv_stepsize"),
            air_speed_cap: self.console.float("sv_air_max_wishspeed"),
            ..Default::default()
        };
        // Derived rather than a convar of its own, so changing gravity keeps
        // jump height where the designer put it.
        params.jump_impulse = params.jump_for_height(self.console.float("sv_jump_height"));
        params
    }

    /// Tell every trigger whether the player is inside it, and take the
    /// damage the ones that deal it are dealing.
    fn update_triggers(&mut self, dt: f32) {
        let Some(level) = &self.level else { return };
        let hull = self.player.movement.hull();
        let player_box = Aabb::new(
            self.player.movement.origin + hull.mins,
            self.player.movement.origin + hull.maxs,
        );

        let triggers: Vec<(EntityId, usize, Vec3)> = self
            .entities
            .iter()
            .filter(|e| e.classname.to_lowercase().starts_with("trigger_"))
            .filter_map(|e| e.brush_model.map(|m| (e.id, m, e.origin)))
            .collect();

        let player_entity = self.player.entity;
        let mut hurt = 0.0;
        let mut entered: Vec<EntityId> = Vec::new();
        for (id, model_index, offset) in triggers {
            let Some(model) = level.bsp.models.get(model_index) else { continue };
            let bounds = model.bounds();
            let moved = Aabb::new(bounds.min + offset, bounds.max + offset);
            // A box overlap is enough: trigger brushes are convex volumes and
            // the exact brush test costs more than it is worth here.
            let inside = moved.intersects(&player_box);

            // Gathered before the touch update, because a `trigger_once`
            // removes itself in there and would otherwise deal nothing on the
            // tick it fired.
            let live = !self.entities.get(id).is_some_and(|e| e.fields.bool("disabled", false));
            if inside && live {
                hurt += kerosene_game::triggers::hurt_per_second(&self.entities, id) * dt;
                let was = self.entities.get(id).is_some_and(|e| e.fields.bool("occupied", false));
                if !was { entered.push(id) }
            }
            kerosene_game::triggers::update_touch(&mut self.entities, id, inside, player_entity);
        }

        // Volumes that act on the player when they arrive rather than while
        // they stay. Applied after the touch pass so that a teleport lands
        // the player somewhere the same tick's outputs have already fired
        // from -- the wire and the move belong to the same moment.
        for id in entered {
            self.enter_trigger(id);
        }

        // Applied once, after the loop: two overlapping hurt volumes should
        // cost two lots of damage, but should not be able to kill and respawn
        // the player halfway through a list they are still being iterated
        // against.
        if hurt > 0.0 {
            self.hurt_player(hurt, "hurt");
        }
    }

    /// Act on a trigger the player has just entered.
    fn enter_trigger(&mut self, id: EntityId) {
        if let Some((dir, speed)) = kerosene_game::triggers::push_of(&self.entities, id) {
            // Added to what the player already had, so running onto a pad
            // carries your speed with you instead of replacing it. Leaving
            // the ground explicitly, or the next tick's ground check would
            // flatten a straight-up launch before it started.
            self.player.movement.velocity += dir * speed;
            self.player.movement.on_ground = false;
        }

        if let Some(target) = kerosene_game::triggers::teleport_target(&self.entities, id) {
            let destination = self
                .entities
                .find_by_name(&target)
                .first()
                .copied()
                .and_then(|to| self.entities.get(to).map(|e| e.origin));
            match destination {
                Some(origin) => {
                    // The view is left alone. Turning the player's head is a
                    // thing the client owns -- angles come from input every
                    // tick, so setting them here would be overwritten before
                    // anyone saw it, and pretending otherwise would be worse
                    // than not doing it.
                    self.player.movement.origin = origin;
                    self.player.previous_origin = origin;
                    self.player.movement.velocity = Vec3::ZERO;
                    self.player.movement.on_ground = false;
                }
                None => self.console.warn(format!(
                    "trigger_teleport points at `{target}`, which is not in this map"
                )),
            }
        }
    }

    fn apply_fall_damage(&mut self, speed: f32) {
        // Below the safe speed, landing costs nothing. Above it, damage rises
        // with the excess -- Source's curve, near enough.
        let safe = self.console.float("sv_falldamage_safe");
        if speed <= safe { return; }
        let scale = self.console.float("sv_falldamage_scale");
        self.hurt_player((speed - safe) * scale, &format!("fall damage at {speed:.0} ku/s"));
    }

    /// Take health off the player, and respawn them if it runs out.
    ///
    /// One place rather than one per source of damage, because "what happens
    /// at zero" is a rule about the player and not about the thing that hurt
    /// them -- and because a second copy would be the one that forgot to
    /// respawn.
    pub fn hurt_player(&mut self, amount: f32, reason: &str) {
        if amount <= 0.0 || self.player.health <= 0.0 { return }
        self.player.health -= amount;
        self.console.developer(format!("{reason}: -{amount:.0} hp ({:.0} left)", self.player.health.max(0.0)));
        if self.player.health <= 0.0 {
            self.console.print("you died");
            self.player.health = 100.0;
            self.spawn_player();
        }
    }

    /// Press whatever the player is looking at.
    ///
    /// A trace from the eye rather than a radius around the player: standing
    /// between two buttons and pressing the one you are facing is the whole
    /// expectation, and a proximity test cannot honour it.
    fn use_what_is_in_front(&mut self) {
        let Some(level) = &self.level else { return };
        let range = self.console.float("sv_use_range").max(1.0);
        let eye = self.player.movement.eye_position();
        let end = eye + self.player.view_angles.forward() * range;

        let world = LevelCollision::new(&level.bsp, &self.entities);
        let trace = world.trace(eye, end, Vec3::ZERO, Vec3::ZERO, contents::MASK_PLAYER_SOLID);
        // Model 0 is the world itself. Walls are not usable, and reporting a
        // hit on one as a failed use would be noise on every missed press.
        if !trace.hit() || trace.model == 0 { return }

        let Some(target) = self
            .entities
            .iter()
            .find(|e| e.brush_model == Some(trace.model))
            .map(|e| e.id)
        else {
            return;
        };

        // Through the queue, so a use arrives the same way a wired output
        // would: same ordering, same delays, same rules.
        self.entities.queue_input(
            kerosene_entity::Target::Handle(target),
            "Use",
            "",
            0.0,
            self.player.entity,
            self.player.entity,
        );
    }

    /// Where the eye is, interpolated between the last two ticks.
    ///
    /// Without this the view snaps at the tick rate, which is visible as a
    /// judder on any display refreshing faster than 64 Hz -- which is all of
    /// them now.
    pub fn interpolated_eye(&self, alpha: f32) -> Vec3 {
        let hull = self.player.movement.hull();
        let position = self
            .player
            .previous_origin
            .lerp(self.player.movement.origin, alpha.clamp(0.0, 1.0));
        position + Vec3::Z * hull.view_height
    }

    /// Queue a map change for the start of the next frame.
    ///
    /// Deferred rather than immediate because a map change unloads everything
    /// the current tick is standing on.
    pub fn request_map(&mut self, name: &str) {
        self.pending_map = Some(name.to_string());
    }

    pub fn has_pending_map(&self) -> bool { self.pending_map.is_some() }

    /// Contents the player is standing in, for water and trigger checks.
    pub fn player_contents(&self) -> u32 {
        match &self.level {
            Some(level) => level.bsp.point_contents_brushes(self.player.movement.origin + Vec3::Z * 4.0),
            None => contents::EMPTY,
        }
    }
}

/// Register the engine's convars.
fn register_cvars(console: &mut Console) {
    console.register_cvar_ranged("sv_tickrate", "64", Some(10.0), Some(256.0), ConVarFlags::NONE, "Server simulation steps per second.");
    console.register_cvar("sv_cheats", "0", ConVarFlags::NOTIFY | ConVarFlags::REPLICATED, "Allow cheat commands and convars.");
    console.register_cvar("sv_gravity", "800", ConVarFlags::REPLICATED, "World gravity, in kerosene units per second squared.");
    console.register_cvar("sv_maxspeed", "320", ConVarFlags::REPLICATED, "Maximum ground speed, in kerosene units per second.");
    console.register_cvar("sv_accelerate", "10", ConVarFlags::REPLICATED, "Ground acceleration.");
    console.register_cvar("sv_airaccelerate", "10", ConVarFlags::REPLICATED, "Air acceleration.");
    console.register_cvar("sv_friction", "4", ConVarFlags::REPLICATED, "Ground friction.");
    console.register_cvar("sv_stopspeed", "100", ConVarFlags::REPLICATED, "Speed below which friction is applied as though at this speed.");
    console.register_cvar("sv_stepsize", "18", ConVarFlags::REPLICATED, "Tallest step walked up without jumping.");
    console.register_cvar("sv_jump_height", "57", ConVarFlags::REPLICATED, "Height a jump reaches, in kerosene units.");
    console.register_cvar("sv_air_max_wishspeed", "30", ConVarFlags::REPLICATED, "Air acceleration speed cap. This is what makes air strafing work.");
    console.register_cvar("sv_falldamage_safe", "580", ConVarFlags::REPLICATED, "Landing speed below which falling is harmless.");
    console.register_cvar("sv_falldamage_scale", "0.25", ConVarFlags::REPLICATED, "Damage per unit/s of landing speed above the safe threshold.");
    console.register_cvar("sv_noclip", "0", ConVarFlags::CHEAT, "Fly through walls.");
    console.register_cvar("sv_use_range", "80", ConVarFlags::REPLICATED, "How far the use key reaches, in kerosene units.");

    console.register_cvar("cl_fov", "90", ConVarFlags::ARCHIVE, "Horizontal field of view at 4:3.");
    console.register_cvar_ranged("sensitivity", "3", Some(0.01), Some(100.0), ConVarFlags::ARCHIVE, "Mouse sensitivity.");
    console.register_cvar("m_yaw", "0.022", ConVarFlags::ARCHIVE, "Yaw degrees per mouse count.");
    console.register_cvar("m_pitch", "0.022", ConVarFlags::ARCHIVE, "Pitch degrees per mouse count.");
    console.register_cvar("m_invert", "0", ConVarFlags::ARCHIVE, "Invert mouse pitch.");

    console.register_cvar("r_drawworld", "1", ConVarFlags::CHEAT, "Draw world geometry.");
    console.register_cvar("r_fullbright", "0", ConVarFlags::CHEAT, "Ignore lightmaps.");
    console.register_cvar("r_lightmap", "1", ConVarFlags::CHEAT, "Apply lightmaps.");
    console.register_cvar("r_novis", "0", ConVarFlags::CHEAT, "Ignore the PVS and draw everything.");
    console.register_cvar("r_speeds", "0", ConVarFlags::NONE, "Show per-frame render statistics.");
    console.register_cvar_ranged("mat_exposure", "1.0", Some(0.01), Some(16.0), ConVarFlags::ARCHIVE, "Overall brightness.");
    console.register_cvar("fps_max", "0", ConVarFlags::ARCHIVE, "Frame rate cap; 0 for unlimited.");

    console.register_cvar_ranged("volume", "0.7", Some(0.0), Some(1.0), ConVarFlags::ARCHIVE, "Master sound volume.");
}

/// Register the engine's commands.
///
/// Commands that need engine state set a request on the console for the host
/// to act on, rather than reaching into the engine: a `ConCommand` handler
/// only gets the console, and threading the whole engine through it would make
/// every command able to do anything.
fn register_commands(console: &mut Console) {
    console.register_command(
        "toggleconsole",
        ConVarFlags::NONE,
        "Open or close the developer console.",
        |con, _| con.request(requests::TOGGLE_CONSOLE, ""),
    );

    console.register_command("map", ConVarFlags::NONE, "Load a map: map <name>", |con, args| {
        match args.get(1) {
            Some(name) => {
                let name = name.to_string();
                con.request(requests::MAP, name);
            }
            None => con.warn("usage: map <name>"),
        }
    });

    console.register_command(
        "script",
        ConVarFlags::CHEAT,
        "Run script source: script <code>",
        |con, args| {
            // Everything after the command word, unsplit: script source has
            // spaces in it and tokenising it would be actively wrong.
            let source = args.rest.clone();
            if source.trim().is_empty() {
                con.warn("usage: script <code>");
                return;
            }
            con.request(requests::SCRIPT, source);
        },
    );

    console.register_command(
        "script_execute",
        ConVarFlags::CHEAT,
        "Load and run a script file: script_execute <name>",
        |con, args| match args.get(1) {
            Some(name) => {
                let name = name.to_string();
                con.request(requests::SCRIPT_FILE, name);
            }
            None => con.warn("usage: script_execute <name>"),
        },
    );

    console.register_command(
        "script_reload",
        ConVarFlags::CHEAT,
        "Forget every loaded script and load them again.",
        |con, _| con.request(requests::SCRIPT_RELOAD, ""),
    );

    console.register_command(
        "condump",
        ConVarFlags::NONE,
        "Write the console scrollback to a file: condump <path>",
        |con, args| {
            let path = args.get(1).unwrap_or("condump.txt").to_string();
            let text: String = con
                .log()
                .map(|line| format!("{}\n", line.text))
                .collect();
            match std::fs::write(&path, text) {
                Ok(()) => con.print(format!("wrote {} lines to {path}", con.log_len())),
                Err(e) => con.error(format!("could not write {path}: {e}")),
            }
        },
    );

    console.register_command(
        "play",
        ConVarFlags::NONE,
        "Play a sound, heard flat: play <name>",
        |con, args| match args.get(1) {
            Some(name) => {
                let name = name.to_string();
                con.request(requests::PLAY_SOUND, name);
            }
            None => con.warn("usage: play <name>"),
        },
    );

    console.register_command(
        "stopsound",
        ConVarFlags::NONE,
        "Stop every sound.",
        |con, _| con.request(requests::STOP_SOUND, ""),
    );

    console.register_command(
        "snd_restart",
        ConVarFlags::NONE,
        "Forget every loaded sound and reopen the audio device.",
        |con, _| con.request(requests::SOUND_RESTART, ""),
    );

    console.register_command("quit", ConVarFlags::NONE, "Exit.", |con, _| {
        con.request(requests::QUIT, "");
    });
    console.register_command("exit", ConVarFlags::NONE, "Exit.", |con, _| {
        con.request(requests::QUIT, "");
    });

    console.register_command("noclip", ConVarFlags::CHEAT, "Toggle flying through walls.", |con, _| {
        let on = con.bool("sv_noclip");
        con.set_bool("sv_noclip", !on);
        let state = if on { "off" } else { "on" };
        con.print(format!("noclip {state}"));
    });

    console.register_command("version", ConVarFlags::NONE, "Show the engine version.", |con, _| {
        con.print(format!("Kerosene {}", env!("CARGO_PKG_VERSION")));
    });
}

/// Poll the console for requests engine commands left behind.
pub fn take_console_requests(engine: &mut Engine) -> Vec<(String, String)> {
    let mut unhandled = Vec::new();
    for (kind, payload) in engine.console.take_requests() {
        match kind.as_str() {
            requests::MAP => engine.request_map(&payload),
            requests::QUIT => engine.should_quit = true,
            requests::SCRIPT => match engine.run_script(&payload) {
                Ok(Some(value)) => engine.console.echo(value),
                Ok(None) => {}
                Err(e) => engine.console.error(format!("script: {e}")),
            },
            requests::SCRIPT_FILE => {
                if let Err(e) = engine.load_script(&payload) {
                    engine.console.error(format!("script_execute: {e}"));
                }
            }
            requests::SCRIPT_RELOAD => engine.reload_scripts(),
            requests::PLAY_SOUND => {
                let vfs = engine.vfs.clone();
                if engine.audio.play(&vfs, &payload, None, 1.0).is_none() {
                    engine.console.warn(format!("could not play `{payload}`"));
                }
            }
            requests::STOP_SOUND => engine.audio.stop_all(),
            requests::SOUND_RESTART => {
                engine.audio = crate::audio::AudioSystem::open();
                let vfs = engine.vfs.clone();
                engine.audio.load_scripts(&vfs);
                let status = engine.audio.status.clone();
                engine.console.print(format!("audio: {status}"));
            }
            // Not ours. The console can ask for things the *host* owns --
            // opening the console itself, most obviously -- and the engine
            // has no business knowing a window exists. Handing them back
            // beats teaching it.
            _ => unhandled.push((kind, payload)),
        }
    }
    unhandled
}

/// Report requests nobody claimed.
///
/// For a caller with nothing to add -- a headless server has no console to
/// open -- so that an unrecognised request is still said out loud rather than
/// dropped on the floor.
pub fn report_unhandled(engine: &mut Engine, requests: Vec<(String, String)>) {
    for (kind, _) in requests {
        engine.console.warn(format!("unknown host request `{kind}`"));
    }
}

/// Where a map's `.kerobsp` should be, given its name.
pub fn map_path(name: &str) -> String { format!("maps/{name}.kerobsp") }

/// Whether a path looks like a map name rather than a file.
pub fn is_bare_map_name(name: &str) -> bool {
    !name.contains('/') && !name.contains('.') && Path::new(name).extension().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A content tree with the given files in it, empty.
    fn tree(name: &str, files: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kerosene-engine-maps-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for file in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"not really a map").unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vfs_over(dir: &std::path::Path) -> Vfs {
        let mut vfs = Vfs::new();
        vfs.add_directory(dir, "GAME");
        vfs
    }

    fn not_found(name: &str) -> VfsError {
        VfsError::NotFound(format!("maps/{name}.kerobsp"))
    }

    #[test]
    fn a_map_that_was_never_compiled_is_told_so_and_told_what_to_run() {
        let dir = tree("uncompiled", &["maps/arena.keromap"]);
        let said = explain_missing_map(&vfs_over(&dir), "arena", &not_found("arena"));

        assert!(said.contains("has not been compiled"), "{said}");
        assert!(said.contains("cleave maps/arena.keromap"), "{said}");
        assert!(said.contains("build-content.sh"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_map_nobody_has_heard_of_gets_the_list_of_ones_that_exist() {
        let dir = tree("wrong-name", &["maps/arena.kerobsp", "maps/lobby.kerobsp"]);
        let said = explain_missing_map(&vfs_over(&dir), "areena", &not_found("areena"));

        assert!(said.contains("arena, lobby"), "{said}");
        assert!(!said.contains("has not been compiled"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_content_tree_says_nothing_is_compiled_rather_than_nothing_exists() {
        let dir = tree("empty", &[]);
        let said = explain_missing_map(&vfs_over(&dir), "arena", &not_found("arena"));

        assert!(said.contains("no compiled maps"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_search_paths_are_always_listed() {
        let dir = tree("paths", &["maps/arena.keromap"]);
        let said = explain_missing_map(&vfs_over(&dir), "arena", &not_found("arena"));

        assert!(said.contains("searched:"), "{said}");
        assert!(said.contains(&dir.display().to_string()), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_nothing_mounted_it_says_so() {
        let said = explain_missing_map(&Vfs::new(), "arena", &not_found("arena"));
        assert!(said.contains("(nothing mounted)"), "{said}");
    }

    #[test]
    fn a_failure_that_is_not_a_missing_file_is_reported_as_itself() {
        // A truncated archive is not a map you forgot to compile, and telling
        // someone to run the compiler would send them the wrong way.
        let dir = tree("io", &["maps/arena.keromap"]);
        let said = explain_missing_map(
            &vfs_over(&dir),
            "arena",
            &VfsError::BadPath("maps/arena.kerobsp".into()),
        );

        assert!(!said.contains("has not been compiled"), "{said}");
        assert!(said.contains("not a usable virtual path"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
