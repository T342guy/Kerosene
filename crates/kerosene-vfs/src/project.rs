// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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
//!     "game"     "my-mod"        // the Cargo package that is the game
//!     "bin"      "mymod"         // its binary, when not named after the package
//!
//!     // The tree to create, if the standard one is not wanted. Repeat the
//!     // key rather than separating with commas.
//!     "dir"      "maps"
//!     "dir"      "materials"
//!
//!     // Steam, for a build with the `steam` feature. See the Steam page of
//!     // the game developer docs.
//!     "steam_appid" "480"
//!     "steam_depot" "481"            // defaults to the app id plus one
//!
//!     // What the game awards and counts. Declared, so a typo in a map is
//!     // an error on the console rather than an achievement nobody gets.
//!     "achievements"
//!     {
//!         "ACH_FIRST_DOOR"  "Open the first door"
//!     }
//!     "stats"
//!     {
//!         "doors_opened"    "int"
//!         "distance"        "float"
//!     }
//!     "dlc"
//!     {
//!         "1234560"         "Soundtrack"
//!     }
//! }
//! ```
//!
//! `content` is relative to the file, so the project can be moved or cloned
//! anywhere and still be right. Everything but the block itself is optional:
//! a project file with nothing in it still marks a directory as a project,
//! and `content` defaults to `content` beside it, then to the directory the
//! file is in.

use kerosene_kv::KeyValues;
use std::path::{Path, PathBuf};

/// The extension a project file carries.
pub const EXTENSION: &str = "keroproj";

/// What a project says about itself.
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// The Cargo package whose binary *is* the game.
    ///
    /// A project that only holds content has none, and runs and ships the
    /// engine's own runtime instead. Naming a package is what turns a content
    /// tree into a game somebody can be handed: `kiln --ship` builds it, and
    /// the editor builds and launches it on F9.
    pub game: Option<String>,
    /// The name of that package's binary, when it is not the package's own
    /// name -- a package `my-game` with `[[bin]] name = "mygame"`.
    pub bin: Option<String>,
    /// The directories the content tree is made of, when the project says.
    ///
    /// The tree is created on first run from [`root::CONTENT_DIRS`], which is
    /// the layout every tool assumes. This is how a project that wants a
    /// different one says so and has it believed -- the same bargain as
    /// `content`: inference is a guess, and this is the way to overrule it.
    ///
    /// `None`, which is every project written so far, means the defaults.
    ///
    /// [`root::CONTENT_DIRS`]: crate::root::CONTENT_DIRS
    pub dirs: Option<Vec<String>>,
    /// The Steam app id, when the game ships on Steam.
    pub steam_appid: Option<u32>,
    /// The depot `kiln --ship --steam` uploads to. Defaults to the app id
    /// plus one, which is what Steamworks gives a new app.
    pub steam_depot: Option<u32>,
    /// Declared achievements: id, then display name.
    pub achievements: Vec<(String, String)>,
    /// Declared stats: name, then `int` or `float`.
    pub stats: Vec<(String, String)>,
    /// Declared DLC: app id, then name.
    pub dlc: Vec<(u32, String)>,
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
        let kv = KeyValues::parse(text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        // Accept the block either as the document root or nested inside one,
        // because both are things people write and neither is wrong.
        let block = kv.block("project").unwrap_or(&kv);

        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let content = match block
            .get("content")
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
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
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
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
            bin: block
                .get("bin")
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .map(str::to_string),
            // Repeated `dir` keys rather than one comma-separated value: the
            // format has `get_all` for exactly this, and a list somebody has
            // to punctuate correctly is a list somebody will punctuate wrong.
            dirs: {
                let named: Vec<String> = block
                    .get_all("dir")
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .map(str::to_string)
                    .collect();
                (!named.is_empty()).then_some(named)
            },
            steam_appid: number(block, "steam_appid", path)?,
            steam_depot: number(block, "steam_depot", path)?,
            achievements: pairs(block, "achievements"),
            stats: pairs(block, "stats"),
            dlc: pairs(block, "dlc")
                .into_iter()
                .map(|(id, name)| {
                    id.parse::<u32>().map(|id| (id, name)).map_err(|_| {
                        anyhow::anyhow!("{}: dlc `{id}` is not an app id", path.display())
                    })
                })
                .collect::<anyhow::Result<_>>()?,
        })
    }

    /// Write a project file describing a content tree beside it.
    ///
    /// Used to start a project rather than to maintain one: the file is meant
    /// to be edited by hand afterwards, so it is written with the comments a
    /// person would want and nothing they would have to work around.
    pub fn write_new(path: &Path, name: &str, content_relative: &str) -> anyhow::Result<()> {
        Self::write_with(path, name, content_relative, &[])
    }

    /// [`Project::write_new`], with more keys after `name` and `content`:
    /// `startmap`, `game`, anything the project file knows.
    pub fn write_with(
        path: &Path,
        name: &str,
        content_relative: &str,
        extra: &[(&str, &str)],
    ) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("creating {}: {e}", parent.display()))?;
        }
        // Through the KeyValues writer, so a name with a quote in it -- or
        // a content path with a backslash -- comes back out of the parser
        // as it went in.
        let mut kv = KeyValues::new("project");
        kv.push("name", name);
        kv.push("content", content_relative);
        for (key, value) in extra {
            kv.push(*key, *value);
        }
        let body = format!(
            "// A Kerosene project. Every tool reads this to find the content\n\
             // tree, so there is one answer rather than one guess per tool.\n\
             // `content` is relative to this file, so the project can live anywhere.\n\
             {}",
            kv.to_text()
        );
        std::fs::write(path, body)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))?;
        Ok(())
    }
}

/// An optional whole number, refused rather than ignored when it is not one:
/// a mistyped app id quietly read as "none" would ship a game with Steam
/// switched off.
fn number(block: &KeyValues, key: &str, path: &Path) -> anyhow::Result<Option<u32>> {
    match block.get(key).map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => v
            .parse()
            .map(Some)
            .map_err(|_| anyhow::anyhow!("{}: {key} `{v}` is not a number", path.display())),
    }
}

/// Every key/value pair in a named sub-block, in order.
fn pairs(block: &KeyValues, name: &str) -> Vec<(String, String)> {
    block
        .blocks(name)
        .flat_map(|b| b.pairs())
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
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
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
        })
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
                    !matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::RootDir
                    )
                }) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".")
    }
    out
}

#[cfg(test)]
mod tests;
