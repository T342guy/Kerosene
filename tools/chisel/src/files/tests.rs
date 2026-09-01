// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "chisel-files-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn touch(path: &Path) {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).unwrap(); }
    std::fs::write(path, "").unwrap();
}

#[test]
fn a_bare_name_becomes_a_map_in_the_project() {
    let root = Path::new("/projects/game/content");
    assert_eq!(
        resolve("arena", root).unwrap(),
        PathBuf::from("/projects/game/content/maps/arena.keromap")
    );
}

#[test]
fn a_name_that_already_says_keromap_is_not_said_twice() {
    let root = Path::new("/projects/game/content");
    assert_eq!(
        resolve("arena.keromap", root).unwrap(),
        PathBuf::from("/projects/game/content/maps/arena.keromap")
    );
    assert_eq!(
        resolve("arena.KEROMAP", root).unwrap(),
        PathBuf::from("/projects/game/content/maps/arena.KEROMAP")
    );
}

#[test]
fn a_name_with_a_dot_in_it_keeps_the_dot() {
    // `arena.v2` is a name, not an extension to be replaced: saving it as
    // `arena.keromap` would write over a different map.
    let root = Path::new("/projects/game/content");
    assert_eq!(
        resolve("arena.v2", root).unwrap(),
        PathBuf::from("/projects/game/content/maps/arena.v2.keromap")
    );
}

#[test]
fn a_subdirectory_under_maps_is_allowed() {
    let root = Path::new("/projects/game/content");
    assert_eq!(
        resolve("chapter1/arena", root).unwrap(),
        PathBuf::from("/projects/game/content/maps/chapter1/arena.keromap")
    );
}

#[test]
fn an_absolute_path_is_taken_at_its_word() {
    let root = Path::new("/projects/game/content");
    assert_eq!(
        resolve("/elsewhere/mine", root).unwrap(),
        PathBuf::from("/elsewhere/mine.keromap")
    );
}

#[test]
fn an_empty_name_is_refused_rather_than_guessed_at() {
    assert!(resolve("   ", Path::new("/content")).is_err());
}

#[test]
fn a_name_cannot_climb_out_of_the_project() {
    let error = resolve("../../secret", Path::new("/content")).unwrap_err();
    assert!(error.contains(".."), "expected the error to name the problem: {error}");
}

#[test]
fn surrounding_space_is_not_part_of_a_name() {
    let root = Path::new("/content");
    assert_eq!(resolve("  arena \t", root).unwrap(), root.join("maps/arena.keromap"));
}

#[test]
fn a_map_in_the_project_is_labelled_by_its_place_in_it() {
    let root = Path::new("/projects/game/content");
    assert_eq!(label(&root.join("maps/arena.keromap"), root), "maps/arena.keromap");
}

#[test]
fn a_map_outside_the_project_is_labelled_in_full() {
    let root = Path::new("/projects/game/content");
    assert_eq!(label(Path::new("/elsewhere/mine.keromap"), root), "/elsewhere/mine.keromap");
}

#[test]
fn maps_are_listed_from_the_project_including_subdirectories() {
    let root = scratch("list");
    touch(&root.join("maps/arena.keromap"));
    touch(&root.join("maps/chapter1/start.keromap"));
    touch(&root.join("maps/arena.kerobsp"));
    touch(&root.join("maps/notes.txt"));

    let found = maps_in(&root);
    assert_eq!(found, vec![root.join("maps/arena.keromap"), root.join("maps/chapter1/start.keromap")]);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn listing_a_project_with_no_maps_directory_is_empty_rather_than_an_error() {
    let root = scratch("list-empty");
    assert!(maps_in(&root).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn renaming_takes_the_compiled_map_with_it() {
    let root = scratch("rename");
    let from = root.join("maps/old.keromap");
    touch(&from);
    touch(&root.join("maps/old.kerobsp"));
    touch(&root.join("maps/old.keroleak"));

    let to = root.join("maps/new.keromap");
    let moved = move_map(&from, &to).unwrap();

    assert!(to.is_file());
    assert!(!from.exists());
    assert!(root.join("maps/new.kerobsp").is_file(), "the compiled map follows its source");
    assert!(!root.join("maps/old.kerobsp").exists(), "and does not stay behind under the old name");
    assert_eq!(moved.len(), 2);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn renaming_a_map_nobody_has_compiled_moves_only_the_map() {
    let root = scratch("rename-uncompiled");
    let from = root.join("maps/old.keromap");
    touch(&from);

    let moved = move_map(&from, &root.join("maps/new.keromap")).unwrap();
    assert!(moved.is_empty());
    assert!(root.join("maps/new.keromap").is_file());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn renaming_into_a_directory_that_does_not_exist_yet_creates_it() {
    let root = scratch("rename-mkdir");
    let from = root.join("maps/old.keromap");
    touch(&from);

    move_map(&from, &root.join("maps/chapter2/new.keromap")).unwrap();
    assert!(root.join("maps/chapter2/new.keromap").is_file());

    let _ = std::fs::remove_dir_all(&root);
}
