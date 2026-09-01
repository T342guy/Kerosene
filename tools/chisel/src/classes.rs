// SPDX-License-Identifier: MPL-2.0
//! Loading the game's entity class definitions.
//!
//! The shipped definitions are compiled into the game crate
//! (`kerosene_game::schema::BUILTIN`) and parsed here first, so Chisel knows
//! that a `func_door` has a `speed` and answers to `Open` even when the
//! content tree has no `.kerodef` file in it. On-disk `.kerodef` files are
//! still read and merged *over* the built-in set, exactly as Hammer reads an
//! FGD, so a mod can override a class by dropping its own file beside the
//! game's -- and a tree with no such file loses nothing.
//!
//! Without a schema at all the inspector can only show the keys an entity
//! already carries, which for a freshly placed entity is none of them. The
//! built-in set makes that state unreachable for the shipped game; [`load`]
//! still reports what it found.

use std::path::{Path, PathBuf};
use kerosene_entity::Schema;

/// The extension a class definition file uses.
pub const EXTENSION: &str = "kerodef";

/// What a scan of the content tree turned up.
pub struct Loaded {
    pub schema: Schema,
    /// Files that parsed, in the order they were merged.
    pub files: Vec<PathBuf>,
    /// Files that did not, with the reason. Shown rather than swallowed: a
    /// schema that silently failed to load looks exactly like a game with no
    /// entity properties.
    pub errors: Vec<String>,
}

impl Loaded {
    /// A one-line summary for the status bar.
    pub fn summary(&self) -> String {
        if !self.errors.is_empty() {
            return format!("entity definitions: {}", self.errors.join("; "));
        }
        match self.files.len() {
            0 => format!("{} entity classes (built in)", self.schema.len()),
            1 => format!(
                "{} entity classes from {} plus the built-in set",
                self.schema.len(),
                display(&self.files[0])
            ),
            n => format!("{} entity classes from {n} files plus the built-in set", self.schema.len()),
        }
    }
}

fn display(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Load the built-in schema, then every `.kerodef` under a content root.
///
/// The built-in set comes first so the editor is never without entity
/// definitions. Files found on disk are merged in sorted path order and a
/// later definition of a class replaces an earlier one, so a mod can drop its
/// own file in beside the game's and override a class without editing it.
pub fn load(content_root: &Path) -> Loaded {
    let schema = match Schema::parse(kerosene_game::schema::BUILTIN) {
        Ok(schema) => schema,
        Err(e) => {
            return Loaded {
                schema: Schema::default(),
                files: Vec::new(),
                errors: vec![format!("built-in schema: {e}")],
            };
        }
    };

    let mut files = Vec::new();
    collect(content_root, &mut files);
    files.sort();

    let mut loaded = Loaded { schema, files: Vec::new(), errors: Vec::new() };
    for path in files {
        match std::fs::read_to_string(&path) {
            Ok(text) => match Schema::parse(&text) {
                Ok(schema) => {
                    loaded.schema.merge(schema);
                    loaded.files.push(path);
                }
                Err(e) => loaded.errors.push(format!("{}: {e}", display(&path))),
            },
            Err(e) => loaded.errors.push(format!("{}: {e}", display(&path))),
        }
    }
    loaded
}

/// Recurse a content tree, but not far. Definition files live near the top;
/// walking the whole of `materials/` looking for them is wasted work.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    const MAX_DEPTH: usize = 2;
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth > 0 { walk(&path, depth - 1, out); }
            } else if path.extension().and_then(|e| e.to_str()) == Some(EXTENSION) {
                out.push(path);
            }
        }
    }
    walk(dir, MAX_DEPTH, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chisel-classes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_shipped_definitions_load() {
        // The shipped definitions are compiled into the game crate, so they
        // load whether or not a `.kerodef` mirror file is present on disk.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let loaded = load(&root);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let door = loaded.schema.get("func_door").expect("the sample game has doors");
        assert!(door.key("speed").is_some());
        assert!(door.has_input("Open"));
        assert!(door.has_output("OnFullyOpen"));
        // The universal inputs reach every class through the shared base.
        assert!(door.has_input("Kill"));
    }

    #[test]
    fn the_built_in_definitions_cover_an_empty_tree() {
        // The bug this module exists to prevent: an editor pointed at a tree
        // with no `.kerodef` file must still know what a `func_door` is.
        let dir = scratch("empty");
        let loaded = load(&dir);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        assert!(loaded.files.is_empty(), "no files to read: {:?}", loaded.files);
        assert!(!loaded.schema.is_empty(), "the built-in schema is always present");
        assert!(loaded.schema.get("func_door").is_some());
        assert!(loaded.schema.get("light").is_some());
        assert!(loaded.summary().contains("built in"), "{}", loaded.summary());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_broken_file_names_itself() {
        let dir = scratch("broken");
        std::fs::write(dir.join("bad.kerodef"), r#"class { "name" "c" "base" "Nope" }"#).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.errors.len(), 1);
        assert!(loaded.errors[0].starts_with("bad.kerodef:"), "{:?}", loaded.errors);
        assert!(loaded.summary().contains("bad.kerodef"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_later_file_overrides_an_earlier_one() {
        let dir = scratch("override");
        std::fs::write(dir.join("a-game.kerodef"), r#"class { "name" "func_x" "help" "first" }"#).unwrap();
        std::fs::write(dir.join("b-mod.kerodef"), r#"class { "name" "func_x" "help" "second" }"#).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.files.len(), 2);
        assert_eq!(loaded.schema.get("func_x").unwrap().help, "second");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn definitions_in_a_subdirectory_are_found() {
        let dir = scratch("nested");
        std::fs::create_dir_all(dir.join("cfg")).unwrap();
        std::fs::write(dir.join("cfg/game.kerodef"), r#"class { "name" "func_y" }"#).unwrap();
        let loaded = load(&dir);
        assert!(loaded.schema.get("func_y").is_some(), "{:?}", loaded.errors);
        std::fs::remove_dir_all(&dir).ok();
    }
}
