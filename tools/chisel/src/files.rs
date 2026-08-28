// SPDX-License-Identifier: LGPL-3.0-or-later
//! Naming, finding and moving map files.
//!
//! The editor used to have exactly one thing it could do with a file: write
//! it to `maps/untitled.voidmap`. There was no way to name a map, no way to
//! rename one, and a new document -- which is what the editor opens with --
//! had no path at all, so ctrl-S put "no path to save to" in the status bar
//! and did nothing else. From the outside that is an editor that cannot save.
//!
//! What a name means is decided here rather than in the dialog, because it is
//! the part worth being sure about: typing `arena` has to mean the same thing
//! every time, and a name that quietly escapes the content tree is a map
//! nobody finds again.

use std::path::{Path, PathBuf};

/// The extension every map carries.
pub const MAP_EXTENSION: &str = "voidmap";

/// Build artefacts that belong to a map and follow it when it moves.
///
/// A `.voidbsp` left behind under the old name is worse than clutter: the
/// game still loads it, so a renamed map appears to work under a name that no
/// longer exists and to be missing under the one that does.
pub const ARTEFACTS: &[&str] = &["voidbsp", "voidprt", "voidleak"];

/// Turn what someone typed into the path of a map.
///
/// A bare name means a map in this project: `arena` is
/// `<content>/maps/arena.voidmap`, which is where the compilers and the game
/// will look for it. An absolute path is taken at its word, for the case
/// where a map genuinely lives somewhere else.
pub fn resolve(typed: &str, content_root: &Path) -> Result<PathBuf, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err("a map needs a name".into());
    }
    if typed.split(['/', '\\']).any(|part| part == "..") {
        // Not a security boundary -- anyone running the editor can write
        // anywhere. It is that `../../thing` resolves against a directory the
        // typist is not looking at, so the file lands somewhere they will not
        // think to look for it.
        return Err("a map name cannot contain `..`; type a full path instead".into());
    }

    let path = Path::new(typed);
    let base = if path.is_absolute() {
        path.to_path_buf()
    } else {
        content_root.join("maps").join(path)
    };
    Ok(with_map_extension(&base))
}

/// The same path, ending in `.voidmap`.
///
/// Appended rather than replaced: `arena.v2` is a name someone chose, and
/// turning it into `arena.voidmap` would silently save over a different map.
pub fn with_map_extension(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(MAP_EXTENSION)) {
        return path.to_path_buf();
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(MAP_EXTENSION);
    path.with_file_name(name)
}

/// The name to show for a map path, relative to the project when it is in it.
///
/// `maps/arena.voidmap` rather than the full path, because the full path is
/// mostly the same forty characters on every map and the name is the part
/// being read.
pub fn label(path: &Path, content_root: &Path) -> String {
    path.strip_prefix(content_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every map in a project, in a stable order.
pub fn maps_in(content_root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(&content_root.join("maps"), 0, &mut found);
    found.sort();
    found
}

/// How deep under `maps/` to look. Deep enough for `maps/chapter1/arena`,
/// shallow enough that a stray symlink cannot cost a second at startup.
const MAX_DEPTH: usize = 3;

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH { return }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, depth + 1, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(MAP_EXTENSION)) {
            out.push(path);
        }
    }
}

/// Move a map and the build artefacts that belong to it.
///
/// Returns what moved besides the map itself, so the editor can say. A
/// missing artefact is not an error: most maps have never been compiled.
pub fn move_map(from: &Path, to: &Path) -> std::io::Result<Vec<PathBuf>> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to)?;

    let mut moved = Vec::new();
    for extension in ARTEFACTS {
        let source = from.with_extension(extension);
        if !source.is_file() { continue }
        let target = to.with_extension(extension);
        if std::fs::rename(&source, &target).is_ok() {
            moved.push(target);
        }
    }
    Ok(moved)
}

#[cfg(test)]
mod tests;
