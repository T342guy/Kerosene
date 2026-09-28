// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;
use kerosene_vfs::Archive;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest() -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("base/MANIFEST"))
        .expect("the manifest is beside the vault");
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[test]
fn the_base_content_mounts_and_holds_what_the_engine_asks_for_by_default() {
    let archive = Archive::from_static(BASE_VAULT, "base").expect("the base vault reads");
    for needed in [
        "ui/hud.kui",
        "ui/menus/pause.kui",
        "scripts/kerosene.ksnd",
        "materials/dev/grid.kmat_c",
        "materials/dev/grid.ktex",
        "materials/tools/nodraw.kmat_c",
        "maps/kerosene_room.kbsp",
        "models/props/cube.kmdl",
    ] {
        assert!(archive.contains(needed), "base content is missing {needed}");
    }
    let mut vfs = Vfs::new();
    mount(&mut vfs);
    assert!(vfs.exists("ui/hud.kui"));
}

#[test]
fn every_base_material_is_compiled_and_loads() {
    let archive = Archive::from_static(BASE_VAULT, "base").unwrap();
    let names: Vec<String> = archive
        .entries()
        .iter()
        .filter_map(|e| {
            let name = e.path.strip_prefix("materials/")?.strip_suffix(".kmat_c")?;
            Some(name.to_string())
        })
        .collect();
    assert!(names.len() > 10, "{names:?}");
    assert!(
        !archive.entries().iter().any(|e| e.path.ends_with(".kmat")),
        "material sources are not shipped"
    );
    let mut vfs = Vfs::new();
    mount(&mut vfs);
    for name in names {
        kerosene_asset::Material::load(&vfs, &name).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn every_manifest_line_is_in_the_base_vault() {
    let archive = Archive::from_static(BASE_VAULT, "base").unwrap();
    for line in manifest() {
        let found = match line.strip_suffix('/') {
            Some(_) => archive.entries().iter().any(|e| e.path.starts_with(&line)),
            None => archive.contains(&line),
        };
        assert!(
            found,
            "{line} is listed in base/MANIFEST but not packed: run scripts/build-content.sh"
        );
    }
}

/// The vault holds the files it was packed from. Hand-written files are
/// committed, so they are always compared; compiled ones only when a build
/// has made them, since a fresh clone has none.
#[test]
fn the_base_vault_matches_the_content_it_was_packed_from() {
    let content = repo().join("content");
    let archive = Archive::from_static(BASE_VAULT, "base").unwrap();
    let mut stale = Vec::new();
    for entry in archive.entries() {
        let disk = content.join(&entry.path);
        let Ok(bytes) = std::fs::read(&disk) else {
            continue;
        };
        let packed = archive.read(&entry.path).unwrap().unwrap();
        // A checkout that turned LF into CRLF (Windows without the
        // repository's .gitattributes) holds the same text.
        if packed != bytes && packed != strip_cr(&bytes) {
            stale.push(entry.path.clone());
        }
    }
    assert!(
        stale.is_empty(),
        "base.vault is out of date for {stale:?}: run scripts/build-content.sh"
    );
}

fn strip_cr(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|&b| b != b'\r').collect()
}
