// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Starting a game from the command line: what a game's `main` is.
//!
//! ```text
//! mygame +map mg_intro
//! mygame +map mg_intro +sv_gravity 200 +developer 1
//! mygame --headless 600 +map mg_intro     # simulate without a display
//! ```
//!
//! Arguments beginning with `+` are console commands, exactly as Source's
//! are, so anything settable at the console is settable on the command line
//! with no separate flag needing to exist for it.
//!
//! `--headless` runs the simulation with no window at all. That mode is not
//! a testing convenience bolted on the side: it is what a dedicated server
//! is, and the engine is structured so that it needs nothing from the
//! renderer.
//!
//! Everything a runtime binary does before the engine exists is here --
//! the logger, the arguments, finding the content tree, the saved engine
//! settings, the archives -- so that a game's binary is one call:
//!
//! ```no_run
//! # struct MyGame; impl kerosene_engine::Game for MyGame {}
//! use kerosene_engine::launch::{LaunchOptions, launch};
//!
//! fn main() -> anyhow::Result<()> {
//!     launch(MyGame, LaunchOptions::new("My Game", env!("CARGO_PKG_VERSION")))
//! }
//! ```
//!
//! The stock `kerosene` runtime is exactly that call with the stock game.

use crate::engine::{Engine, EngineConfig, report_unhandled, take_console_requests};
use crate::game::Game;
use crate::input::InputState;
use anyhow::Result;
use kerosene_math::Angles;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Who is launching: the game's name and version, and what it wants said
/// about it. Built with [`LaunchOptions::new`]:
///
/// ```
/// # use kerosene_engine::launch::LaunchOptions;
/// let options = LaunchOptions::new("My Game", env!("CARGO_PKG_VERSION"))
///     .extra_help("game options:\n  --hard  Start on hard.");
/// assert_eq!(options.app_id, "my-game");
/// ```
///
/// Non-exhaustive, so a new option is never a breaking change for a game.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct LaunchOptions {
    /// The game's name: the window's title, `--help`'s first word and the
    /// crash box's heading.
    pub name: &'static str,
    /// The game's own version, not Kerosene's: pass
    /// `env!("CARGO_PKG_VERSION")` from the game's crate.
    pub version: &'static str,
    /// What the desktop knows the game as: the Wayland app id and X11
    /// `WM_CLASS`, which a `.desktop` file's icon is matched by. The name,
    /// lowercased with spaces as dashes, unless set.
    pub app_id: String,
    /// Lines appended to `--help`, for a game with flags of its own to
    /// mention. Empty for none.
    pub extra_help: &'static str,
    /// The arguments to parse, without the program name. `None` reads the
    /// process's own.
    pub args: Option<Vec<String>>,
}

impl LaunchOptions {
    /// A game called `name`, at `version` -- its own version, from its own
    /// `Cargo.toml`.
    pub fn new(name: &'static str, version: &'static str) -> Self {
        LaunchOptions {
            name,
            version,
            app_id: app_id_for(name),
            extra_help: "",
            args: None,
        }
    }

    /// Say what the desktop should know the game as. See
    /// [`LaunchOptions::app_id`](struct.LaunchOptions.html#structfield.app_id).
    pub fn app_id(mut self, id: impl Into<String>) -> Self {
        self.app_id = id.into();
        self
    }

    /// Add lines to the end of `--help`.
    pub fn extra_help(mut self, text: &'static str) -> Self {
        self.extra_help = text;
        self
    }

    /// Parse these arguments rather than the process's own.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = Some(args.into_iter().map(Into::into).collect());
        self
    }
}

/// A name as an app id: lowercase, with anything but letters and digits a
/// single dash. "My Game!" is `my-game`.
fn app_id_for(name: &str) -> String {
    let mut id = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    let id = id.trim_end_matches('-');
    if id.is_empty() {
        "kerosene".to_string()
    } else {
        id.to_string()
    }
}

/// Start `game` the way the `kerosene` binary starts the stock one.
///
/// Installs the engine's logger and crash handler, parses the arguments,
/// finds the content tree and the project file, reads the saved engine
/// settings, mounts every archive in the tree, and then either opens a
/// window or runs headless for the asked number of ticks.
pub fn launch(game: impl Game, options: LaunchOptions) -> Result<()> {
    // The engine's own relay rather than env_logger: everything logged
    // anywhere in the engine has to be readable from the in-game console, and
    // a logger that only writes to stderr cannot do that.
    let log = kerosene_console::install_logger(kerosene_console::logging::level_from_env(
        log::LevelFilter::Info,
    ));
    kerosene_console::install_crash_handler(Some(log.clone()));

    let args: Vec<String> = options
        .args
        .clone()
        .unwrap_or_else(|| std::env::args().skip(1).collect());
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", help_text(&options));
        return Ok(());
    }

    let parsed = parse_args(&args)?;
    let headless = parsed.headless_ticks;
    let mut config = config_from(parsed, Some(log));
    config.title = options.name.to_string();
    config.app_id = options.app_id.clone();
    config.version = options.version.to_string();
    log::info!(
        "{} {} (Kerosene {})",
        options.name,
        options.version,
        crate::VERSION
    );
    // A player with a window has no terminal to read a crash report in.
    if headless.is_none() {
        kerosene_console::logging::crash_dialog(options.name);
    }

    // Started from its folder rather than from Steam, a Steam game has no
    // Steam session. Valve's answer is to start it again through the client
    // and quit this copy; `restart_through_steam` knows when not to (debug
    // builds, a `steam_appid.txt` beside the executable).
    if config.platform.use_steam
        && let Some(appid) = config.platform.steam_appid
        && kerosene_platform::restart_through_steam(appid)
    {
        log::info!("relaunching through Steam");
        return Ok(());
    }

    match headless {
        Some(ticks) => run_headless(config, Box::new(game), ticks),
        None => crate::host::run_with(config, Box::new(game)),
    }
}

/// The command line, taken apart.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedArgs {
    pub content_paths: Vec<PathBuf>,
    pub archives: Vec<PathBuf>,
    pub map: Option<String>,
    pub commands: Vec<String>,
    pub headless_ticks: Option<u64>,
    /// `--no-steam`: run without the store even when the build and the
    /// project have it.
    pub no_steam: bool,
}

/// Take a command line apart: `--content`, `--vault`, `--headless`, and
/// `+command args` for everything else, with `+map` lifted out because the
/// engine needs it before the console starts.
pub fn parse_args(args: &[String]) -> Result<ParsedArgs> {
    let mut parsed = ParsedArgs::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--content" => {
                let value = next(args, &mut i, "--content")?;
                parsed.content_paths.push(PathBuf::from(value));
            }
            "--vault" => {
                let value = next(args, &mut i, "--vault")?;
                parsed.archives.push(PathBuf::from(value));
            }
            "--no-steam" => parsed.no_steam = true,
            "--headless" => {
                let value = next(args, &mut i, "--headless")?;
                parsed.headless_ticks = Some(value.parse()?);
            }
            other if other.starts_with('+') => {
                let command = other.trim_start_matches('+').to_string();
                let mut parts = vec![command.clone()];
                while i + 1 < args.len()
                    && !args[i + 1].starts_with('+')
                    && !args[i + 1].starts_with("--")
                {
                    i += 1;
                    parts.push(args[i].clone());
                }
                if command == "map" && parts.len() > 1 {
                    parsed.map = Some(parts[1].clone());
                } else {
                    parsed.commands.push(parts.join(" "));
                }
            }
            other => anyhow::bail!("unrecognised argument {other:?}. Try --help."),
        }
        i += 1;
    }

    Ok(parsed)
}

/// Turn parsed arguments into an engine config, finding whatever they did
/// not say: the content tree, the project's start map, the saved engine
/// settings and the archives in the tree.
pub fn config_from(
    parsed: ParsedArgs,
    log: Option<Arc<kerosene_console::LogRelay>>,
) -> EngineConfig {
    // A headless run is a server or a test: it has no player signed in to a
    // store, and no business asking the Steam client for one.
    let wants_store = parsed.headless_ticks.is_none() && !parsed.no_steam;
    let explicit_content = parsed.content_paths.first().cloned();
    let mut config = EngineConfig {
        // Headless has no listener, so opening a sound card would be work
        // nobody can hear.
        audio: parsed.headless_ticks.is_none(),
        log,
        content_paths: parsed.content_paths,
        archives: parsed.archives,
        map: parsed.map,
        startup_commands: parsed.commands,
        ..Default::default()
    };
    if config.content_paths.is_empty() {
        // Searched for, not assumed. `./content` is only right when the game
        // is started from the repository root; started any other way it
        // mounted a directory that did not exist and then reported every
        // asset in the game as missing. The editor and the compilers find the
        // tree the same way, from the same code, so they cannot disagree.
        let found = kerosene_vfs::root::find(None, None);
        match found {
            Some(found) => {
                log::info!("{}", kerosene_vfs::root::describe(&Some(found.clone())));
                // A tree that is missing a directory is a tree where some tool
                // is about to look broken. Make the ones that are not there,
                // taking the project's word for the layout when it gives one.
                kerosene_vfs::root::scaffold(
                    &found.root,
                    found.project.as_ref().and_then(|p| p.dirs.as_deref()),
                );
                // A project that names a start map is answering the question
                // the binary with no arguments is otherwise stuck on: a game
                // launched from a shortcut has nobody to type `+map` for it.
                if config.map.is_none()
                    && let Some(project) = &found.project
                    && let Some(start) = &project.start_map
                {
                    log::info!("{}: starting on {start}", project.name);
                    config.map = Some(start.clone());
                }
                config.content_paths.push(found.root);
            }
            None => {
                // Not an error any more: the engine's base content is a
                // game's worth of textures, sounds, UI and a map to walk
                // around. `content/` is where anything it writes goes.
                log::info!(
                    "no content tree found: running on the engine's base content. \
                     Start from a project directory, or pass --content, for a game's own."
                );
                config.content_paths.push(PathBuf::from("content"));
            }
        }
    }
    // With nothing saying which map, the base content's demo rather than an
    // empty window.
    if config.map.is_none() && config.base_content {
        log::info!("no start map: opening {}", crate::base::DEMO_MAP);
        config.map = Some(crate::base::DEMO_MAP.to_string());
    }

    // What the project declares for the store: the Steam app id, its
    // achievements, stats and DLC. Read from the project whichever way the
    // content was found, since `--content` names a tree, not a game.
    let project =
        kerosene_vfs::root::find(explicit_content.as_deref(), None).and_then(|found| found.project);
    if let Some(project) = project {
        config.platform = platform_config(&project);
    }
    config.platform.use_steam = wants_store;

    // The engine config always exists: read it out of the content tree,
    // writing the defaults the first time anything runs. It is where the
    // renderer is chosen, so it is read before the window is made.
    // A tree that is not there -- a game running on the base content alone --
    // runs on the defaults rather than creating a directory to write them to.
    if let Some(root) = config.content_paths.first().filter(|r| r.is_dir()).cloned() {
        let conf = kerosene_config::EngineConf::load_or_create(&root);
        config.renderer = conf.renderer;
        config.window_width = conf.width;
        config.window_height = conf.height;
        config.vsync = conf.vsync;
    }

    // A vault sitting in the content tree is mounted without being asked for.
    // That is what shipping looks like: the game a player installs has its
    // content packed, and needing a command-line flag to see it would mean
    // the shipped game only ran when launched from a script.
    if config.archives.is_empty() {
        for root in config.content_paths.clone() {
            config.archives.extend(vaults_in(&root));
        }
    }
    config
}

/// The store settings a project declares.
pub fn platform_config(project: &kerosene_vfs::Project) -> kerosene_platform::PlatformConfig {
    kerosene_platform::PlatformConfig {
        steam_appid: project.steam_appid,
        use_steam: false,
        achievements: project.achievements.clone(),
        stats: project
            .stats
            .iter()
            .map(|(name, kind)| (name.clone(), kerosene_platform::StatKind::parse(kind)))
            .collect(),
        dlc: project.dlc.clone(),
        local_cloud: None,
    }
}

/// Requests only a window has anything to do with. Headless, there is no
/// keyboard to bind and no console to open, and `config.cfg` is full of
/// bindings; saying so for each would bury every warning that matters.
fn host_only(kind: &str) -> bool {
    use kerosene_console::requests::*;
    [BIND, UNBIND, UNBIND_ALL, BIND_LIST, TOGGLE_CONSOLE].contains(&kind)
}

/// Run the simulation with no display for `ticks` ticks, then report.
pub fn run_headless(config: EngineConfig, game: Box<dyn Game>, ticks: u64) -> Result<()> {
    let mut engine = Engine::with_game(&config, game);
    engine.console.run_buffered();
    let mut unclaimed = take_console_requests(&mut engine);
    unclaimed.retain(|(kind, _)| !host_only(kind));
    report_unhandled(&mut engine, unclaimed);

    // The configured map is already pending inside the engine; taking it
    // here rather than loading it again keeps one load, and one error.
    if let Some(map) = engine.take_pending_map() {
        engine.load_map(&map)?;
    }

    let interval = engine.tick_interval();
    // Walking forward the whole time, so the run exercises movement,
    // collision and triggers rather than only the entity queue.
    let input = InputState {
        forward: 1.0,
        view_angles: Angles::ZERO,
        ..Default::default()
    };

    let started = std::time::Instant::now();
    for _ in 0..ticks {
        engine.tick(interval, &input);
        engine.platform_frame(interval);
        engine.console.run_buffered();
        let mut unclaimed = take_console_requests(&mut engine);
        unclaimed.retain(|(kind, _)| !host_only(kind));
        report_unhandled(&mut engine, unclaimed);
        // `map` from a script or a startup command lands here, since there
        // is no `frame` to pick it up.
        engine.load_pending_map();
        if engine.should_quit {
            break;
        }
    }
    let elapsed = started.elapsed().as_secs_f32();

    let simulated = engine.tick_count as f32 * interval;
    println!("--- headless run ---");
    println!(
        "  {} ticks ({simulated:.1}s simulated in {elapsed:.2}s real)",
        engine.tick_count
    );
    if let Some(level) = &engine.level {
        println!(
            "  map: {} ({} faces, {} leaves, {} clusters)",
            level.name,
            level.bsp.faces.len(),
            level.bsp.leaves.len(),
            level.bsp.num_clusters()
        );
    }
    println!("  entities: {}", engine.entities.len());
    println!(
        "  physics: {} props, {} static hulls, {} movers, {} bodies",
        engine.physics.prop_count(),
        engine.physics.static_body_count(),
        engine.physics.mover_count(),
        engine.physics.body_count()
    );
    let player = &engine.player;
    println!("  player at {:?}", player.movement.origin);
    println!(
        "  speed {}, on ground: {}",
        kerosene_math::units::speed(player.movement.ground_speed()),
        player.movement.on_ground
    );
    println!("  health {:.0}", player.health);

    // Anything the run logged as a problem is worth surfacing: a headless run
    // is often the only place anyone reads it.
    let problems: Vec<&str> = engine
        .console
        .log()
        .filter(|l| {
            matches!(
                l.level,
                kerosene_console::LogLevel::Warning | kerosene_console::LogLevel::Error
            )
        })
        .map(|l| l.text.as_str())
        .collect();
    if problems.is_empty() {
        println!("  no warnings");
    } else {
        println!("  {} warnings:", problems.len());
        for p in problems.iter().take(20) {
            println!("    {p}");
        }
    }
    Ok(())
}

/// Every `.vault` archive in a directory, in a stable order.
///
/// Sorted by name so two machines mount the same archives in the same order,
/// and so `pak01` comes before `pak02` -- with loose files still winning over
/// both, which is what makes dropping a file beside a shipped archive work.
pub fn vaults_in(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "vault"))
        .collect();
    found.sort();
    found
}

fn next<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str> {
    *i += 1;
    args.get(*i)
        .map(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("{flag} needs a value"))
}

/// What `--help` prints.
pub fn help_text(options: &LaunchOptions) -> String {
    let name = options.name;
    let bin = name.to_lowercase().replace(' ', "-");
    let mut out = format!(
        "{name} {}

usage: {bin} [options] [+command ...]

options:
  --content <dir>     Mount a content directory. Repeatable; searched in order.
                      With none, the content tree is found: from the working
                      directory, then beside the executable.
  --vault <file>      Mount a .vault archive. With none, every .vault in the
                      content tree is mounted.
  --headless <ticks>  Simulate without a window, then report. This is what a
                      dedicated server runs.
  --no-steam          Run without Steam even when the build and the project
                      have it.
  --help              Show this.

With no +map, the project's `startmap` is loaded if it names one.

Anything starting with + is a console command, so any convar can be set:
  {bin} +map kero_start
  {bin} +map kero_start +sv_gravity 200 +developer 1
  {bin} --headless 600 +map kero_start
",
        options.version
    );
    if !options.extra_help.is_empty() {
        out.push('\n');
        out.push_str(options.extra_help.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plus_map_is_lifted_and_the_rest_become_commands() {
        let parsed = parse_args(&args(&[
            "--content",
            "c",
            "+map",
            "kero_start",
            "+sv_gravity",
            "200",
            "+developer",
            "1",
            "--headless",
            "60",
        ]))
        .unwrap();
        assert_eq!(parsed.content_paths, vec![PathBuf::from("c")]);
        assert_eq!(parsed.map.as_deref(), Some("kero_start"));
        assert_eq!(parsed.commands, vec!["sv_gravity 200", "developer 1"]);
        assert_eq!(parsed.headless_ticks, Some(60));
    }

    #[test]
    fn a_bare_plus_map_is_a_command_and_unknown_flags_are_refused() {
        let parsed = parse_args(&args(&["+map"])).unwrap();
        assert_eq!(parsed.map, None);
        assert_eq!(parsed.commands, vec!["map"]);
        assert!(parse_args(&args(&["--nope"])).is_err());
        assert!(parse_args(&args(&["--content"])).is_err());
    }

    #[test]
    fn an_app_id_is_the_name_lowercased_and_dashed() {
        assert_eq!(app_id_for("My Game!"), "my-game");
        assert_eq!(app_id_for("  Half  Life 3 "), "half-life-3");
        assert_eq!(app_id_for("???"), "kerosene");
        assert_eq!(LaunchOptions::new("A B", "1").app_id("ab").app_id, "ab");
    }

    #[test]
    fn help_names_the_game_and_carries_its_extra_lines() {
        let text = help_text(
            &LaunchOptions::new("My Game", "1.0").extra_help("game options:\n  --cheats  Yes."),
        );
        assert!(text.starts_with("My Game 1.0\n"));
        assert!(text.contains("usage: my-game [options]"));
        assert!(text.ends_with("  --cheats  Yes.\n"));
    }

    #[test]
    fn a_named_content_tree_is_used_as_is_and_its_vaults_are_mounted() {
        let dir = std::env::temp_dir().join(format!("kerosene-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pak02.vault"), b"").unwrap();
        std::fs::write(dir.join("pak01.vault"), b"").unwrap();
        let parsed = ParsedArgs {
            content_paths: vec![dir.clone()],
            headless_ticks: Some(1),
            ..Default::default()
        };
        let config = config_from(parsed, None);
        assert_eq!(config.content_paths, vec![dir.clone()]);
        assert!(!config.audio, "headless opens no device");
        assert_eq!(
            config.archives,
            vec![dir.join("pak01.vault"), dir.join("pak02.vault")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
