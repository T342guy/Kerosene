// SPDX-License-Identifier: LGPL-3.0-or-later
//! Assembling a distribution -- the step after the content is built.
//!
//! Everything up to here produces *content*: textures, models, maps, and a
//! `.vault` holding them. None of that is a thing you can hand somebody. A
//! game is an executable, an archive, a project file telling the executable
//! where the archive is, and the notices the licence requires -- arranged so
//! that double-clicking the executable works.
//!
//! Assembling that by hand is the step everyone gets wrong, and the two ways
//! to get it wrong are opposites:
//!
//! * **Shipping too little.** Forgetting the licence texts is the common one,
//!   because nothing breaks when you do. So they are written by this code
//!   rather than remembered by a person, and they are compiled into `kiln`
//!   so that shipping works from a directory that is not a checkout.
//!
//! * **Shipping too much.** Sweeping the build directory into the archive
//!   puts Chisel and the compilers in a player's hands. That is not merely
//!   wasted space: the tools are ordinary copyleft binaries, so distributing
//!   one obliges you to distribute its source too. A game that ships no
//!   compilers owes nobody anything for them, which is why [`ship`] copies a
//!   named list of files rather than a directory, and why a test asserts that
//!   no tool ever appears in the result.
//!
//! What comes out:
//!
//! ```text
//! dist/
//!   my_game            the game, or the engine runtime when a project has none
//!   my_game.keroproj   content = "content", so the game finds its own archive
//!   content/
//!     my_game.vault
//!   COPYING            GPL-3.0, which the LGPL builds on
//!   COPYING.LESSER     LGPL-3.0
//!   README.txt         what this is, and the notice LGPL section 4(a) asks for
//! ```

use crate::{Settings, slug};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::SystemTime;

/// The licence texts, compiled in.
///
/// Read from the repository at build time rather than found on disk at run
/// time, because `kiln` installed somewhere else still has to be able to
/// write them, and a licence file that is missing when it matters is the
/// whole failure this module exists to prevent.
const GPL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../COPYING"));
const LGPL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../COPYING.LESSER"));

/// The Rust toolchain this engine is built with.
///
/// Part of the distribution rather than a development detail: relinking a
/// modified engine against a game requires the same compiler, so the version
/// is something a recipient needs to be told.
const TOOLCHAIN: &str = "1.94";

/// What was assembled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shipped {
    pub root: PathBuf,
    /// The game executable, as named in the distribution.
    pub binary: PathBuf,
    pub archive: PathBuf,
    /// The engine shared library, when the build produced one.
    ///
    /// Absent means the engine is linked statically into the binary, which is
    /// a licence question rather than a technical one -- see [`ship`].
    pub engine_library: Option<PathBuf>,
    /// Every file written that exists to satisfy a licence.
    pub notices: Vec<PathBuf>,
}

impl Shipped {
    /// Whether the engine can be replaced in this distribution.
    ///
    /// The LGPL's whole mechanism is that a recipient can build a modified
    /// engine and use it with the program they were given. A separate shared
    /// library allows that; a static link does not.
    pub fn engine_is_replaceable(&self) -> bool { self.engine_library.is_some() }
}

/// Assemble a distribution into `out`.
///
/// The content must already be built: this stage copies, it does not compile,
/// and shipping last week's maps because nobody noticed the archive was stale
/// is a bad enough failure to be worth refusing rather than warning about.
pub fn ship(settings: &Settings, out: &Path) -> Result<Shipped> {
    let source = game_binary(settings)?;
    ship_from(settings, out, &source)
}

/// Assemble from a binary that has already been found or built.
///
/// Split from [`ship`] so that the assembly can be tested without a compiler:
/// what goes into a distribution, and what must not, is the part worth
/// pinning down, and running cargo to find out would make those tests
/// minutes long and dependent on the machine.
pub(crate) fn ship_from(settings: &Settings, out: &Path, source: &Path) -> Result<Shipped> {
    let archive = settings.archive();
    if !settings.dry_run {
        check_archive(settings, &archive)?;
    }

    let name = match &settings.project {
        Some(project) => slug(&project.name),
        None => "kerosene".to_string(),
    };
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.clone() };

    let library = engine_library_beside(source);

    let shipped = Shipped {
        root: out.to_path_buf(),
        binary: out.join(&exe),
        archive: out.join("content").join(archive.file_name().unwrap_or_default()),
        engine_library: library
            .as_ref()
            .map(|l| out.join(l.file_name().unwrap_or_default())),
        notices: ["COPYING", "COPYING.LESSER", "README.txt"]
            .iter()
            .map(|n| out.join(n))
            .collect(),
    };

    if settings.dry_run {
        println!("  would assemble {} from {}", out.display(), source.display());
        return Ok(shipped);
    }

    std::fs::create_dir_all(out.join("content"))
        .with_context(|| format!("creating {}", out.display()))?;

    copy(source, &shipped.binary)?;
    copy(&archive, &shipped.archive)?;
    if let (Some(from), Some(to)) = (&library, &shipped.engine_library) {
        copy(from, to)?;
    }

    write_project(settings, &out.join(format!("{name}.keroproj")), &name)?;
    std::fs::write(out.join("COPYING"), GPL)?;
    std::fs::write(out.join("COPYING.LESSER"), LGPL)?;
    std::fs::write(out.join("README.txt"), readme(settings, &name, &shipped))?;

    println!("  {} -> {}", source.display(), shipped.binary.display());
    println!("  {} -> {}", archive.display(), shipped.archive.display());
    if let Some(library) = &shipped.engine_library {
        println!("  engine library -> {}", library.display());
    }
    println!("  wrote COPYING, COPYING.LESSER and README.txt");
    Ok(shipped)
}

/// Refuse to ship an archive that is missing or older than the content in it.
fn check_archive(settings: &Settings, archive: &Path) -> Result<()> {
    if !archive.is_file() {
        bail!(
            "{} does not exist. Run kiln with no --only first, so there is \
             something to ship.",
            archive.display()
        );
    }
    let stale = newer_than(&settings.content, archive);
    if !stale.is_empty() {
        let listed: Vec<String> = stale
            .iter()
            .take(5)
            .map(|p| format!("  {}", p.display()))
            .collect();
        let more = stale.len().saturating_sub(5);
        bail!(
            "{} is older than {} file(s) in the content tree, so shipping it \
             would ship content nobody has built:\n{}{}\nRun kiln again first.",
            archive.display(),
            stale.len(),
            listed.join("\n"),
            if more > 0 { format!("\n  ... and {more} more") } else { String::new() },
        );
    }
    Ok(())
}

/// Files under `dir` modified more recently than `reference`.
///
/// The archive is written last, so anything newer than it changed after the
/// pack and is not in it.
fn newer_than(dir: &Path, reference: &Path) -> Vec<PathBuf> {
    let Ok(when) = reference.metadata().and_then(|m| m.modified()) else {
        return Vec::new();
    };
    let mut newer = Vec::new();
    collect_newer(dir, reference, when, &mut newer);
    newer.sort();
    newer
}

fn collect_newer(dir: &Path, skip: &Path, when: SystemTime, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_newer(&path, skip, when, out);
        } else if path != skip
            && entry.metadata().and_then(|m| m.modified()).is_ok_and(|m| m > when)
        {
            out.push(path);
        }
    }
}

/// Find, and if necessary build, the executable this project ships.
///
/// A project that names a Cargo package is a game: build it. A project that
/// names none is a content tree, and what it ships is the engine's own
/// runtime -- which is already built and sitting beside `kiln`.
fn game_binary(settings: &Settings) -> Result<PathBuf> {
    let Some(package) = settings.project.as_ref().and_then(|p| p.game.as_deref()) else {
        return kerosene_vfs::toolchain::path("kerosene").context(
            "no `game` key in the project and no `kerosene` beside kiln, so there is \
             nothing to ship. Add `\"game\" \"<cargo package>\"` to the .keroproj, \
             or run kiln from beside the engine.",
        );
    };

    let from = settings
        .project
        .as_ref()
        .and_then(|p| p.path.parent())
        .unwrap_or(Path::new("."))
        .to_path_buf();

    if settings.dry_run {
        println!("  would run: cargo build --release -p {package}");
    } else {
        println!("  building {package}");
        let status = std::process::Command::new("cargo")
            .args(["build", "--release", "-p", package])
            .current_dir(&from)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("running cargo. A project with a `game` key is built from source.")?;
        if !status.success() {
            bail!("cargo build -p {package} failed ({status})");
        }
    }

    built_binary(&from, package).with_context(|| {
        format!("cargo built {package}, but its binary is not under any target/release above {}",
            from.display())
    })
}

/// Where cargo left the binary, by climbing for the workspace's target/.
///
/// Cheaper and less brittle than parsing `cargo metadata`, which would need a
/// JSON dependency to answer a question a directory walk answers.
fn built_binary(from: &Path, package: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{package}.exe") } else { package.to_string() };
    let mut at = Some(from);
    while let Some(dir) = at {
        let candidate = dir.join("target").join("release").join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
        at = dir.parent();
    }
    None
}

/// The engine shared library beside a built binary, when there is one.
///
/// Its absence is not an error -- a statically linked build runs perfectly
/// well -- but it decides what [`readme`] has to say about relinking.
fn engine_library_beside(binary: &Path) -> Option<PathBuf> {
    let dir = binary.parent()?;
    let names = if cfg!(windows) {
        ["kerosene_engine.dll", "libkerosene_engine.dll"]
    } else if cfg!(target_os = "macos") {
        ["libkerosene_engine.dylib", "kerosene_engine.dylib"]
    } else {
        ["libkerosene_engine.so", "kerosene_engine.so"]
    };
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// Write the project file the shipped game reads.
///
/// Deliberately not a copy of the developer's own: theirs points at a content
/// tree in a checkout and may name a Cargo package that is not being shipped.
/// The shipped one says the two things a player's copy needs.
fn write_project(settings: &Settings, path: &Path, name: &str) -> Result<()> {
    let title = settings.project.as_ref().map_or(name, |p| p.name.as_str());
    let mut body = String::new();
    body.push_str("// Written by `kiln --ship`. The game reads this to find its content.\n");
    body.push_str("project\n{\n");
    body.push_str(&format!("\t\"name\" \"{title}\"\n"));
    body.push_str("\t\"content\" \"content\"\n");
    if let Some(map) = settings.project.as_ref().and_then(|p| p.start_map.as_deref()) {
        body.push_str(&format!("\t\"startmap\" \"{map}\"\n"));
    }
    body.push_str("}\n");
    std::fs::write(path, body)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The notice LGPL section 4(a) asks for, plus what a recipient needs to act on it.
///
/// Written out in full rather than pointing at a URL, because the obligation
/// travels with the copy and a link can rot. What it says depends on how the
/// engine was linked: with a shared library the recipient already has
/// everything they need, and without one they are owed more.
fn readme(settings: &Settings, name: &str, shipped: &Shipped) -> String {
    let title = settings.project.as_ref().map_or(name, |p| p.name.as_str());
    let mut out = String::new();

    out.push_str(title);
    out.push('\n');
    out.push_str(&"=".repeat(title.len()));
    out.push_str("\n\nBuilt with Kerosene.\n\n");

    out.push_str(
        "Kerosene is free software: you can redistribute it and modify it under\n\
         the terms of the GNU Lesser General Public License, version 3 or (at your\n\
         option) any later version. The full terms are in COPYING.LESSER, and in\n\
         COPYING, which holds the GNU General Public License that the Lesser GPL\n\
         builds on. Kerosene comes with ABSOLUTELY NO WARRANTY.\n\n",
    );

    out.push_str("Replacing the engine\n--------------------\n\n");
    if shipped.engine_is_replaceable() {
        out.push_str(&format!(
            "The engine is the shared library shipped alongside this program, not part\n\
             of the executable. To run this program against your own build of Kerosene,\n\
             compile the engine and replace that file. Both must be built with the same\n\
             Rust compiler -- this program was built with {TOOLCHAIN}.\n\n"
        ));
    } else {
        out.push_str(&format!(
            "The engine is linked statically into this executable, so it cannot be\n\
             replaced by swapping a file. If you received this program without source,\n\
             you are entitled under section 4 of the Lesser GPL to what you need in\n\
             order to relink it against your own build of Kerosene; ask whoever\n\
             distributed it. Any such rebuild uses Rust {TOOLCHAIN}.\n\n"
        ));
    }

    out.push_str("Engine source\n-------------\n\n");
    out.push_str(
        "The corresponding source for Kerosene must be available to everyone who\n\
         receives this program. If you are redistributing this build, say here where\n\
         to obtain it.\n\n",
    );

    out.push_str("Other notices\n-------------\n\n");
    out.push_str(
        "This program embeds typefaces licensed under the SIL Open Font License 1.1\n\
         and the Ubuntu Font Licence 1.0. Both permit redistribution; both require\n\
         their notices to be preserved.\n\n\
         This program includes smartstring, by Bodil Stokke, licensed under the\n\
         Mozilla Public License 2.0. The source is available from\n\
         https://github.com/bodil/smartstring and under the terms of that licence.\n\
         MPL-2.0 is file-level copyleft: it reaches only its own files, and this\n\
         notice is what it asks of a program that carries them unmodified.\n\n\
         This program contains no Valve or id Software code, assets or data, and is\n\
         not affiliated with, endorsed by or sponsored by either company.\n",
    );

    out
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to)
        .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
    // Copying does not carry the executable bit on every platform, and a game
    // that will not start because of a permission bit is a miserable first
    // impression.
    #[cfg(unix)]
    if from.metadata().is_ok_and(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(to)?.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        std::fs::set_permissions(to, permissions)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
