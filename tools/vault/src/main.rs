// SPDX-License-Identifier: LGPL-3.0-or-later
//! Vault -- the VoidEngine content archive tool.
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
//! ```text
//! vault pack content -o content.vault
//! vault list content.vault
//! vault verify content.vault
//! vault unpack content.vault -o extracted
//! ```

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use void_vfs::{Archive, ArchiveBuilder};

#[derive(Parser, Debug)]
#[command(name = "vault", version, about = "Pack and inspect VoidEngine content archives")]
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

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    match Args::parse().command {
        Command::Pack { directory, output, extensions, excludes } => {
            pack(&directory, &output, &extensions, &excludes)
        }
        Command::List { archive, long } => list(&archive, long),
        Command::Verify { archive } => verify(&archive),
        Command::Unpack { archive, output } => unpack(&archive, &output),
    }
}

fn pack(dir: &Path, out: &Path, extensions: &[String], excludes: &[String]) -> Result<()> {
    if !dir.is_dir() { bail!("{} is not a directory", dir.display()); }

    let wanted: Vec<String> = extensions.iter().map(|e| e.trim_start_matches('.').to_lowercase()).collect();
    let mut builder = ArchiveBuilder::new();
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();

    let mut total = 0u64;
    let mut skipped = 0usize;
    for (disk, virtual_path) in files {
        if !wanted.is_empty() {
            let ext = virtual_path.rsplit('.').next().unwrap_or("").to_lowercase();
            if !wanted.contains(&ext) { skipped += 1; continue; }
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

    if builder.is_empty() {
        bail!("nothing to pack from {} (check --ext and --exclude)", dir.display());
    }

    let size = builder.write(out).with_context(|| format!("writing {}", out.display()))?;
    println!("vault: packed {} files ({:.1} KiB of content) into {}",
        builder.len(), total as f64 / 1024.0, out.display());
    if skipped > 0 { println!("  {skipped} files skipped by filters"); }
    println!("  archive is {:.1} KiB", size as f64 / 1024.0);
    Ok(())
}

/// Walk a directory, pairing each file with the virtual path it will hold.
fn collect(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else if path.is_file() {
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
    println!("vault: {} ({} files, {:.1} KiB)", path.display(), archive.len(), total as f64 / 1024.0);
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
        println!("vault: {} -- all {} entries verified", path.display(), archive.len());
        Ok(())
    } else {
        for b in &bad { println!("  {b}"); }
        bail!("{} of {} entries failed verification", bad.len(), archive.len())
    }
}

fn unpack(path: &Path, out: &Path) -> Result<()> {
    let archive = Archive::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut written = 0usize;
    for entry in archive.entries() {
        let Some(data) = archive.read(&entry.path)? else { continue };
        // Entry paths were normalised when packed and cannot contain `..`, so
        // joining them onto the output directory is safe.
        let target = out.join(&entry.path);
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
