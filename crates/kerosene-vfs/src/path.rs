// SPDX-License-Identifier: LGPL-3.0-or-later
//! Virtual path normalisation.
//!
//! Asset paths come from map files, material references and tool arguments,
//! authored on whatever platform, so they arrive in every spelling:
//! `Materials\Dev\Grid`, `materials/dev/grid`, `./materials//dev/grid`. They
//! all have to resolve to one key, and none of them may escape the game tree.

/// Normalise a virtual path: lowercase, forward slashes, no redundant parts.
///
/// Returns `None` if the path tries to climb above the search root. That check
/// is not a nicety: map and material files are untrusted content that ships
/// between users, and `../../../../etc/passwd` in a `$basetexture` must not
/// resolve to anything.
pub fn normalize(path: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => {
                // Refuse rather than clamp: silently resolving to the root
                // would let a crafted path reach a sibling it should not see.
                out.pop()?;
            }
            p => out.push(p),
        }
    }
    if out.is_empty() { return None; }
    Some(out.join("/"))
}

/// The case-folded form, for looking a path up in an archive.
///
/// Archives store their entries folded, so a name is found however it was
/// typed -- which is the behaviour content wants, since a map written on
/// Windows routinely disagrees with the file on disk about capitals.
///
/// Directories cannot do the same by string alone: on Linux the filesystem is
/// case-sensitive, so folding a path is how `FINALSmusic.flac` becomes a file
/// that does not exist. [`crate::Vfs`] resolves those by looking, not by
/// lowercasing.
pub fn key(path: &str) -> Option<String> {
    normalize(path).map(|p| p.to_lowercase())
}

/// Find a file in `dir` whose path matches `relative` ignoring case.
///
/// Walked rather than folded, and only after the exact name has already
/// missed, so the cost falls on the lookup that was going to fail anyway.
/// Content paths are case-insensitive by contract -- an archive makes them so
/// for free -- and a loose tree has to agree, or a game works from a checkout
/// and breaks the moment it is packed.
pub fn find_ignoring_case(dir: &std::path::Path, relative: &str) -> Option<std::path::PathBuf> {
    let mut at = dir.to_path_buf();
    let mut parts = relative.split('/').peekable();

    while let Some(part) = parts.next() {
        let wanted = part.to_lowercase();
        let matched = std::fs::read_dir(&at).ok()?.flatten().find(|entry| {
            entry.file_name().to_string_lossy().to_lowercase() == wanted
        })?;
        at = matched.path();
        // Every part but the last has to be a directory to keep walking.
        if parts.peek().is_some() && !at.is_dir() {
            return None;
        }
    }
    at.is_file().then_some(at)
}

/// Lowercase extension without the dot, if any.
pub fn extension(path: &str) -> Option<String> {
    let file = path.rsplit('/').next()?;
    let (_, ext) = file.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_lowercase())
}

/// Directory portion of a virtual path, or `""` at the root.
pub fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Replace or add an extension.
pub fn with_extension(path: &str, ext: &str) -> String {
    let base = match path.rfind('.') {
        Some(i) if !path[i..].contains('/') => &path[..i],
        _ => path,
    };
    format!("{base}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_collapse_to_one_shape() {
        // Separators, leading slashes and `.` parts all go; capitals stay,
        // because a directory on Linux is case-sensitive and a folded path is
        // how `FINALSmusic.flac` becomes a file that does not exist.
        let want = Some("materials/dev/grid.keromat".to_string());
        for src in [
            r"materials\dev\grid.keromat",
            "materials/dev/grid.keromat",
            "./materials//dev/grid.keromat",
            "/materials/dev/grid.keromat",
        ] {
            assert_eq!(normalize(src), want, "{src}");
        }
    }

    #[test]
    fn normalize_keeps_case_and_key_folds_it() {
        // Two jobs, and conflating them is what broke lookups: an archive
        // stores folded keys and can be asked in any case, while a directory
        // has to be asked for the name the filesystem actually holds.
        assert_eq!(normalize("Sound/Ambient/Track.WAV").as_deref(), Some("Sound/Ambient/Track.WAV"));
        assert_eq!(key("Sound/Ambient/Track.WAV").as_deref(), Some("sound/ambient/track.wav"));
        assert_eq!(key(r"Materials\Dev\Grid.keromat").as_deref(), Some("materials/dev/grid.keromat"));
    }

    #[test]
    fn a_file_is_found_by_a_name_in_the_wrong_case() {
        // The bug this exists for: an asset named with capitals was invisible
        // to the engine on a case-sensitive filesystem, and the error named a
        // lowercased path nobody had written.
        let dir = std::env::temp_dir().join(format!(
            "kerosenevfs-case-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sound/Ambient")).unwrap();
        std::fs::write(dir.join("Sound/Ambient/FINALSmusic.flac"), b"x").unwrap();

        for spelling in [
            "Sound/Ambient/FINALSmusic.flac",
            "sound/ambient/finalsmusic.flac",
            "SOUND/AMBIENT/FINALSMUSIC.FLAC",
        ] {
            assert!(
                find_ignoring_case(&dir, spelling).is_some(),
                "{spelling} should have been found"
            );
        }
        assert!(find_ignoring_case(&dir, "sound/ambient/nothing.flac").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_in_the_path_is_not_mistaken_for_the_file() {
        let dir = std::env::temp_dir().join(format!(
            "kerosenevfs-casedir-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sound/Ambient")).unwrap();

        // The path exists as a directory, and a directory is not a file.
        assert!(find_ignoring_case(&dir, "sound/ambient").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interior_dotdot_resolves() {
        assert_eq!(normalize("materials/dev/../props/x.keromat").as_deref(), Some("materials/props/x.keromat"));
    }

    #[test]
    fn escaping_the_root_is_refused() {
        // The security case: a hostile material reference must not reach out.
        for bad in ["../secrets", "materials/../../etc/passwd", "..", "a/../.."] {
            assert_eq!(normalize(bad), None, "{bad} should be refused");
        }
    }

    #[test]
    fn empty_paths_have_no_key() {
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("///"), None);
    }

    #[test]
    fn extension_and_parent() {
        assert_eq!(extension("a/b/c.KEROMAT").as_deref(), Some("keromat"));
        assert_eq!(extension("a/b/noext"), None);
        assert_eq!(parent("a/b/c.keromat"), "a/b");
        assert_eq!(parent("c.keromat"), "");
        assert_eq!(with_extension("maps/kero_start.keromap", "kerobsp"), "maps/kero_start.kerobsp");
        assert_eq!(with_extension("maps/noext", "kerobsp"), "maps/noext.kerobsp");
    }
}
