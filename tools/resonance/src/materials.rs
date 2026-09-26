// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What each surface in the map does to sound.
//!
//! A ray that hits a wall needs to know how much of it comes back, per band,
//! and the wall is a texinfo index. This is the table from one to the other,
//! built once from the map's material names and the `.keromat` files behind
//! them, so the hot loop is an array lookup.

use kerosene_asset::{AcousticProfile, Material, SurfaceProperty, material_path};
use kerosene_bsp::Bsp;
use std::path::Path;

/// Absorption per texinfo index.
pub struct Absorption {
    per_texinfo: Vec<[f32; 4]>,
    fallback: [f32; 4],
    /// Material names that could not be read, for the report.
    pub missing: Vec<String>,
}

impl Absorption {
    /// Build the table by asking `lookup` for each material the map names.
    /// A material `lookup` cannot find gets the default profile and is
    /// listed in `missing`.
    pub fn build(bsp: &Bsp, mut lookup: impl FnMut(&str) -> Option<AcousticProfile>) -> Absorption {
        let fallback = AcousticProfile::of(&SurfaceProperty::Default).0;
        let mut by_texdata: Vec<Option<[f32; 4]>> = Vec::with_capacity(bsp.texdata.len());
        let mut missing = Vec::new();
        for i in 0..bsp.texdata.len() {
            let name = bsp.texdata_name(i);
            let profile = if name.is_empty() { None } else { lookup(name) };
            if profile.is_none() && !name.is_empty() && !missing.iter().any(|m| m == name) {
                missing.push(name.to_string());
            }
            by_texdata.push(profile.map(|p| [p.band(0), p.band(1), p.band(2), p.band(3)]));
        }
        let per_texinfo = bsp
            .texinfo
            .iter()
            .map(|ti| {
                by_texdata
                    .get(ti.texdata as usize)
                    .copied()
                    .flatten()
                    .unwrap_or(fallback)
            })
            .collect();
        missing.sort();
        Absorption {
            per_texinfo,
            fallback,
            missing,
        }
    }

    /// The table for a map whose materials are all in one content tree.
    ///
    /// Reads each `.keromat` through the VFS the way the engine will, so a
    /// material in a `.vault` counts as much as one on disk.
    pub fn from_content(bsp: &Bsp, content: &Path) -> Absorption {
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
        Absorption::build(bsp, |name| {
            let text = vfs.read_string(&material_path(name)).ok()?;
            Material::parse(&text).ok().map(|m| m.acoustics())
        })
    }

    /// Every surface the default: for a map with no content tree, or a test.
    pub fn uniform(bsp: &Bsp, profile: AcousticProfile) -> Absorption {
        Absorption::build(bsp, |_| Some(profile))
    }

    /// Absorption per band of the surface a trace hit.
    #[inline]
    pub fn of(&self, texinfo: i32) -> [f32; 4] {
        usize::try_from(texinfo)
            .ok()
            .and_then(|i| self.per_texinfo.get(i))
            .copied()
            .unwrap_or(self.fallback)
    }
}
