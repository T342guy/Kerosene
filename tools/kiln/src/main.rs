// SPDX-License-Identifier: LGPL-3.0-or-later
//! `kiln` -- build a project's content.
//!
//! ```text
//! kiln                              # build everything, from here
//! kiln --content path/to/content    # or from there
//! kiln --only maps --fast           # just relight, quickly
//! kiln --dry-run                    # say what would run
//! kiln --tools                      # which compilers can be found
//! kiln --ship dist                  # build, then assemble something shippable
//! kiln --ship dist --only ship      # assemble what is already built
//! ```

use anyhow::{Result, bail};
use clap::Parser;
use kiln::{Settings, Stage};
use std::path::PathBuf;
use void_vfs::toolchain;

#[derive(Parser, Debug)]
#[command(name = "kiln", version, about = "Build a VoidEngine project's content")]
struct Args {
    /// The content tree. With none, the project is found the way every other
    /// tool finds it.
    #[arg(long)]
    content: Option<PathBuf>,

    /// Run only these stages: textures, sounds, models, maps, pack. Repeatable.
    #[arg(long = "only", value_name = "STAGE")]
    only: Vec<String>,

    /// Skip the expensive visibility and lighting passes.
    #[arg(long)]
    fast: bool,

    /// Say what would run, and run nothing.
    #[arg(long)]
    dry_run: bool,

    /// Compile a map even if it leaks.
    #[arg(long)]
    ignore_leaks: bool,

    /// Treat `.obj` sources as void units rather than metres.
    #[arg(long)]
    model_units: bool,

    /// List the tools that can be found, and stop.
    #[arg(long)]
    tools: bool,

    /// Assemble a distribution into this directory once the content is built.
    #[arg(long, value_name = "DIR")]
    ship: Option<PathBuf>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let args = Args::parse();

    if args.tools {
        println!("tools kiln can find:");
        for (name, found) in toolchain::available() {
            let where_ = toolchain::path(name)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| if found { "on PATH".into() } else { "not found".into() });
            println!("  {:<9} {:<3} {where_}", name, if found { "ok" } else { "--" });
        }
        return Ok(());
    }

    let found = void_vfs::root::find(args.content.as_deref(), None);
    println!("{}", void_vfs::root::describe(&found));
    let Some(found) = found else {
        bail!("nothing to build. Run kiln from a project, or pass --content");
    };

    let mut stages = Vec::new();
    for name in &args.only {
        match Stage::parse(name) {
            Some(stage) => stages.push(stage),
            None => bail!(
                "unknown stage {name:?}. Try textures, sounds, models, maps, pack or ship."
            ),
        }
    }
    if stages.is_empty() { stages = Stage::ALL.to_vec() }
    // Asking for a distribution is asking for the stage that makes one, so it
    // does not also have to be named with --only. Naming it explicitly still
    // works, and is how you assemble without rebuilding.
    if args.ship.is_some() && !stages.contains(&Stage::Ship) {
        stages.push(Stage::Ship);
    }

    let settings = Settings {
        content: found.root,
        project: found.project,
        stages,
        fast: args.fast,
        dry_run: args.dry_run,
        ignore_leaks: args.ignore_leaks,
        models_in_metres: !args.model_units,
        ship_to: args.ship,
    };

    let report = kiln::build(&settings)?;

    println!();
    println!(
        "built {} textures ({} up to date), {} sounds ({} up to date), {} models, {} maps",
        report.textures,
        report.textures_skipped,
        report.sounds,
        report.sounds_skipped,
        report.models,
        report.maps
    );
    if let Some(archive) = &report.packed {
        println!("packed into {}", archive.display());
    }
    if let Some(shipped) = &report.shipped {
        println!("shipped into {}", shipped.root.display());
        // Said plainly and every time, because it is the difference between a
        // distribution that satisfies the licence on its own and one whose
        // obligations fall on whoever hands it out.
        if !shipped.engine_is_replaceable() {
            println!();
            println!("note: the engine is linked statically into this build, so a");
            println!("      recipient cannot replace it. That is fine for a game you");
            println!("      ship with source; a closed-source one needs the engine as a");
            println!("      shared library. README.txt in the distribution says so too.");
        }
    }

    // Last, and on its own, because it is the thing worth reading: a leaking
    // map compiles and then behaves like a broken renderer.
    if !report.leaking.is_empty() {
        println!();
        println!("{} map(s) LEAK and will not light or cull correctly:", report.leaking.len());
        for name in &report.leaking {
            println!("  {name} -- open it in Chisel; the leak is drawn as a red line");
        }
    }
    Ok(())
}
