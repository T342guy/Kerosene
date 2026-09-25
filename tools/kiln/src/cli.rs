// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The command-line surface of Kiln, exposed as a `run` the unified toolset
//! calls for the `kiln` subcommand.
//!
//! ```text
//! kerosene-tools kiln                              # build everything, from here
//! kerosene-tools kiln --content path/to/content    # or from there
//! kerosene-tools kiln --only maps --fast           # just relight, quickly
//! kerosene-tools kiln --dry-run                    # say what would run
//! kerosene-tools kiln --tools                      # which pieces are present
//! kerosene-tools kiln --ship dist                  # build, then assemble
//! kerosene-tools kiln --ship dist --steam          # ... for Steam, with Valve's library
//! kerosene-tools kiln --ship dist --steam-upload me  # ... and upload it with steamcmd
//! ```

use crate::{Settings, Stage};
use anyhow::{Result, bail};
use clap::Parser;
use kerosene_vfs::toolchain;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "kiln", version, about = "Build a Kerosene project's content")]
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

    /// Rebuild sounds even when their compiled form looks up to date.
    #[arg(long)]
    force: bool,

    /// Treat `.obj` sources as kerosene units rather than metres.
    #[arg(long)]
    model_units: bool,

    /// List the pieces that can be found, and stop.
    #[arg(long)]
    tools: bool,

    /// Assemble a distribution into this directory once the content is built.
    #[arg(long, value_name = "DIR")]
    ship: Option<PathBuf>,

    /// Ship for Steam: build with the `steam` feature, put Valve's
    /// redistributable beside the game, and write SteamPipe build scripts.
    #[arg(long)]
    steam: bool,

    /// With --steam: also write steam_appid.txt, so the build runs outside
    /// the Steam client. For testing; it is never uploaded.
    #[arg(long)]
    steam_dev: bool,

    /// With --steam: upload the build with steamcmd, signed in as this
    /// account. steamcmd asks for the password itself.
    #[arg(long, value_name = "ACCOUNT")]
    steam_upload: Option<String>,
}

/// Entry point for the `kiln` subcommand of the unified toolset.
pub fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(std::iter::once("kiln".to_string()).chain(args));

    if args.tools {
        println!("tools kiln can find:");
        for (name, found) in toolchain::available() {
            let where_ = if toolchain::TOOLSET.contains(&name) {
                // A subcommand of the one toolset executable: always present.
                "part of the toolset".to_string()
            } else {
                toolchain::path(name)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| {
                        if found {
                            "on PATH".into()
                        } else {
                            "not found".into()
                        }
                    })
            };
            println!(
                "  {:<9} {:<3} {where_}",
                name,
                if found { "ok" } else { "--" }
            );
        }
        return Ok(());
    }

    let found = kerosene_vfs::root::find(args.content.as_deref(), None);
    println!("{}", kerosene_vfs::root::describe(&found));
    let Some(found) = found else {
        bail!("nothing to build. Run kiln from a project, or pass --content");
    };

    let mut stages = Vec::new();
    for name in &args.only {
        match Stage::parse(name) {
            Some(stage) => stages.push(stage),
            None => {
                bail!("unknown stage {name:?}. Try textures, sounds, models, maps, pack or ship.")
            }
        }
    }
    if stages.is_empty() {
        stages = Stage::ALL.to_vec()
    }
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
        force: args.force,
        models_in_metres: !args.model_units,
        steam: (args.steam || args.steam_dev || args.steam_upload.is_some()).then(|| {
            crate::steam::SteamShip {
                dev: args.steam_dev,
                upload_as: args.steam_upload.clone(),
            }
        }),
        ship_to: args.ship,
    };
    if settings.steam.is_some() && settings.ship_to.is_none() {
        bail!("--steam is a way of shipping; say where with --ship <dir>");
    }

    let report = crate::build(&settings)?;

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
    }

    // Last, and on its own, because it is the thing worth reading: a leaking
    // map compiles and then behaves like a broken renderer.
    if !report.leaking.is_empty() {
        println!();
        println!(
            "{} map(s) LEAK and were not lit or culled:",
            report.leaking.len()
        );
        for name in &report.leaking {
            println!("  {name} -- open it in Chisel; the leak is drawn as a red line");
        }
        // A build server should see this as the failure it is; the summary
        // above is for the person reading the log.
        anyhow::bail!("{} map(s) leak", report.leaking.len());
    }
    Ok(())
}
