// SPDX-License-Identifier: MPL-2.0
//! Finding the other tools.
//!
//! The compilers are separate programs on purpose -- that is the shape of the
//! whole toolchain -- which means anything that drives them has to answer
//! "where is `cleave`?" Chisel answers it when you press F9 and Kiln answers
//! it on every build, and they had better answer it the same way.
//!
//! Beside this executable first, then whatever is on `PATH`. Beside first
//! because a checkout and an install both put the tools in one directory, and
//! picking up a *different* version of `cleave` from `PATH` is a way to spend
//! an afternoon on a bug that was fixed weeks ago.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Every tool the pipeline can call, for reporting which are present.
pub const TOOLS: &[&str] =
    &["chisel", "cleave", "umbra", "radiance", "alchemy", "timbre", "forge", "vault", "kiln", "kerosene"];

/// Where a sibling tool lives, if it is next to this executable.
pub fn path(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
    candidate.is_file().then_some(candidate)
}

/// A command that runs a sibling tool, falling back to `PATH`.
pub fn command(name: &str) -> Command {
    match path(name) {
        Some(path) => Command::new(path),
        None => Command::new(name),
    }
}

/// Whether a tool can be run at all.
///
/// Costs a process launch when the tool is not a sibling, which is why it is
/// asked once for a report rather than before every stage.
pub fn is_available(name: &str) -> bool {
    path(name).is_some()
        || Command::new(name)
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
}

/// Which of the tools are present, in a fixed order.
pub fn available() -> Vec<(&'static str, bool)> {
    TOOLS.iter().map(|&name| (name, is_available(name))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_that_does_not_exist_is_not_found_beside_us() {
        assert!(path("definitely-not-a-kerosene-tool").is_none());
    }

    #[test]
    fn a_command_for_an_unknown_tool_still_builds_and_names_it() {
        // Falling back to the bare name is deliberate: it lets an installed
        // toolchain on PATH work, and produces a "not found" naming the tool
        // rather than a panic.
        let command = command("definitely-not-a-kerosene-tool");
        assert_eq!(command.get_program(), "definitely-not-a-kerosene-tool");
    }

    #[test]
    fn the_tools_are_listed_in_a_fixed_order() {
        let names: Vec<&str> = available().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, TOOLS);
    }

    #[test]
    fn the_test_binary_finds_its_own_siblings() {
        // Run from `target/debug/deps`, so the tools are one directory up
        // rather than beside us -- which is exactly the case this must not
        // pretend to handle. It should simply find nothing, not panic.
        let _ = path("cleave");
    }
}
