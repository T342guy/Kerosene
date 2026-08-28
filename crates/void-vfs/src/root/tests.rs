// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;

/// A scratch directory, cleaned up by the caller.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "voidroot-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A directory laid out the way a project is.
fn project(at: &Path) {
    std::fs::create_dir_all(at.join("maps")).unwrap();
    std::fs::create_dir_all(at.join("materials")).unwrap();
    std::fs::write(at.join(MARKER), "// classes go here").unwrap();
}

#[test]
fn the_definitions_file_marks_a_root() {
    let dir = scratch("marker");
    std::fs::write(dir.join(MARKER), "").unwrap();
    assert!(is_content_root(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn maps_and_materials_together_mark_one_too() {
    // A project that has not written its own class definitions yet is still a
    // project.
    let dir = scratch("dirs");
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    assert!(!is_content_root(&dir), "maps alone is not enough");
    std::fs::create_dir_all(dir.join("materials")).unwrap();
    assert!(is_content_root(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_directory_is_not_a_root() {
    // Asserted against `climb` rather than `find`, because `find` goes on to
    // try the working directory -- and in a test binary that is a real
    // project, so it would rightly succeed and prove nothing.
    let dir = scratch("empty");
    assert!(!is_content_root(&dir));
    assert_eq!(climb(&dir), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_explicit_root_wins_even_when_it_looks_wrong() {
    // Being overruled by a guess is worse than being told the answer is
    // empty: if someone names a directory, that is the directory.
    let dir = scratch("explicit");
    project(&dir.join("real"));
    let found = find(Some(&dir.join("elsewhere")), Some(&dir.join("real/maps/x.voidmap")))
        .expect("an explicit path is always taken");
    assert_eq!(found.root, dir.join("elsewhere"));
    assert!(found.why.contains("--content"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_finds_the_tree_it_lives_in() {
    // This is the case that was broken: opening a map from anywhere should
    // find that map's content, not the content of wherever a shell was.
    let dir = scratch("beside");
    project(&dir);
    let map = dir.join("maps/level.voidmap");
    std::fs::write(&map, "").unwrap();

    let found = find(None, Some(&map)).expect("found from the map");
    assert_eq!(found.root, dir);
    assert!(found.why.contains("next to the map"), "{}", found.why);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_in_a_repository_finds_the_content_directory() {
    // The layout this repository has: `<repo>/content/maps/x.voidmap`, with
    // the root one level in rather than at the top.
    let dir = scratch("repo");
    project(&dir.join("content"));
    std::fs::create_dir_all(dir.join("crates")).unwrap();
    let map = dir.join("content/maps/level.voidmap");
    std::fs::write(&map, "").unwrap();

    assert_eq!(find(None, Some(&map)).unwrap().root, dir.join("content"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn it_climbs_out_of_a_subdirectory() {
    // A map filed under `maps/chapter1/`, or a binary run from `target/debug`.
    let dir = scratch("climb");
    project(&dir);
    let deep = dir.join("maps/chapter1/act2");
    std::fs::create_dir_all(&deep).unwrap();
    let map = deep.join("level.voidmap");
    std::fs::write(&map, "").unwrap();

    assert_eq!(find(None, Some(&map)).unwrap().root, dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn it_gives_up_rather_than_climbing_to_the_root_of_the_disk() {
    // Finding some unrelated directory six levels up and calling it content
    // would be worse than finding nothing. A project at the top of this tree
    // proves the limit bites rather than that there was nothing to find.
    let dir = scratch("nothing");
    project(&dir);
    let deep = dir.join("a/b/c/d/e/f/g/h");
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(climb(&deep), None, "climbed further than {MAX_CLIMB} levels");
    // ...and from just inside the limit it does find it.
    assert_eq!(climb(&dir.join("a/b/c")), Some(dir.clone()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_no_map_it_still_looks_around() {
    // Starting Chisel with no arguments, from a project directory.
    let dir = scratch("cwd");
    project(&dir);
    // `find` consults the working directory, which a test cannot change
    // safely in parallel -- so check the piece it uses instead.
    assert_eq!(climb(&dir.join("maps")), Some(dir.clone()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_it_found_is_something_a_person_can_read() {
    let dir = scratch("describe");
    project(&dir);
    let text = describe(&find(None, Some(&dir.join("maps/x.voidmap"))));
    assert!(text.contains(&dir.display().to_string()), "{text}");
    assert!(text.contains("next to the map"), "{text}");

    let nothing = describe(&None);
    assert!(nothing.contains("--content"), "it should say how to fix it: {nothing}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_repositorys_own_content_tree_is_found_from_a_map_in_it() {
    // The real thing, not a fixture.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let map = repo.join("content/maps/void_start.voidmap");
    if !map.exists() { return }

    let found = find(None, Some(&map)).expect("the sample map's content is findable");
    assert!(found.root.join(MARKER).is_file(), "{} has no {MARKER}", found.root.display());
}

// ---- a project file beats every guess ------------------------------------

#[test]
fn a_project_file_names_the_content_and_the_search_stops_guessing() {
    let dir = scratch("project-wins");
    // A directory that *looks* like a content root, and a project file that
    // says the content is somewhere else entirely. The project wins.
    project(&dir);
    std::fs::create_dir_all(dir.join("elsewhere/maps")).unwrap();
    std::fs::write(dir.join("game.voidproj"), "project { \"content\" \"elsewhere\" }").unwrap();

    let found = find(None, Some(&dir.join("maps/x.voidmap"))).unwrap();
    assert_eq!(found.root, dir.join("elsewhere"));
    assert_eq!(found.project.as_ref().map(|p| p.path.clone()), Some(dir.join("game.voidproj")));
    assert!(found.why.contains("project"), "{}", found.why);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_further_up_beats_a_content_tree_closer_down() {
    // The whole reason to write one: a stated answer that loses to an
    // inferred one is not an answer.
    let dir = scratch("project-depth");
    std::fs::write(dir.join("game.voidproj"), "project { \"content\" \"real\" }").unwrap();
    std::fs::create_dir_all(dir.join("real/maps")).unwrap();

    let deep = dir.join("a/b/c");
    project(&deep);

    let found = find(None, Some(&deep.join("maps/x.voidmap"))).unwrap();
    assert_eq!(found.root, dir.join("real"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_explicit_content_directory_still_beats_a_project_file() {
    let dir = scratch("explicit-wins");
    std::fs::write(dir.join("game.voidproj"), "project { \"content\" \"elsewhere\" }").unwrap();

    let asked = dir.join("what/i/asked/for");
    let found = find(Some(&asked), Some(&dir.join("maps/x.voidmap"))).unwrap();
    assert_eq!(found.root, asked);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_file_that_will_not_parse_falls_through_to_the_search() {
    // A broken project file should cost you a warning, not an editor.
    let dir = scratch("project-broken");
    project(&dir);
    std::fs::write(dir.join("game.voidproj"), "project { \"content\"").unwrap();

    let found = find(None, Some(&dir.join("maps/x.voidmap"))).unwrap();
    assert_eq!(found.root, dir);
    assert!(found.project.is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_it_found_names_the_project_file_when_one_decided_it() {
    let dir = scratch("project-describe");
    std::fs::create_dir_all(dir.join("content/maps")).unwrap();
    std::fs::write(dir.join("game.voidproj"), "project { }").unwrap();

    let text = describe(&find(None, Some(&dir.join("content/maps/x.voidmap"))));
    assert!(text.contains("game.voidproj"), "{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_sitting_in_its_own_content_tree_is_not_claimed_by_a_project_elsewhere() {
    // Nearness decides between places. A project file over the working
    // directory -- which, running these tests, is the repository's own --
    // must not reach across the disk and claim a map that has a content tree
    // around it already.
    let dir = scratch("nearness");
    project(&dir);

    let found = find(None, Some(&dir.join("maps/x.voidmap"))).unwrap();
    assert_eq!(found.root, dir);
    assert!(found.project.is_none());

    let _ = std::fs::remove_dir_all(&dir);
}
