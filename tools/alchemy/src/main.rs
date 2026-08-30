// SPDX-License-Identifier: LGPL-3.0-or-later
//! `alchemy` -- the command-line front end to the texture tool.
//!
//! Turns source art into the formats the engine loads: `.png` and friends into
//! `.kerotex`, and material definitions into `.keromat`. This is the VTFEdit/vtex
//! analogue, and it exists for the same reason: the engine should load
//! textures, not decode and mipmap them.
//!
//! ```text
//! alchemy compile art/grid.png -o materials/dev/grid.kerotex
//! alchemy compile art/grid_n.png --normal -o materials/dev/grid_normal.kerotex
//! alchemy material dev/grid --basetexture dev/grid -o materials/dev/grid.keromat
//! alchemy batch art -o materials --make-materials
//! alchemy build content              # the whole texture set for a project
//! alchemy info materials/dev/grid.kerotex
//! ```

use alchemy::{batch, build_flags, compile_image, devtex, info, write_material};
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "alchemy", version, about = "Compile textures and author materials")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Write the standard developer and tool texture set.
    DevTextures {
        /// Where the art tree lives; `dev/` and `tools/` go under it.
        #[arg(short, long, default_value = "content/art")]
        output: PathBuf,
        /// Also write the matching materials, under this root.
        #[arg(long)]
        materials: Option<PathBuf>,
    },
    /// Compile an image into a .kerotex.
    Compile {
        image: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Tangent-space normal map: kept out of sRGB and never tinted.
        #[arg(long)]
        normal: bool,
        /// Clamp at the edges instead of repeating.
        #[arg(long)]
        clamp: bool,
        /// Nearest-neighbour sampling.
        #[arg(long)]
        point: bool,
        /// Interface art: no mipmaps, always clamped.
        #[arg(long)]
        ui: bool,
        /// Drop the alpha channel when the image does not need it.
        #[arg(long)]
        opaque: bool,
    },
    /// Write a .keromat material definition.
    Material {
        /// Material name, as geometry refers to it (e.g. `dev/grid`).
        name: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "lit")]
        shader: String,
        #[arg(long)]
        basetexture: Option<String>,
        #[arg(long)]
        bumpmap: Option<String>,
        #[arg(long, default_value = "default")]
        surfaceprop: String,
        #[arg(long)]
        translucent: bool,
        /// Extra `key=value` parameters. Repeatable.
        #[arg(long = "set")]
        extra: Vec<String>,
    },
    /// Compile every image in a directory tree.
    Batch {
        directory: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Also write a matching .keromat next to each texture.
        #[arg(long)]
        make_materials: bool,
    },
    /// Build every texture a content tree needs, from its own sources.
    ///
    /// The developer set, then every image under `art/`. This is what Chisel
    /// runs on the way to opening its window.
    Build {
        /// The content root: `art/` and `materials/` live under it.
        #[arg(default_value = "content")]
        content: PathBuf,
    },
    /// Describe a compiled .kerotex or .keromat.
    Info { file: PathBuf },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    match Args::parse().command {
        Command::Compile { image, output, normal, clamp, point, ui, opaque } => {
            let out = output.unwrap_or_else(|| image.with_extension("kerotex"));
            let flags = build_flags(normal, clamp, point, ui);
            let size = compile_image(&image, &out, flags, opaque)?;
            println!("  wrote {} ({:.1} KiB)", out.display(), size as f64 / 1024.0);
            Ok(())
        }
        Command::Material { name, output, shader, basetexture, bumpmap, surfaceprop, translucent, extra } => {
            let out = output.unwrap_or_else(|| PathBuf::from(kerosene_asset::material_path(&name)));
            write_material(&name, &out, &shader, basetexture, bumpmap, &surfaceprop, translucent, &extra)
        }
        Command::Batch { directory, output, make_materials } => {
            let report = batch(&directory, &output, make_materials)?;
            println!("alchemy: compiled {} textures into {}", report.compiled, output.display());
            if report.skipped > 0 { println!("  {} already up to date", report.skipped); }
            if report.materials > 0 { println!("  wrote {} materials", report.materials); }
            if report.kept > 0 {
                println!("  kept {} existing materials (delete one to regenerate it)", report.kept);
            }
            Ok(())
        }
        Command::DevTextures { output, materials } => {
            let textures = devtex::write_all(&output)?;
            println!("  textures under {}: {textures}", output.display());
            if let Some(root) = materials {
                let written = devtex::write_materials(&root)?;
                println!("  materials under {}: {written}", root.display());
            }
            Ok(())
        }
        Command::Build { content } => {
            let build = alchemy::build_textures(&content)?;
            println!("alchemy: {build}");
            Ok(())
        }
        Command::Info { file } => info(&file),
    }
}

