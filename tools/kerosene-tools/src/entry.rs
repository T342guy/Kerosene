// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The toolset's `main`, as a function a game can call.
//!
//! `kerosene-tools` is this with the defaults. A game that wants an editor
//! that knows its classes and launches *it* on F9 ships a second binary
//! whose `main` is the same call with its own [`Options`]:
//!
//! ```no_run
//! const SCHEMA: &str = "class { \"name\" \"item_pickup\" }"; // usually include_str!
//! fn main() -> anyhow::Result<()> {
//!     kerosene_tools::main_with(kerosene_tools::Options {
//!         name: "mygame-tools",
//!         version: env!("CARGO_PKG_VERSION"),
//!         schema: &[SCHEMA],
//!         game: Some("mygame"),
//!         ..Default::default()
//!     })
//! }
//! ```

use crate::{Launch, SUBCOMMANDS, Tab, run_gui, run_subcommand};
use anyhow::Result;
use kerosene_vfs::toolchain::Runtime;
use std::path::PathBuf;

/// What a toolset binary is for.
#[derive(Clone, Debug)]
pub struct Options {
    /// The binary's name, as help prints it.
    pub name: &'static str,
    pub version: &'static str,
    /// The `.kerodef` text of the game's own classes, shown in the editor
    /// after the stock ones. Usually one `include_str!`.
    pub schema: &'static [&'static str],
    /// The Cargo package that is the game, when this toolset is the game's
    /// own: the editor builds and launches it on F9 whatever the project
    /// file says. `None` asks the project file.
    pub game: Option<&'static str>,
    /// That package's binary, when it is not named after the package.
    pub bin: Option<&'static str>,
    /// The arguments, without the program name. `None` reads the process's.
    pub args: Option<Vec<String>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            name: "kerosene-tools",
            version: env!("CARGO_PKG_VERSION"),
            schema: &[],
            game: None,
            bin: None,
            args: None,
        }
    }
}

impl Options {
    /// The runtime this toolset launches, when it names one itself.
    ///
    /// The package is built from the working directory, which for a game
    /// running its own tools is its checkout.
    pub fn runtime(&self) -> Option<Runtime> {
        self.game.map(|name| Runtime::Package {
            name: name.to_string(),
            bin: self.bin.map(str::to_string),
            project_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        })
    }

    fn launch(&self, tab: Tab, content: Option<PathBuf>, map: Option<PathBuf>) -> Launch {
        Launch {
            tab,
            content,
            map,
            schema: self.schema.to_vec(),
            runtime: self.runtime(),
        }
    }
}

/// Run the toolset: the window with no arguments, a headless stage with a
/// subcommand.
pub fn main_with(options: Options) -> Result<()> {
    // One logger for the whole toolset, GUI and headless alike.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .try_init();
    // A crash in the editor should leave something behind besides a closed
    // window. No relay here -- env_logger is the logger -- so the report
    // carries the panic and a backtrace.
    kerosene_console::install_crash_handler(None);

    let args: Vec<String> = options
        .args
        .clone()
        .unwrap_or_else(|| std::env::args().skip(1).collect());
    let Some(first) = args.first().cloned() else {
        return run_gui(options.launch(Tab::Project, None, None));
    };

    match first.as_str() {
        "-h" | "--help" => {
            print!("{}", usage(&options));
            Ok(())
        }
        "-V" | "--version" => {
            println!("{} {}", options.name, options.version);
            Ok(())
        }
        // `chisel` and bare `timbre` are the two tools that are a window, so
        // they open the toolset on the matching tab rather than a headless
        // stage. The compilers and the build stages stay subcommands.
        "chisel" => {
            let (content, map) = parse_editor_args(&args[1..]);
            run_gui(options.launch(Tab::Editor, content, map))
        }
        "timbre" if opens_sound_window(&args[1..]) => {
            run_gui(options.launch(Tab::Sound, first_content_flag(&args[1..]), None))
        }
        other => run_subcommand(other, args[1..].to_vec()),
    }
}

/// `chisel` accepts an optional map path and an optional `--content <dir>`.
fn parse_editor_args(args: &[String]) -> (Option<PathBuf>, Option<PathBuf>) {
    let mut map = None;
    let mut content = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--content" => {
                i += 1;
                content = args.get(i).map(PathBuf::from);
            }
            "--no-build" => {
                // The texture build is done by the editor on the way in; the
                // flag is accepted for familiarity and has no GUI equivalent.
            }
            other if !other.starts_with('-') => map = Some(PathBuf::from(other)),
            _ => {}
        }
        i += 1;
    }
    (content, map)
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

fn first_content_flag(args: &[String]) -> Option<PathBuf> {
    args.iter()
        .position(|a| a == "--content")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

/// What `--help` prints.
pub fn usage(options: &Options) -> String {
    let name = options.name;
    let mut out = format!(
        "{name} -- the editor and every compiler, in one application.

usage:
  {name:<26} open the toolset window, on the project page
  {name} chisel [map.keromap]     open the editor
  {name} timbre                   open the sound editor

headless stages, for scripts and build servers:
"
    );
    for (sub, about) in SUBCOMMANDS {
        out.push_str(&format!("  {name} {sub:<9} {about}\n"));
    }
    out.push_str(&format!(
        "\nEach stage accepts its own --help: {name} cleave --help\n"
    ));
    match options.game {
        Some(game) => out.push_str(&format!(
            "The game is the `{game}` package: the editor builds it and runs it on F9.\n"
        )),
        None => out.push_str(
            "The engine runtime is `kerosene`, its own binary, and not part of this set;\n\
             a project whose file names a `game` package runs that instead.\n",
        ),
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
    fn editor_arguments_are_a_map_and_a_content_flag() {
        let (content, map) =
            parse_editor_args(&args(&["a.keromap", "--content", "c", "--no-build"]));
        assert_eq!(content, Some(PathBuf::from("c")));
        assert_eq!(map, Some(PathBuf::from("a.keromap")));
    }

    #[test]
    fn timbre_opens_a_window_unless_it_is_a_stage() {
        assert!(opens_sound_window(&args(&[])));
        assert!(opens_sound_window(&args(&["edit"])));
        assert!(!opens_sound_window(&args(&["build", "x"])));
        assert!(!opens_sound_window(&args(&["--help"])));
    }

    #[test]
    fn a_games_options_name_its_package_and_schema() {
        let options = Options {
            name: "mygame-tools",
            schema: &["class { \"name\" \"item_pickup\" }"],
            game: Some("my-game"),
            bin: Some("mygame"),
            ..Default::default()
        };
        let launch = options.launch(Tab::Editor, None, None);
        assert_eq!(launch.schema.len(), 1);
        match launch.runtime {
            Some(Runtime::Package { name, bin, .. }) => {
                assert_eq!(name, "my-game");
                assert_eq!(bin.as_deref(), Some("mygame"));
            }
            other => panic!("{other:?}"),
        }
        let help = usage(&options);
        assert!(help.starts_with("mygame-tools --"));
        assert!(help.contains("`my-game` package"));
        assert!(Options::default().runtime().is_none());
    }

    #[test]
    fn version_and_help_return_without_a_window() {
        main_with(Options {
            args: Some(args(&["--version"])),
            ..Default::default()
        })
        .unwrap();
        main_with(Options {
            args: Some(args(&["--help"])),
            ..Default::default()
        })
        .unwrap();
    }
}
