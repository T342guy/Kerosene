// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Every file extension Kerosene reads or writes, in one table.
//!
//! They are short on purpose -- a `k` and what the file is -- so a directory
//! listing reads as names rather than as a column of `.kero` prefixes. The
//! crates below this one in the dependency graph (the asset, audio, script
//! and map crates) keep their own copy of the one they own, and a test in
//! the engine checks that every copy says what this table says.
//!
//! No extension is written into a file: each binary format starts with its
//! own four-byte magic, so a file's name and its contents are separate
//! promises.

/// Map source, as the editor writes it. KeyValues text.
pub const MAP: &str = "kmap";
/// Compiled map. Binary.
pub const BSP: &str = "kbsp";
/// Model. Binary.
pub const MODEL: &str = "kmdl";
/// Material. KeyValues text.
pub const MATERIAL: &str = "kmat";
/// Texture, with its mips. Binary.
pub const TEXTURE: &str = "ktex";
/// Project file. KeyValues text.
pub const PROJECT: &str = "kproj";
/// Game UI layout. XML.
pub const UI_LAYOUT: &str = "kui";
/// Game UI stylesheet. A CSS subset.
pub const UI_STYLE: &str = "kcss";
/// Script. Rhai.
pub const SCRIPT: &str = "kscr";
/// Entity class definitions. KeyValues text.
pub const CLASSES: &str = "kdef";
/// Compiled sound. Binary.
pub const AUDIO: &str = "kaud";
/// Soundscripts. KeyValues text.
pub const SOUNDSCRIPT: &str = "ksnd";
/// Saved game. JSON.
pub const SAVE: &str = "ksav";
/// Walkable-surface graph for navigation. Binary.
pub const WALK: &str = "kwalk";
/// Portal file a map compile leaves for the visibility pass. Text.
pub const PORTALS: &str = "kprt";
/// Leak trace a map compile leaves when the world is open. Text.
pub const LEAK: &str = "kleak";
/// Kiln's stamp saying how a map was last built. Text.
pub const BUILD_STAMP: &str = "kbuild";
/// Hand-written settings: the engine's, a texture set's, Timbre's.
/// KeyValues text.
pub const CONFIG: &str = "kcfg";
/// Content archive.
pub const ARCHIVE: &str = "vault";

/// The extensions of files the content build writes and can write again
/// from sources beside them: textures, sounds, and everything a map compile
/// leaves. What `kiln --clean` deletes, and what a project's `.gitignore`
/// leaves out. Models are not here: a `.kmdl` may have come from
/// somewhere with no source to rebuild it from.
pub const COMPILED: &[&str] = &[TEXTURE, AUDIO, BSP, PORTALS, WALK, LEAK, BUILD_STAMP];

/// What goes into an archive: the compiled formats and the loose data the
/// engine reads directly. Sources -- `.png`, `.obj`, `.wav`, `.kmap` -- are
/// left out: shipping them doubles the download to deliver files the engine
/// can read a smaller version of.
pub const PACKED: &[&str] = &[
    TEXTURE,
    MATERIAL,
    MODEL,
    BSP,
    WALK,
    SCRIPT,
    SOUNDSCRIPT,
    AUDIO,
    CLASSES,
    // The game UI: layouts and stylesheets are read as they are written, and
    // fonts a stylesheet names with `@font-face` are loaded as they are.
    UI_LAYOUT,
    UI_STYLE,
    "ttf",
    "otf",
];

/// Whether `path` ends in `ext`, ignoring case: `ARENA.KMAP` is a map on a
/// filesystem that does not care, and should be one here too.
pub fn is(path: &std::path::Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn every_extension_is_short_and_distinct() {
        let all = [
            MAP,
            BSP,
            MODEL,
            MATERIAL,
            TEXTURE,
            PROJECT,
            UI_LAYOUT,
            UI_STYLE,
            SCRIPT,
            CLASSES,
            AUDIO,
            SOUNDSCRIPT,
            SAVE,
            WALK,
            PORTALS,
            LEAK,
            BUILD_STAMP,
            CONFIG,
            ARCHIVE,
        ];
        let unique: std::collections::BTreeSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        assert!(all.iter().all(|e| e.len() <= 6), "{all:?}");
    }

    #[test]
    fn hand_written_files_are_never_cleaned() {
        for kept in [MAP, MATERIAL, MODEL, CONFIG, PROJECT, SCRIPT] {
            assert!(!COMPILED.contains(&kept), "{kept}");
        }
    }

    #[test]
    fn matching_ignores_case() {
        assert!(is(Path::new("maps/ARENA.KMAP"), MAP));
        assert!(is(Path::new("a.kmap"), MAP));
        assert!(!is(Path::new("a.kmap.bak"), MAP));
        assert!(!is(Path::new("kmap"), MAP));
    }
}
