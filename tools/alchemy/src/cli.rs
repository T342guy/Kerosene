// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The command-line surface of Alchemy, exposed as a `run` the unified
//! toolset calls for the `alchemy` subcommand.
//!
//! ```text
//! kerosene-tools alchemy compile art/grid.png -o materials/dev/grid.kerotex
//! kerosene-tools alchemy material dev/grid --basetexture dev/grid
//! kerosene-tools alchemy batch art -o materials --make-materials
//! kerosene-tools alchemy build content
//! kerosene-tools alchemy new-texture Walls/brick --basecolor ~/brick.png --normal ~/brick_n.png
//! kerosene-tools alchemy texture-set content/textures/Walls/brick
//! kerosene-tools alchemy info materials/dev/grid.kerotex
//! ```

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use crate::{
    batch, build_flags, build_textures, compile_image, compile_set, devtex, info, new_texture,
    write_material,
};

#[derive(Parser, Debug)]
#[command(
    name = "alchemy",
    version,
    about = "Compile textures and author materials"
)]
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
    /// Compile one texture set folder.
    ///
    /// The folder is a set if it holds a base colour; everything else it holds
    /// -- normals, roughness, emissive, occlusion -- comes along with it.
    TextureSet {
        /// The set's folder, e.g. `content/textures/Walltextures/variant1`.
        directory: PathBuf,
        /// Where the compiled textures go.
        #[arg(short, long, default_value = "content/materials")]
        output: PathBuf,
        /// The tree the set's name is derived from. Defaults to the first
        /// `textures` directory above it, then to the set's parent.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Start a new texture set: make the folder, bring the images in, write
    /// its config.
    ///
    /// The deliberate way to add a texture, as opposed to dropping a PNG
    /// somewhere and relying on a filename suffix to be guessed correctly.
    NewTexture {
        /// What to call it, e.g. `Walltextures/variant1` or `brick_red`.
        name: String,
        /// The content root the set is created under.
        #[arg(short, long, default_value = "content")]
        content: PathBuf,
        /// The base colour. The only image a set cannot do without.
        #[arg(long)]
        basecolor: PathBuf,
        #[arg(long)]
        normal: Option<PathBuf>,
        #[arg(long)]
        roughness: Option<PathBuf>,
        #[arg(long)]
        emissive: Option<PathBuf>,
        #[arg(long)]
        ao: Option<PathBuf>,
        #[arg(long, default_value = "lit")]
        shader: String,
        #[arg(long, default_value = "default")]
        surfaceprop: String,
        /// Compile it straight away, rather than leaving it for the next build.
        #[arg(long)]
        build: bool,
    },
    /// Describe a compiled .kerotex or .keromat.
    Info { file: PathBuf },
}

/// Entry point for the `alchemy` subcommand of the unified toolset.
pub fn run(args: Vec<String>) -> Result<()> {
    let args = Args::parse_from(std::iter::once("alchemy".to_string()).chain(args));
    match args.command {
        Command::Compile {
            image,
            output,
            normal,
            clamp,
            point,
            ui,
            opaque,
        } => {
            let out = output.unwrap_or_else(|| image.with_extension("kerotex"));
            let flags = build_flags(normal, clamp, point, ui);
            let size = compile_image(&image, &out, flags, opaque)?;
            println!(
                "  wrote {} ({:.1} KiB)",
                out.display(),
                size as f64 / 1024.0
            );
            Ok(())
        }
        Command::Material {
            name,
            output,
            shader,
            basetexture,
            bumpmap,
            surfaceprop,
            translucent,
            extra,
        } => {
            let out = output.unwrap_or_else(|| PathBuf::from(kerosene_asset::material_path(&name)));
            write_material(
                &name,
                &out,
                &shader,
                basetexture,
                bumpmap,
                &surfaceprop,
                translucent,
                &extra,
            )
        }
        Command::Batch {
            directory,
            output,
            make_materials,
        } => {
            let report = batch(&directory, &output, make_materials)?;
            println!(
                "alchemy: compiled {} textures into {}",
                report.compiled,
                output.display()
            );
            if report.skipped > 0 {
                println!("  {} already up to date", report.skipped);
            }
            if report.materials > 0 {
                println!("  wrote {} materials", report.materials);
            }
            if report.kept > 0 {
                println!(
                    "  kept {} existing materials (delete one to regenerate it)",
                    report.kept
                );
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
            let build = build_textures(&content)?;
            println!("alchemy: {build}");
            Ok(())
        }
        Command::TextureSet {
            directory,
            output,
            root,
        } => {
            let root = root.unwrap_or_else(|| textures_root_of(&directory));
            let Some(set) = kerosene_asset::textureset::TextureSet::discover(&directory, &root)
            else {
                bail!(
                    "{} is not a texture set: it has no base colour image in it \
                     (basecolor.png, albedo.png, diffuse.png, ...)",
                    directory.display()
                );
            };
            let report = compile_set(&set, &output)?;
            println!(
                "alchemy: {} -- {} compiled, {} up to date",
                set.name, report.compiled, report.skipped
            );
            if report.material {
                println!("  wrote {}.keromat", set.name);
            } else {
                println!("  kept the existing {}.keromat", set.name);
            }
            Ok(())
        }
        Command::NewTexture {
            name,
            content,
            basecolor,
            normal,
            roughness,
            emissive,
            ao,
            shader,
            surfaceprop,
            build,
        } => new_texture(
            &name,
            &content,
            &basecolor,
            normal.as_deref(),
            roughness.as_deref(),
            emissive.as_deref(),
            ao.as_deref(),
            &shader,
            &surfaceprop,
            build,
        ),
        Command::Info { file } => info(&file),
    }
}

/// The `textures` directory a set sits under, for working out its name.
///
/// Climbing to it rather than taking the parent means
/// `alchemy texture-set content/textures/Walls/v1` names the set `Walls_v1`,
/// the same as a full build would, instead of `v1`. A set compiled by hand and
/// a set compiled by the build must end up with the same name, or the material
/// written by one will not be found by the other.
fn textures_root_of(directory: &Path) -> PathBuf {
    let mut at = directory;
    while let Some(parent) = at.parent() {
        if parent.file_name().is_some_and(|n| n == "textures") {
            return parent.to_path_buf();
        }
        at = parent;
    }
    directory.parent().unwrap_or(directory).to_path_buf()
}
