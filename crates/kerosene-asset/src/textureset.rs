// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Texture sets -- a surface's maps, gathered in one folder.
//!
//! A single PNG is not a surface. A surface is a colour, the bumps in it, how
//! rough it is, what it glows with and where it self-shadows -- five images
//! that belong together and are useless apart. The flat `art/` tree cannot say
//! that: it has one file per texture and one naming convention (`_normal`,
//! `_n`) bolted on to guess at the rest, which works only for the one map
//! anybody thought to name.
//!
//! So a texture is a *folder*:
//!
//! ```text
//! content/textures/Walltextures/variant1/
//!     basecolor.png
//!     normal.png
//!     roughness.png
//!     texture.kconfig      (optional)
//! ```
//!
//! and it names itself from where it sits -- `Walltextures/variant1` becomes
//! `Walltextures_variant1`. The path is already unique and already describes
//! the thing, so making somebody restate it in a file would be asking for a
//! second answer that can disagree with the first.
//!
//! [`CONFIG_FILENAME`] is how you overrule any of that. Every key in it is
//! optional, because the point of the folder convention is that the common
//! case needs no file at all; the config exists for the texture that wants a
//! different name, an image the discovery rules would not find, or a surface
//! property no amount of looking at pixels could work out.

use crate::{Shader, TextureFlags};
use kerosene_kv::KeyValues;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file that overrides a set's inferred definition.
pub const CONFIG_FILENAME: &str = "texture.kconfig";

/// Image extensions a set will pick up, matching what Alchemy can compile.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "tga"];

/// One of the maps a surface is made of.
///
/// A closed set, like [`Shader`], and for the same reason: every one is a real
/// binding in the renderer, so a sixth kind is a code change rather than a
/// string somebody can invent in a config and have silently ignored.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum MapKind {
    /// The colour. The only one a set cannot do without.
    Base,
    /// Tangent-space normals.
    Normal,
    /// Microfacet roughness, 0 mirror to 1 matte. Read from red.
    Roughness,
    /// What the surface emits on its own, independent of any light reaching
    /// it. The "light map" of an authored PBR set -- not to be confused with
    /// the lightmap atlas Radiance bakes, which is per-face and per-map.
    Emissive,
    /// Baked ambient occlusion, darkening what the surface shadows itself.
    Ao,
}

impl MapKind {
    /// Every kind, in the order a material lists them.
    pub const ALL: [MapKind; 5] = [
        MapKind::Base,
        MapKind::Normal,
        MapKind::Roughness,
        MapKind::Emissive,
        MapKind::Ao,
    ];

    /// What this kind appends to the set's name to make its texture name.
    ///
    /// The base map has none: a set called `brick` compiles to a texture
    /// called `brick`, so geometry that referred to it before it grew a normal
    /// map still refers to it now.
    pub fn suffix(self) -> &'static str {
        match self {
            MapKind::Base => "",
            MapKind::Normal => "_normal",
            MapKind::Roughness => "_rough",
            MapKind::Emissive => "_emissive",
            MapKind::Ao => "_ao",
        }
    }

    /// The `texture.kconfig` key that names this map explicitly.
    pub fn key(self) -> &'static str {
        match self {
            MapKind::Base => "basecolor",
            MapKind::Normal => "normal",
            MapKind::Roughness => "roughness",
            MapKind::Emissive => "emissive",
            MapKind::Ao => "ao",
        }
    }

    /// The material parameter this map is wired into.
    ///
    /// `$selfillummask` rather than `$emissive` because that is the key Source
    /// uses and the one [`crate::Material::referenced_textures`] already packs.
    pub fn material_param(self) -> &'static str {
        match self {
            MapKind::Base => "$basetexture",
            MapKind::Normal => "$bumpmap",
            MapKind::Roughness => "$roughness",
            MapKind::Emissive => "$selfillummask",
            MapKind::Ao => "$ao",
        }
    }

    /// File stems that mean this kind, lowercase.
    ///
    /// Generous on purpose: these names come out of whatever tool the artist
    /// baked them in, and rejecting `albedo.png` because the engine wanted
    /// `basecolor.png` would be a rule that exists only to be tripped over.
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            MapKind::Base => &["basecolor", "base_color", "albedo", "diffuse", "color", "base", "col", "d"],
            MapKind::Normal => &["normal", "normalmap", "normal_map", "nrm", "bump", "n"],
            MapKind::Roughness => &["roughness", "rough", "rgh", "r"],
            MapKind::Emissive => &["emissive", "emission", "selfillum", "self_illum", "glow", "light", "e"],
            MapKind::Ao => &["ao", "occlusion", "ambientocclusion", "ambient_occlusion", "ambient"],
        }
    }

    /// Which kind a file stem names, if any.
    fn from_stem(stem: &str) -> Option<MapKind> {
        let stem = stem.to_lowercase();
        MapKind::ALL
            .into_iter()
            .find(|kind| kind.aliases().contains(&stem.as_str()))
    }

    /// How this map is compiled: whether it is colour or measurement.
    ///
    /// Normals and roughness/AO are both kept out of sRGB, but they are kept
    /// out for different reasons and the renderer treats them differently, so
    /// they carry different flags.
    pub fn flags(self) -> TextureFlags {
        match self {
            MapKind::Base | MapKind::Emissive => TextureFlags::NONE,
            MapKind::Normal => TextureFlags::NORMAL_MAP,
            MapKind::Roughness | MapKind::Ao => TextureFlags::DATA,
        }
    }
}

/// A surface's maps and how to shade them, gathered from one folder.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureSet {
    /// What geometry calls this set: `Walltextures_variant1`.
    pub name: String,
    /// The folder it was found in.
    pub directory: PathBuf,
    /// The source image for each map that exists, absolute or as given.
    pub maps: BTreeMap<MapKind, PathBuf>,
    pub shader: Shader,
    pub surface_prop: String,
    pub clamp: bool,
    pub point: bool,
    pub translucent: bool,
}

impl TextureSet {
    /// The name a folder gives itself: its path under `root`, joined with `_`.
    ///
    /// `textures/Walltextures/variant1` under `textures` is
    /// `Walltextures_variant1`. A folder directly in the root keeps its own
    /// name. Separators become `_` rather than staying `/` because the result
    /// is a texture name, and texture names are flat -- `material_path` turns
    /// them straight into a filename.
    pub fn name_from_path(dir: &Path, root: &Path) -> String {
        let relative = dir.strip_prefix(root).unwrap_or(dir);
        let joined = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("_");
        if joined.is_empty() {
            // The root itself is a set: name it after the root's own folder,
            // so it is called something rather than nothing.
            root.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "texture".to_string())
        } else {
            joined
        }
    }

    /// The texture name for one of this set's maps.
    pub fn texture_name(&self, kind: MapKind) -> String {
        format!("{}{}", self.name, kind.suffix())
    }

    /// Read a folder as a texture set, or decide it is not one.
    ///
    /// `None` means "no recognised map here", which is how a walk tells an
    /// intermediate directory (`textures/Walltextures/`) from a set. It is not
    /// an error: most directories in a tree are not texture sets.
    pub fn discover(dir: &Path, root: &Path) -> Option<TextureSet> {
        let config = dir.join(CONFIG_FILENAME);
        let config = match std::fs::read_to_string(&config) {
            Ok(text) => match KeyValues::parse(&text) {
                Ok(kv) => Some(kv),
                Err(e) => {
                    // A broken config is a warning, not a refusal: the folder
                    // still has images in it, and losing a whole texture to a
                    // stray brace is a poor trade.
                    log::warn!("{}: {e}; using the folder's own layout", config.display());
                    None
                }
            },
            Err(_) => None,
        };
        let block = config
            .as_ref()
            .map(|kv| kv.block("texture").unwrap_or(kv));

        let mut maps = BTreeMap::new();

        // Discovery first, so an explicit key overrides rather than competes.
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut found: Vec<(MapKind, PathBuf)> = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() || !has_image_extension(&path) {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if let Some(kind) = MapKind::from_stem(stem) {
                    found.push((kind, path));
                }
            }
            // Sorted so two files that both claim a kind resolve the same way
            // on every machine rather than by directory order.
            found.sort();
            for (kind, path) in found {
                maps.entry(kind).or_insert(path);
            }
        }

        if let Some(block) = block {
            for kind in MapKind::ALL {
                let Some(named) = block.get(kind.key()).map(str::trim).filter(|v| !v.is_empty())
                else {
                    continue;
                };
                let path = dir.join(named);
                if path.is_file() {
                    maps.insert(kind, path);
                } else {
                    log::warn!(
                        "{}: {} names {named}, which is not in the folder",
                        dir.join(CONFIG_FILENAME).display(),
                        kind.key()
                    );
                }
            }
        }

        // A set with no base colour is not a set. Bumps and roughness modulate
        // a colour; on their own they have nothing to modulate, and treating
        // the folder as a texture would produce a material that draws the
        // missing-texture checkerboard.
        if !maps.contains_key(&MapKind::Base) {
            return None;
        }

        let block = config.as_ref().map(|kv| kv.block("texture").unwrap_or(kv));
        let name = block
            .and_then(|b| b.get("name"))
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| TextureSet::name_from_path(dir, root));

        let shader = block
            .and_then(|b| b.get("shader"))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                Shader::from_name(s).unwrap_or_else(|| {
                    log::warn!("unknown shader '{s}' in {}, using lit", dir.display());
                    Shader::Lit
                })
            })
            .unwrap_or(Shader::Lit);

        Some(TextureSet {
            name,
            directory: dir.to_path_buf(),
            maps,
            shader,
            surface_prop: block
                .and_then(|b| b.get("surfaceprop"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("default")
                .to_string(),
            clamp: flag(block, "clamp"),
            point: flag(block, "point"),
            translucent: flag(block, "translucent"),
        })
    }

    /// The flags one of this set's maps compiles with, folding in the
    /// set-wide sampling choices.
    pub fn flags_for(&self, kind: MapKind) -> TextureFlags {
        let mut flags = kind.flags();
        if self.clamp {
            flags = flags | TextureFlags::CLAMP;
        }
        if self.point {
            flags = flags | TextureFlags::POINT_SAMPLE;
        }
        flags
    }

    /// The material this set describes, ready to write.
    ///
    /// Only the maps that exist are wired up: a parameter naming a texture
    /// that was never compiled is a load failure waiting to happen.
    pub fn to_material(&self) -> crate::Material {
        let mut material = crate::Material::new(self.shader);
        for kind in MapKind::ALL {
            if self.maps.contains_key(&kind) {
                material.set(kind.material_param(), self.texture_name(kind));
            }
        }
        material.set("$surfaceprop", self.surface_prop.clone());
        if self.translucent {
            material.set("$translucent", "1");
        }
        material
    }

    /// A starting `texture.kconfig` for this set, as a person would write it.
    ///
    /// Written by `alchemy new-texture` so a fresh set comes with the file
    /// that documents what it can say, rather than with nothing and a
    /// reference to look up.
    pub fn to_config(&self) -> String {
        let mut kv = KeyValues::new("texture");
        kv.push("name", self.name.clone());
        for kind in MapKind::ALL {
            if let Some(path) = self.maps.get(&kind) {
                let file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                kv.push(kind.key(), file);
            }
        }
        kv.push("shader", self.shader.name());
        kv.push("surfaceprop", self.surface_prop.clone());
        format!(
            "// A Kerosene texture set. Every key is optional: delete one and\n\
             // the folder's own layout decides instead.\n\
             {}\n",
            kv.to_text()
        )
    }
}

fn flag(block: Option<&KeyValues>, key: &str) -> bool {
    block.is_some_and(|b| b.get_or(key, false))
}

/// Whether a path is an image Alchemy can compile.
pub fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// Every texture set under `root`, depth first and sorted.
///
/// Walks *through* a set's folder as well as into it, so a set that contains
/// variants as subfolders works: only the folders that actually hold a base
/// colour become sets.
pub fn walk(root: &Path) -> Vec<TextureSet> {
    let mut out = Vec::new();
    collect(root, root, &mut out);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<TextureSet>) {
    if let Some(set) = TextureSet::discover(dir, root) {
        out.push(set);
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for child in dirs {
        collect(root, &child, out);
    }
}

#[cfg(test)]
mod tests;
