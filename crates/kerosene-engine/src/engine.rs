// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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

use crate::acoustics;
use crate::collision::{LevelCollision, PlayerCollision};
use crate::game::Game;
use crate::input::InputState;
use crate::physics::PhysicsProps;
use kerosene_audio::ReverbParams;
use kerosene_bsp::{Bsp, contents};
use kerosene_console::{ConVarFlags, Console, requests};
use kerosene_entity::{ClassRegistry, EntityId, EntityWorld, SpawnError, Value};
use kerosene_math::{Aabb, Angles, Pose, Quat, Vec3};
use kerosene_physics::{MoveInput, MoveParams, MoveState};
use kerosene_script::rhai;
use kerosene_vfs::{Vfs, VfsError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod commands;
mod config;
mod debug;
mod level;
mod lifecycle;
mod player;
mod tick;
pub use commands::{SCREENSHOT, report_unhandled, take_console_requests};
use commands::{register_commands, register_cvars};
pub use config::{EngineConfig, explain_missing_map};
/// Server tick rate. 64 is Source's modern default: fine enough that
/// movement feels continuous, coarse enough to be affordable.
pub const DEFAULT_TICKRATE: f32 = 64.0;

/// A loaded level.
pub struct Level {
    pub name: String,
    /// Shared with the host's section builder, which works on it off the
    /// main thread.
    pub bsp: std::sync::Arc<Bsp>,
    /// Which streamed sections are resident. See [`crate::streaming`].
    pub streaming: crate::streaming::Streaming,
    /// What the sky is tinted, from the map's `light_environment`.
    ///
    /// Kept on the level rather than read per frame: it cannot change while a
    /// map is loaded, and the entity that names it is inert at runtime, so
    /// this is the only moment anything asks.
    pub sky_color: Vec3,
    /// Where characters may walk, from `maps/<name>.kwalk`. See
    /// [`Engine::nav`].
    pub nav: Option<crate::nav::Nav>,
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

/// Whether a classname is a trigger volume's, case-insensitively.
///
/// Compared in place: `to_lowercase` per entity per tick was a measurable
/// share of a tick's allocations for a thing that is asked every tick.
pub fn is_trigger_class(classname: &str) -> bool {
    classname
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("trigger_"))
}

/// The engine.
pub struct Engine {
    pub console: Console,
    /// The global log relay, drained into the console once a frame.
    pub(crate) log: Option<std::sync::Arc<kerosene_console::LogRelay>>,
    /// The script VM. Empty until a map with a script loads.
    pub(crate) script: kerosene_script::ScriptHost,
    /// Sound. The mixer runs whether or not a device opened.
    pub audio: crate::audio::AudioSystem,
    pub(crate) vfs: Arc<Vfs>,
    /// Compiled resources read through `vfs`, cached by path. Emptied of
    /// whatever nothing holds on every map load.
    pub(crate) resources: kerosene_resource::Resources,
    pub(crate) level: Option<Level>,
    pub entities: EntityWorld,
    /// The classes every entity world is built with: the game's, asked for
    /// once, so a map load never needs the game object itself.
    pub(crate) registry: Arc<ClassRegistry>,
    /// The game. `None` only while one of its hooks is running -- see
    /// [`crate::game`] for why.
    pub(crate) game: Option<Box<dyn Game>>,
    /// Rigid-body props and the static world they rest on.
    pub(crate) physics: PhysicsProps,
    /// Animated models' clips and skeletons, loaded as `prop_dynamic`s ask.
    pub(crate) animations: crate::animation::Animations,
    pub player: PlayerState,
    /// The prop the pick-up tool is carrying, and how to keep it facing the
    /// player as they turn.
    held_prop: Option<HeldProp>,
    /// Accumulated real time not yet simulated.
    accumulator: f32,
    /// Each moving brush model's pose as of the end of the previous tick, by
    /// BSP model index, so rendering can interpolate a door or rotating
    /// brush's motion instead of snapping it to a new pose every tick.
    pub(crate) previous_brush_poses: HashMap<usize, (Vec3, Angles)>,
    /// Bumped by every successful `load_map`, so a host can tell "the same
    /// map, loaded again" from "nothing happened" -- a name cannot.
    pub(crate) load_generation: u64,
    /// Whether this engine was asked to open an audio device, so that
    /// `snd_restart` reopens what was opened and not a device a headless
    /// run never wanted.
    wants_audio: bool,
    /// Total simulated time.
    pub(crate) time: f32,
    pub(crate) tick_count: u64,
    /// Set when the console asks for a different map.
    pub(crate) pending_map: Option<String>,
    /// A saved game to load, or a level change to make, at the start of the
    /// next frame. See [`crate::save`].
    pub(crate) pending_change: Option<crate::save::PendingChange>,
    /// Set by the `quit` command.
    pub(crate) should_quit: bool,
    /// Set by the `pause` command: stopped until it is typed again,
    /// whatever else is open.
    paused_by_command: bool,
    /// Set by the host while something that is not the world has the
    /// player's attention -- the console, the store's overlay, another
    /// window. See [`Engine::set_host_paused`].
    host_paused: bool,
    /// Set by the host while its window is in the background, for
    /// `snd_mute_losefocus`.
    background: bool,
    /// The health the player spawns with. See [`Engine::set_player_max_health`].
    pub(crate) max_health: f32,
    /// Set once [`Engine::shutdown`] has told the game.
    shut_down: bool,
    /// Owns every background thread the engine runs; see [`lifecycle`].
    pub(crate) lifecycle: kerosene_lifecycle::Manager,
    /// The workers that build level sections, if they started.
    pub(crate) workers: Option<kerosene_lifecycle::Pool>,
    /// What the game has drawn with [`Engine::debug_line`] and friends.
    pub(crate) debug_draw: crate::debug_draw::DebugDraw,
    /// The game's dice. See [`Engine::rng`].
    pub(crate) rng: kerosene_math::Rng,
    /// Key bindings, and which actions are held. The host feeds it keys;
    /// a game reads it for its own `+actions` and changes it for a key
    /// rebinding screen.
    pub input: crate::input::InputSystem,
    /// Punches, shakes, zoom and the game's camera. See [`crate::view`].
    pub(crate) view: crate::view::ViewEffects,
    /// Set when the engine points the player itself -- a spawn, a loaded
    /// save, a level change, [`Engine::set_view_angles`] -- so the rest of
    /// this frame's ticks use that facing rather than the input the host read
    /// before it happened.
    pub(crate) view_forced: bool,
    /// A save an entity asked for, made at the end of the tick.
    pub(crate) pending_save: Option<String>,
    /// The loaded map's file, watched under `map_autoreload`.
    pub(crate) map_watch: crate::hotload::MapWatch,
    /// Where New Game goes. See [`EngineConfig::new_game_map`].
    pub(crate) new_game_map: String,
    /// The game's name, for the main menu to show.
    pub(crate) title: String,
    /// Whether a load waits a frame for the loading screen to be drawn: a
    /// window's host turns it on. See [`crate::frontend`].
    pub(crate) loading_screen: bool,
    /// Whether the loading screen is up for the load about to happen.
    pub(crate) loading_shown: bool,
    /// Whether the game was started because there was no main menu to show.
    pub(crate) started_without_menu: bool,
    /// The sound each entity last started, so it can be stopped. Kept here
    /// rather than in the entity's fields: a handle means nothing to another
    /// process's mixer, and a saved one would stop some other sound.
    pub(crate) entity_voices: std::collections::HashMap<EntityId, kerosene_audio::SoundHandle>,
    /// The game UI: its store, layers, world panels and decals.
    pub ui: crate::ui::GameUi,
    /// The store the game ships on -- Steam, or nothing. See
    /// [`crate::platform`].
    pub platform: kerosene_platform::Platform,
}

/// The prop the pick-up tool is carrying, and the orientation that keeps its
/// grabbed face toward the player.
#[derive(Clone, Copy)]
struct HeldProp {
    id: EntityId,
    /// The prop's yaw minus the player's yaw at grab time, so each tick the
    /// prop's yaw is rebuilt as the player's current yaw plus this offset and
    /// the prop turns with them.
    yaw_offset: f32,
    /// The pitch and roll it was grabbed at, preserved rather than flattened,
    /// so a prop resting on its side stays on its side but still faces you.
    pitch: f32,
    roll: f32,
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
    /// Whether the attack button was down last tick, so a throw fires once on
    /// the press rather than every tick it is held.
    pub attack_held: bool,
    /// How far the player has travelled on the ground since the last footstep,
    /// so footstep sounds land at a stride rather than every tick.
    pub step_distance: f32,
    /// Which footstep sound played last, so left/right steps alternate.
    pub step_index: u8,
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
            attack_held: false,
            step_distance: 0.0,
            step_index: 0,
        }
    }
}

impl Engine {
    /// An engine with no game: no entity classes beyond what the engine
    /// itself needs, nothing on any hook. What tests and a bare server use.
    pub fn new(config: &EngineConfig) -> Engine {
        Engine::with_game(config, Box::new(()))
    }

    /// An engine running `game`.
    ///
    /// The game is asked for its classes here and set up after the console
    /// exists but before the saved settings, the autoexec and the command
    /// line run, so anything it registers is usable from all three.
    pub fn with_game(config: &EngineConfig, mut game: Box<dyn Game>) -> Engine {
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
        // The store comes up before the file system is sealed, because
        // subscribed Workshop items are more archives to mount: a map from
        // the Workshop is one more layer, found like any other.
        let mut platform = kerosene_platform::Platform::new(config.platform.clone());
        for (id, dir) in platform.workshop_items() {
            match vfs.mount_archives_in(&dir, "WORKSHOP") {
                Ok(0) => {}
                Ok(n) => {
                    log::info!("workshop item {id}: mounted {n} archive(s)");
                    platform.note_workshop_mounted(id, dir);
                }
                Err(e) => log::warn!("workshop item {id}: {e}"),
            }
        }
        // Last of all, the engine's own: the textures, sounds and UI a game
        // has before it has any, found only where nothing above has them.
        if config.base_content {
            crate::base::mount(&mut vfs);
        }
        // First of all, the player's own: written to before anything else,
        // and read first, so their config and saves are what is found. The
        // content tree stays mounted beneath it, so saves an older build left
        // there are still found.
        if let Some(dir) = &config.user_dir {
            match std::fs::create_dir_all(dir) {
                Ok(()) => {
                    vfs.add_directory_front(dir, "USER");
                    log::info!("player files: {}", dir.display());
                }
                Err(e) => log::warn!(
                    "could not make {} ({e}); keeping player files in the content tree",
                    dir.display()
                ),
            }
        }
        // Loose files win over packed ones, so a developer can drop a file
        // beside a shipped archive and see it immediately.
        let vfs = Arc::new(vfs);

        let mut console = Console::new();
        register_cvars(&mut console);
        // The launch config says first; a saved config.cfg, run later, has
        // the last word.
        console.set("r_vsync", if config.vsync { "1" } else { "0" });
        register_commands(&mut console);
        debug::register(&mut console);
        crate::frontend::register(&mut console);
        crate::ui::register(&mut console);
        crate::platform::register(&mut console);
        crate::save::register(&mut console);
        // Who is running, for `version`: read-only in all but name, since
        // nothing sets it after this.
        let running = match config.version.is_empty() {
            true => String::new(),
            false => format!("{} {}", config.title, config.version),
        };
        console.register_cvar(
            "_game",
            &running,
            ConVarFlags::HIDDEN,
            "The game's name and version, as it launched.",
        );

        // Wire `exec` to the filesystem.
        let exec_vfs = vfs.clone();
        console.set_exec_handler(move |name| {
            let path = if name.contains('/') {
                name.to_string()
            } else {
                format!("cfg/{name}")
            };
            exec_vfs.read_string(&path).ok()
        });

        // Logging convars, wired to the relay if there is one. Both are
        // no-ops without it, which is the headless and test case.
        console.register_cvar(
            "con_logfile",
            "",
            ConVarFlags::NONE,
            "Write every log line to this file. Empty closes it.",
        );
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

        let mut registry = ClassRegistry::new();
        game.classes(&mut registry);
        let registry = Arc::new(registry);
        let entities = EntityWorld::new(registry.clone());

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
            resources: kerosene_resource::Resources::new(),
            level: None,
            entities,
            registry,
            game: None,
            physics: PhysicsProps::new(),
            animations: crate::animation::Animations::new(),
            player: PlayerState::default(),
            held_prop: None,
            accumulator: 0.0,
            previous_brush_poses: HashMap::new(),
            load_generation: 0,
            wants_audio: config.audio,
            time: 0.0,
            tick_count: 0,
            pending_map: config.map.clone(),
            pending_change: None,
            should_quit: false,
            paused_by_command: false,
            host_paused: false,
            background: false,
            max_health: 100.0,
            shut_down: false,
            lifecycle: kerosene_lifecycle::Manager::new(),
            workers: None,
            debug_draw: Default::default(),
            rng: kerosene_math::Rng::default(),
            input: crate::input::InputSystem::new(),
            view: Default::default(),
            view_forced: false,
            pending_save: None,
            map_watch: Default::default(),
            new_game_map: config
                .new_game_map
                .clone()
                .or_else(|| config.map.clone())
                .unwrap_or_else(|| crate::base::FALLBACK_MAP.to_string()),
            loading_screen: false,
            loading_shown: false,
            started_without_menu: false,
            title: config.title.clone(),
            entity_voices: Default::default(),
            ui: crate::ui::GameUi::default(),
            platform,
        };

        let vfs = engine.vfs.clone();
        engine.audio.load_scripts(&vfs);

        // Engine, then module crates, then the game: its own threads, if it
        // has any, come up after everything it builds on.
        let (manager, workers) = lifecycle::start(game.modules());
        engine.lifecycle = manager;
        engine.workers = workers;

        game.setup(&mut engine);
        engine.game = Some(game);
        debug::register_completers(&mut engine);
        engine.load_console_history();
        let title = engine.title.clone();
        engine.ui_set("game.title", title);

        // Saved settings first, then the person's own autoexec, then the
        // command line -- so each later one can overrule the one before it.
        // `config.cfg` is written by the host on quit (`Engine::config_text`)
        // and is where archived convars and bindings come back from; a first
        // run has neither file, and says nothing about it.
        for cfg in ["config.cfg", "autoexec.cfg"] {
            if engine.vfs.exists(&format!("cfg/{cfg}")) {
                engine.console.enqueue(format!("exec {cfg}"));
            }
        }
        for command in &config.startup_commands {
            engine.console.enqueue(command.clone());
        }
        engine
    }

    /// The text of `cfg/config.cfg`: every archived convar that is not at
    /// its default, and every binding, as console lines.
    ///
    /// Written by the host on quit and exec'd on the next start, which is
    /// how `sensitivity`, `cl_fov` and a rebound key survive a restart.
    pub fn config_text(&self, bindings: &str) -> String {
        let mut out = String::from(
            "// Written by Kerosene on exit. Edit autoexec.cfg for settings of \
             your own; this file is overwritten.\n",
        );
        for (name, value) in self.console.archived() {
            out.push_str(&format!("{name} \"{value}\"\n"));
        }
        if !bindings.is_empty() {
            out.push_str("unbindall\n");
            out.push_str(bindings);
            out.push('\n');
        }
        out
    }

    pub fn tick_rate(&self) -> f32 {
        self.console.float("sv_tickrate").max(1.0)
    }

    /// How far through the current tick interval the real clock has got, in
    /// `0..1`: the blend a renderer wants between the previous and current
    /// simulation states.
    pub fn interpolation_alpha(&self) -> f32 {
        (self.accumulator / self.tick_interval()).clamp(0.0, 1.0)
    }

    /// Game time to draw at: the last tick's, less the part of a tick not yet
    /// simulated, so an animation moves smoothly between ticks the way the
    /// camera and the props do.
    pub fn render_time(&self, alpha: f32) -> f32 {
        self.entities.time - (1.0 - alpha) * self.tick_interval()
    }

    /// See `load_generation`.
    pub fn load_generation(&self) -> u64 {
        self.load_generation
    }

    pub fn tick_interval(&self) -> f32 {
        1.0 / self.tick_rate()
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
        if !self.hold_for_loading_screen() {
            self.load_pending_map();
        }
        self.watch_map(real_dt);

        // Every frame, paused or not: a menu animates and a death screen
        // counts down while the world is stopped.
        self.with_game_mut(|game, engine| game.frame(engine, real_dt));

        self.update_volume();

        // Paused, the clock does not run at all: nothing accumulates, so
        // unpausing does not arrive as a burst of catch-up ticks.
        if self.is_paused() {
            self.accumulator = 0.0;
            return 0;
        }

        let interval = self.tick_interval();
        // `host_timescale` stretches real time, not the tick: each tick is
        // the same length, and more or fewer of them run.
        let scaled = real_dt * self.console.float("host_timescale").clamp(0.01, 10.0);
        self.accumulator = (self.accumulator + scaled).min(interval * 8.0);

        let mut input = *input;
        let mut ticks = 0;
        while self.accumulator >= interval {
            self.accumulator -= interval;
            if std::mem::take(&mut self.view_forced) {
                input.view_angles = self.player.view_angles;
            }
            self.tick(interval, &input);
            ticks += 1;
        }
        ticks
    }

    /// Simulated time since the engine started, in seconds.
    pub fn time(&self) -> f32 {
        self.time
    }

    /// Fixed ticks simulated since the engine started.
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// The engine's random number generator: what a game rolls its dice
    /// with. Seeded from the map's name when one loads and kept in saved
    /// games, so the same start and the same inputs give the same game.
    pub fn rng(&mut self) -> &mut kerosene_math::Rng {
        &mut self.rng
    }

    /// Tell the game the engine is going away, once: the host calls this as
    /// it exits, windowed or headless. Anything after it is a mistake, so
    /// the engine should be dropped next.
    pub fn shutdown(&mut self) {
        if std::mem::replace(&mut self.shut_down, true) {
            return;
        }
        self.with_game_mut(|game, engine| game.shutdown(engine));
        // After the game, reverse of how it all came up. Pending section
        // builds are dropped; ones under way finish first.
        self.workers = None;
        self.lifecycle.stop();
    }

    /// Ask the host to exit at the end of this frame: what `quit` does.
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Whether something asked to exit.
    pub fn quit_requested(&self) -> bool {
        self.should_quit
    }

    /// Whether a map is loaded.
    pub fn has_level(&self) -> bool {
        self.level.is_some()
    }

    /// The loaded map's name.
    pub fn map_name(&self) -> Option<&str> {
        self.level.as_ref().map(|l| l.name.as_str())
    }

    /// The content the engine reads from: loose directories, archives and
    /// the base content, as one tree.
    pub fn vfs(&self) -> &Arc<Vfs> {
        &self.vfs
    }

    /// The loaded map, BSP and all. Outside the SemVer promise: see
    /// `kerosene::internals`.
    #[doc(hidden)]
    pub fn level(&self) -> Option<&Level> {
        self.level.as_ref()
    }

    /// The rigid-body props. Outside the SemVer promise.
    #[doc(hidden)]
    pub fn physics(&self) -> &PhysicsProps {
        &self.physics
    }

    /// The rigid-body props, to change. Outside the SemVer promise.
    #[doc(hidden)]
    pub fn physics_mut(&mut self) -> &mut PhysicsProps {
        &mut self.physics
    }

    /// Animated models' clips and skeletons. Outside the SemVer promise.
    #[doc(hidden)]
    pub fn animations(&self) -> &crate::animation::Animations {
        &self.animations
    }

    /// Animated models, to load and pose. Outside the SemVer promise.
    #[doc(hidden)]
    pub fn animations_mut(&mut self) -> &mut crate::animation::Animations {
        &mut self.animations
    }

    /// An animated entity's pose now, as skinning matrices. Outside the
    /// SemVer promise.
    #[doc(hidden)]
    pub fn animated_palette(&mut self, id: EntityId) -> Option<Vec<kerosene_math::Mat4>> {
        let now = self.entities.time;
        self.animations.palette(&self.vfs, &self.entities, id, now)
    }

    /// The map script's VM. Outside the SemVer promise; a game talks to
    /// scripts through [`Engine::run_script`] and entity I/O.
    #[doc(hidden)]
    pub fn script(&self) -> &kerosene_script::ScriptHost {
        &self.script
    }

    /// The process log relay the console drains, if one was installed.
    #[doc(hidden)]
    pub fn log_relay(&self) -> Option<&std::sync::Arc<kerosene_console::LogRelay>> {
        self.log.as_ref()
    }

    /// Whether the world is stopped: by the `pause` command, or -- with
    /// `sv_pause_on_menu`, the default -- by the pause menu being open or the
    /// host saying something else has the player's attention.
    ///
    /// A paused engine still runs its console, loads maps and draws; only
    /// the ticks stop. Nothing is paused without a level, since there is
    /// nothing to stop.
    pub fn is_paused(&self) -> bool {
        if self.level.is_none() {
            return false;
        }
        if self.paused_by_command {
            return true;
        }
        self.console.bool("sv_pause_on_menu")
            && (self.host_paused || self.ui.system.is_visible(crate::ui::MENU_LAYER))
    }

    /// Tell the engine whether something outside the world has the player:
    /// the console open, the store's overlay up, the window in the
    /// background. Honoured by [`Engine::is_paused`] under
    /// `sv_pause_on_menu`.
    pub fn set_host_paused(&mut self, paused: bool) {
        self.host_paused = paused;
    }

    /// Tell the engine whether its window is in the background, where
    /// `snd_mute_losefocus` silences it.
    pub fn set_background(&mut self, background: bool) {
        self.background = background;
    }

    /// Spawn an entity of any class, with keyvalues, as if the map had it:
    /// `origin`, `angles`, `model`, `targetname` and the rest, read the way
    /// the map loader reads them, then its spawn handler runs. What a game
    /// uses to put an NPC, a pickup or a projectile into the world.
    ///
    /// ```ignore
    /// let id = engine.spawn_entity("item_pickup", &[
    ///     ("origin", "128 0 16"),
    ///     ("item", "gem"),
    /// ]);
    /// ```
    pub fn spawn_entity(&mut self, classname: &str, keys: &[(&str, &str)]) -> EntityId {
        self.entities.spawn_with(classname, keys)
    }

    /// Spawn a physics prop at a point, with a named model.
    ///
    /// Used by `phys_spawn` and by a `prop_dynamic_spawner` when it fires. The
    /// body is not made here: the next [`PhysicsProps::sync_and_step`] pass
    /// notices the new entity and gives it one, which is the one path every
    /// prop takes however it came to exist.
    pub fn spawn_prop(&mut self, model: &str, origin: Vec3) -> EntityId {
        let id = self.entities.spawn("prop_physics");
        if let Some(e) = self.entities.get_mut(id) {
            e.origin = origin;
            e.fields.set("model", Value::Text(model.to_string()));
        }
        id
    }
}

/// Where a map's `.kbsp` should be, given its name.
pub fn map_path(name: &str) -> String {
    format!("maps/{name}.kbsp")
}

/// Whether a path looks like a map name rather than a file.
pub fn is_bare_map_name(name: &str) -> bool {
    !name.contains('/') && !name.contains('.') && Path::new(name).extension().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_math::angle_diff;

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
        VfsError::NotFound(format!("maps/{name}.kbsp"))
    }

    #[test]
    fn a_map_that_was_never_compiled_is_told_so_and_told_what_to_run() {
        let dir = tree("uncompiled", &["maps/arena.kmap"]);
        let said = explain_missing_map(&vfs_over(&dir), "arena", &not_found("arena"));

        assert!(said.contains("has not been compiled"), "{said}");
        assert!(
            said.contains("kerosene-tools cleave maps/arena.kmap"),
            "{said}"
        );
        assert!(said.contains("kerosene-tools play"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_map_nobody_has_heard_of_gets_the_list_of_ones_that_exist() {
        let dir = tree("wrong-name", &["maps/arena.kbsp", "maps/lobby.kbsp"]);
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
        let dir = tree("paths", &["maps/arena.kmap"]);
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
    fn interpolated_brush_poses_land_exactly_on_the_endpoints() {
        // Renderers rely on alpha=0/1 being exact: a frame drawn right on a
        // tick boundary must show the real pose, not something a hair off it
        // from floating-point slop in the blend.
        let mut engine = Engine::new(&EngineConfig::default());
        let id = engine.entities.spawn("door");
        let previous = Vec3::new(0.0, 0.0, 0.0);
        let current = Vec3::new(0.0, 0.0, 128.0);
        if let Some(e) = engine.entities.get_mut(id) {
            e.brush_model = Some(1);
            e.origin = current;
            e.angles = Angles::new(0.0, 90.0, 0.0);
        }
        engine
            .previous_brush_poses
            .insert(1, (previous, Angles::ZERO));

        let at_start = engine.interpolated_brush_model_poses(0.0);
        let at_end = engine.interpolated_brush_model_poses(1.0);
        let (_, start_pose) = at_start.iter().find(|(m, _)| *m == 1).unwrap();
        let (_, end_pose) = at_end.iter().find(|(m, _)| *m == 1).unwrap();

        assert_eq!(start_pose.origin, previous);
        assert_eq!(start_pose.angles, Angles::ZERO);
        assert_eq!(end_pose.origin, current);
        assert_eq!(end_pose.angles, Angles::new(0.0, 90.0, 0.0));
    }

    #[test]
    fn interpolated_brush_poses_blend_at_the_midpoint() {
        let mut engine = Engine::new(&EngineConfig::default());
        let id = engine.entities.spawn("door");
        if let Some(e) = engine.entities.get_mut(id) {
            e.brush_model = Some(1);
            e.origin = Vec3::new(0.0, 0.0, 128.0);
            e.angles = Angles::new(0.0, 90.0, 0.0);
        }
        engine
            .previous_brush_poses
            .insert(1, (Vec3::ZERO, Angles::ZERO));

        let mid = engine.interpolated_brush_model_poses(0.5);
        let (_, pose) = mid.iter().find(|(m, _)| *m == 1).unwrap();
        assert!((pose.origin.z - 64.0).abs() < 1e-4, "{pose:?}");
        assert!((angle_diff(pose.angles.yaw, 45.0)).abs() < 0.01, "{pose:?}");
    }

    #[test]
    fn a_brush_model_with_no_recorded_history_is_not_interpolated() {
        // A model that appeared this tick (or before `Engine` ever tracked
        // it) has nothing to blend from; it should render at its current
        // pose rather than at some default like the world origin.
        let mut engine = Engine::new(&EngineConfig::default());
        let id = engine.entities.spawn("door");
        let current = Vec3::new(12.0, -4.0, 8.0);
        if let Some(e) = engine.entities.get_mut(id) {
            e.brush_model = Some(1);
            e.origin = current;
        }

        let poses = engine.interpolated_brush_model_poses(0.5);
        let (_, pose) = poses.iter().find(|(m, _)| *m == 1).unwrap();
        assert_eq!(pose.origin, current);
    }

    #[test]
    fn a_failure_that_is_not_a_missing_file_is_reported_as_itself() {
        // A truncated archive is not a map you forgot to compile, and telling
        // someone to run the compiler would send them the wrong way.
        let dir = tree("io", &["maps/arena.kmap"]);
        let said = explain_missing_map(
            &vfs_over(&dir),
            "arena",
            &VfsError::BadPath("maps/arena.kbsp".into()),
        );

        assert!(!said.contains("has not been compiled"), "{said}");
        assert!(said.contains("not a usable virtual path"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
