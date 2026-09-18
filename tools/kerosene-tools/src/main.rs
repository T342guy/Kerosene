// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! `kerosene-tools` -- the Kerosene toolset.
//!
//! Run with no subcommand it opens the one window that holds every tool: a
//! project page, the world editor, the sound editor, a build form and an
//! archive form, with an activity bar of icons down the left edge to switch
//! between them and an output panel along the bottom that every job logs
//! into. That is the developer's door into the engine.
//!
//! The stages also run headless, as subcommands, for scripts and build
//! servers:
//!
//! ```text
//! kerosene-tools                          the toolset window
//! kerosene-tools cleave map.keromap       the compilers, one at a time
//! kerosene-tools kiln [...]               a whole project build
//! kerosene-tools vault <cmd>              content archives
//! ```
//!
//! The engine runtime is `kerosene`, its own binary, and not part of this set:
//! a game ships the runtime and an archive, never the tools.

use anyhow::Result;
use kerosene_tools::{Launch, SUBCOMMANDS, Tab, run_gui, run_subcommand};

fn main() -> Result<()> {
    // One logger for the whole toolset, GUI and headless alike.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();
    // A crash in the editor should leave something behind besides a closed
    // window. No relay here -- env_logger is the logger -- so the report
    // carries the panic and a backtrace.
    kerosene_console::install_crash_handler(None);

    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first().cloned() else {
        return run_gui(Launch::default());
    };

    match first.as_str() {
        "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        "-V" | "--version" => {
            println!("kerosene-tools {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        // `chisel` and bare `timbre` are the two tools that are a window, so
        // they open the toolset on the matching tab rather than a headless
        // stage. The compilers and the build stages stay subcommands.
        "chisel" => run_gui(parse_editor_launch(&args[1..])),
        "timbre" if opens_sound_window(&args[1..]) => run_gui(Launch {
            tab: Tab::Sound,
            content: first_content_flag(&args[1..]),
            map: None,
        }),
        other => run_subcommand(other, args[1..].to_vec()),
    }
}

/// `chisel` accepts an optional map path and an optional `--content <dir>`.
fn parse_editor_launch(args: &[String]) -> Launch {
    let mut map = None;
    let mut content = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--content" => {
                i += 1;
                content = args.get(i).map(std::path::PathBuf::from);
            }
            "--no-build" => {
                // The texture build is done by the editor on the way in; the
                // flag is accepted for familiarity and has no GUI equivalent.
            }
            other if !other.starts_with('-') => map = Some(std::path::PathBuf::from(other)),
            _ => {}
        }
        i += 1;
    }
    Launch {
        tab: Tab::Editor,
        content,
        map,
    }
}

/// Whether a bare `timbre` invocation opens the window rather than running a
/// headless stage (`build`, `compile` or `info`).
fn opens_sound_window(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        None => true,
        Some("edit") => true,
        Some("build") | Some("compile") | Some("info") => false,
        // Asking for help or the version is asking the command line, and
        // must reach clap rather than open a window with the answer in it.
        Some("-h" | "--help" | "-V" | "--version") => false,
        Some(other) => other.starts_with('-'),
    }
}

fn first_content_flag(args: &[String]) -> Option<std::path::PathBuf> {
    args.iter()
        .position(|a| a == "--content")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
}

fn print_usage() {
    println!("Kerosene toolset -- the editor and every compiler, in one application.");
    println!();
    println!("usage:");
    println!("  kerosene-tools                          open the toolset window, on the project page");
    println!("  kerosene-tools chisel [map.keromap]     open the editor");
    println!("  kerosene-tools timbre                   open the sound editor");
    println!();
    println!("headless stages, for scripts and build servers:");
    for (name, about) in SUBCOMMANDS {
        println!("  kerosene-tools {name:<9} {about}");
    }
    println!();
    println!("Each stage accepts its own --help: kerosene-tools cleave --help");
    println!("The engine runtime is `kerosene`, its own binary, and not part of this set.");
}
