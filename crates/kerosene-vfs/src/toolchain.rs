// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Finding the other pieces of the toolchain.
//!
//! The tools used to be separate programs; now they are one executable, the
//! unified toolset, with each former program a subcommand. The engine runtime
//! is still its own binary, because a shipped game is the runtime and an
//! archive, not the tools -- see [`kiln::ship`](tools/kiln/src/ship.rs).
//!
//! Two things therefore need finding, and they need finding the same way from
//! everywhere that looks:
//!
//! * A *subcommand* is always present: it is compiled into this very binary.
//!   [`command`] runs it by re-invoking the executable with the subcommand as
//!   its first argument, which keeps the old crash-isolation property -- a
//!   compiler that fails does not take the editor down with it.
//! * The *runtime* is a sibling binary, beside this executable first and then
//!   on `PATH`. Beside first because a checkout and an install both put the
//!   two in one directory.

use crate::project::Project;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The toolset's own subcommands, all compiled into the one executable.
pub const TOOLSET: &[&str] = &[
    "chisel",
    "cleave",
    "umbra",
    "resonance",
    "radiance",
    "alchemy",
    "timbre",
    "forge",
    "vault",
    "kiln",
];

/// The runtime, still its own binary so a game can ship without the tools.
pub const RUNTIME: &str = "kerosene";

/// Every name [`available`] reports, in a fixed order.
pub const ALL: &[&str] = &[
    "chisel",
    "cleave",
    "umbra",
    "resonance",
    "radiance",
    "alchemy",
    "timbre",
    "forge",
    "vault",
    "kiln",
    "kerosene",
];

/// A command that runs one of the toolset's subcommands.
///
/// This is the executable re-invoking itself, so the subcommand is always
/// present and the two can never disagree about which version runs.
pub fn command(name: &str) -> Command {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("kerosene-tools"));
    let mut cmd = Command::new(exe);
    cmd.arg(name);
    cmd
}

/// Where a *sibling binary* lives, if it is next to this executable.
///
/// Used for the runtime, which is not a subcommand. The tools themselves are
/// not looked up this way any more: they are inside this binary.
pub fn path(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    });
    candidate.is_file().then_some(candidate)
}

/// Whether a sibling binary can be run at all.
///
/// Costs a process launch when the binary is not a sibling, which is why it
/// is asked once for a report rather than before every stage.
pub fn is_available(name: &str) -> bool {
    path(name).is_some()
        || Command::new(name)
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
}

/// Which binary is the game: what F9 in the editor launches and what
/// `kiln --ship` copies.
///
/// A content-only project runs the stock runtime, a sibling of the tools.
/// A project that names a Cargo package *is* that package's binary, built
/// from its source on demand -- so the editor launches the game as the
/// developer has it now, not a stale copy from somewhere else. A game that
/// re-hosts the toolset (`kerosene::tools`) says which package it is
/// outright.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Runtime {
    /// A binary by name: beside this executable first, then on `PATH`.
    Sibling(String),
    /// An exact path.
    Binary(PathBuf),
    /// A Cargo package, built in `project_dir` with `cargo build -p name`.
    Package {
        name: String,
        /// The binary the package produces, when it is not named after
        /// the package.
        bin: Option<String>,
        project_dir: PathBuf,
    },
}

/// How a package is built: `debug` for the editor's F9, `release` for a
/// shipped copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    fn dir(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Runtime::Sibling(RUNTIME.to_string())
    }
}

impl Runtime {
    /// The runtime a project runs: its `game` package, or the stock one.
    pub fn for_project(project: Option<&Project>) -> Runtime {
        match project.and_then(|p| p.game.as_deref().map(|g| (p, g))) {
            Some((project, name)) => Runtime::Package {
                name: name.to_string(),
                bin: project.bin.clone(),
                project_dir: project
                    .path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(".")),
            },
            None => Runtime::default(),
        }
    }

    /// One line saying what will run.
    pub fn describe(&self) -> String {
        match self {
            Runtime::Sibling(name) => match path(name) {
                Some(p) => format!("{name} ({})", p.display()),
                None => format!("{name} (on PATH)"),
            },
            Runtime::Binary(p) => p.display().to_string(),
            Runtime::Package { name, bin, .. } => match bin {
                Some(bin) => format!("cargo package {name} (binary {bin})"),
                None => format!("cargo package {name}"),
            },
        }
    }

    /// The binary's file name.
    pub fn binary_name(&self) -> String {
        let stem = match self {
            Runtime::Sibling(name) => name.clone(),
            Runtime::Binary(p) => {
                return p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
            Runtime::Package { name, bin, .. } => bin.clone().unwrap_or_else(|| name.clone()),
        };
        if cfg!(windows) {
            format!("{stem}.exe")
        } else {
            stem
        }
    }

    /// Whether it can be run without building anything first.
    ///
    /// A package is available when `cargo` is, since building it is how it
    /// is run; the build itself may still fail, and says so then.
    pub fn is_available(&self) -> bool {
        match self {
            Runtime::Sibling(name) => is_available(name),
            Runtime::Binary(p) => p.is_file(),
            Runtime::Package { .. } => is_available("cargo"),
        }
    }
}

/// Where cargo left a package's binary, by climbing from `from` for the
/// workspace's `target/` directory.
///
/// Cheaper and less brittle than parsing `cargo metadata`, which would need
/// a JSON dependency to answer a question a directory walk answers.
pub fn built_binary(from: &Path, bin: &str, profile: Profile) -> Option<PathBuf> {
    let file = if cfg!(windows) {
        format!("{bin}.exe")
    } else {
        bin.to_string()
    };
    let mut at = Some(from);
    while let Some(dir) = at {
        let candidate = dir.join("target").join(profile.dir()).join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
        at = dir.parent();
    }
    None
}

/// Build a package and say where its binary is.
///
/// Cargo's own output goes through `log`, a line at a time, so an editor can
/// show the build as it happens and a headless tool can print it.
pub fn build_package(
    project_dir: &Path,
    package: &str,
    bin: Option<&str>,
    profile: Profile,
    log: &mut dyn FnMut(&str),
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::io::{BufRead, BufReader};

    let mut args = vec!["build", "-p", package];
    if profile == Profile::Release {
        args.push("--release");
    }
    log(&format!("cargo {}", args.join(" ")));
    let mut child = Command::new("cargo")
        .args(&args)
        .current_dir(project_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "running cargo in {}. A project with a `game` key is built from source.",
                project_dir.display()
            )
        })?;
    if let Some(err) = child.stderr.take() {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            log(&line);
        }
    }
    let status = child.wait().context("waiting for cargo")?;
    if !status.success() {
        anyhow::bail!("cargo build -p {package} failed ({status})");
    }
    let bin = bin.unwrap_or(package);
    built_binary(project_dir, bin, profile).with_context(|| {
        format!(
            "cargo built {package}, but no binary `{bin}` is under any target/{} above {}. \
             If the package's binary has another name, say so with `\"bin\"` in the project file.",
            profile.dir(),
            project_dir.display()
        )
    })
}

/// A command that runs the runtime, building it first when it is a
/// package.
pub fn resolve(
    runtime: &Runtime,
    profile: Profile,
    log: &mut dyn FnMut(&str),
) -> anyhow::Result<Command> {
    Ok(match runtime {
        Runtime::Sibling(name) => match path(name) {
            Some(p) => Command::new(p),
            None => Command::new(name),
        },
        Runtime::Binary(p) => Command::new(p),
        Runtime::Package {
            name,
            bin,
            project_dir,
        } => Command::new(build_package(
            project_dir,
            name,
            bin.as_deref(),
            profile,
            log,
        )?),
    })
}

/// Which pieces of the toolchain are present, in a fixed order.
///
/// The nine subcommands are always present -- they are this binary. The
/// runtime depends on a sibling binary being installed.
pub fn available() -> Vec<(&'static str, bool)> {
    ALL.iter()
        .map(|&name| (name, TOOLSET.contains(&name) || is_available(name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_that_does_not_exist_is_not_found_beside_us() {
        assert!(path("definitely-not-a-kerosene-tool").is_none());
    }

    #[test]
    fn the_toolset_subcommands_are_all_known() {
        // The dispatcher and this list must not drift: a subcommand that is
        // compiled in but not listed here would be invisible to `--tools`.
        assert_eq!(TOOLSET.len(), 10);
        assert!(TOOLSET.contains(&"resonance"));
        assert!(TOOLSET.contains(&"cleave"));
        assert!(TOOLSET.contains(&"chisel"));
    }

    #[test]
    fn the_runtime_is_listed_after_the_tools() {
        assert_eq!(ALL.last(), Some(&RUNTIME));
    }

    #[test]
    fn a_project_with_a_game_key_runs_that_package_from_its_own_directory() {
        let project = Project {
            path: PathBuf::from("/games/mine/mine.keroproj"),
            name: "Mine".into(),
            content: PathBuf::from("/games/mine/content"),
            start_map: None,
            game: Some("my-game".into()),
            bin: Some("mygame".into()),
            dirs: None,
        };
        let runtime = Runtime::for_project(Some(&project));
        assert_eq!(
            runtime,
            Runtime::Package {
                name: "my-game".into(),
                bin: Some("mygame".into()),
                project_dir: PathBuf::from("/games/mine"),
            }
        );
        assert_eq!(runtime.binary_name().trim_end_matches(".exe"), "mygame");
        assert!(runtime.describe().contains("my-game"));
        assert_eq!(
            Runtime::for_project(None),
            Runtime::Sibling("kerosene".into())
        );
    }

    #[test]
    fn a_built_binary_is_found_by_climbing_to_the_workspace_target() {
        let root = std::env::temp_dir().join(format!("kerosene-toolchain-{}", std::process::id()));
        let member = root.join("games").join("mine");
        let built = root.join("target").join("debug");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::create_dir_all(&built).unwrap();
        let file = if cfg!(windows) {
            "mygame.exe"
        } else {
            "mygame"
        };
        std::fs::write(built.join(file), b"").unwrap();
        assert_eq!(
            built_binary(&member, "mygame", Profile::Debug),
            Some(built.join(file))
        );
        assert_eq!(built_binary(&member, "mygame", Profile::Release), None);
        assert_eq!(built_binary(&member, "other", Profile::Debug), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolving_a_sibling_falls_back_to_the_name_on_path() {
        let mut log = |_: &str| {};
        let command = resolve(
            &Runtime::Sibling("definitely-not-a-kerosene-tool".into()),
            Profile::Debug,
            &mut log,
        )
        .unwrap();
        assert_eq!(
            command.get_program().to_string_lossy(),
            "definitely-not-a-kerosene-tool"
        );
    }

    #[test]
    fn a_command_for_a_subcommand_reinvokes_this_binary() {
        // It should name the current executable plus the subcommand, so the
        // subcommand is always present and the version cannot drift.
        let command = command("cleave");
        let program = command.get_program();
        assert!(
            program.to_string_lossy().contains("kerosene"),
            "{program:?}"
        );
    }

    #[test]
    fn the_names_are_listed_in_a_fixed_order() {
        let names: Vec<&str> = available().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ALL);
    }
}
