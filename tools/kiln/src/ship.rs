// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Assembling a distribution -- the step after the content is built.
//!
//! Everything up to here produces *content*: textures, models, maps, and a
//! `.vault` holding them. None of that is a thing you can hand somebody. A
//! game is an executable, an archive, a project file telling the executable
//! where the archive is, and the notices the licences require -- arranged so
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
//!   wasted space: the tools are ordinary GPL binaries, so distributing
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
//!   LICENSE            the GNU General Public License, version 3, full text
//!   LICENSE-EXCEPTION  the Kerosene Exception: the linking permission and
//!                      the attribution terms, full text
//!   README.txt         what this is, and the notices the licence asks for
//! ```

use crate::{Settings, slug};
use anyhow::{Context, Result, bail};
use kerosene_vfs::toolchain;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The licence texts, compiled in.
///
/// Read from the repository at build time rather than found on disk at run
/// time, because `kiln` installed somewhere else still has to be able to
/// write them, and a licence file that is missing when it matters is the
/// whole failure this module exists to prevent.
const GPL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../LICENSE"));
const EXCEPTION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../LICENSE-EXCEPTION"
));

/// What was assembled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shipped {
    pub root: PathBuf,
    /// The game executable, as named in the distribution.
    pub binary: PathBuf,
    pub archive: PathBuf,
    /// Every file written that exists to satisfy a licence.
    pub notices: Vec<PathBuf>,
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
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.clone()
    };

    let shipped = Shipped {
        root: out.to_path_buf(),
        binary: out.join(&exe),
        archive: out
            .join("content")
            .join(archive.file_name().unwrap_or_default()),
        notices: ["LICENSE", "LICENSE-EXCEPTION", "README.txt"]
            .iter()
            .map(|n| out.join(n))
            .collect(),
    };

    if settings.dry_run {
        println!(
            "  would assemble {} from {}",
            out.display(),
            source.display()
        );
        return Ok(shipped);
    }

    std::fs::create_dir_all(out.join("content"))
        .with_context(|| format!("creating {}", out.display()))?;

    copy(source, &shipped.binary)?;
    copy(&archive, &shipped.archive)?;

    write_project(settings, &out.join(format!("{name}.keroproj")), &name)?;
    std::fs::write(out.join("LICENSE"), GPL)?;
    std::fs::write(out.join("LICENSE-EXCEPTION"), EXCEPTION)?;
    std::fs::write(out.join("README.txt"), readme(settings, &name))?;

    println!("  {} -> {}", source.display(), shipped.binary.display());
    println!("  {} -> {}", archive.display(), shipped.archive.display());
    println!("  wrote LICENSE, LICENSE-EXCEPTION and README.txt");
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
            if more > 0 {
                format!("\n  ... and {more} more")
            } else {
                String::new()
            },
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
            && entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|m| m > when)
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
    let runtime = toolchain::Runtime::for_project(settings.project.as_ref());
    match runtime {
        toolchain::Runtime::Package {
            name,
            bin,
            project_dir,
        } => {
            if settings.dry_run {
                println!("  would run: cargo build --release -p {name}");
                return toolchain::built_binary(
                    &project_dir,
                    bin.as_deref().unwrap_or(&name),
                    toolchain::Profile::Release,
                )
                .with_context(|| format!("{name} has not been built in release yet"));
            }
            println!("  building {name}");
            let mut log = |line: &str| println!("  {line}");
            toolchain::build_package(
                &project_dir,
                &name,
                bin.as_deref(),
                toolchain::Profile::Release,
                &mut log,
            )
        }
        _ => toolchain::path(toolchain::RUNTIME).context(
            "no `game` key in the project and no `kerosene` beside kiln, so there is \
             nothing to ship. Add `\"game\" \"<cargo package>\"` to the .keroproj, \
             or run kiln from beside the engine.",
        ),
    }
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
    if let Some(map) = settings
        .project
        .as_ref()
        .and_then(|p| p.start_map.as_deref())
    {
        body.push_str(&format!("\t\"startmap\" \"{map}\"\n"));
    }
    body.push_str("}\n");
    std::fs::write(path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The notice the licence asks of a program that carries Kerosene.
///
/// Written out in full rather than pointing at a URL, because the obligation
/// travels with the copy and a link can rot. It states the licence, what the
/// Kerosene Exception asks of a game shipped under it -- engine source, this
/// notice, the attribution screen, and the "modified from" line when the
/// engine is not the stock one -- and disclaims the warranty.
fn readme(settings: &Settings, name: &str) -> String {
    let title = settings.project.as_ref().map_or(name, |p| p.name.as_str());
    let mut out = String::new();

    out.push_str(title);
    out.push('\n');
    out.push_str(&"=".repeat(title.len()));
    out.push_str("\n\nBuilt with Kerosene.\n\n");

    out.push_str(
        "Kerosene is licensed under the GNU General Public License, version 3 or\n\
         (at your option) any later version, with the additional terms of the\n\
         Kerosene Exception. The full texts are in LICENSE and LICENSE-EXCEPTION.\n\n\
         The exception permits a program to link Kerosene and ship under its own\n\
         terms, on conditions: the engine part of the program stays under the GPL\n\
         with its source available; the program carries this notice; it shows an\n\
         attribution screen when it starts; and if the engine has been modified,\n\
         the whole modified engine says it is modified from Kerosene, names the\n\
         version it diverged from, and is published as source where anyone can\n\
         obtain it.\n\n\
         If you change Kerosene itself, please contribute the change back as a\n\
         pull request rather than releasing a modified Kerosene of your own, so\n\
         the fix exists once for everyone.\n\n\
         Kerosene comes with ABSOLUTELY NO WARRANTY.\n\n",
    );

    out.push_str("Engine source\n-------------\n\n");
    out.push_str(
        "The corresponding source for Kerosene must be available to everyone who\n\
         receives this program. If you are redistributing this build, say here where\n\
         to obtain it. If the engine in this build is modified from Kerosene, also\n\
         say which Kerosene version or commit it diverged from and where the\n\
         complete source of the modified engine is published.\n\n\
         Kerosene's own source: https://github.com/t342guy/kerosene\n\n",
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
    if from
        .metadata()
        .is_ok_and(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0)
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(to)?.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        std::fs::set_permissions(to, permissions)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
