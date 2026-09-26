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
        "ui/hud.keroui",
        "ui/menus/pause.keroui",
        "scripts/kerosene.kerosnd",
        "materials/dev/grid.keromat",
        "materials/dev/grid.kerotex",
        "materials/tools/nodraw.keromat",
        "maps/kero_start.kerobsp",
        "models/props/cube.keromdl",
    ] {
        assert!(archive.contains(needed), "base content is missing {needed}");
    }
    let mut vfs = Vfs::new();
    mount(&mut vfs);
    assert!(vfs.exists("ui/hud.keroui"));
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
        if packed != bytes {
            stale.push(entry.path.clone());
        }
    }
    assert!(
        stale.is_empty(),
        "base.vault is out of date for {stale:?}: run scripts/build-content.sh"
    );
}
