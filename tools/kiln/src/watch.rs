// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Building again whenever a source changes: `kiln --watch`, `play --watch`.
//!
//! Polled rather than subscribed to: a look at every file's modification
//! time twice a second costs next to nothing on a content tree, works the
//! same on every platform and filesystem (network shares included), and
//! needs no crate. What the build itself writes -- compiled textures,
//! sounds, maps, the archive -- is left out of the picture, or every build
//! would set off the next.

use crate::{Report, Settings};
use kerosene_vfs::ext;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How often the tree is looked at.
const POLL: Duration = Duration::from_millis(500);

/// Every source file under a content tree and when it was last written.
pub type Snapshot = BTreeMap<PathBuf, SystemTime>;

/// Whether a file is something a build reads rather than writes.
fn is_source(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.') || n.ends_with(".tmp"));
    let written = ext::COMPILED
        .iter()
        .chain([ext::MODEL, ext::ARCHIVE].iter())
        .any(|e| ext::is(path, e));
    !hidden && !written
}

/// A picture of the tree's sources, to compare with the next one.
pub fn snapshot(content: &Path) -> Snapshot {
    fn walk(dir: &Path, out: &mut Snapshot) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                walk(&path, out);
            } else if kind.is_file()
                && is_source(&path)
                && let Ok(when) = entry.metadata().and_then(|m| m.modified())
            {
                out.insert(path, when);
            }
        }
    }
    let mut out = Snapshot::new();
    walk(content, &mut out);
    out
}

/// What changed between two pictures: added, edited and deleted alike.
pub fn changed(before: &Snapshot, after: &Snapshot) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = after
        .iter()
        .filter(|(path, when)| before.get(*path) != Some(when))
        .map(|(path, _)| path.clone())
        .collect();
    out.extend(
        before
            .keys()
            .filter(|path| !after.contains_key(*path))
            .cloned(),
    );
    out
}

/// Build again each time a source changes, until `keep_going` says stop.
///
/// A burst of saves -- an editor writing a map and then its backup -- is
/// waited out, so it makes one build rather than two. Each build's outcome
/// goes to `built`; a failed one does not end the watch, since the next save
/// is usually the fix.
pub fn watch(
    settings: &Settings,
    mut keep_going: impl FnMut() -> bool,
    mut built: impl FnMut(anyhow::Result<Report>),
) {
    let mut last = snapshot(&settings.content);
    while keep_going() {
        std::thread::sleep(POLL);
        let mut now = snapshot(&settings.content);
        if changed(&last, &now).is_empty() {
            continue;
        }
        // Settle: until two looks in a row agree.
        loop {
            std::thread::sleep(POLL);
            let again = snapshot(&settings.content);
            if again == now {
                break;
            }
            now = again;
        }
        let names: Vec<String> = changed(&last, &now)
            .iter()
            .map(|p| {
                p.strip_prefix(&settings.content)
                    .unwrap_or(p)
                    .display()
                    .to_string()
            })
            .collect();
        println!();
        println!("==> changed: {}", names.join(", "));
        built(crate::build(settings));
        // Taken after the build, so anything it rewrote is not a change.
        last = snapshot(&settings.content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_watched_and_what_the_build_writes_is_not() {
        let dir = std::env::temp_dir().join(format!("kiln-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for file in [
            "maps/a.kmap",
            "maps/a.kbsp",
            "maps/a.kbuild",
            "art/wall.png",
            "materials/wall.kmat",
            "materials/wall.ktex",
            "models/crate.kmdl",
            "sound/hum.wav",
            "sound/hum.kaud",
            "content.vault",
            "maps/.a.kmap.123.tmp",
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
        }
        let before = snapshot(&dir);
        let seen: Vec<String> = before
            .keys()
            .map(|p| {
                p.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            seen,
            [
                "art/wall.png",
                "maps/a.kmap",
                "materials/wall.kmat",
                "sound/hum.wav"
            ]
        );

        std::fs::remove_file(dir.join("sound/hum.wav")).unwrap();
        std::fs::write(dir.join("maps/b.kmap"), "y").unwrap();
        let after = snapshot(&dir);
        let mut what = changed(&before, &after);
        what.sort();
        assert_eq!(what, [dir.join("maps/b.kmap"), dir.join("sound/hum.wav")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
