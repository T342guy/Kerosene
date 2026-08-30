// SPDX-License-Identifier: LGPL-3.0-or-later
//! `timbre` -- the Kerosene sound compiler.
//!
//! ```text
//! timbre                                  # open the window
//! timbre build                            # compile a project's sounds
//! timbre build --content path/to/content --force
//! timbre compile sound/door/move.wav --gain 0.8 --mono
//! timbre compile sound/music/theme.flac --encoding pcm16
//! timbre info sound/door/move.keroaud
//! ```
//!
//! Run with no arguments it opens a window, because the useful things to know
//! about a sound -- what it looks like, what it peaks at, whether the gain you
//! chose clips it -- are things to see and hear rather than to read. Every one
//! of them is also available as a flag, because a build server has no screen.

mod gui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use timbre::Options;
use kerosene_audio::compiled::{self, Encoding};

#[derive(Parser, Debug)]
#[command(name = "timbre", version, about = "Compile sounds into .keroaud")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile every sound in a project.
    Build {
        /// The content tree. With none, the project is found the way every
        /// other tool finds it.
        #[arg(long)]
        content: Option<PathBuf>,
        /// Rebuild everything, not only what changed.
        #[arg(long)]
        force: bool,
    },
    /// Compile one file: `.wav`, `.flac` or `.mp3`.
    Compile {
        source: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// `adpcm` for a quarter the size, `pcm16` to keep every bit.
        #[arg(long, default_value = "adpcm")]
        encoding: String,
        /// Multiplied into every sample before encoding.
        #[arg(long, default_value_t = 1.0)]
        gain: f32,
        /// Fold stereo to mono, so it can be placed in the world.
        #[arg(long)]
        mono: bool,
    },
    /// Say what a compiled sound holds.
    Info { file: PathBuf },
    /// Open the window, which is also what happens with no arguments.
    Edit {
        #[arg(long)]
        content: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    match Args::parse().command {
        None => edit(None),
        Some(Command::Edit { content }) => edit(content),
        Some(Command::Build { content, force }) => build(content, force),
        Some(Command::Compile { source, output, encoding, gain, mono }) => {
            let encoding = Encoding::parse(&encoding)
                .ok_or_else(|| anyhow::anyhow!("unknown encoding {encoding:?}; try adpcm or pcm16"))?;
            if !(gain > 0.0 && gain.is_finite()) {
                bail!("gain must be positive");
            }
            let output = output.unwrap_or_else(|| timbre::output_for(&source));
            let options = Options { encoding, gain, mono, looping: None };
            let done = timbre::compile(&source, &output, &options)?;
            println!("{done}");
            for warning in &done.warnings {
                println!("  note: {warning}");
            }
            Ok(())
        }
        Some(Command::Info { file }) => {
            let bytes = std::fs::read(&file)?;
            let info = compiled::read_info(&bytes)?;
            println!("{}", file.display());
            println!("  {:.3}s, {} Hz, {} channel(s)", info.duration(), info.sample_rate, info.channels);
            println!("  {} frames, {} encoding", info.frames, info.encoding.name());
            println!("  peaks at {:.3} of full scale", info.peak);
            if info.looping.is_empty() {
                println!("  does not loop");
            } else {
                println!("  loops {}..{}", info.looping.start, info.looping.end);
            }
            println!(
                "  {} in the world",
                if info.can_be_positioned() { "can be placed" } else { "cannot be placed (stereo)" }
            );
            Ok(())
        }
    }
}

fn content_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let found = kerosene_vfs::root::find(explicit.as_deref(), None);
    println!("{}", kerosene_vfs::root::describe(&found));
    match found {
        Some(found) => Ok(found.root),
        None => bail!("no content tree found. Run timbre from a project, or pass --content"),
    }
}

fn build(content: Option<PathBuf>, force: bool) -> Result<()> {
    let root = content_root(content)?;
    let batch = timbre::build_sounds(&root, force)?;

    for done in &batch.compiled {
        println!("{done}");
        for warning in &done.warnings {
            println!("  note: {warning}");
        }
    }
    // Not prefixed with the path: every error from a compile already names the
    // file it is about, and saying it twice reads as two problems.
    for (_, error) in &batch.failed {
        eprintln!("error: {error}");
    }
    println!();
    println!("{batch}");

    if !batch.failed.is_empty() {
        bail!("{} sound(s) failed to compile", batch.failed.len());
    }
    Ok(())
}

fn edit(content: Option<PathBuf>) -> Result<()> {
    let root = content_root(content)?;
    let app = gui::Timbre::open(&root)?;
    kerosene_ui::run("Timbre -- Kerosene sound compiler", (1180, 760), app)
}
