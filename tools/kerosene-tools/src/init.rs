// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `kerosene-tools init` -- start a project.
//!
//! Everything else in the toolset assumes a content tree already exists. This
//! is the one command that makes one: a `.kproj` saying where the content
//! is, and the directories under it that every other tool expects to find.
//!
//! It exists because "make a new project" was, until now, a thing you did by
//! reading the docs and creating seven directories by hand -- and a tree with
//! one of them missing does not announce itself. It looks like whichever tool
//! went looking for it being broken.
//!
//! Running it on a directory that is already a project fills in what is
//! missing and leaves the rest alone, so it is safe to run twice and useful
//! on a tree that predates a directory the engine has since started using.

use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "init", version, about = "Start a Kerosene project")]
struct Args {
    /// Where the project goes. Created if it is not there.
    #[arg(default_value = ".")]
    directory: PathBuf,
    /// What to call it. Defaults to the directory's own name.
    #[arg(short, long)]
    name: Option<String>,
    /// The content tree, relative to the project file.
    #[arg(long, default_value = "content")]
    content: String,
}

pub fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(std::iter::once("init".to_string()).chain(args));
    let made = init_project(&args.directory, args.name.as_deref(), &args.content)?;

    if made.existed {
        println!(
            "init: {} already names this project",
            made.project.display()
        );
    } else {
        println!("init: wrote {}", made.project.display());
    }
    println!("init: content tree at {}", made.content.display());
    if made.created.is_empty() {
        println!("  every directory was already there");
    } else {
        for name in &made.created {
            println!("  created {name}/");
        }
    }
    println!("\nNext: put a texture in with `alchemy new-texture`, or open the editor.");
    Ok(())
}

/// What [`init_project`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Initialized {
    /// The project file.
    pub project: PathBuf,
    /// Whether the project file was already there, and so left alone.
    pub existed: bool,
    /// The content tree it names.
    pub content: PathBuf,
    /// The directories that were missing and are now made.
    pub created: Vec<String>,
}

/// Make `directory` a project: a project file, unless there is one, and
/// the content tree beside it. What `init` does, and what the toolset's
/// start page does when asked to make a project of a folder.
///
/// `name` defaults to the directory's own name; `content` is relative to
/// the project file.
pub fn init_project(directory: &Path, name: Option<&str>, content: &str) -> Result<Initialized> {
    std::fs::create_dir_all(directory)
        .with_context(|| format!("creating {}", directory.display()))?;

    // Canonicalised only for the name and for what gets printed: `init .`
    // should say where it made a project, not say it made one in ".".
    let directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());

    let name = name.map(str::to_string).unwrap_or_else(|| {
        directory
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Kerosene Project".to_string())
    });

    // An existing project file is left exactly as it is. It is the one file
    // here somebody is expected to have edited, and rewriting it to add a
    // directory would be a poor trade.
    let existing = kerosene_vfs::project::in_directory(&directory);
    let existed = existing.is_some();
    let project_path = match existing {
        Some(path) => path,
        None => {
            let path = directory.join(format!(
                "{}.{}",
                slug(&name),
                kerosene_vfs::project::EXTENSION
            ));
            kerosene_vfs::Project::write_new(&path, &name, content)?;
            path
        }
    };

    let project = kerosene_vfs::Project::read(&project_path)?;
    let created = kerosene_vfs::root::scaffold(&project.content, project.dirs.as_deref());
    Ok(Initialized {
        project: project_path,
        existed,
        content: project.content,
        created,
    })
}

/// A project name as a filename: lowercase, spaces to dashes, nothing exotic.
/// The same spelling Kiln gives the archive, so `My Game.kproj` and
/// `my_game.vault` are recognisably one thing.
fn slug(name: &str) -> String {
    match kiln::slug(name).as_str() {
        "content" => "project".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
