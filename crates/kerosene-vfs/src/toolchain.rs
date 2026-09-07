// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
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

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The toolset's own subcommands, all compiled into the one executable.
pub const TOOLSET: &[&str] =
    &["chisel", "cleave", "umbra", "radiance", "alchemy", "timbre", "forge", "vault", "kiln"];

/// The runtime, still its own binary so a game can ship without the tools.
pub const RUNTIME: &str = "kerosene";

/// Every name [`available`] reports, in a fixed order.
pub const ALL: &[&str] =
    &["chisel", "cleave", "umbra", "radiance", "alchemy", "timbre", "forge", "vault", "kiln", "kerosene"];

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
    let candidate = dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
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
        assert_eq!(TOOLSET.len(), 9);
        assert!(TOOLSET.contains(&"cleave"));
        assert!(TOOLSET.contains(&"chisel"));
    }

    #[test]
    fn the_runtime_is_listed_after_the_tools() {
        assert_eq!(ALL.last(), Some(&RUNTIME));
    }

    #[test]
    fn a_command_for_a_subcommand_reinvokes_this_binary() {
        // It should name the current executable plus the subcommand, so the
        // subcommand is always present and the version cannot drift.
        let command = command("cleave");
        let program = command.get_program();
        assert!(program.to_string_lossy().contains("kerosene"), "{program:?}");
    }

    #[test]
    fn the_names_are_listed_in_a_fixed_order() {
        let names: Vec<&str> = available().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ALL);
    }
}
