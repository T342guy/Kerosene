// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Every kind of asset: what it is written as, what it compiles to, and
//! what compiles it.
//!
//! Source 2 keeps this table as data (`assettypes_common.txt`); Kerosene
//! keeps it as code, in one place, so the build, the packager and the
//! editor's asset browser read the same answer. A new asset type is a row
//! here and a compiler, not a hunt through the tools.

use kerosene_vfs::ext;

/// One kind of asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetType {
    /// What a person calls it.
    pub name: &'static str,
    /// The extensions of the files it is written as. Several when the
    /// compiler takes several formats in.
    pub sources: &'static [&'static str],
    /// The extension of the file the runtime loads.
    pub compiled: &'static str,
    /// The kind in the compiled file's container header, for a type that has
    /// moved into the container. `None` for one still in a format of its own.
    pub kind: Option<[u8; 4]>,
    /// The toolset stage that compiles it.
    pub compiler: &'static str,
}

pub const TEXTURE: AssetType = AssetType {
    name: "texture",
    sources: &["png", "jpg", "jpeg", "tga"],
    compiled: ext::TEXTURE,
    kind: None,
    compiler: "alchemy",
};

pub const MATERIAL: AssetType = AssetType {
    name: "material",
    sources: &[ext::MATERIAL],
    compiled: ext::MATERIAL_COMPILED,
    kind: Some(*b"KMAT"),
    compiler: "alchemy",
};

pub const MODEL: AssetType = AssetType {
    name: "model",
    sources: &["obj", "gltf", "glb"],
    compiled: ext::MODEL,
    kind: None,
    compiler: "forge",
};

pub const SOUND: AssetType = AssetType {
    name: "sound",
    sources: &["wav", "flac", "mp3"],
    compiled: ext::AUDIO,
    kind: None,
    compiler: "timbre",
};

pub const MAP: AssetType = AssetType {
    name: "map",
    sources: &[ext::MAP],
    compiled: ext::BSP,
    kind: None,
    compiler: "cleave",
};

/// Every asset type.
pub const ASSET_TYPES: &[AssetType] = &[TEXTURE, MATERIAL, MODEL, SOUND, MAP];

/// The type whose sources have this extension, ignoring case.
pub fn by_source(extension: &str) -> Option<&'static AssetType> {
    ASSET_TYPES
        .iter()
        .find(|t| t.sources.iter().any(|s| s.eq_ignore_ascii_case(extension)))
}

/// The type whose compiled files have this extension, ignoring case.
pub fn by_compiled(extension: &str) -> Option<&'static AssetType> {
    ASSET_TYPES
        .iter()
        .find(|t| t.compiled.eq_ignore_ascii_case(extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_extension_belongs_to_two_types() {
        let mut seen = std::collections::HashSet::new();
        for t in ASSET_TYPES {
            for e in t.sources.iter().chain([&t.compiled]) {
                assert!(seen.insert(*e), "{e} is claimed twice");
            }
        }
    }

    #[test]
    fn what_the_runtime_loads_is_what_the_packager_packs() {
        for t in ASSET_TYPES {
            assert!(
                ext::PACKED.contains(&t.compiled),
                "{} files ({}) are not packed",
                t.name,
                t.compiled
            );
            for s in t.sources {
                assert!(!ext::PACKED.contains(s), "{} source {s} is packed", t.name);
            }
        }
    }

    #[test]
    fn lookups_ignore_case() {
        assert_eq!(by_source("PNG"), Some(&TEXTURE));
        assert_eq!(by_compiled("KMAT_C"), Some(&MATERIAL));
        assert_eq!(by_source("kmat_c"), None);
    }
}
