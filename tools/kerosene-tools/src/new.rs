// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `kerosene-tools new` -- start a game.
//!
//! ```text
//! kerosene-tools new mygame
//! cd mygame
//! cargo play
//! ```
//!
//! Makes a Cargo package that depends on `kerosene` and is a game from its
//! first build: a `Game` with a class of its own, its own toolset binary, a
//! project file naming both, a starter map that uses the class, and cargo
//! aliases for the loop -- `cargo play`, `cargo tools`, `cargo ship`.
//! Everything is built on the developer's machine from source, so the same
//! three commands work on Linux, Windows and macOS.
//!
//! `--content-only` makes a project with no Rust in it instead, which runs
//! on the stock `kerosene` runtime: that is `init`, in a new directory.

use anyhow::{Context, Result, bail};
use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "new", version, about = "Start a Kerosene game")]
struct Args {
    /// The directory to make. Must not exist, or be empty.
    directory: PathBuf,
    /// The game's name, as its window and README say it. Defaults to the
    /// directory's name.
    #[arg(short, long)]
    name: Option<String>,
    /// A content-only project on the stock runtime: no Rust, no build.
    #[arg(long)]
    content_only: bool,
    /// Depend on the Kerosene checkout at this path, rather than the
    /// published crate.
    #[arg(long, value_name = "DIR", conflicts_with_all = ["kerosene_git", "kerosene_version"])]
    kerosene_path: Option<PathBuf>,
    /// Depend on Kerosene from this git repository.
    #[arg(long, value_name = "URL", conflicts_with = "kerosene_version")]
    kerosene_git: Option<String>,
    /// Depend on this published version of Kerosene. The toolset's own by
    /// default.
    #[arg(long, value_name = "VERSION")]
    kerosene_version: Option<String>,
    /// Do not make the directory a git repository.
    #[arg(long)]
    no_git: bool,
}

/// The files a game starts as, filled in by [`fill`].
const CARGO_TOML: &str = include_str!("new/template/Cargo.toml.in");
const CARGO_CONFIG: &str = include_str!("new/template/cargo-config.toml.in");
const MAIN_RS: &str = include_str!("new/template/main.rs.in");
const TOOLS_RS: &str = include_str!("new/template/tools.rs.in");
const GAME_RS: &str = include_str!("new/template/game.rs.in");
const GITIGNORE: &str = include_str!("new/template/gitignore.in");
const README: &str = include_str!("new/template/README.md.in");
const GITATTRIBUTES: &str = include_str!("new/template/gitattributes.in");
const CI: &str = include_str!("new/template/ci.yml.in");

/// Package names a game cannot have: Kerosene itself, which it depends on,
/// and the names Rust keeps for its own crates.
const RESERVED: &[&str] = &[
    "kerosene",
    "std",
    "core",
    "alloc",
    "test",
    "proc-macro",
    "self",
];

/// Where the dependency on Kerosene comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// crates.io, at this version.
    Version(String),
    /// A checkout on disk.
    Path(PathBuf),
    /// A git repository.
    Git(String),
    /// A git repository, at a release tag: the version, as tags are named.
    GitTag(String, String),
}

impl Source {
    /// The dependency, as the inline table `Cargo.toml` spells it. Without
    /// the engine's default features, so the game's own `audio` feature is
    /// the switch.
    pub fn to_toml(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('\\', "/").replace('"', "\\\""));
        let from = match self {
            Source::Version(v) => format!("version = {}", quote(v)),
            Source::Path(p) => format!("path = {}", quote(&plain_path(p))),
            Source::Git(url) => format!("git = {}", quote(url)),
            Source::GitTag(url, tag) => format!("git = {}, tag = {}", quote(url), quote(tag)),
        };
        format!("{{ {from}, default-features = false }}")
    }

    /// The Kerosene this toolset was built from: a checkout of it when it
    /// was built from one that is still there, its repository when it was
    /// installed from git, and otherwise the published crate at the
    /// toolset's own version.
    fn default_for_this_toolset() -> Source {
        Source::for_toolset_at(Path::new(env!("CARGO_MANIFEST_DIR")))
    }

    /// [`Source::default_for_this_toolset`], for a toolset whose source is
    /// at `manifest_dir`.
    fn for_toolset_at(manifest_dir: &Path) -> Source {
        let text = manifest_dir.display().to_string().replace('\\', "/");
        // `cargo install --git` and `cargo install` build in Cargo's own
        // caches, which it may empty; a game must not point into them.
        if text.contains("/git/checkouts/") {
            return Source::GitTag(
                env!("CARGO_PKG_REPOSITORY").to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            );
        }
        let checkout = manifest_dir.join("../../crates/kerosene");
        if !text.contains("/registry/src/") && checkout.join("Cargo.toml").is_file() {
            return Source::Path(checkout.canonicalize().unwrap_or(checkout));
        }
        Source::Version(env!("CARGO_PKG_VERSION").to_string())
    }
}

/// A path as a person would write it. Windows' `canonicalize` answers in
/// the `\\?\C:\...` form, which Cargo does not accept in a manifest; the
/// drive-letter form means the same file.
fn plain_path(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => rest.to_string(),
        _ => text,
    }
}

/// What a new game is called, in each of the places it is called something.
#[derive(Clone, Debug, PartialEq)]
pub struct Names {
    /// As people read it: "Orbital Drift".
    pub title: String,
    /// The Cargo package and binary: `orbital-drift`.
    pub package: String,
    /// The Rust type: `OrbitalDrift`.
    pub type_name: String,
    /// The starter map: `orbital_drift_start`.
    pub map: String,
}

impl Names {
    pub fn from_title(title: &str) -> Names {
        let words: Vec<String> = title
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        let words = if words.is_empty() {
            vec!["game".to_string()]
        } else {
            words
        };
        let mut package = words.join("-");
        // A package name starts with a letter.
        if !package.starts_with(|c: char| c.is_ascii_alphabetic()) {
            package = format!("game-{package}");
        }
        // A game called "Kerosene" would depend on itself.
        if RESERVED.contains(&package.as_str()) {
            package = format!("{package}-game");
        }
        let mut type_name: String = words
            .iter()
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            })
            .collect();
        if !type_name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            type_name = format!("Game{type_name}");
        }
        // `Game` is the trait every game implements; a type of the same name
        // would read as the trait.
        if type_name == "Game" {
            type_name = "TheGame".into();
        }
        Names {
            title: title.trim().to_string(),
            map: format!("{}_start", package.replace('-', "_")),
            package,
            type_name,
        }
    }
}

/// Fill a template's `@KEY@` holes.
fn fill(template: &str, names: &Names, source: &Source) -> String {
    template
        .replace("@NAME@", &names.title)
        .replace("@PACKAGE@", &names.package)
        .replace("@TYPE@", &names.type_name)
        .replace("@MAP@", &names.map)
        .replace("@KEROSENE@", &source.to_toml())
        .replace("@MSRV@", env!("CARGO_PKG_RUST_VERSION"))
}

pub fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(std::iter::once("new".to_string()).chain(args));
    let dir = &args.directory;
    if dir.exists() && std::fs::read_dir(dir).map(|mut d| d.next().is_some())? {
        bail!(
            "{} already has files in it. `new` starts a game in a fresh directory; \
             `init` makes a project of what is already there.",
            dir.display()
        );
    }
    let title = match &args.name {
        Some(name) => name.clone(),
        None => dir
            .canonicalize()
            .ok()
            .or_else(|| std::env::current_dir().ok().map(|c| c.join(dir)))
            .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "game".into()),
    };
    let names = Names::from_title(&title);
    if args.content_only {
        make_content_only(dir, &names)?;
        println!(
            "new: made the content-only project {} in {}",
            names.title,
            dir.display()
        );
        println!("\nNext:\n  cd {}\n  kerosene-tools play", dir.display());
        return Ok(());
    }
    let source = if let Some(path) = &args.kerosene_path {
        Source::Path(
            path.canonicalize()
                .with_context(|| format!("{}", path.display()))?,
        )
    } else if let Some(url) = &args.kerosene_git {
        Source::Git(url.clone())
    } else if let Some(version) = &args.kerosene_version {
        Source::Version(version.clone())
    } else {
        Source::default_for_this_toolset()
    };
    make_game(dir, &names, &source)?;
    let git = !args.no_git && git_init(dir);
    println!("new: made the game {} in {}", names.title, dir.display());
    if git {
        println!("  and a git repository for it");
    }
    println!("  kerosene from {}", source.to_toml());
    println!(
        "\nNext:\n  cd {}\n  cargo play        # the first build takes a few minutes",
        dir.display()
    );
    Ok(())
}

/// Write a game crate into `dir`.
pub fn make_game(dir: &Path, names: &Names, source: &Source) -> Result<()> {
    let write = |path: &str, text: String| -> Result<()> {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    };
    write("Cargo.toml", fill(CARGO_TOML, names, source))?;
    write(".cargo/config.toml", fill(CARGO_CONFIG, names, source))?;
    write("src/main.rs", fill(MAIN_RS, names, source))?;
    write("src/tools.rs", fill(TOOLS_RS, names, source))?;
    write("src/game.rs", fill(GAME_RS, names, source))?;
    write(".gitignore", fill(GITIGNORE, names, source))?;
    write("README.md", fill(README, names, source))?;
    write(".gitattributes", fill(GITATTRIBUTES, names, source))?;
    write(".github/workflows/ci.yml", fill(CI, names, source))?;
    project(dir, names, Some(&names.package))?;
    Ok(())
}

/// Make `dir` a git repository, unless it is already inside one or there is
/// no git. Quietly: a game without version control is still a game.
fn git_init(dir: &Path) -> bool {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    !git(&["rev-parse", "--is-inside-work-tree"]) && git(&["init", "--quiet"])
}

/// A project with no Rust: the project file, the tree, the starter map with
/// no class of the game's own in it.
pub fn make_content_only(dir: &Path, names: &Names) -> Result<()> {
    project(dir, names, None)
}

/// The project file, the content tree and the starter map.
fn project(dir: &Path, names: &Names, game: Option<&str>) -> Result<()> {
    let mut keys = vec![("startmap", names.map.as_str())];
    if let Some(game) = game {
        keys.push(("game", game));
    }
    let file = dir.join(format!(
        "{}.{}",
        names.package,
        kerosene_vfs::project::EXTENSION
    ));
    kerosene_vfs::Project::write_with(&file, &names.title, "content", &keys)?;
    let content = dir.join("content");
    std::fs::create_dir_all(&content)?;
    kerosene_vfs::root::scaffold(&content, None);
    let map = kerosene_map::starter::room(game.is_some());
    let problems = map.validate();
    if !problems.is_empty() {
        bail!("the starter map is wrong: {problems:?}");
    }
    let maps = content.join("maps");
    let path = maps.join(format!("{}.keromap", names.map));
    std::fs::create_dir_all(&maps)?;
    std::fs::write(&path, map.to_text()).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
