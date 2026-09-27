// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The Kerosene toolset, as a library.
//!
//! The whole toolset is one GUI application ([`toolset::Toolset`]): a project
//! page, the world editor, the sound editor, a build form and an archive form
//! behind one window, with one output panel for every job's log. The same stages are also exposed as headless subcommands, so a
//! script or build server can drive them without a screen.
//!
//! The engine knows nothing about any of this. That boundary is what lets a
//! game ship as just the runtime and an archive; the toolset is a developer's
//! tool, never a player's.

pub mod doctor;
pub mod entry;
pub mod init;
pub mod new;
pub mod panels;
pub mod play;
pub mod project;
pub mod toolset;
pub mod workshop;

pub use entry::{Options, main_with};
pub use toolset::{Launch, Tab, Toolset, run_gui};

/// The headless subcommands, in the order help prints them.
pub const SUBCOMMANDS: &[(&str, &str)] = &[
    (
        "new",
        "start a game: a Cargo package, its project and a map",
    ),
    ("init", "start a project: a .kproj and the tree beside it"),
    ("cleave", "compile a .kmap into a .kbsp"),
    ("umbra", "compute the PVS for a compiled map"),
    (
        "resonance",
        "work out what each part of a compiled map sounds like",
    ),
    ("radiance", "bake static lighting into a compiled map"),
    ("alchemy", "compile textures and author materials"),
    ("forge", "compile source meshes into .kmdl models"),
    ("timbre", "compile sounds into .kaud"),
    ("kiln", "build a whole project's content"),
    ("play", "build what changed, then run the game"),
    ("clean", "delete what the content build wrote"),
    ("doctor", "check this machine can build and run a game"),
    ("vault", "pack and inspect content archives"),
    (
        "workshop",
        "upload a map to the Steam Workshop (a build with --features steam)",
    ),
];

/// Run one headless stage: `kerosene-tools <subcommand> <args...>`.
pub fn run_subcommand(name: &str, args: Vec<String>) -> anyhow::Result<()> {
    match name {
        "new" => new::run(args),
        "init" => init::run(args),
        "cleave" => cleave::run(args),
        "umbra" => umbra::run(args),
        "resonance" => resonance::run(args),
        "radiance" => radiance::run(args),
        "alchemy" => alchemy::run(args),
        "forge" => forge::run(args),
        "timbre" => timbre::run(args),
        "kiln" => kiln::run(args),
        "play" => play::run(args, None),
        "clean" => kiln::run(std::iter::once("--clean".to_string()).chain(args).collect()),
        "doctor" => doctor::run(args),
        "vault" => vault::run(args),
        "workshop" => workshop::run(args),
        other => match did_you_mean(other) {
            Some(guess) => anyhow::bail!("unknown command {other:?}. Did you mean `{guess}`?"),
            None => anyhow::bail!("unknown command {other:?}. `kerosene-tools --help` lists them"),
        },
    }
}

/// The command nearest `typed`, if one is close enough to be what was meant:
/// within two edits, or one for a short name.
pub fn did_you_mean(typed: &str) -> Option<&'static str> {
    let names = SUBCOMMANDS.iter().map(|(n, _)| *n).chain(["chisel"]);
    names
        .map(|name| (edit_distance(typed, name), name))
        .filter(|&(d, name)| d <= if name.len() <= 4 { 1 } else { 2 })
        .min_by_key(|&(d, _)| d)
        .map(|(_, name)| name)
}

/// Levenshtein distance, by characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substitute = previous + usize::from(ca != cb);
            previous = row[j + 1];
            row[j + 1] = substitute.min(row[j] + 1).min(previous + 1);
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod suggest_tests {
    use super::*;

    #[test]
    fn a_typo_is_answered_with_the_command_it_nearly_was() {
        assert_eq!(did_you_mean("kilm"), Some("kiln"));
        assert_eq!(did_you_mean("chisle"), Some("chisel"));
        assert_eq!(did_you_mean("docter"), Some("doctor"));
        assert_eq!(did_you_mean("pley"), Some("play"));
        assert_eq!(did_you_mean("banana"), None);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
