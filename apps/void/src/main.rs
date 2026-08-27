// SPDX-License-Identifier: LGPL-3.0-or-later
//! `void` -- the VoidEngine runtime.
//!
//! ```text
//! void +map void_start
//! void +map void_start +sv_gravity 200 +developer 1
//! void --headless 600 +map void_start     # simulate without a display
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
use void_engine::engine::{Engine, EngineConfig, take_console_requests};
use void_engine::input::InputState;
use void_math::Angles;

fn main() -> Result<()> {
    // The engine's own relay rather than env_logger: everything logged
    // anywhere in the engine has to be readable from the in-game console, and
    // a logger that only writes to stderr cannot do that.
    let log = void_console::install_logger(void_console::logging::level_from_env(
        log::LevelFilter::Info,
    ));

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    let parsed = parse_args(&args)?;
    let mut config = EngineConfig {
        log: Some(log),
        content_paths: parsed.content_paths,
        archives: parsed.archives,
        map: parsed.map,
        startup_commands: parsed.commands,
    };
    if config.content_paths.is_empty() {
        config.content_paths.push(PathBuf::from("content"));
    }

    match parsed.headless_ticks {
        Some(ticks) => run_headless(config, ticks),
        None => void_engine::host::run(config),
    }
}

/// Run the simulation with no display.
fn run_headless(config: EngineConfig, ticks: u64) -> Result<()> {
    let mut engine = Engine::new(&config);
    engine.console.run_buffered();
    take_console_requests(&mut engine);

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
        take_console_requests(&mut engine);
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
    let player = &engine.player;
    println!("  player at {:?}", player.movement.origin);
    println!("  speed {}, on ground: {}", void_math::units::speed(player.movement.ground_speed()), player.movement.on_ground);
    println!("  health {:.0}", player.health);

    // Anything the run logged as a problem is worth surfacing: a headless run
    // is often the only place anyone reads it.
    let problems: Vec<&str> = engine
        .console
        .log()
        .filter(|l| matches!(l.level, void_console::LogLevel::Warning | void_console::LogLevel::Error))
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

fn next<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str> {
    *i += 1;
    args.get(*i)
        .map(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("{flag} needs a value"))
}

fn print_help() {
    println!("VoidEngine {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("usage: void [options] [+command ...]");
    println!();
    println!("options:");
    println!("  --content <dir>     Mount a content directory. Repeatable; searched in order.");
    println!("  --vault <file>      Mount a .vault archive.");
    println!("  --headless <ticks>  Simulate without a window, then report. This is what a");
    println!("                      dedicated server runs.");
    println!("  --help              Show this.");
    println!();
    println!("Anything starting with + is a console command, so any convar can be set:");
    println!("  void +map void_start");
    println!("  void +map void_start +sv_gravity 200 +developer 1");
    println!("  void --headless 600 +map void_start");
}
