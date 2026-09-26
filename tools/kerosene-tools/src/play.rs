// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `kerosene-tools play` -- build what changed, then run the game.
//!
//! ```text
//! kerosene-tools play                     # the project's start map
//! kerosene-tools play +map mg_intro       # anything after is the game's
//! kerosene-tools play --full              # light the maps properly first
//! kerosene-tools play --release -- --headless 600
//! ```
//!
//! The one command for the edit-and-try loop, and what a game crate's
//! `cargo play` alias runs. Kiln builds whatever is newer than its compiled
//! form -- textures, sounds, models, maps -- with the maps' visibility and
//! lighting skimped unless `--full` asks otherwise, and skips everything
//! that is already current, so a second `play` with nothing changed starts
//! the game at once. Then the game is built, if it is a Cargo package, and
//! run with the rest of the arguments.
//!
//! Nothing is packed: the engine reads loose files before archives, so the
//! freshly built files are what it sees.

use anyhow::{Context, Result, bail};
use kerosene_vfs::toolchain::{self, Profile, Runtime};
use std::path::PathBuf;

/// What `play` was asked: its own flags, and everything for the game.
#[derive(Debug, Default, PartialEq)]
struct Args {
    content: Option<PathBuf>,
    full: bool,
    release: bool,
    quiet: bool,
    /// Passed to the game untouched.
    game: Vec<String>,
}

fn parse(args: &[String]) -> Result<Args> {
    let mut parsed = Args::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--full" => parsed.full = true,
            "--release" => parsed.release = true,
            "-q" | "--quiet" => parsed.quiet = true,
            "--content" => {
                i += 1;
                let dir = args.get(i).context("--content needs a directory")?;
                parsed.content = Some(PathBuf::from(dir));
            }
            "--" => {
                parsed.game.extend(args[i + 1..].iter().cloned());
                break;
            }
            // The first thing that is not play's own is the game's, and so
            // is everything after it: `play +map x --headless 60`.
            _ => {
                parsed.game.extend(args[i..].iter().cloned());
                break;
            }
        }
        i += 1;
    }
    Ok(parsed)
}

pub const HELP: &str = "\
usage: play [--full] [--release] [--quiet] [--content <dir>] [--] [game arguments...]

Build whatever changed since the last build, then run the game.

  --full            Build maps with full visibility and lighting (slower).
  --release         Build and run the game optimised.
  --quiet, -q       Keep the compilers' output to errors.
  --content <dir>   The content tree, when it cannot be found from here.

Everything else goes to the game: play +map mg_intro +sv_cheats 1
";

/// Run `play`. `runtime` is the game a toolset binary names for itself;
/// `None` asks the project file, then falls back to the stock runtime.
pub fn run(args: Vec<String>, runtime: Option<Runtime>) -> Result<()> {
    if args.first().is_some_and(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return Ok(());
    }
    let args = parse(&args)?;

    let found = kerosene_vfs::root::find(args.content.as_deref(), None);
    println!("{}", kerosene_vfs::root::describe(&found));
    let project = found.as_ref().and_then(|f| f.project.clone());

    // No tree is not a failure: a game crate with no content of its own
    // yet runs on the engine's base content.
    if let Some(found) = &found {
        let settings = kiln::Settings {
            content: found.root.clone(),
            project: project.clone(),
            stages: vec![
                kiln::Stage::Textures,
                kiln::Stage::Sounds,
                kiln::Stage::Models,
                kiln::Stage::Maps,
            ],
            fast: !args.full,
            quiet: args.quiet,
            ..Default::default()
        };
        let report = kiln::build(&settings)?;
        // A leaking map still has last build's BSP, or none; either way the
        // game says what it could not load, and the leak is worth a line.
        for map in &report.leaking {
            println!("warning: {map} leaks and was not rebuilt; open it in Chisel to see where");
        }
    }

    let runtime = runtime.unwrap_or_else(|| Runtime::for_project(project.as_ref()));
    let profile = if args.release {
        Profile::Release
    } else {
        Profile::Debug
    };
    println!("==> run {}", runtime.describe());
    let mut command = toolchain::resolve(&runtime, profile, &mut |line| println!("  {line}"))?;
    // Run from the project, so the game finds the same tree Kiln just built.
    if let Some(dir) = project.as_ref().and_then(|p| p.path.parent()) {
        command.current_dir(dir);
    }
    if let Some(content) = args.content.as_ref().and_then(|c| c.canonicalize().ok()) {
        command.arg("--content").arg(content);
    }
    let status = command
        .args(&args.game)
        .status()
        .with_context(|| format!("starting {}", runtime.describe()))?;
    if !status.success() {
        bail!("the game exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plays_own_flags_come_first_and_the_rest_is_the_games() {
        let parsed = parse(&args(&["--full", "-q", "+map", "x", "--full"])).unwrap();
        assert!(parsed.full && parsed.quiet && !parsed.release);
        assert_eq!(parsed.game, args(&["+map", "x", "--full"]));

        let parsed = parse(&args(&["--release", "--", "--headless", "60"])).unwrap();
        assert!(parsed.release);
        assert_eq!(parsed.game, args(&["--headless", "60"]));

        let parsed = parse(&args(&["--content", "c"])).unwrap();
        assert_eq!(parsed.content, Some(PathBuf::from("c")));
        assert!(parse(&args(&["--content"])).is_err());
    }
}
