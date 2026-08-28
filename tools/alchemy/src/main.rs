// SPDX-License-Identifier: LGPL-3.0-or-later
//! Alchemy -- the VoidEngine texture and material tool.
//!
//! Turns source art into the formats the engine loads: `.png` and friends into
//! `.voidtex`, and material definitions into `.voidmat`. This is the VTFEdit/vtex
//! analogue, and it exists for the same reason: the engine should load
//! textures, not decode and mipmap them.
//!
//! ```text
//! alchemy compile art/grid.png -o materials/dev/grid.voidtex
//! alchemy compile art/grid_n.png --normal -o materials/dev/grid_normal.voidtex
//! alchemy material dev/grid --basetexture dev/grid -o materials/dev/grid.voidmat
//! alchemy batch art -o materials --make-materials
//! alchemy info materials/dev/grid.voidtex
//! ```

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use void_asset::{Material, Shader, Texture, TextureFlags, material::MaterialError};
use void_asset::texture::PixelFormat;

mod devtex;
mod font;

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
    /// Compile an image into a .voidtex.
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
    /// Write a .voidmat material definition.
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
        /// Also write a matching .voidmat next to each texture.
        #[arg(long)]
        make_materials: bool,
    },
    /// Describe a compiled .voidtex or .voidmat.
    Info { file: PathBuf },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    match Args::parse().command {
        Command::Compile { image, output, normal, clamp, point, ui, opaque } => {
            let out = output.unwrap_or_else(|| image.with_extension("voidtex"));
            let flags = build_flags(normal, clamp, point, ui);
            let size = compile_image(&image, &out, flags, opaque)?;
            println!("  wrote {} ({:.1} KiB)", out.display(), size as f64 / 1024.0);
            Ok(())
        }
        Command::Material { name, output, shader, basetexture, bumpmap, surfaceprop, translucent, extra } => {
            let out = output.unwrap_or_else(|| PathBuf::from(void_asset::material_path(&name)));
            write_material(&name, &out, &shader, basetexture, bumpmap, &surfaceprop, translucent, &extra)
        }
        Command::Batch { directory, output, make_materials } => {
            batch(&directory, &output, make_materials)
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
        Command::Info { file } => info(&file),
    }
}

fn build_flags(normal: bool, clamp: bool, point: bool, ui: bool) -> TextureFlags {
    let mut flags = TextureFlags::NONE;
    if normal { flags = flags | TextureFlags::NORMAL_MAP; }
    if clamp { flags = flags | TextureFlags::CLAMP; }
    if point { flags = flags | TextureFlags::POINT_SAMPLE; }
    // Interface art is always clamped: a UI element that wraps is a bug.
    if ui { flags = flags | TextureFlags::UI | TextureFlags::CLAMP; }
    flags
}

fn compile_image(source: &Path, out: &Path, flags: TextureFlags, force_opaque: bool) -> Result<u64> {
    let image = image::open(source)
        .with_context(|| format!("reading {}", source.display()))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();

    // Alpha costs a quarter of the memory, so drop it when it carries nothing.
    let has_alpha = !force_opaque && rgba.pixels().any(|p| p.0[3] != 255);
    let (format, pixels) = if has_alpha {
        (PixelFormat::Rgba8, rgba.into_raw())
    } else {
        let rgb: Vec<u8> = rgba.pixels().flat_map(|p| [p.0[0], p.0[1], p.0[2]]).collect();
        (PixelFormat::Rgb8, rgb)
    };

    let mut flags = flags;
    if has_alpha { flags = flags | TextureFlags::TRANSLUCENT; }

    let texture = Texture::build(width, height, format, flags, pixels)
        .with_context(|| format!("compiling {}", source.display()))?;

    println!("alchemy: {} -> {}x{}, {:?}, {} mips",
        source.display(), width, height, format, texture.mip_count());

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = texture.to_bytes();
    std::fs::write(out, &bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(bytes.len() as u64)
}

#[allow(clippy::too_many_arguments)]
fn write_material(
    name: &str,
    out: &Path,
    shader: &str,
    basetexture: Option<String>,
    bumpmap: Option<String>,
    surfaceprop: &str,
    translucent: bool,
    extra: &[String],
) -> Result<()> {
    let shader = Shader::from_name(shader)
        .with_context(|| format!("unknown shader '{shader}'; try lit, unlit, sky, water or ui"))?;

    let mut material = Material::new(shader);
    material.set("$basetexture", basetexture.unwrap_or_else(|| name.to_string()));
    if let Some(bump) = bumpmap { material.set("$bumpmap", bump); }
    material.set("$surfaceprop", surfaceprop);
    if translucent { material.set("$translucent", "1"); }

    for pair in extra {
        let Some((key, value)) = pair.split_once('=') else {
            bail!("--set expects key=value, got {pair:?}");
        };
        // Accept `basetexture=x` as well as `$basetexture=x`.
        let key = if key.starts_with('$') { key.to_string() } else { format!("${key}") };
        material.set(&key, value);
    }

    if let Some(parent) = out.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(out, material.to_text())
        .with_context(|| format!("writing {}", out.display()))?;
    println!("alchemy: wrote {} ({} shader)", out.display(), shader.name());
    Ok(())
}

fn batch(dir: &Path, out_root: &Path, make_materials: bool) -> Result<()> {
    if !dir.is_dir() { bail!("{} is not a directory", dir.display()); }

    let mut images = Vec::new();
    collect_images(dir, dir, &mut images)?;
    images.sort();
    if images.is_empty() { bail!("no images found under {}", dir.display()); }

    let mut compiled = 0usize;
    let mut kept = 0usize;
    for (path, relative) in &images {
        let name = relative.trim_end_matches(|c| c != '.').trim_end_matches('.');
        // A file ending in `_normal` or `_n` is taken to be a normal map. The
        // convention beats a flag here: batch compiles run unattended.
        let is_normal = name.ends_with("_normal") || name.ends_with("_n");
        let flags = build_flags(is_normal, false, false, false);

        let out = out_root.join(format!("{name}.voidtex"));
        compile_image(path, &out, flags, false)?;
        compiled += 1;

        if make_materials && !is_normal {
            let mat_path = out_root.join(format!("{name}.voidmat"));
            // Never overwrite a material that already exists. Materials are
            // authored -- a designer sets the surface property, the shader,
            // the blend mode -- and this only generates a starting point.
            // Clobbering that on every batch compile would be a good way to
            // lose an afternoon's work.
            if mat_path.exists() {
                kept += 1;
                continue;
            }
            let mut material = Material::new(Shader::Lit);
            material.set("$basetexture", name);
            // Wire up a matching normal map if one was compiled alongside.
            if images.iter().any(|(_, r)| r.starts_with(&format!("{name}_normal."))) {
                material.set("$bumpmap", format!("{name}_normal"));
            }
            if let Some(parent) = mat_path.parent() { std::fs::create_dir_all(parent)?; }
            std::fs::write(&mat_path, material.to_text())?;
        }
    }

    println!("alchemy: compiled {compiled} textures into {}", out_root.display());
    if kept > 0 {
        println!("  kept {kept} existing materials (delete one to regenerate it)");
    }
    Ok(())
}

fn collect_images(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_images(root, &path, out)?;
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "tga"))
        {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            out.push((path.clone(), relative.to_string_lossy().replace('\\', "/")));
        }
    }
    Ok(())
}

fn info(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;

    match path.extension().and_then(|e| e.to_str()) {
        Some("voidtex") => {
            let tex = Texture::from_bytes(&bytes)?;
            println!("{}", path.display());
            println!("  {}x{}, {:?}", tex.width(), tex.height(), tex.format);
            println!("  {} mip levels, {:.1} KiB", tex.mip_count(), bytes.len() as f64 / 1024.0);
            println!("  reflectivity {:.3} {:.3} {:.3}",
                tex.reflectivity.x, tex.reflectivity.y, tex.reflectivity.z);
            let mut flags = Vec::new();
            for (flag, name) in [
                (TextureFlags::CLAMP, "clamp"),
                (TextureFlags::POINT_SAMPLE, "point"),
                (TextureFlags::NORMAL_MAP, "normal map"),
                (TextureFlags::TRANSLUCENT, "translucent"),
                (TextureFlags::UI, "ui"),
            ] {
                if tex.flags.contains(flag) { flags.push(name); }
            }
            println!("  flags: {}", if flags.is_empty() { "none".into() } else { flags.join(", ") });
        }
        Some("voidmat") => {
            let text = String::from_utf8(bytes).context("material is not UTF-8")?;
            let material = Material::parse(&text).map_err(|e: MaterialError| anyhow::anyhow!(e))?;
            println!("{}", path.display());
            println!("  shader: {}", material.shader.name());
            println!("  surface: {}", material.surface_property());
            println!("  textures: {}", material.referenced_textures().join(", "));
            for (k, v) in material.params() {
                println!("    {k} = {v}");
            }
        }
        _ => bail!("{} is not a .voidtex or .voidmat", path.display()),
    }
    Ok(())
}
