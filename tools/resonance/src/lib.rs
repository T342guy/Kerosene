// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Resonance -- the Kerosene acoustics compiler.
//!
//! Reads a compiled `.kerobsp`, listens to every leaf by throwing rays at
//! its walls, and writes back what each part of the map sounds like: how
//! long sound lingers in each band, how soon the first echo returns, how
//! open to the sky it is. The engine's reverb reads those numbers straight
//! off the map, so an empty concrete hall rings and a carpeted office does
//! not, without a designer placing anything.
//!
//! ```text
//! cleave     map.keromap   ->  map.kerobsp + map.keroprt
//! umbra      map.kerobsp   ->  map.kerobsp with visibility
//! resonance  map.kerobsp   ->  map.kerobsp with acoustics   <- you are here
//! radiance   map.kerobsp   ->  map.kerobsp with lighting
//! ```
//!
//! It runs after Umbra because it reads the portal file to know which leaves
//! touch, and independently of Radiance: light and sound share nothing but
//! the walls. It reads the map's materials the way the engine does, so a
//! `$surfaceprop` of carpet deadens a room and `$acoustics` on a material
//! overrides that outright.
//!
//! This is a library that the unified toolset invokes as the `resonance`
//! subcommand.

pub mod materials;
pub mod overrides;
pub mod probe;
pub mod rng;
pub mod rooms;

use anyhow::{Context, Result};
use clap::Parser;
use kerosene_bsp::Bsp;
use kerosene_bsp::acoustics::{Acoustics, room_flags};
use rayon::prelude::*;
use std::path::PathBuf;
use std::time::Instant;
use umbra::prt::PortalGraph;

pub use materials::Absorption;
pub use probe::{LeafAcoustics, Options, Skipped};
pub use rooms::{Adjacency, Tolerance};

#[derive(Parser, Debug)]
#[command(
    name = "resonance",
    version,
    about = "Work out what each part of a compiled .kerobsp sounds like"
)]
struct Args {
    /// The .kerobsp to add acoustics to, modified in place.
    map: PathBuf,

    /// The portal file. Defaults to the map path with a .keroprt extension;
    /// without one, leaves are joined by touching bounds instead.
    #[arg(long)]
    portals: Option<PathBuf>,

    /// The content tree holding the map's materials. Found from the map's
    /// project when not given.
    #[arg(long)]
    content: Option<PathBuf>,

    /// Fewer rays per leaf. Rougher, several times faster; for iterating.
    #[arg(long, conflicts_with = "extra")]
    fast: bool,

    /// Four times the rays. For a final build of a map that matters.
    #[arg(long)]
    extra: bool,

    /// List every room after the summary.
    #[arg(long)]
    rooms: bool,

    /// Report what would happen without writing anything.
    #[arg(long)]
    dry_run: bool,
}

/// What a run found, for the report.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub leaves: usize,
    pub probed: usize,
    pub inherited: usize,
    pub solid: usize,
    pub unplaced: usize,
    pub rooms: usize,
    pub outdoor_rooms: usize,
    pub water_rooms: usize,
    /// Rooms a designer's `env_acoustic_override` changed.
    pub overridden_rooms: usize,
    /// Median mid-band decay across rooms, weighted by nothing.
    pub rt60_min: f32,
    pub rt60_median: f32,
    pub rt60_max: f32,
}

/// Run the whole thing on a map in memory: probe, inherit, cluster.
pub fn compute(
    bsp: &Bsp,
    graph: Option<&PortalGraph>,
    absorption: &Absorption,
    options: &Options,
) -> (Acoustics, Report) {
    let bounds = probe::leaf_bounds(bsp);
    let progress = Progress::new("probing", bsp.leaves.len());
    let mut leaves: Vec<Result<LeafAcoustics, Skipped>> = (0..bsp.leaves.len())
        .into_par_iter()
        .map(|leaf| {
            let result = probe::probe_leaf(bsp, leaf, absorption, options);
            progress.tick();
            result
        })
        .collect();

    let usable: Vec<bool> = leaves.iter().map(|l| *l != Err(Skipped::Solid)).collect();
    let adjacency = match graph {
        Some(graph) => Adjacency::from_portals(bsp, graph),
        None => Adjacency::from_bounds(&bounds, &usable),
    };
    let inherited = rooms::inherit(&mut leaves, &adjacency);
    let mut acoustics = rooms::cluster(&leaves, &bounds, &adjacency, Tolerance::DEFAULT);
    let overridden = overrides::apply(bsp, &mut acoustics, &bounds, &overrides::Override::all(bsp));

    let mut report = Report {
        leaves: bsp.leaves.len(),
        probed: leaves
            .iter()
            .filter(|l| l.is_ok_and(|a| !a.inherited))
            .count(),
        inherited,
        solid: leaves.iter().filter(|l| **l == Err(Skipped::Solid)).count(),
        unplaced: leaves
            .iter()
            .filter(|l| matches!(l, Err(Skipped::Tiny) | Err(Skipped::NoPoint)))
            .count(),
        rooms: acoustics.rooms.len(),
        outdoor_rooms: acoustics
            .rooms
            .iter()
            .filter(|r| r.has(room_flags::OUTDOOR))
            .count(),
        water_rooms: acoustics
            .rooms
            .iter()
            .filter(|r| r.has(room_flags::WATER))
            .count(),
        overridden_rooms: overridden,
        ..Default::default()
    };
    let mut mids: Vec<f32> = acoustics.rooms.iter().map(|r| r.rt60[1]).collect();
    mids.sort_by(f32::total_cmp);
    if !mids.is_empty() {
        report.rt60_min = mids[0];
        report.rt60_median = mids[mids.len() / 2];
        report.rt60_max = mids[mids.len() - 1];
    }
    (acoustics, report)
}

/// Entry point for the `resonance` subcommand of the unified toolset.
pub fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(std::iter::once("resonance".to_string()).chain(args));
    let started = Instant::now();

    let mut bsp =
        Bsp::load(&args.map).with_context(|| format!("loading {}", args.map.display()))?;
    println!(
        "resonance: {} ({} leaves)",
        args.map.display(),
        bsp.leaves.len()
    );
    if bsp.visibility.is_empty() {
        println!("  warning: the map has no visibility; run umbra first so sound can be occluded");
    }

    let prt_path = args
        .portals
        .clone()
        .unwrap_or_else(|| args.map.with_extension("keroprt"));
    let graph = match std::fs::read_to_string(&prt_path) {
        Ok(text) => Some(
            PortalGraph::parse(&text).with_context(|| format!("parsing {}", prt_path.display()))?,
        ),
        Err(_) => {
            println!(
                "  warning: no portal file at {}; joining leaves by their bounds instead",
                prt_path.display()
            );
            None
        }
    };

    let content = kerosene_vfs::root::find(args.content.as_deref(), Some(&args.map));
    let absorption = match &content {
        Some(found) => {
            println!("  materials from {} ({})", found.root.display(), found.why);
            Absorption::from_content(&bsp, &found.root)
        }
        None => {
            println!(
                "  warning: no content tree found for {}; every surface will absorb like the \
                 default material (pass --content, or put the map in a project)",
                args.map.display()
            );
            Absorption::uniform(
                &bsp,
                kerosene_asset::AcousticProfile::of(&kerosene_asset::SurfaceProperty::Default),
            )
        }
    };
    if !absorption.missing.is_empty() {
        let shown: Vec<&str> = absorption
            .missing
            .iter()
            .take(8)
            .map(String::as_str)
            .collect();
        println!(
            "  warning: {} material(s) could not be read and absorb like the default: {}{}",
            absorption.missing.len(),
            shown.join(", "),
            if absorption.missing.len() > shown.len() {
                ", ..."
            } else {
                ""
            }
        );
    }

    let options = if args.fast {
        Options::FAST
    } else if args.extra {
        Options::EXTRA
    } else {
        Options::DEFAULT
    };
    println!(
        "  {} rays per probe, up to {} bounces",
        options.rays, options.max_bounces
    );

    let (acoustics, report) = compute(&bsp, graph.as_ref(), &absorption, &options);
    println!(
        "  {} leaves: {} probed, {} inherited, {} solid, {} unplaced",
        report.leaves, report.probed, report.inherited, report.solid, report.unplaced
    );
    println!(
        "  {} rooms ({} outdoor, {} under water, {} overridden); 500 Hz decay {:.2}s / {:.2}s / {:.2}s min/median/max",
        report.rooms,
        report.outdoor_rooms,
        report.water_rooms,
        report.overridden_rooms,
        report.rt60_min,
        report.rt60_median,
        report.rt60_max
    );
    if args.rooms {
        print_rooms(&acoustics);
    }

    bsp.acoustics = Some(acoustics);
    if args.dry_run {
        println!(
            "  dry run: nothing written ({:.2}s)",
            started.elapsed().as_secs_f32()
        );
        return Ok(());
    }
    let size = bsp
        .save(&args.map)
        .with_context(|| format!("writing {}", args.map.display()))?;
    println!(
        "  wrote {} ({:.1} KiB) in {:.2}s",
        args.map.display(),
        size as f64 / 1024.0,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

fn print_rooms(acoustics: &Acoustics) {
    println!(
        "  {:>5} {:>6} {:>7} {:>7} {:>7} {:>7} {:>6} {:>5} {:>5} {:>6}  flags",
        "room", "leaves", "rt125", "rt500", "rt2k", "rt8k", "path", "pre", "open", "wet"
    );
    for (i, r) in acoustics.rooms.iter().enumerate() {
        let mut flags = String::new();
        for (bit, name) in [
            (room_flags::WATER, "water"),
            (room_flags::OUTDOOR, "outdoor"),
            (room_flags::OVERRIDE, "override"),
            (room_flags::INHERITED, "inherited"),
        ] {
            if r.has(bit) {
                if !flags.is_empty() {
                    flags.push(' ');
                }
                flags.push_str(name);
            }
        }
        println!(
            "  {i:>5} {:>6} {:>7.2} {:>7.2} {:>7.2} {:>7.2} {:>6.0} {:>5.0} {:>5.2} {:>6.2}  {flags}",
            r.leaf_count,
            r.rt60[0],
            r.rt60[1],
            r.rt60[2],
            r.rt60[3],
            r.mean_free_path,
            r.predelay * 1000.0,
            r.openness,
            r.wet,
        );
    }
}

/// A line every tenth of the way through the probe, from whichever thread
/// crosses the mark, so a big map does not look hung.
struct Progress {
    label: String,
    total: usize,
    done: std::sync::atomic::AtomicUsize,
    started: Instant,
}

impl Progress {
    fn new(label: &str, total: usize) -> Progress {
        Progress {
            label: label.to_string(),
            total,
            done: std::sync::atomic::AtomicUsize::new(0),
            started: Instant::now(),
        }
    }

    fn tick(&self) {
        let done = self.done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let step = (self.total / 10).max(1);
        if self.total >= 500 && done.is_multiple_of(step) && done < self.total {
            println!(
                "  {:<10} {}% ({done}/{} leaves, {:.0}s)",
                self.label,
                done * 100 / self.total,
                self.total,
                self.started.elapsed().as_secs_f32()
            );
        }
    }
}

#[cfg(test)]
mod tests;
