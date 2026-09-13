// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;
use std::path::Path;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kerosene-init-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn init(dir: &Path, extra: &[&str]) -> Result<()> {
    let mut args = vec![dir.to_string_lossy().into_owned()];
    args.extend(extra.iter().map(|s| s.to_string()));
    run(args)
}

#[test]
fn a_fresh_directory_becomes_a_project_the_search_can_find() {
    let dir = scratch("fresh");
    init(&dir.join("mygame"), &["--name", "My Game"]).unwrap();

    let project_path = dir.join("mygame/my-game.keroproj");
    assert!(project_path.is_file());

    let project = kerosene_vfs::Project::read(&project_path).unwrap();
    assert_eq!(project.name, "My Game");
    assert_eq!(project.content, dir.join("mygame/content"));

    // The tree is complete, and the search recognises it -- which is the only
    // thing that actually matters about having made it.
    for name in kerosene_vfs::root::CONTENT_DIRS {
        assert!(
            project.content.join(name).is_dir(),
            "{name}/ should have been created"
        );
    }
    assert!(kerosene_vfs::root::is_content_root(&project.content));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_directory_name_is_the_default_project_name() {
    let dir = scratch("default-name");
    init(&dir.join("orbital"), &[]).unwrap();

    assert!(dir.join("orbital/orbital.keroproj").is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn running_it_twice_leaves_the_project_file_alone() {
    // The project file is the one thing here somebody is expected to have
    // edited. Re-running init must not rewrite it.
    let dir = scratch("twice");
    init(&dir.join("g"), &["--name", "G"]).unwrap();

    let path = dir.join("g/g.keroproj");
    let mut project = kerosene_vfs::Project::read(&path).unwrap();
    project.start_map = Some("intro".to_string());
    std::fs::write(
        &path,
        "project { \"name\" \"G\" \"content\" \"content\" \"startmap\" \"intro\" }",
    )
    .unwrap();

    init(&dir.join("g"), &[]).unwrap();

    let reread = kerosene_vfs::Project::read(&path).unwrap();
    assert_eq!(reread.start_map, Some("intro".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn it_fills_in_a_directory_a_tree_has_since_started_needing() {
    let dir = scratch("fills-in");
    init(&dir.join("g"), &[]).unwrap();

    // An older tree that predates `textures/`.
    std::fs::remove_dir_all(dir.join("g/content/textures")).unwrap();
    init(&dir.join("g"), &[]).unwrap();

    assert!(dir.join("g/content/textures").is_dir());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_that_names_its_own_directories_gets_those() {
    let dir = scratch("custom-dirs");
    std::fs::create_dir_all(dir.join("g")).unwrap();
    std::fs::write(
        dir.join("g/g.keroproj"),
        "project { \"content\" \"content\" \"dir\" \"maps\" \"dir\" \"materials\" }",
    )
    .unwrap();

    init(&dir.join("g"), &[]).unwrap();

    assert!(dir.join("g/content/maps").is_dir());
    assert!(!dir.join("g/content/sound").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_that_is_not_a_filename_still_produces_one() {
    assert_eq!(slug("My Game!"), "my-game");
    assert_eq!(slug("  "), "project");
    assert_eq!(slug("Kerosene"), "kerosene");
}
