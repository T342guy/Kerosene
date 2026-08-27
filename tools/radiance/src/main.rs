//! Radiance -- the VoidEngine lighting compiler.
//!
//! Reads a compiled `.voidbsp`, bakes static lighting into every face, and writes
//! the result back. This is the third and last compile stage, mirroring
//! Source's `vrad`:
//!
//! ```text
//! cleave   map.voidmap   ->  map.voidbsp + map.voidprt
//! umbra    map.voidbsp   ->  map.voidbsp with visibility
//! radiance map.voidbsp   ->  map.voidbsp with lighting        <- you are here
//! ```
//!
//! Lighting is authored as entities in the map -- `light`, `light_spot`,
//! `light_environment` -- so a designer changes it in the editor rather than
//! in a separate file.

mod bake;
mod lights;

use anyhow::{Context, Result};
use bake::BakeOptions;
use clap::Parser;
use lights::LightSet;
use std::path::PathBuf;
use std::time::Instant;
use void_bsp::Bsp;

#[derive(Parser, Debug)]
#[command(name = "radiance", version, about = "Bake static lighting into a compiled .voidbsp")]
struct Args {
    /// The .voidbsp to light, modified in place.
    map: PathBuf,

    /// Samples per luxel per axis. Higher softens shadow edges at a
    /// quadratic cost.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=8))]
    samples: u32,

    /// How many times light bounces off surfaces. 0 is direct light only.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(0..=8))]
    bounces: u32,

    /// Overall exposure multiplier.
    #[arg(long, default_value_t = 1.0)]
    scale: f32,

    /// Multiplier on the ambient term alone.
    #[arg(long, default_value_t = 1.0)]
    ambient_scale: f32,

    /// Fast preview: one sample per luxel, no bounces.
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

    let lights = LightSet::from_bsp(&bsp);
    println!("radiance: {} ({} faces, {} lights)",
        args.map.display(), bsp.faces.len(), lights.lights.len());

    if lights.is_empty() {
        // Worth saying plainly: a map with no lights compiles fine and is
        // then pitch black, which looks like a broken renderer.
        println!("  warning: this map has no light entities, so it will render black.");
        println!("  Add a 'light' entity, or a 'light_environment' for sun and sky.");
    }
    if lights.has_sun {
        let c = lights.sky_color;
        println!("  sky emits {:.0} {:.0} {:.0}", c.x, c.y, c.z);
    } else {
        println!("  note: no light_environment, so sky surfaces emit nothing");
    }

    let options = if args.fast {
        BakeOptions { supersample: 1, bounces: 0, scale: args.scale, ambient_scale: args.ambient_scale }
    } else {
        BakeOptions {
            supersample: args.samples,
            bounces: args.bounces,
            scale: args.scale,
            ambient_scale: args.ambient_scale,
        }
    };

    let stats = bake::bake(&mut bsp, &lights, &options);

    println!("  {} faces lit, {} unlit (sky, nodraw or tool surfaces)",
        stats.faces_lit, stats.faces_unlit);
    println!("  {} luxels at {}x supersampling", stats.luxels, options.supersample);
    if options.bounces > 0 {
        println!("  {} bounce patches over {} bounce(s)", stats.bounce_patches, options.bounces);
    }
    println!("  lighting lump {:.1} KiB", (stats.luxels * 4) as f64 / 1024.0);

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
