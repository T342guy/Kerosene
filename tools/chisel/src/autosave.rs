// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Autosave: the map as it stands, written beside the map every minute.
//!
//! The copy is `arena.kmap~`, never `arena.kmap`: the map is only ever
//! written by a save the designer asked for, so a crash or a bad afternoon
//! cannot overwrite the last good version with a worse one. The `~` also
//! keeps the copy out of every scan for maps, which look for `.kmap`. A save
//! removes it, and opening a map that has a newer one asks whether to
//! recover it.

use kerosene_map::Map;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Seconds between autosaves of a map that has changed.
pub const INTERVAL: f64 = 60.0;

/// Where the autosave of a map lives. A map with no file yet gets
/// `untitled.kmap~` in the project's `maps/`.
pub fn path_for(map: Option<&Path>, content_root: &Path) -> PathBuf {
    let base = map
        .map(Path::to_path_buf)
        .unwrap_or_else(|| content_root.join("maps").join("untitled.kmap"));
    let mut name = base.file_name().unwrap_or_default().to_os_string();
    name.push("~");
    base.with_file_name(name)
}

/// Write the map to its autosave file, atomically.
pub fn write(map: &Map, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    kerosene_vfs::write_atomic(path, map.to_text().as_bytes())
}

/// Delete an autosave, if there is one.
pub fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Whether the autosave holds work the map itself does not: it exists, and
/// was written after the map was (or the map is not there at all).
pub fn is_newer(autosave: &Path, map: Option<&Path>) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let Some(auto) = modified(autosave) else {
        return false;
    };
    match map.and_then(modified) {
        Some(saved) => auto > saved,
        None => true,
    }
}

/// When a file was written, for the recovery prompt to say so.
pub fn age_label(path: &Path) -> String {
    let secs = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs());
    match secs {
        None => "earlier".into(),
        Some(s) if s < 90 => "moments ago".into(),
        Some(s) if s < 3600 => format!("{} minutes ago", s / 60),
        Some(s) if s < 86400 => format!("{} hours ago", s / 3600),
        Some(s) => format!("{} days ago", s / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_sits_beside_the_map_under_another_extension() {
        let p = path_for(Some(Path::new("/p/maps/arena.kmap")), Path::new("/p"));
        assert_eq!(p, Path::new("/p/maps/arena.kmap~"));
        assert_ne!(p.extension().unwrap(), "kmap");
    }

    #[test]
    fn a_map_with_no_file_gets_a_name_in_the_project() {
        let p = path_for(None, Path::new("/p"));
        assert_eq!(p, Path::new("/p/maps/untitled.kmap~"));
    }

    #[test]
    fn write_then_remove() {
        let dir = std::env::temp_dir().join(format!("chisel-autosave-{}", std::process::id()));
        let path = dir.join("m.kmap~");
        write(&Map::new(), &path).unwrap();
        assert!(path.exists());
        assert!(
            is_newer(&path, Some(&dir.join("m.kmap"))),
            "no map: the copy is all there is"
        );
        remove(&path);
        assert!(!path.exists());
        assert!(!is_newer(&path, None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
