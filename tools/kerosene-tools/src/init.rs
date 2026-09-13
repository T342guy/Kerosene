// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! `kerosene-tools init` -- start a project.
//!
//! Everything else in the toolset assumes a content tree already exists. This
//! is the one command that makes one: a `.keroproj` saying where the content
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
use std::path::PathBuf;

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

    std::fs::create_dir_all(&args.directory)
        .with_context(|| format!("creating {}", args.directory.display()))?;

    // Canonicalised only for the name and for what gets printed: `init .`
    // should say where it made a project, not say it made one in ".".
    let directory = args
        .directory
        .canonicalize()
        .unwrap_or_else(|_| args.directory.clone());

    let name = args.name.unwrap_or_else(|| {
        directory
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Kerosene Project".to_string())
    });

    // An existing project file is left exactly as it is. It is the one file
    // here somebody is expected to have edited, and rewriting it to add a
    // directory would be a poor trade.
    let existing = kerosene_vfs::project::in_directory(&directory);
    let project_path = match existing {
        Some(path) => {
            println!("init: {} already names this project", path.display());
            path
        }
        None => {
            let path = directory.join(format!(
                "{}.{}",
                slug(&name),
                kerosene_vfs::project::EXTENSION
            ));
            kerosene_vfs::Project::write_new(&path, &name, &args.content)?;
            println!("init: wrote {}", path.display());
            path
        }
    };

    let project = kerosene_vfs::Project::read(&project_path)?;
    let made = kerosene_vfs::root::scaffold(&project.content, project.dirs.as_deref());

    println!("init: content tree at {}", project.content.display());
    if made.is_empty() {
        println!("  every directory was already there");
    } else {
        for name in &made {
            println!("  created {name}/");
        }
    }
    println!("\nNext: put a texture in with `alchemy new-texture`, or open the editor.");
    Ok(())
}

/// A project name as a filename: lowercase, spaces to dashes, nothing exotic.
fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests;
