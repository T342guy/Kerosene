// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Finding the content tree.
//!
//! Every tool in the engine needs the same answer to the same question: where
//! is the content? Chisel reads entity classes and materials out of it, the
//! compilers write into it, and the runtime mounts it as a search path. When
//! each of them worked that out its own way, they disagreed -- and disagreeing
//! about a directory looks, from the outside, like every one of them being
//! broken in a different way at once.
//!
//! So the answer lives here, once, and they all ask for it.
//!
//! The old answer was to assume `./content` relative to whatever directory the
//! process happened to be started from. Run from the repository root that
//! worked. Run any other way -- an installed binary, a map opened from a file
//! manager, `cargo run` from a subdirectory -- it silently found nothing, and
//! the tool looked broken rather than misconfigured.
//!
//! So this searches instead, in the order the answer is most likely to be
//! right, and says which one it took, because a wrong guess that explains
//! itself costs a minute and a silent one costs an afternoon.

use crate::project::Project;
use std::path::{Path, PathBuf};

/// The file that marks a directory as a content root beyond doubt.
const MARKER: &str = "kerosene.kerodef";

/// How far up a tree to look before giving up.
const MAX_CLIMB: usize = 6;

/// Whether a directory is a content root.
///
/// The definitions file is the strong signal. Failing that, a directory with
/// both `maps` and `materials` in it is one -- a project that has not written
/// its own class definitions yet is still a project.
pub fn is_content_root(dir: &Path) -> bool {
    if dir.join(MARKER).is_file() {
        return true;
    }
    dir.join("maps").is_dir() && dir.join("materials").is_dir()
}

/// Where a content root was found, so the choice can be explained.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub root: PathBuf,
    pub why: &'static str,
    /// The project file that named it, when one did.
    ///
    /// Carried along because a project says more than where its content is --
    /// what it is called, which map it starts on -- and the caller that asked
    /// where the content was is the caller that wants the rest of it.
    pub project: Option<Project>,
}

impl Found {
    /// A root nothing but the search knows about.
    fn guessed(root: PathBuf, why: &'static str) -> Found {
        Found {
            root,
            why,
            project: None,
        }
    }
}

/// Find the content tree.
///
/// Three places are tried, nearest first: the tree the map being opened lives
/// in, then the working directory, then the directory the executable is in.
/// The map's own tree comes first on purpose -- opening
/// `~/maps/mine/level.keromap` should find `~/maps/mine`'s content, not the
/// content of wherever a shell happened to be.
///
/// Within each place a project file wins over a guess, even a guess that
/// would have been found closer: `--content` aside, a written answer is the
/// only kind anyone can correct, and one that loses to inference is not an
/// answer at all. Between places, nearness still decides -- a project file on
/// the far side of the disk does not get to claim a map that is sitting in a
/// content tree of its own.
pub fn find(explicit: Option<&Path>, map: Option<&Path>) -> Option<Found> {
    if let Some(dir) = explicit {
        // An explicit path is taken at its word even if it looks wrong: being
        // overruled by a guess is worse than being told the answer is empty.
        return Some(Found::guessed(dir.to_path_buf(), "given with --content"));
    }

    let map_dir = map.and_then(|m| m.parent()).map(Path::to_path_buf);
    let working = std::env::current_dir().ok();
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));

    let places: [(Option<PathBuf>, &'static str, &'static str); 3] = [
        (
            map_dir,
            "named by the project holding the map",
            "found next to the map",
        ),
        (
            working,
            "named by the project in the working directory",
            "found from the working directory",
        ),
        (
            beside_exe,
            "named by the project beside the executable",
            "found next to the executable",
        ),
    ];

    for (from, stated, guessed) in &places {
        let Some(from) = from else { continue };
        if let Some(project) = climb_for_project(from) {
            return Some(Found {
                root: project.content.clone(),
                why: stated,
                project: Some(project),
            });
        }
        if let Some(root) = climb(from) {
            return Some(Found::guessed(root, guessed));
        }
    }

    None
}

/// Walk up from a directory looking for a project file.
fn climb_for_project(from: &Path) -> Option<Project> {
    let mut at = from.to_path_buf();
    for _ in 0..MAX_CLIMB {
        if let Some(file) = crate::project::in_directory(&at) {
            match Project::read(&file) {
                Ok(project) => return Some(project),
                // A project file that will not parse is a problem worth
                // reporting and not worth stopping for: falling through to
                // the search leaves a broken editor rather than no editor.
                Err(e) => log::warn!("ignoring {}: {e}", file.display()),
            }
        }
        if !at.pop() {
            break;
        }
    }
    None
}

/// Walk up from a directory looking for a content root, or for something
/// holding one.
fn climb(from: &Path) -> Option<PathBuf> {
    let mut at = from.to_path_buf();
    for _ in 0..MAX_CLIMB {
        if is_content_root(&at) {
            return Some(at);
        }
        // A repository holds its content in `content/`, and a map is usually
        // at `<root>/maps/name.keromap`, so both are worth a look at each
        // level rather than only at the end.
        let candidate = at.join("content");
        if is_content_root(&candidate) {
            return Some(candidate);
        }

        if !at.pop() {
            break;
        }
    }
    None
}

// ---- making a content tree --------------------------------------------------

/// The directories a content tree always has.
///
/// Every one of them is somewhere a tool writes or reads by convention:
/// `maps` and `materials` are what [`is_content_root`] looks for, `art` and
/// `textures` are the two texture sources, and the rest are where Forge,
/// Timbre and the script loader expect to find their inputs.
///
/// They exist as a list rather than as seven `create_dir_all` calls spread
/// across the tools because a directory a tool forgot to make is a tool that
/// looks broken on a fresh project, and that is exactly the class of problem
/// the content-root search was written to end.
pub const CONTENT_DIRS: &[&str] = &[
    "maps",
    "materials",
    "art",
    "textures",
    "models",
    "sound",
    "scripts",
];

/// Create any of a content tree's directories that are missing.
///
/// Returns the ones it made, so a caller can say what it did rather than
/// making a project appear out of nowhere.
///
/// Never fails the caller: a read-only install is a warning, not a reason to
/// refuse to start, for the same reason [`kerosene_config::EngineConf`]'s
/// loader writes its defaults and shrugs when it cannot. The engine has to
/// work on a fresh clone and on a locked-down one alike.
///
/// `dirs` is what the project asked for, when it asked for anything --
/// see [`Project::dirs`]. A project that says nothing gets [`CONTENT_DIRS`],
/// which is every project that exists today.
pub fn scaffold(root: &Path, dirs: Option<&[String]>) -> Vec<String> {
    let wanted: Vec<String> = match dirs {
        Some(named) => named.to_vec(),
        None => CONTENT_DIRS.iter().map(|d| d.to_string()).collect(),
    };

    let mut made = Vec::new();
    for name in wanted {
        // A project's own list is text somebody typed, so it does not get to
        // name a directory outside the tree.
        let name = name.trim().trim_matches('/');
        if name.is_empty() || name.contains("..") {
            log::warn!("ignoring content directory {name:?}: it must be a name inside the tree");
            continue;
        }
        let path = root.join(name);
        if path.is_dir() {
            continue;
        }
        match std::fs::create_dir_all(&path) {
            Ok(()) => made.push(name.to_string()),
            Err(e) => log::warn!("could not create {} ({e})", path.display()),
        }
    }

    if !made.is_empty() {
        log::info!("created content directories: {}", made.join(", "));
    }
    made
}

/// What to tell the user about the content root, in one line.
pub fn describe(found: &Option<Found>) -> String {
    match found {
        Some(f) => match &f.project {
            Some(project) => format!(
                "content: {} ({}, {})",
                f.root.display(),
                f.why,
                project.path.display()
            ),
            None => format!("content: {} ({})", f.root.display(), f.why),
        },
        None => "no content tree found -- no entity classes, no materials. \
                 Start from a project directory, or pass --content."
            .to_string(),
    }
}

#[cfg(test)]
mod tests;
