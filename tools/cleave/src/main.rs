//! Cleave -- the VoidEngine BSP compiler.
//!
//! Takes a `.vmap` and produces a `.vbsp` the engine can load, plus a `.prt`
//! portal graph for Umbra and, when the world is not sealed, a `.lin` leak
//! trace Chisel can draw.
//!
//! This is the first of the three compile stages, mirroring Source's
//! vbsp/vvis/vrad split:
//!
//! ```text
//! cleave map.vmap     ->  map.vbsp + map.prt
//! umbra  map.vbsp     ->  map.vbsp with visibility
//! radiance map.vbsp   ->  map.vbsp with lighting
//! ```

use anyhow::{Context, Result};
use cleave::pipeline;
use clap::Parser;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "cleave", version, about = "Compile a .vmap into a .vbsp")]
struct Args {
    /// The .vmap file to compile.
    map: PathBuf,

    /// Where to write the .vbsp. Defaults to the input path with the extension changed.
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
    let map = void_map::Map::parse(&text)
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

    if let Some(leak) = &output.leak {
        println!("  LEAK: the world is not sealed (traced from {:?})", leak.from);
    }

    if args.dry_run {
        println!("  dry run: nothing written ({:.2}s)", started.elapsed().as_secs_f32());
        return Ok(());
    }

    let out_path = args.output.unwrap_or_else(|| args.map.with_extension("vbsp"));
    let size = void_bsp::write_bsp(&output.bsp, &out_path)
        .with_context(|| format!("writing {}", out_path.display()))?;

    let prt_path = out_path.with_extension("prt");
    std::fs::write(&prt_path, &output.prt)
        .with_context(|| format!("writing {}", prt_path.display()))?;

    if let Some(leak) = &output.leak {
        let lin_path = out_path.with_extension("lin");
        std::fs::write(&lin_path, leak.to_lin())?;
        println!("  wrote {} (load it in Chisel to see the leak)", lin_path.display());
    }

    println!("  wrote {} ({:.1} KiB) and {} in {:.2}s",
        out_path.display(),
        size as f64 / 1024.0,
        prt_path.display(),
        started.elapsed().as_secs_f32());
    Ok(())
}
