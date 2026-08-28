// SPDX-License-Identifier: LGPL-3.0-or-later
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

use std::path::{Path, PathBuf};

/// The file that marks a directory as a content root beyond doubt.
const MARKER: &str = "voidengine.voiddef";

/// How far up a tree to look before giving up.
const MAX_CLIMB: usize = 6;

/// Whether a directory is a content root.
///
/// The definitions file is the strong signal. Failing that, a directory with
/// both `maps` and `materials` in it is one -- a project that has not written
/// its own class definitions yet is still a project.
pub fn is_content_root(dir: &Path) -> bool {
    if dir.join(MARKER).is_file() { return true }
    dir.join("maps").is_dir() && dir.join("materials").is_dir()
}

/// Where a content root was found, so the choice can be explained.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub root: PathBuf,
    pub why: &'static str,
}

/// Find the content tree.
///
/// In order: what the user asked for, the tree the map being opened lives in,
/// the working directory, and the tree beside the executable. The map's own
/// tree comes before the working directory on purpose -- opening
/// `~/maps/mine/level.voidmap` should find `~/maps/mine`'s content, not the
/// content of wherever a shell happened to be.
pub fn find(explicit: Option<&Path>, map: Option<&Path>) -> Option<Found> {
    if let Some(dir) = explicit {
        // An explicit path is taken at its word even if it looks wrong: being
        // overruled by a guess is worse than being told the answer is empty.
        return Some(Found { root: dir.to_path_buf(), why: "given with --content" });
    }

    if let Some(map) = map
        && let Some(root) = map.parent().and_then(climb)
    {
        return Some(Found { root, why: "found next to the map" });
    }

    if let Some(root) = std::env::current_dir().ok().and_then(|d| climb(&d)) {
        return Some(Found { root, why: "found from the working directory" });
    }

    if let Some(root) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .and_then(|dir| climb(&dir))
    {
        return Some(Found { root, why: "found next to the executable" });
    }

    None
}

/// Walk up from a directory looking for a content root, or for something
/// holding one.
fn climb(from: &Path) -> Option<PathBuf> {
    let mut at = from.to_path_buf();
    for _ in 0..MAX_CLIMB {
        if is_content_root(&at) { return Some(at) }
        // A repository holds its content in `content/`, and a map is usually
        // at `<root>/maps/name.voidmap`, so both are worth a look at each
        // level rather than only at the end.
        let candidate = at.join("content");
        if is_content_root(&candidate) { return Some(candidate) }

        if !at.pop() { break }
    }
    None
}

/// What to tell the user about the content root, in one line.
pub fn describe(found: &Option<Found>) -> String {
    match found {
        Some(f) => format!("content: {} ({})", f.root.display(), f.why),
        None => "no content tree found -- no entity classes, no materials. \
                 Start from a project directory, or pass --content."
            .to_string(),
    }
}

#[cfg(test)]
mod tests;
