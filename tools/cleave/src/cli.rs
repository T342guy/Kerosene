// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The command-line surface of Cleave, exposed as a `run` the unified
//! toolset calls for the `cleave` subcommand.
//!
//! ```text
//! kerosene-tools cleave map.keromap [-o out.kerobsp] [--content DIR] [--ignore-leaks] [--no-fill] [--dry-run] [-v]
//! ```

use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::pipeline;

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

    /// The content tree whose compiled textures size the map's faces.
    /// Found from the map's project file when not given.
    #[arg(long)]
    content: Option<PathBuf>,

    /// Compile only what is inside this box, sealed by its own walls:
    /// "minx miny minz maxx maxy maxz". The map's own cordon is used when
    /// it is active and this is not given.
    #[arg(long, allow_hyphen_values = true)]
    cordon: Option<String>,

    /// Ignore the cordon the map carries.
    #[arg(long)]
    no_cordon: bool,
}

/// Parse `"minx miny minz maxx maxy maxz"`.
fn parse_cordon(text: &str) -> Result<kerosene_math::Aabb> {
    let nums: Vec<f32> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f32>())
        .collect::<std::result::Result<_, _>>()
        .with_context(|| format!("--cordon {text:?}: six numbers are needed"))?;
    if nums.len() != 6 {
        anyhow::bail!(
            "--cordon {text:?}: six numbers are needed, got {}",
            nums.len()
        );
    }
    let a = kerosene_math::Vec3::new(nums[0], nums[1], nums[2]);
    let b = kerosene_math::Vec3::new(nums[3], nums[4], nums[5]);
    Ok(kerosene_math::Aabb::new(a.min(b), a.max(b)))
}

/// The pixel size of every material's base texture, read from the compiled
/// content the way the engine will read it.
///
/// Through the VFS rather than the source PNG, for the reason Chisel gives:
/// the engine draws the `.kerotex`, and a size read from anything else is a
/// guess about what Alchemy did. A material with no compiled texture behind
/// it is left out, and the pipeline warns about it by name.
fn texture_sizes(
    map: &kerosene_map::Map,
    content: &std::path::Path,
) -> HashMap<String, (u32, u32)> {
    let mut vfs = kerosene_vfs::Vfs::new();
    vfs.add_directory(content, "GAME");
    for archive in std::fs::read_dir(content)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "vault"))
    {
        let _ = vfs.mount_archive(&archive, "GAME");
    }

    let mut sizes = HashMap::new();
    for (_, solid) in map.all_solids() {
        for side in &solid.sides {
            let key = side.material.to_ascii_lowercase();
            if sizes.contains_key(&key) {
                continue;
            }
            let Some(size) = texture_size(&vfs, &side.material) else {
                continue;
            };
            sizes.insert(key, size);
        }
    }
    sizes
}

fn texture_size(vfs: &kerosene_vfs::Vfs, material: &str) -> Option<(u32, u32)> {
    let text = vfs
        .read_string(&kerosene_asset::material_path(material))
        .ok()?;
    let parsed = kerosene_asset::Material::parse(&text).ok()?;
    // The same fallback the renderer applies: a material with no
    // `$basetexture` draws the texture of its own name.
    let base = parsed.base_texture().unwrap_or(material);
    let bytes = vfs.read(&kerosene_asset::texture_path(base)).ok()?;
    let texture = kerosene_asset::Texture::from_bytes(&bytes).ok()?;
    Some((texture.width(), texture.height()))
}

/// Entry point for the `cleave` subcommand of the unified toolset.
pub fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(std::iter::once("cleave".to_string()).chain(args));
    let started = Instant::now();

    let text = std::fs::read_to_string(&args.map)
        .with_context(|| format!("reading {}", args.map.display()))?;
    let map = kerosene_map::Map::parse(&text)
        .with_context(|| format!("parsing {}", args.map.display()))?;

    println!(
        "cleave: {} ({} brushes, {} entities)",
        args.map.display(),
        map.solid_count(),
        map.entities.len()
    );

    // Structural problems are worth reporting all at once: a designer would
    // rather fix five brushes in one pass than five compiles.
    let problems = map.validate();
    if !problems.is_empty() {
        for p in &problems {
            println!("  error: {p}");
        }
        anyhow::bail!(
            "{} brush or entity problems must be fixed first",
            problems.len()
        );
    }
    let content = kerosene_vfs::root::find(args.content.as_deref(), Some(&args.map));
    let sizes = match &content {
        Some(found) => texture_sizes(&map, &found.root),
        None => {
            println!(
                "  warning: no content tree found for {}; faces will be scaled as if every \
                 texture were {}x{} (pass --content, or put the map in a project)",
                args.map.display(),
                crate::emit::DEFAULT_TEXTURE_SIZE.0,
                crate::emit::DEFAULT_TEXTURE_SIZE.1
            );
            HashMap::new()
        }
    };

    let cordon = match &args.cordon {
        Some(text) => Some(parse_cordon(text)?),
        None if args.no_cordon => None,
        None => map.cordon.as_ref().filter(|c| c.active).map(|c| c.bounds),
    };
    if let Some(b) = cordon {
        println!(
            "  cordon: {} {} {} to {} {} {}",
            b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z
        );
    }
    let options = pipeline::CompileOptions {
        ignore_leaks: args.ignore_leaks,
        no_fill: args.no_fill,
        verbose: args.verbose,
        texture_sizes: sizes,
        cordon,
    };

    let out_path = args
        .output
        .clone()
        .unwrap_or_else(|| args.map.with_extension("kerobsp"));
    let leak_path = out_path.with_extension("keroleak");

    let output = match pipeline::compile(&map, &options) {
        Ok(o) => o,
        Err(pipeline::CompileError::Leaked(leak)) if !args.dry_run => {
            // A leak is the one failure worth extra help: the trace is written
            // even though the compile failed, so Chisel can draw the red line
            // and a person can follow it to the hole -- on a default compile,
            // not only after they knew to ask for --ignore-leaks.
            std::fs::write(&leak_path, leak.to_lin())
                .with_context(|| format!("writing {}", leak_path.display()))?;
            println!(
                "  LEAK: wrote {} (load it in Chisel to see the leak)",
                leak_path.display()
            );
            // Whatever an earlier, sealed build left behind is stale now: the
            // engine must not load a BSP that no longer matches the map.
            for stale in [
                &out_path,
                &out_path.with_extension("keroprt"),
                &out_path.with_extension("kerowalk"),
            ] {
                if stale.exists() {
                    let _ = std::fs::remove_file(stale);
                }
            }
            return Err(pipeline::CompileError::Leaked(leak).into());
        }
        Err(e) => return Err(e.into()),
    };

    for w in &output.warnings {
        if w.brush_id != 0 {
            println!("  warning: brush {}: {}", w.brush_id, w.message);
        } else {
            println!("  warning: {}", w.message);
        }
    }

    let s = &output.stats;
    println!(
        "  csg      {} source brushes, {} hidden faces removed",
        s.source_brushes, s.faces_removed_by_csg
    );
    println!(
        "  tree     {} nodes, {} leaves, depth {}, {} brush splits",
        s.tree_nodes, s.tree_leaves, s.tree_depth, s.brush_splits
    );
    println!(
        "  portals  {} portals ({} too small to keep)",
        s.portals, s.tiny_portals
    );
    println!(
        "  fill     {} leaves outside the world removed, {} clusters",
        s.leaves_filled, s.clusters
    );
    println!("  output   {} faces, {} vertices", s.faces, s.vertices);
    if s.sections > 1 {
        let names: Vec<&str> = output.bsp.sections[1..]
            .iter()
            .map(|x| x.name.as_str())
            .collect();
        println!(
            "  sections {} streamed: {}",
            s.sections - 1,
            names.join(", ")
        );
    }
    println!("  walkmap  {} walkable faces", output.walk.len());

    if let Some(leak) = &output.leak {
        println!(
            "  LEAK: the world is not sealed (traced from {:?})",
            leak.from
        );
    }

    if args.dry_run {
        println!(
            "  dry run: nothing written ({:.2}s)",
            started.elapsed().as_secs_f32()
        );
        return Ok(());
    }

    let size = kerosene_bsp::write_bsp(&output.bsp, &out_path)
        .with_context(|| format!("writing {}", out_path.display()))?;

    let prt_path = out_path.with_extension("keroprt");
    std::fs::write(&prt_path, &output.prt)
        .with_context(|| format!("writing {}", prt_path.display()))?;

    let walk_path = out_path.with_extension("kerowalk");
    output
        .walk
        .write(&walk_path)
        .with_context(|| format!("writing {}", walk_path.display()))?;

    // The trace describes *this* compile. A sealed map must clear the one
    // left by an earlier broken build, or every later compile looks like it
    // leaked -- the editor loads the file, not the result, and has no way to
    // tell a stale trace from a fresh one.
    match &output.leak {
        Some(leak) => {
            std::fs::write(&leak_path, leak.to_lin())?;
            println!(
                "  wrote {} (load it in Chisel to see the leak)",
                leak_path.display()
            );
        }
        None => {
            if leak_path.exists() {
                std::fs::remove_file(&leak_path).with_context(|| {
                    format!("removing the stale leak trace {}", leak_path.display())
                })?;
                println!(
                    "  the map is sealed; removed the old {}",
                    leak_path.display()
                );
            }
        }
    }

    println!(
        "  wrote {} ({:.1} KiB), {} and {} in {:.2}s",
        out_path.display(),
        size as f64 / 1024.0,
        prt_path.display(),
        walk_path.display(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}
