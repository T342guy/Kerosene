// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Alchemy -- the Kerosene texture and material tool.
//!
//! Turns source art into the formats the engine loads: `.png` and friends into
//! `.kerotex`, and material definitions into `.keromat`. This is the
//! VTFEdit/vtex analogue, and it exists for the same reason: the engine should
//! load textures, not decode and mipmap them.
//!
//! This is a library as well as a command, because Chisel builds the texture
//! set before it opens its window. Shelling out to a sibling binary for that
//! would mean the editor only worked when the binary was on the path and next
//! to the right version of itself -- a way to look broken that has nothing to
//! do with textures.

pub mod devtex;
pub mod font;

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use kerosene_asset::texture::PixelFormat;
use kerosene_asset::{Material, Shader, Texture, TextureFlags, material::MaterialError};

pub fn build_flags(normal: bool, clamp: bool, point: bool, ui: bool) -> TextureFlags {
    let mut flags = TextureFlags::NONE;
    if normal { flags = flags | TextureFlags::NORMAL_MAP; }
    if clamp { flags = flags | TextureFlags::CLAMP; }
    if point { flags = flags | TextureFlags::POINT_SAMPLE; }
    // Interface art is always clamped: a UI element that wraps is a bug.
    if ui { flags = flags | TextureFlags::UI | TextureFlags::CLAMP; }
    flags
}

pub fn compile_image(source: &Path, out: &Path, flags: TextureFlags, force_opaque: bool) -> Result<u64> {
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

    log::debug!("{} -> {}x{}, {:?}, {} mips",
        source.display(), width, height, format, texture.mip_count());

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = texture.to_bytes();
    std::fs::write(out, &bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(bytes.len() as u64)
}

#[allow(clippy::too_many_arguments)]
pub fn write_material(
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

/// What a batch compile did, so a caller can say so without reading stdout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    /// Images compiled into a `.kerotex`.
    pub compiled: usize,
    /// Images whose `.kerotex` was already newer than the source.
    pub skipped: usize,
    /// Materials left alone because one was already authored.
    pub kept: usize,
    /// Materials written from scratch.
    pub materials: usize,
}

impl Batch {
    /// Whether anything on disk changed.
    pub fn did_anything(self) -> bool {
        self.compiled > 0 || self.materials > 0
    }
}

/// Compile every image under `dir` into `out_root`.
///
/// Images whose output is already newer than the source are skipped. That is
/// what makes this cheap enough to run on the way into the editor: a build
/// with nothing to do does nothing, and costs no more than a directory walk.
pub fn batch(dir: &Path, out_root: &Path, make_materials: bool) -> Result<Batch> {
    if !dir.is_dir() { bail!("{} is not a directory", dir.display()); }

    let mut images = Vec::new();
    collect_images(dir, dir, &mut images)?;
    images.sort();
    if images.is_empty() { bail!("no images found under {}", dir.display()); }

    let mut report = Batch::default();
    for (path, relative) in &images {
        let name = relative.trim_end_matches(|c| c != '.').trim_end_matches('.');
        // A file ending in `_normal` or `_n` is taken to be a normal map. The
        // convention beats a flag here: batch compiles run unattended.
        let is_normal = name.ends_with("_normal") || name.ends_with("_n");
        let flags = build_flags(is_normal, false, false, false);

        let out = out_root.join(format!("{name}.kerotex"));
        if is_up_to_date(path, &out) {
            report.skipped += 1;
        } else {
            compile_image(path, &out, flags, false)?;
            report.compiled += 1;
        }

        if make_materials && !is_normal {
            let mat_path = out_root.join(format!("{name}.keromat"));
            // Never overwrite a material that already exists. Materials are
            // authored -- a designer sets the surface property, the shader,
            // the blend mode -- and this only generates a starting point.
            // Clobbering that on every batch compile would be a good way to
            // lose an afternoon's work.
            if mat_path.exists() {
                report.kept += 1;
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
            report.materials += 1;
        }
    }

    Ok(report)
}

/// Whether `out` was written after `source` last changed.
///
/// A missing or unreadable timestamp counts as out of date. Recompiling
/// something that did not need it costs a moment; skipping something that did
/// leaves a texture that does not match its source, and no way to tell.
fn is_up_to_date(source: &Path, out: &Path) -> bool {
    let Ok(built) = std::fs::metadata(out).and_then(|m| m.modified()) else { return false };
    let Ok(written) = std::fs::metadata(source).and_then(|m| m.modified()) else { return false };
    built >= written
}

// ---- the whole texture build, as one call -----------------------------------

/// What a full texture build did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Build {
    /// The generated developer PNGs written under `art/`.
    pub dev_art: devtex::Written,
    /// The generated developer materials written under `materials/`.
    pub dev_materials: devtex::Written,
    /// What compiling the art tree did.
    pub textures: Batch,
}

impl Build {
    /// Whether anything on disk changed.
    pub fn did_anything(self) -> bool {
        self.dev_art.changed > 0 || self.dev_materials.changed > 0 || self.textures.did_anything()
    }
}

impl std::fmt::Display for Build {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.did_anything() {
            return write!(f, "textures already built ({} up to date)", self.textures.skipped);
        }
        write!(f, "built {} textures", self.textures.compiled)?;
        if self.textures.skipped > 0 { write!(f, ", {} up to date", self.textures.skipped)?; }
        if self.dev_art.changed > 0 { write!(f, ", {} developer images", self.dev_art.changed)?; }
        if self.textures.materials > 0 { write!(f, ", {} new materials", self.textures.materials)?; }
        Ok(())
    }
}

/// Build every texture a content tree needs, from its own sources.
///
/// The developer set is generated first, so the batch compile below picks it
/// up in the same pass; its materials are written by the generator rather than
/// inferred, because it knows a sky is not lit and a tool texture is not
/// shaded, and nothing can work that out from a PNG.
///
/// This is the whole texture half of a content build, and it is one function
/// because three callers need exactly it: the build script, Chisel on the way
/// to opening its window, and Chisel again before a map compile. Each of them
/// having its own idea of what "build the textures" meant is how the editor
/// came to open with no textures in it while the build script insisted
/// everything was fine.
pub fn build_textures(content_root: &Path) -> Result<Build> {
    let art = content_root.join("art");
    let materials = content_root.join("materials");

    let mut build = Build {
        dev_art: devtex::write_all(&art)?,
        dev_materials: devtex::write_materials(&materials)?,
        textures: Batch::default(),
    };
    build.textures = batch(&art, &materials, true)?;
    Ok(build)
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

pub fn info(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;

    match path.extension().and_then(|e| e.to_str()) {
        Some("kerotex") => {
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
        Some("keromat") => {
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
        _ => bail!("{} is not a .kerotex or .keromat", path.display()),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
