// SPDX-License-Identifier: MPL-2.0
//! `.keroproj` -- a project's own account of where its content is.
//!
//! Everything up to here *infers* the content root: climb the tree looking
//! for something that has the shape of one. That works, and it is what makes
//! a fresh clone open without configuration, but inference is a guess and a
//! guess can be wrong in ways nobody can correct. There was no way to say
//! "the content is *here*" and have every tool believe it.
//!
//! A project file is that way. It sits at the top of a project, it names the
//! content directory, and every tool that finds it stops guessing.
//!
//! ```text
//! project
//! {
//!     "name"     "My Mod"
//!     "content"  "content"
//!     "startmap" "mm_intro"
//!     "game"     "my-mod"
//! }
//! ```
//!
//! `content` is relative to the file, so the project can be moved or cloned
//! anywhere and still be right. Everything but the block itself is optional:
//! a project file with nothing in it still marks a directory as a project,
//! and `content` defaults to `content` beside it, then to the directory the
//! file is in.

use std::path::{Path, PathBuf};
use kerosene_kv::KeyValues;

/// The extension a project file carries.
pub const EXTENSION: &str = "keroproj";

/// What a project says about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// The file this was read from.
    pub path: PathBuf,
    /// What to call it, for a title bar. Defaults to the file's own name.
    pub name: String,
    /// The content tree, resolved against the project file's directory.
    pub content: PathBuf,
    /// The map to load when nothing else says which. Optional: a project
    /// that is a library of maps has no one answer, and inventing one would
    /// be worse than admitting it.
    pub start_map: Option<String>,
    /// The Cargo package whose binary *is* the game, for `kiln --ship`.
    ///
    /// A project that only holds content has none, and ships the engine's own
    /// runtime instead. Naming a package is what turns a content tree into a
    /// game somebody can be handed.
    pub game: Option<String>,
}

impl Project {
    /// Read a project file.
    pub fn read(path: &Path) -> anyhow::Result<Project> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        Project::parse(&text, path)
    }

    /// Parse a project file whose contents are already in hand.
    pub fn parse(text: &str, path: &Path) -> anyhow::Result<Project> {
        let kv = KeyValues::parse(text)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        // Accept the block either as the document root or nested inside one,
        // because both are things people write and neither is wrong.
        let block = kv.block("project").unwrap_or(&kv);

        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let content = match block.get("content").map(str::trim).filter(|c| !c.is_empty()) {
            Some(relative) => dir.join(relative),
            // No `content` key: the conventional layout first, then the
            // project directory itself, for a project that is its own tree.
            None if dir.join("content").is_dir() => dir.join("content"),
            None => dir.clone(),
        };

        let name = block
            .get("name")
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
            });

        Ok(Project {
            path: path.to_path_buf(),
            name,
            content: normalise(&content),
            start_map: block
                .get("startmap")
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_string),
            game: block
                .get("game")
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .map(str::to_string),
        })
    }

    /// Write a project file describing a content tree beside it.
    ///
    /// Used to start a project rather than to maintain one: the file is meant
    /// to be edited by hand afterwards, so it is written with the comments a
    /// person would want and nothing they would have to work around.
    pub fn write_new(path: &Path, name: &str, content_relative: &str) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = format!(
            "// A Kerosene project. Every tool reads this to find the content\n\
             // tree, so there is one answer rather than one guess per tool.\n\
             project\n\
             {{\n\
             \t\"name\" \"{name}\"\n\
             \t// Relative to this file, so the project can live anywhere.\n\
             \t\"content\" \"{content_relative}\"\n\
             }}\n"
        );
        std::fs::write(path, body)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))?;
        Ok(())
    }
}

/// The first project file directly in a directory, by name.
///
/// Sorted, so a directory that somehow has two does not depend on the order
/// the filesystem hands them back -- which differs between machines and is
/// exactly the kind of thing that makes a bug reproduce for one person only.
pub fn in_directory(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION)))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Tidy `a/./b` and `a/b/../c` out of a path, without touching the disk.
///
/// `Path::canonicalize` would do more and require the path to exist, which a
/// content directory named by a project file that has not been built yet
/// need not.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // Only collapse when there is something to collapse into; a
                // leading `..` is meaningful and must survive.
                if out.components().next_back().is_some_and(|c| {
                    !matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir)
                }) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    if out.as_os_str().is_empty() { out.push(".") }
    out
}

#[cfg(test)]
mod tests;
