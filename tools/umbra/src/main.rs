//! Umbra -- the VoidEngine visibility compiler.
//!
//! Reads a compiled `.vbsp` and the `.prt` portal graph Cleave wrote beside
//! it, works out which clusters can see which, and writes the result back into
//! the map's visibility lump.
//!
//! This is the second of the three compile stages, mirroring Source's `vvis`:
//!
//! ```text
//! cleave   map.vmap   ->  map.vbsp + map.prt
//! umbra    map.vbsp   ->  map.vbsp with visibility     <- you are here
//! radiance map.vbsp   ->  map.vbsp with lighting
//! ```
//!
//! Vis is the slowest stage of any BSP compile and the one that matters most
//! for framerate. `--fast` skips the expensive pass and leaves an
//! over-estimate, which is what you want while a level's layout is still
//! moving.

mod bitset;
mod flow;
mod prt;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::time::Instant;
use void_bsp::{Bsp, VisBuilder};

#[derive(Parser, Debug)]
#[command(name = "umbra", version, about = "Compute the PVS for a compiled .vbsp")]
struct Args {
    /// The .vbsp to add visibility to, modified in place.
    map: PathBuf,

    /// The portal file. Defaults to the map path with a .prt extension.
    #[arg(long)]
    portals: Option<PathBuf>,

    /// Stop after the base estimate. Much faster, and leaves far too much
    /// visible -- for iterating, not for shipping.
    #[arg(long)]
    fast: bool,

    /// Report what would happen without writing anything.
    #[arg(long)]
    dry_run: bool,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let args = Args::parse();
    let started = Instant::now();

    let mut bsp = Bsp::load(&args.map)
        .with_context(|| format!("loading {}", args.map.display()))?;

    let prt_path = args.portals.unwrap_or_else(|| args.map.with_extension("prt"));
    let prt_text = std::fs::read_to_string(&prt_path).with_context(|| {
        format!(
            "reading {}. Umbra needs the portal file Cleave writes next to the map.",
            prt_path.display()
        )
    })?;

    let graph = prt::PortalGraph::parse(&prt_text)
        .with_context(|| format!("parsing {}", prt_path.display()))?;

    println!("umbra: {} ({} clusters, {} portals)",
        args.map.display(), graph.clusters, graph.portal_count() / 2);

    if graph.clusters == 0 {
        println!("  nothing to do: the map has no visibility clusters");
        return Ok(());
    }

    let result = flow::compute(&graph, args.fast);

    // The headline number: how much of the map an average room can see. Lower
    // is better, and it is the single best predictor of framerate.
    let average = result.final_visible as f64 / graph.clusters as f64;
    let base_average = result.base_visible as f64 / graph.clusters as f64;
    println!("  base  {base_average:.1} clusters visible per cluster");
    if args.fast {
        println!("  fast vis: keeping the base estimate");
    } else {
        let saved = 100.0 * (1.0 - result.final_visible as f64 / result.base_visible.max(1) as f64);
        println!("  full  {average:.1} clusters visible per cluster ({saved:.0}% culled)");
    }

    let mut builder = VisBuilder::new(graph.clusters);
    for from in 0..graph.clusters {
        for to in result.cluster_vis[from].iter_set() {
            builder.set_visible(from, to);
        }
    }
    // Sound carries one room further than sight does.
    builder.derive_pas();
    bsp.visibility = builder.build();

    println!("  vis lump {} bytes", bsp.visibility.len());

    if args.dry_run {
        println!("  dry run: nothing written ({:.2}s)", started.elapsed().as_secs_f32());
        return Ok(());
    }

    let size = bsp.save(&args.map)
        .with_context(|| format!("writing {}", args.map.display()))?;
    println!("  wrote {} ({:.1} KiB) in {:.2}s",
        args.map.display(), size as f64 / 1024.0, started.elapsed().as_secs_f32());
    Ok(())
}
