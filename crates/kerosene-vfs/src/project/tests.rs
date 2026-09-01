// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "keroproj-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_project_names_its_content_relative_to_itself() {
    let dir = scratch("relative");
    let file = dir.join("mine.keroproj");
    std::fs::write(&file, "project { \"name\" \"Mine\" \"content\" \"assets\" }").unwrap();

    let project = Project::read(&file).unwrap();
    assert_eq!(project.name, "Mine");
    assert_eq!(project.content, dir.join("assets"));
    assert_eq!(project.start_map, None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_content_path_that_climbs_is_resolved_rather_than_left_as_written() {
    // `../shared` beside a project file is a real layout: two mods over one
    // content tree. The path has to come out as somewhere, not as a string
    // with a `..` in the middle that every later `join` compounds.
    let dir = scratch("climbing");
    let file = dir.join("mod.keroproj");
    std::fs::write(&file, "project { \"content\" \"../shared\" }").unwrap();

    let project = Project::read(&file).unwrap();
    assert_eq!(project.content, dir.parent().unwrap().join("shared"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_with_no_content_key_takes_the_conventional_directory() {
    let dir = scratch("default-content");
    std::fs::create_dir_all(dir.join("content")).unwrap();
    let file = dir.join("game.keroproj");
    std::fs::write(&file, "project { }").unwrap();

    assert_eq!(Project::read(&file).unwrap().content, dir.join("content"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_that_is_its_own_content_tree_needs_no_content_key() {
    let dir = scratch("self-content");
    let file = dir.join("game.keroproj");
    std::fs::write(&file, "project { }").unwrap();

    // No `content/` beside it, so the project directory is the tree. This is
    // the shape of a shipped game, where the project file sits in the folder
    // it describes.
    assert_eq!(Project::read(&file).unwrap().content, dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_with_no_name_is_called_after_its_file() {
    let dir = scratch("unnamed");
    let file = dir.join("skyfall.keroproj");
    std::fs::write(&file, "project { \"content\" \".\" }").unwrap();

    assert_eq!(Project::read(&file).unwrap().name, "skyfall");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_start_map_is_read_when_there_is_one() {
    let dir = scratch("startmap");
    let file = dir.join("g.keroproj");
    std::fs::write(&file, "project { \"content\" \".\" \"startmap\" \"mm_intro\" }").unwrap();

    assert_eq!(Project::read(&file).unwrap().start_map.as_deref(), Some("mm_intro"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_value_counts_as_absent_rather_than_as_an_answer() {
    let dir = scratch("blank");
    std::fs::create_dir_all(dir.join("content")).unwrap();
    let file = dir.join("g.keroproj");
    std::fs::write(&file, "project { \"content\" \"  \" \"startmap\" \"\" \"name\" \"\" }").unwrap();

    let project = Project::read(&file).unwrap();
    assert_eq!(project.content, dir.join("content"), "a blank path is not a path");
    assert_eq!(project.start_map, None);
    assert_eq!(project.name, "g");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_block_is_accepted_at_the_top_level_too() {
    // Both shapes get written by hand, and neither is wrong.
    let dir = scratch("bare");
    let file = dir.join("g.keroproj");
    std::fs::write(&file, "\"content\" \"assets\"").unwrap();

    assert_eq!(Project::read(&file).unwrap().content, dir.join("assets"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_first_project_file_in_a_directory_is_found_by_name() {
    let dir = scratch("in-dir");
    std::fs::write(dir.join("zulu.keroproj"), "project { }").unwrap();
    std::fs::write(dir.join("alpha.keroproj"), "project { }").unwrap();
    std::fs::write(dir.join("notes.txt"), "").unwrap();

    assert_eq!(in_directory(&dir), Some(dir.join("alpha.keroproj")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_directory_with_no_project_file_has_none() {
    let dir = scratch("in-dir-empty");
    std::fs::write(dir.join("notes.txt"), "").unwrap();
    assert_eq!(in_directory(&dir), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_written_project_reads_back_as_what_was_asked_for() {
    let dir = scratch("write");
    let file = dir.join("new.keroproj");
    Project::write_new(&file, "Fresh Start", "content").unwrap();

    let project = Project::read(&file).unwrap();
    assert_eq!(project.name, "Fresh Start");
    assert_eq!(project.content, dir.join("content"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_file_that_will_not_parse_is_an_error_naming_the_file() {
    let dir = scratch("broken");
    let file = dir.join("bad.keroproj");
    std::fs::write(&file, "project { \"content\" ").unwrap();

    let error = Project::read(&file).unwrap_err().to_string();
    assert!(error.contains("bad.keroproj"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- the game a project ships -----------------------------------------

#[test]
fn a_project_can_name_the_cargo_package_that_is_the_game() {
    let dir = scratch("game-key");
    let path = dir.join("p.keroproj");
    std::fs::write(&path, r#"project { "name" "Thing" "game" "thing-game" }"#).unwrap();

    assert_eq!(Project::read(&path).unwrap().game.as_deref(), Some("thing-game"));
}

#[test]
fn a_project_with_no_game_key_names_no_game() {
    // The common case, and it has to stay quiet: a content-only project is a
    // perfectly ordinary thing, and it ships the engine's own runtime.
    let dir = scratch("no-game-key");
    let path = dir.join("p.keroproj");
    std::fs::write(&path, r#"project { "name" "Thing" }"#).unwrap();

    assert_eq!(Project::read(&path).unwrap().game, None);
}

#[test]
fn a_blank_game_key_counts_as_absent() {
    let dir = scratch("blank-game-key");
    let path = dir.join("p.keroproj");
    std::fs::write(&path, "project { \"game\" \"   \" }").unwrap();

    assert_eq!(Project::read(&path).unwrap().game, None);
}
