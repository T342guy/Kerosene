// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kiln-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "").unwrap();
}

#[test]
fn every_stage_has_a_name_that_parses_back() {
    for stage in Stage::ALL {
        assert_eq!(Stage::parse(stage.name()), Some(stage), "{}", stage.name());
    }
    assert_eq!(Stage::parse("  MAPS "), Some(Stage::Maps), "names are forgiving of typing");
    assert_eq!(Stage::parse("lighting"), None);
}

#[test]
fn the_archive_is_named_after_the_project_and_lives_in_the_content_tree() {
    let settings = Settings {
        content: PathBuf::from("/game/content"),
        project: Some(Project {
            path: PathBuf::from("/game/thing.voidproj"),
            name: "My Great Mod".into(),
            content: PathBuf::from("/game/content"),
            start_map: None,
        }),
        ..Settings::default()
    };
    assert_eq!(settings.archive(), PathBuf::from("/game/content/my_great_mod.vault"));
}

#[test]
fn a_project_with_no_name_still_produces_a_usable_archive_name() {
    let settings = Settings { content: PathBuf::from("/game/content"), ..Settings::default() };
    assert_eq!(settings.archive(), PathBuf::from("/game/content/content.vault"));
}

#[test]
fn a_name_of_nothing_but_punctuation_does_not_become_a_filename_of_nothing() {
    assert_eq!(slug("!!!"), "content");
    assert_eq!(slug("  "), "content");
    assert_eq!(slug("Half-Life 2: Update"), "half_life_2_update");
}

#[test]
fn sources_are_found_recursively_and_in_a_stable_order() {
    let dir = scratch("sources");
    touch(&dir.join("props/crate.obj"));
    touch(&dir.join("arch.obj"));
    touch(&dir.join("props/notes.txt"));
    touch(&dir.join("props/tree.OBJ"));

    let found = sources(&dir, "obj");
    assert_eq!(found, vec![
        dir.join("arch.obj"),
        dir.join("props/crate.obj"),
        dir.join("props/tree.OBJ"),
    ]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_directory_yields_no_sources_rather_than_an_error() {
    assert!(sources(Path::new("/definitely/not/here"), "obj").is_empty());
}

#[test]
fn a_dry_run_touches_nothing_and_says_what_it_would_do() {
    let dir = scratch("dry");
    touch(&dir.join("art/props/crate.obj"));
    touch(&dir.join("maps/arena.voidmap"));

    let settings = Settings { content: dir.clone(), dry_run: true, ..Settings::default() };
    let report = build(&settings).unwrap();

    assert_eq!(report.models, 1, "it counted the model it would build");
    assert_eq!(report.maps, 1);
    assert_eq!(report.textures, 0, "and compiled nothing");
    assert!(!dir.join("models").exists(), "a dry run writes nothing");
    assert!(!dir.join("arena.voidbsp").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn naming_a_stage_runs_only_that_one() {
    let dir = scratch("only");
    touch(&dir.join("art/props/crate.obj"));
    touch(&dir.join("maps/arena.voidmap"));

    let settings = Settings {
        content: dir.clone(),
        stages: vec![Stage::Models],
        dry_run: true,
        ..Settings::default()
    };
    let report = build(&settings).unwrap();

    assert_eq!(report.models, 1);
    assert_eq!(report.maps, 0, "the map stage did not run");
    assert!(report.packed.is_none(), "nor did the pack");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn building_a_tree_that_is_not_there_says_so_rather_than_doing_nothing_quietly() {
    let settings = Settings {
        content: PathBuf::from("/definitely/not/here"),
        ..Settings::default()
    };
    let error = build(&settings).unwrap_err().to_string();
    assert!(error.contains("not a directory"), "{error}");
}

#[test]
fn the_texture_stage_builds_a_real_tree() {
    // Not a dry run: the texture stage is the one Kiln does itself, so it is
    // the one worth checking end to end.
    let dir = scratch("textures");
    let settings = Settings {
        content: dir.clone(),
        stages: vec![Stage::Textures],
        ..Settings::default()
    };
    let report = build(&settings).unwrap();

    assert!(report.textures > 0);
    assert!(dir.join("materials/dev/grid.voidtex").is_file());
    assert!(dir.join("art/dev/grid.png").is_file());

    // And again does nothing, which is what makes it cheap to run always.
    let again = build(&settings).unwrap();
    assert_eq!(again.textures, 0);
    assert!(again.textures_skipped > 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sources_are_never_packed() {
    // Shipping the .png next to the .voidtex doubles the download to deliver
    // a file the engine cannot read.
    for source in ["png", "obj", "voidmap", "voidprt", "voidleak"] {
        assert!(!PACKED.contains(&source), "{source} should not be packed");
    }
    for compiled in ["voidtex", "voidmdl", "voidbsp"] {
        assert!(PACKED.contains(&compiled), "{compiled} should be packed");
    }
}
