// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Vault -- the Kerosene content archive tool.
//!
//! Packs a content tree into a single `.vault` file, the VPK analogue. A mod
//! ships as a handful of archives instead of tens of thousands of loose files,
//! which matters for distribution, for load times, and for keeping content
//! tamper-evident: every entry carries a CRC that is checked on read.
//!
//! Archives mount into the engine's search path alongside loose directories,
//! and loose files win. That is deliberate: during development you drop a file
//! next to a shipped archive and it takes effect immediately, with no repack.
//!
//! This is a library that the unified toolset invokes as the `vault`
//! subcommand:
//!
//! ```text
//! kerosene-tools vault pack content -o content.vault
//! kerosene-tools vault list content.vault
//! kerosene-tools vault verify content.vault
//! kerosene-tools vault unpack content.vault -o extracted
//! ```

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use kerosene_vfs::{Archive, ArchiveBuilder};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "vault",
    version,
    about = "Pack and inspect Kerosene content archives"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Pack a directory tree into an archive.
    Pack {
        /// Directory to pack. Its contents become the archive root.
        directory: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Only include files with these extensions. Repeatable.
        #[arg(short = 'e', long = "ext")]
        extensions: Vec<String>,
        /// Skip files matching these substrings. Repeatable.
        #[arg(long = "exclude")]
        excludes: Vec<String>,
        /// Only include what this file lists: one virtual path per line, a
        /// directory ending in `/` taking everything under it. `#` starts a
        /// comment. Every line must match something, so a renamed file is an
        /// error here rather than a hole in the archive.
        #[arg(long = "list")]
        list: Option<PathBuf>,
    },
    /// List an archive's contents.
    List {
        archive: PathBuf,
        /// Show sizes and checksums.
        #[arg(short, long)]
        long: bool,
    },
    /// Read every entry and check it against its stored checksum.
    Verify { archive: PathBuf },
    /// Extract an archive.
    Unpack {
        archive: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
}

/// Entry point for the `vault` subcommand of the unified toolset.
///
/// The first element of `args` must be anything -- the toolset's dispatcher
/// has already consumed the subcommand word, so the rest of the command line
/// is handed here verbatim and clap parses it as `vault <args>`.
pub fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(std::iter::once("vault".to_string()).chain(args));
    match args.command {
        Command::Pack {
            directory,
            output,
            extensions,
            excludes,
            list,
        } => {
            let listed = match list {
                Some(file) => Some(read_list(&file)?),
                None => None,
            };
            pack(
                &directory,
                &output,
                &extensions,
                &excludes,
                listed.as_deref(),
            )
        }
        Command::List { archive, long } => list(&archive, long),
        Command::Verify { archive } => verify(&archive),
        Command::Unpack { archive, output } => unpack(&archive, &output),
    }
}

/// A pack list: its lines, without comments or blanks, as virtual paths.
fn read_list(file: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("reading the list {}", file.display()))?;
    Ok(parse_list(&text))
}

fn parse_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.replace('\\', "/").to_lowercase())
        .collect()
}

/// Whether a list entry takes a virtual path: the same file, or a
/// directory (ending in `/`) the file is under.
fn listed(entry: &str, virtual_path: &str) -> bool {
    match entry.strip_suffix('/') {
        Some(_) => virtual_path.starts_with(entry),
        None => virtual_path == entry,
    }
}

fn pack(
    dir: &Path,
    out: &Path,
    extensions: &[String],
    excludes: &[String],
    list: Option<&[String]>,
) -> Result<()> {
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }

    let wanted: Vec<String> = extensions
        .iter()
        .map(|e| e.trim_start_matches('.').to_lowercase())
        .collect();
    let mut builder = ArchiveBuilder::new();
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();

    let mut total = 0u64;
    let mut skipped = 0usize;
    let mut used = vec![false; list.map_or(0, <[String]>::len)];
    for (disk, virtual_path) in files {
        if !wanted.is_empty() {
            let ext = virtual_path.rsplit('.').next().unwrap_or("").to_lowercase();
            if !wanted.contains(&ext) {
                skipped += 1;
                continue;
            }
        }
        if let Some(list) = list {
            let folded = virtual_path.to_lowercase();
            match list.iter().position(|entry| listed(entry, &folded)) {
                Some(i) => used[i] = true,
                None => {
                    skipped += 1;
                    continue;
                }
            }
        }
        if excludes.iter().any(|e| virtual_path.contains(e.as_str())) {
            skipped += 1;
            continue;
        }
        total += std::fs::metadata(&disk).map(|m| m.len()).unwrap_or(0);
        builder
            .add_file(&virtual_path, &disk)
            .with_context(|| format!("adding {}", disk.display()))?;
    }

    if let Some(list) = list {
        let unmatched: Vec<&str> = list
            .iter()
            .zip(&used)
            .filter(|(_, used)| !**used)
            .map(|(entry, _)| entry.as_str())
            .collect();
        if !unmatched.is_empty() {
            bail!(
                "the list names what {} does not have (or the filters took): {}",
                dir.display(),
                unmatched.join(", ")
            );
        }
    }

    if builder.is_empty() {
        bail!(
            "nothing to pack from {} (check --ext and --exclude)",
            dir.display()
        );
    }

    let size = builder
        .write(out)
        .with_context(|| format!("writing {}", out.display()))?;
    println!(
        "vault: packed {} files ({:.1} KiB of content) into {}",
        builder.len(),
        total as f64 / 1024.0,
        out.display()
    );
    if skipped > 0 {
        println!("  {skipped} files skipped by filters");
    }
    println!("  archive is {:.1} KiB", size as f64 / 1024.0);
    Ok(())
}

/// Walk a directory, pairing each file with the virtual path it will hold.
fn collect(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        // The entry's own type, not the target's: a symlinked directory is
        // not followed, so a link back up the tree cannot recurse forever
        // and a link out of it cannot pack something the tree does not hold.
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            log::warn!("vault: skipping symlink {}", path.display());
            continue;
        }
        if kind.is_dir() {
            collect(root, &path, out)?;
        } else if kind.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            // Archives always use forward slashes, whatever the host uses.
            let virtual_path = relative.to_string_lossy().replace('\\', "/");
            out.push((path.clone(), virtual_path));
        }
    }
    Ok(())
}

fn list(path: &Path, long: bool) -> Result<()> {
    let archive = Archive::open(path).with_context(|| format!("opening {}", path.display()))?;
    let total: u64 = archive.entries().iter().map(|e| e.size).sum();
    println!(
        "vault: {} ({} files, {:.1} KiB)",
        path.display(),
        archive.len(),
        total as f64 / 1024.0
    );
    for entry in archive.entries() {
        if long {
            println!("  {:>10}  {:08x}  {}", entry.size, entry.crc, entry.path);
        } else {
            println!("  {}", entry.path);
        }
    }
    Ok(())
}

fn verify(path: &Path) -> Result<()> {
    let archive = Archive::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut bad = Vec::new();
    for entry in archive.entries() {
        // `read` checks the CRC itself, so a mismatch surfaces as an error.
        match archive.read(&entry.path) {
            Ok(Some(_)) => {}
            Ok(None) => bad.push(format!("{}: listed but not readable", entry.path)),
            Err(e) => bad.push(format!("{e}")),
        }
    }
    if bad.is_empty() {
        println!(
            "vault: {} -- all {} entries verified",
            path.display(),
            archive.len()
        );
        Ok(())
    } else {
        for b in &bad {
            println!("  {b}");
        }
        bail!(
            "{} of {} entries failed verification",
            bad.len(),
            archive.len()
        )
    }
}

fn unpack(path: &Path, out: &Path) -> Result<()> {
    let archive = Archive::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut written = 0usize;
    for entry in archive.entries() {
        let Some(data) = archive.read(&entry.path)? else {
            continue;
        };
        // `Archive::open` refuses any entry that is not a normalised virtual
        // path, so nothing here is absolute or climbs; the check is repeated
        // because this is the one place a bad name would touch the disk.
        let relative = Path::new(&entry.path);
        if relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            anyhow::bail!(
                "refusing to unpack {:?}: not a path inside the tree",
                entry.path
            );
        }
        let target = out.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&target, &data).with_context(|| format!("writing {}", target.display()))?;
        written += 1;
    }
    println!("vault: extracted {written} files to {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_takes_files_and_directories_and_ignores_comments() {
        let list = parse_list("# base\nui/\n  maps/Start.kerobsp  # the demo\n\n");
        assert_eq!(list, ["ui/", "maps/start.kerobsp"]);
        assert!(listed("ui/", "ui/menus/pause.keroui"));
        assert!(!listed("ui/", "uix/hud.keroui"));
        assert!(listed("maps/start.kerobsp", "maps/start.kerobsp"));
        assert!(!listed("maps/start.kerobsp", "maps/start.kerobsp.bak"));
    }

    #[test]
    fn a_list_entry_that_matches_nothing_is_an_error() {
        let dir = std::env::temp_dir().join(format!("vault-list-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(dir.join("ui/hud.keroui"), "x").unwrap();
        std::fs::write(dir.join("other.keroui"), "y").unwrap();
        let out = dir.join("out.vault");
        let list = parse_list("ui/\n");
        pack(&dir, &out, &[], &[], Some(&list)).unwrap();
        let archive = kerosene_vfs::Archive::open(&out).unwrap();
        assert_eq!(archive.len(), 1, "only what the list names");

        let list = parse_list("ui/\nmaps/gone.kerobsp\n");
        let err = pack(&dir, &out, &[], &[], Some(&list)).unwrap_err();
        assert!(err.to_string().contains("maps/gone.kerobsp"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
