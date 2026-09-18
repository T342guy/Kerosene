// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
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
//! fn main() -> anyhow::Result<()> {
//!     kerosene_engine::launch::launch(MyGame, kerosene_engine::launch::LaunchOptions {
//!         name: "My Game",
//!         version: env!("CARGO_PKG_VERSION"),
//!         ..Default::default()
//!     })
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

/// What the binary calls itself, for `--help` and the log.
#[derive(Clone, Debug)]
pub struct LaunchOptions {
    /// The game's name, as `--help` prints it.
    pub name: &'static str,
    /// Its version, beside the name.
    pub version: &'static str,
    /// Lines appended to `--help`, for a game with flags of its own to
    /// mention. Empty for none.
    pub extra_help: &'static str,
    /// The arguments to parse, without the program name. `None` reads the
    /// process's own.
    pub args: Option<Vec<String>>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions {
            name: "Kerosene",
            version: env!("CARGO_PKG_VERSION"),
            extra_help: "",
            args: None,
        }
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
    let config = config_from(parsed, Some(log));

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
                log::warn!("{}", kerosene_vfs::root::describe(&None));
                config.content_paths.push(PathBuf::from("content"));
            }
        }
    }

    // The engine config always exists: read it out of the content tree,
    // writing the defaults the first time anything runs. It is where the
    // renderer is chosen, so it is read before the window is made.
    if let Some(root) = config.content_paths.first().cloned() {
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

/// Run the simulation with no display for `ticks` ticks, then report.
pub fn run_headless(config: EngineConfig, game: Box<dyn Game>, ticks: u64) -> Result<()> {
    let mut engine = Engine::with_game(&config, game);
    engine.console.run_buffered();
    let unclaimed = take_console_requests(&mut engine);
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
        engine.console.run_buffered();
        let unclaimed = take_console_requests(&mut engine);
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
    fn help_names_the_game_and_carries_its_extra_lines() {
        let text = help_text(&LaunchOptions {
            name: "My Game",
            version: "1.0",
            extra_help: "game options:\n  --cheats  Yes.",
            args: None,
        });
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
