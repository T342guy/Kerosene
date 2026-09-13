// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Alchemy -- the Kerosene texture and material tool.
//!
//! Turns source art into the formats the engine loads: `.png` and friends into
//! `.kerotex`, and material definitions into `.keromat`. This is the
//! VTFEdit/vtex analogue, and it exists for the same reason: the engine should
//! load textures, not decode and mipmap them.
//!
//! This is a library the toolset calls as the `alchemy` subcommand, and it is
//! also called directly by Chisel, which builds the texture set before it
//! opens its window. Running that step in-process rather than re-invoking the
//! toolset means the editor works even when the only binary present is the one
//! it is running as -- a way to look broken that has nothing to do with
//! textures.

mod cli;
pub mod devtex;
pub mod font;

pub use cli::run;

use anyhow::{Context, Result, bail};
use kerosene_asset::texture::PixelFormat;
use kerosene_asset::{
    MapKind, Material, Shader, Texture, TextureFlags, TextureSet, ext, material::MaterialError,
};
use std::path::{Path, PathBuf};

pub fn build_flags(normal: bool, clamp: bool, point: bool, ui: bool) -> TextureFlags {
    let mut flags = TextureFlags::NONE;
    if normal {
        flags = flags | TextureFlags::NORMAL_MAP;
    }
    if clamp {
        flags = flags | TextureFlags::CLAMP;
    }
    if point {
        flags = flags | TextureFlags::POINT_SAMPLE;
    }
    // Interface art is always clamped: a UI element that wraps is a bug.
    if ui {
        flags = flags | TextureFlags::UI | TextureFlags::CLAMP;
    }
    flags
}

pub fn compile_image(
    source: &Path,
    out: &Path,
    flags: TextureFlags,
    force_opaque: bool,
) -> Result<u64> {
    let image = image::open(source).with_context(|| format!("reading {}", source.display()))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();

    // Alpha costs a quarter of the memory, so drop it when it carries nothing.
    let has_alpha = !force_opaque && rgba.pixels().any(|p| p.0[3] != 255);
    let (format, pixels) = if has_alpha {
        (PixelFormat::Rgba8, rgba.into_raw())
    } else {
        let rgb: Vec<u8> = rgba
            .pixels()
            .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
            .collect();
        (PixelFormat::Rgb8, rgb)
    };

    let mut flags = flags;
    if has_alpha {
        flags = flags | TextureFlags::TRANSLUCENT;
    }

    let texture = Texture::build(width, height, format, flags, pixels)
        .with_context(|| format!("compiling {}", source.display()))?;

    log::debug!(
        "{} -> {}x{}, {:?}, {} mips",
        source.display(),
        width,
        height,
        format,
        texture.mip_count()
    );

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
    material.set(
        "$basetexture",
        basetexture.unwrap_or_else(|| name.to_string()),
    );
    if let Some(bump) = bumpmap {
        material.set("$bumpmap", bump);
    }
    material.set("$surfaceprop", surfaceprop);
    if translucent {
        material.set("$translucent", "1");
    }

    for pair in extra {
        let Some((key, value)) = pair.split_once('=') else {
            bail!("--set expects key=value, got {pair:?}");
        };
        // Accept `basetexture=x` as well as `$basetexture=x`.
        let key = if key.starts_with('$') {
            key.to_string()
        } else {
            format!("${key}")
        };
        material.set(&key, value);
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, material.to_text())
        .with_context(|| format!("writing {}", out.display()))?;
    println!(
        "alchemy: wrote {} ({} shader)",
        out.display(),
        shader.name()
    );
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
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }

    let mut images = Vec::new();
    collect_images(dir, dir, &mut images)?;
    images.sort();
    if images.is_empty() {
        bail!("no images found under {}", dir.display());
    }

    let mut report = Batch::default();
    for (path, relative) in &images {
        let name = relative
            .trim_end_matches(|c| c != '.')
            .trim_end_matches('.');
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
            if images
                .iter()
                .any(|(_, r)| r.starts_with(&format!("{name}_normal.")))
            {
                material.set("$bumpmap", format!("{name}_normal"));
            }
            if let Some(parent) = mat_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&mat_path, material.to_text())?;
            report.materials += 1;
        }
    }

    Ok(report)
}

// ---- texture sets -----------------------------------------------------------

/// What compiling one set did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SetReport {
    /// Maps compiled into a `.kerotex`.
    pub compiled: usize,
    /// Maps whose `.kerotex` was already newer than the source.
    pub skipped: usize,
    /// Whether a material was written (as opposed to one already existing).
    pub material: bool,
}

/// Compile every map in a texture set, and write its material.
///
/// Each map carries its own flags -- a normal map is not colour, roughness is
/// not colour and is not a normal map either -- so they cannot share one
/// compile the way a directory of loose images can. The set already knows
/// which is which, which is the whole reason it is a set rather than five
/// files and a naming convention.
pub fn compile_set(set: &TextureSet, out_root: &Path) -> Result<SetReport> {
    let mut report = SetReport::default();

    for (kind, source) in &set.maps {
        let out = out_root.join(format!("{}.{}", set.texture_name(*kind), ext::TEXTURE));
        if is_up_to_date(source, &out) {
            report.skipped += 1;
            continue;
        }
        // Only the base map may be translucent. An alpha channel on a
        // roughness map is a packing artefact, not transparency, and treating
        // it as transparency would make the surface it describes vanish.
        let force_opaque = *kind != MapKind::Base;
        compile_image(source, &out, set.flags_for(*kind), force_opaque)
            .with_context(|| format!("compiling the {kind:?} map of {}", set.name))?;
        report.compiled += 1;
    }

    report.material = write_set_material(set, out_root)?;
    Ok(report)
}

/// Write a set's `.keromat`, unless one is already there.
///
/// Returns whether it wrote anything. The same rule the loose-image batch
/// follows: a material is *authored* -- somebody chose the surface property
/// and the shader -- and regenerating it on every build is how an afternoon's
/// work disappears. Delete the file to get a fresh one.
pub fn write_set_material(set: &TextureSet, out_root: &Path) -> Result<bool> {
    let path = out_root.join(format!("{}.{}", set.name, ext::MATERIAL));
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, set.to_material().to_text())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// Compile every texture set under `dir`.
///
/// A directory that is not there is not an error: `textures/` is optional, and
/// a project that has not made one yet should build, not fail.
pub fn batch_sets(dir: &Path, out_root: &Path) -> Result<Batch> {
    let mut report = Batch::default();
    if !dir.is_dir() {
        return Ok(report);
    }

    for set in kerosene_asset::textureset::walk(dir) {
        let one = compile_set(&set, out_root)?;
        report.compiled += one.compiled;
        report.skipped += one.skipped;
        if one.material {
            report.materials += 1;
        } else {
            report.kept += 1;
        }
    }
    Ok(report)
}

/// Start a new texture set: make the folder, copy the images in, write the
/// config.
///
/// This is the deliberate way to add a texture. The alternative the engine has
/// always had -- drop a PNG under `art/` and hope the `_normal` suffix rule
/// guesses right -- works, but it has nowhere to say a surface is brick, or
/// that it clamps, and no way to hand somebody four maps at once and have them
/// end up as one surface.
///
/// Images are *copied*, not moved or linked: the artist's original stays where
/// it is. The copy is the source of record from then on, which is what makes
/// the content tree self-contained enough to hand to somebody else.
#[allow(clippy::too_many_arguments)]
pub fn new_texture(
    name: &str,
    content: &Path,
    basecolor: &Path,
    normal: Option<&Path>,
    roughness: Option<&Path>,
    emissive: Option<&Path>,
    ao: Option<&Path>,
    shader: &str,
    surfaceprop: &str,
    build: bool,
) -> Result<()> {
    let shader = Shader::from_name(shader)
        .with_context(|| format!("unknown shader '{shader}'; try lit, unlit, sky, water or ui"))?;

    let textures = content.join("textures");
    // The name doubles as the path, so `Walls/brick` makes two directories and
    // is called `Walls_brick` -- the same answer a plain directory walk would
    // reach, which is the point.
    let dir = textures.join(name.replace('\\', "/").trim_matches('/'));
    if dir.exists() {
        bail!(
            "{} already exists; delete it or pick another name",
            dir.display()
        );
    }

    let sources = [
        (MapKind::Base, Some(basecolor)),
        (MapKind::Normal, normal),
        (MapKind::Roughness, roughness),
        (MapKind::Emissive, emissive),
        (MapKind::Ao, ao),
    ];

    // Check every source before creating anything, so a typo in the last
    // argument does not leave a half-made texture behind.
    for (kind, source) in sources.iter().filter_map(|(k, s)| s.map(|s| (k, s))) {
        if !source.is_file() {
            bail!("the {kind:?} map {} is not a file", source.display());
        }
        if !kerosene_asset::textureset::has_image_extension(source) {
            bail!(
                "{} is not an image alchemy can read (png, jpg, jpeg, tga)",
                source.display()
            );
        }
    }

    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let mut maps = std::collections::BTreeMap::new();
    for (kind, source) in sources.iter().filter_map(|(k, s)| s.map(|s| (*k, s))) {
        // Canonical stem, whatever the original was called: the folder should
        // read the same no matter which tool baked its maps.
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_lowercase();
        let stem = kind.aliases()[0];
        let out = dir.join(format!("{stem}.{extension}"));
        std::fs::copy(source, &out)
            .with_context(|| format!("copying {} to {}", source.display(), out.display()))?;
        maps.insert(kind, out);
    }

    let set = TextureSet {
        name: TextureSet::name_from_path(&dir, &textures),
        directory: dir.clone(),
        maps,
        shader,
        surface_prop: surfaceprop.to_string(),
        clamp: false,
        point: false,
        translucent: false,
    };

    let config = dir.join(kerosene_asset::textureset::CONFIG_FILENAME);
    std::fs::write(&config, set.to_config())
        .with_context(|| format!("writing {}", config.display()))?;

    println!("alchemy: created {} ({})", dir.display(), set.name);
    for kind in MapKind::ALL {
        if set.maps.contains_key(&kind) {
            println!("  {:?} -> {}", kind, set.texture_name(kind));
        }
    }

    if build {
        let report = compile_set(&set, &content.join("materials"))?;
        println!("  compiled {} textures", report.compiled);
    } else {
        println!("  run `alchemy build` to compile it");
    }
    Ok(())
}

/// Whether `out` was written after `source` last changed.
///
/// A missing or unreadable timestamp counts as out of date. Recompiling
/// something that did not need it costs a moment; skipping something that did
/// leaves a texture that does not match its source, and no way to tell.
fn is_up_to_date(source: &Path, out: &Path) -> bool {
    let Ok(built) = std::fs::metadata(out).and_then(|m| m.modified()) else {
        return false;
    };
    let Ok(written) = std::fs::metadata(source).and_then(|m| m.modified()) else {
        return false;
    };
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
    /// What compiling the `textures/` folder sets did.
    pub sets: Batch,
}

impl Build {
    /// Whether anything on disk changed.
    pub fn did_anything(self) -> bool {
        self.dev_art.changed > 0
            || self.dev_materials.changed > 0
            || self.textures.did_anything()
            || self.sets.did_anything()
    }
}

impl std::fmt::Display for Build {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let compiled = self.textures.compiled + self.sets.compiled;
        let skipped = self.textures.skipped + self.sets.skipped;
        let materials = self.textures.materials + self.sets.materials;

        if !self.did_anything() {
            return write!(f, "textures already built ({skipped} up to date)");
        }
        write!(f, "built {compiled} textures")?;
        if skipped > 0 {
            write!(f, ", {skipped} up to date")?;
        }
        if self.dev_art.changed > 0 {
            write!(f, ", {} developer images", self.dev_art.changed)?;
        }
        if materials > 0 {
            write!(f, ", {materials} new materials")?;
        }
        Ok(())
    }
}

/// Build every texture a content tree needs, from its own sources.
///
/// Three passes over two source trees. The developer set is generated first,
/// so the batch compile below picks it up in the same pass; its materials are written by the generator rather than
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
        sets: Batch::default(),
    };
    build.textures = batch(&art, &materials, true)?;
    // Sets last, so a set may deliberately shadow a loose image of the same
    // name: the folder is the more specific statement of the two.
    build.sets = batch_sets(&content_root.join("textures"), &materials)?;
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
            println!(
                "  {} mip levels, {:.1} KiB",
                tex.mip_count(),
                bytes.len() as f64 / 1024.0
            );
            println!(
                "  reflectivity {:.3} {:.3} {:.3}",
                tex.reflectivity.x, tex.reflectivity.y, tex.reflectivity.z
            );
            let mut flags = Vec::new();
            for (flag, name) in [
                (TextureFlags::CLAMP, "clamp"),
                (TextureFlags::POINT_SAMPLE, "point"),
                (TextureFlags::NORMAL_MAP, "normal map"),
                (TextureFlags::TRANSLUCENT, "translucent"),
                (TextureFlags::UI, "ui"),
            ] {
                if tex.flags.contains(flag) {
                    flags.push(name);
                }
            }
            println!(
                "  flags: {}",
                if flags.is_empty() {
                    "none".into()
                } else {
                    flags.join(", ")
                }
            );
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
