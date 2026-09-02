// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! `kerosene` -- the Kerosene runtime.
//!
//! ```text
//! kerosene +map kero_start
//! kerosene +map kero_start +sv_gravity 200 +developer 1
//! kerosene --headless 600 +map kero_start     # simulate without a display
//! ```
//!
//! Arguments beginning with `+` are console commands, exactly as Source's
//! are, so anything settable at the console is settable on the command line
//! with no separate flag needing to exist for it.
//!
//! `--headless` runs the simulation with no window at all. That mode is not a
//! testing convenience bolted on the side: it is what a dedicated server is,
//! and the engine is structured so that it needs nothing from the renderer.

use anyhow::Result;
use std::path::PathBuf;
use kerosene_engine::engine::{Engine, EngineConfig, report_unhandled, take_console_requests};
use kerosene_engine::input::InputState;
use kerosene_math::Angles;

fn main() -> Result<()> {
    // The engine's own relay rather than env_logger: everything logged
    // anywhere in the engine has to be readable from the in-game console, and
    // a logger that only writes to stderr cannot do that.
    let log = kerosene_console::install_logger(kerosene_console::logging::level_from_env(
        log::LevelFilter::Info,
    ));

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    let parsed = parse_args(&args)?;
    let mut config = EngineConfig {
        // Headless has no listener, so opening a sound card would be work
        // nobody can hear.
        audio: parsed.headless_ticks.is_none(),
        log: Some(log),
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
                // A project that names a start map is answering the question
                // `kerosene` with no arguments is otherwise stuck on: a game
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

    match parsed.headless_ticks {
        Some(ticks) => run_headless(config, ticks),
        None => kerosene_engine::host::run(config),
    }
}

/// Run the simulation with no display.
fn run_headless(config: EngineConfig, ticks: u64) -> Result<()> {
    let mut engine = Engine::new(&config);
    engine.console.run_buffered();
    let unclaimed = take_console_requests(&mut engine);
    report_unhandled(&mut engine, unclaimed);

    if let Some(map) = config.map.as_deref() {
        engine.load_map(map)?;
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
        if engine.should_quit { break; }
    }
    let elapsed = started.elapsed().as_secs_f32();

    let simulated = engine.tick_count as f32 * interval;
    println!("--- headless run ---");
    println!("  {} ticks ({simulated:.1}s simulated in {elapsed:.2}s real)", engine.tick_count);
    if let Some(level) = &engine.level {
        println!("  map: {} ({} faces, {} leaves, {} clusters)",
            level.name, level.bsp.faces.len(), level.bsp.leaves.len(), level.bsp.num_clusters());
    }
    println!("  entities: {}", engine.entities.len());
    println!("  physics: {} props, {} static hulls, {} movers, {} bodies",
        engine.physics.prop_count(), engine.physics.static_body_count(),
        engine.physics.mover_count(), engine.physics.body_count());
    let player = &engine.player;
    println!("  player at {:?}", player.movement.origin);
    println!("  speed {}, on ground: {}", kerosene_math::units::speed(player.movement.ground_speed()), player.movement.on_ground);
    println!("  health {:.0}", player.health);

    // Anything the run logged as a problem is worth surfacing: a headless run
    // is often the only place anyone reads it.
    let problems: Vec<&str> = engine
        .console
        .log()
        .filter(|l| matches!(l.level, kerosene_console::LogLevel::Warning | kerosene_console::LogLevel::Error))
        .map(|l| l.text.as_str())
        .collect();
    if problems.is_empty() {
        println!("  no warnings");
    } else {
        println!("  {} warnings:", problems.len());
        for p in problems.iter().take(20) { println!("    {p}"); }
    }
    Ok(())
}

#[derive(Default)]
struct ParsedArgs {
    content_paths: Vec<PathBuf>,
    archives: Vec<PathBuf>,
    map: Option<String>,
    commands: Vec<String>,
    headless_ticks: Option<u64>,
}

fn parse_args(args: &[String]) -> Result<ParsedArgs> {
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
                // `+map name` is special: the engine needs it before the
                // console starts, so it is lifted out rather than queued.
                let command = other.trim_start_matches('+').to_string();
                let mut parts = vec![command.clone()];
                while i + 1 < args.len() && !args[i + 1].starts_with('+') && !args[i + 1].starts_with("--") {
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

/// Every `.vault` archive in a directory, in a stable order.
///
/// Sorted by name so two machines mount the same archives in the same order,
/// and so `pak01` comes before `pak02` -- with loose files still winning over
/// both, which is what makes dropping a file beside a shipped archive work.
fn vaults_in(dir: &std::path::Path) -> Vec<PathBuf> {
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

fn print_help() {
    println!("Kerosene {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("usage: kerosene [options] [+command ...]");
    println!();
    println!("options:");
    println!("  --content <dir>     Mount a content directory. Repeatable; searched in order.");
    println!("                      With none, the content tree is found: from the working");
    println!("                      directory, then beside the executable.");
    println!("  --vault <file>      Mount a .vault archive. With none, every .vault in the");
    println!("                      content tree is mounted.");
    println!("  --headless <ticks>  Simulate without a window, then report. This is what a");
    println!("                      dedicated server runs.");
    println!("  --help              Show this.");
    println!();
    println!("With no +map, the project's `startmap` is loaded if it names one.");
    println!();
    println!("Anything starting with + is a console command, so any convar can be set:");
    println!("  kerosene +map kero_start");
    println!("  kerosene +map kero_start +sv_gravity 200 +developer 1");
    println!("  kerosene --headless 600 +map kero_start");
}
