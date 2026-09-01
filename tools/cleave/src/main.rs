// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Cleave -- the Kerosene BSP compiler.
//!
//! Takes a `.keromap` and produces a `.kerobsp` the engine can load, plus a `.keroprt`
//! portal graph for Umbra and, when the world is not sealed, a `.keroleak` leak
//! trace Chisel can draw.
//!
//! This is the first of the three compile stages, mirroring Source's
//! vbsp/vvis/vrad split:
//!
//! ```text
//! cleave map.keromap     ->  map.kerobsp + map.keroprt
//! umbra  map.kerobsp     ->  map.kerobsp with visibility
//! radiance map.kerobsp   ->  map.kerobsp with lighting
//! ```

use anyhow::{Context, Result};
use cleave::pipeline;
use clap::Parser;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "cleave", version, about = "Compile a .keromap into a .kerobsp")]
struct Args {
    /// The .keromap file to compile.
    map: PathBuf,

    /// Where to write the .kerobsp. Defaults to the input path with the extension changed.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Build the map even if it leaks.
    #[arg(long)]
    ignore_leaks: bool,

    /// Keep the space outside the map instead of filling it in. For debugging.
    #[arg(long)]
    no_fill: bool,

    /// Report what would happen without writing anything.
    #[arg(long)]
    dry_run: bool,

    /// Print per-stage detail.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let args = Args::parse();
    let started = Instant::now();

    let text = std::fs::read_to_string(&args.map)
        .with_context(|| format!("reading {}", args.map.display()))?;
    let map = kerosene_map::Map::parse(&text)
        .with_context(|| format!("parsing {}", args.map.display()))?;

    println!("cleave: {} ({} brushes, {} entities)",
        args.map.display(), map.solid_count(), map.entities.len());

    // Structural problems are worth reporting all at once: a designer would
    // rather fix five brushes in one pass than five compiles.
    let problems = map.validate();
    if !problems.is_empty() {
        for p in &problems { println!("  error: {p}"); }
        anyhow::bail!("{} brush or entity problems must be fixed first", problems.len());
    }
    for lint in pipeline::lint_materials(&map) {
        println!("  warning: {lint}");
    }

    let options = pipeline::CompileOptions {
        ignore_leaks: args.ignore_leaks,
        no_fill: args.no_fill,
        verbose: args.verbose,
    };

    let output = match pipeline::compile(&map, &options) {
        Ok(o) => o,
        Err(e) => {
            // A leak is the one failure worth extra help: write the trace out
            // even though the compile failed, so it can be loaded and looked at.
            if let pipeline::CompileError::Leaked(_) = &e {
                println!("  {e}");
                println!("  Run again with --ignore-leaks to write the leak trace.");
            }
            return Err(e.into());
        }
    };

    for w in &output.warnings {
        if w.brush_id != 0 { println!("  warning: brush {}: {}", w.brush_id, w.message); }
        else { println!("  warning: {}", w.message); }
    }

    let s = &output.stats;
    println!("  csg      {} source brushes, {} hidden faces removed", s.source_brushes, s.faces_removed_by_csg);
    println!("  tree     {} nodes, {} leaves, depth {}, {} brush splits",
        s.tree_nodes, s.tree_leaves, s.tree_depth, s.brush_splits);
    println!("  portals  {} portals ({} too small to keep)", s.portals, s.tiny_portals);
    println!("  fill     {} leaves outside the world removed, {} clusters", s.leaves_filled, s.clusters);
    println!("  output   {} faces, {} vertices", s.faces, s.vertices);
    println!("  walkmap  {} walkable faces", output.walk.len());

    if let Some(leak) = &output.leak {
        println!("  LEAK: the world is not sealed (traced from {:?})", leak.from);
    }

    if args.dry_run {
        println!("  dry run: nothing written ({:.2}s)", started.elapsed().as_secs_f32());
        return Ok(());
    }

    let out_path = args.output.unwrap_or_else(|| args.map.with_extension("kerobsp"));
    let size = kerosene_bsp::write_bsp(&output.bsp, &out_path)
        .with_context(|| format!("writing {}", out_path.display()))?;

    let prt_path = out_path.with_extension("keroprt");
    std::fs::write(&prt_path, &output.prt)
        .with_context(|| format!("writing {}", prt_path.display()))?;

    let walk_path = out_path.with_extension("kerowalk");
    output.walk.write(&walk_path)
        .with_context(|| format!("writing {}", walk_path.display()))?;

    // The trace describes *this* compile. A sealed map must clear the one
    // left by an earlier broken build, or every later compile looks like it
    // leaked -- the editor loads the file, not the result, and has no way to
    // tell a stale trace from a fresh one.
    let leak_path = out_path.with_extension("keroleak");
    match &output.leak {
        Some(leak) => {
            std::fs::write(&leak_path, leak.to_lin())?;
            println!("  wrote {} (load it in Chisel to see the leak)", leak_path.display());
        }
        None => {
            if leak_path.exists() {
                std::fs::remove_file(&leak_path).with_context(|| {
                    format!("removing the stale leak trace {}", leak_path.display())
                })?;
                println!("  the map is sealed; removed the old {}", leak_path.display());
            }
        }
    }

    println!("  wrote {} ({:.1} KiB), {} and {} in {:.2}s",
        out_path.display(),
        size as f64 / 1024.0,
        prt_path.display(),
        walk_path.display(),
        started.elapsed().as_secs_f32());
    Ok(())
}
