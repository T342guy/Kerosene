// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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
    for stage in Stage::EVERY {
        assert_eq!(Stage::parse(stage.name()), Some(stage), "{}", stage.name());
    }
    assert_eq!(
        Stage::parse("  MAPS "),
        Some(Stage::Maps),
        "names are forgiving of typing"
    );
    assert_eq!(Stage::parse("lighting"), None);
}

#[test]
fn the_archive_is_named_after_the_project_and_lives_in_the_content_tree() {
    let settings = Settings {
        content: PathBuf::from("/game/content"),
        project: Some(Project {
            path: PathBuf::from("/game/thing.keroproj"),
            name: "My Great Mod".into(),
            content: PathBuf::from("/game/content"),
            start_map: None,
            game: None,
            bin: None,
            dirs: None,
            ..Default::default()
        }),
        ..Settings::default()
    };
    assert_eq!(
        settings.archive(),
        PathBuf::from("/game/content/my_great_mod.vault")
    );
}

#[test]
fn a_project_with_no_name_still_produces_a_usable_archive_name() {
    let settings = Settings {
        content: PathBuf::from("/game/content"),
        ..Settings::default()
    };
    assert_eq!(
        settings.archive(),
        PathBuf::from("/game/content/content.vault")
    );
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
    assert_eq!(
        found,
        vec![
            dir.join("arch.obj"),
            dir.join("props/crate.obj"),
            dir.join("props/tree.OBJ"),
        ]
    );

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
    touch(&dir.join("maps/arena.keromap"));

    let settings = Settings {
        content: dir.clone(),
        dry_run: true,
        ..Settings::default()
    };
    let report = build(&settings).unwrap();

    assert_eq!(report.models, 1, "it counted the model it would build");
    assert_eq!(report.maps, 1);
    assert_eq!(report.textures, 0, "and compiled nothing");
    assert!(!dir.join("models").exists(), "a dry run writes nothing");
    assert!(!dir.join("arena.kerobsp").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn naming_a_stage_runs_only_that_one() {
    let dir = scratch("only");
    touch(&dir.join("art/props/crate.obj"));
    touch(&dir.join("maps/arena.keromap"));

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
    assert!(dir.join("materials/dev/grid.kerotex").is_file());
    assert!(dir.join("art/dev/grid.png").is_file());

    // And again does nothing, which is what makes it cheap to run always.
    let again = build(&settings).unwrap();
    assert_eq!(again.textures, 0);
    assert!(again.textures_skipped > 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sources_are_never_packed() {
    // Shipping the .png next to the .kerotex doubles the download to deliver
    // a file the engine cannot read.
    for source in ["png", "obj", "keromap", "keroprt", "keroleak"] {
        assert!(!PACKED.contains(&source), "{source} should not be packed");
    }
    for compiled in ["kerotex", "keromdl", "kerobsp"] {
        assert!(PACKED.contains(&compiled), "{compiled} should be packed");
    }
}

/// Set a file's modification time, so a test can say which is newer without
/// sleeping.
fn age(path: &Path, seconds_ago: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds_ago);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

#[test]
fn an_output_newer_than_its_source_is_current_and_one_older_is_not() {
    let dir = scratch("current");
    let (source, output) = (dir.join("a.obj"), dir.join("a.keromdl"));
    touch(&source);
    assert!(!is_current(&source, &output), "no output yet");
    touch(&output);
    age(&source, 60);
    assert!(is_current(&source, &output));
    age(&output, 120);
    assert!(!is_current(&source, &output), "the source changed since");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_fast_map_is_current_for_a_fast_build_and_never_for_a_full_one() {
    let dir = scratch("stamp");
    let map = dir.join("maps/a.keromap");
    touch(&map);
    age(&map, 60);
    touch(&map.with_extension("kerobsp"));
    assert!(
        !map_is_current(&map, true),
        "no stamp, no telling how it was built"
    );

    std::fs::write(build_stamp(&map), "fast\n").unwrap();
    assert!(map_is_current(&map, true));
    assert!(
        !map_is_current(&map, false),
        "a full build lights it properly"
    );

    std::fs::write(build_stamp(&map), "full\n").unwrap();
    assert!(map_is_current(&map, true));
    assert!(map_is_current(&map, false));

    age(&map.with_extension("kerobsp"), 120);
    assert!(!map_is_current(&map, true), "edited since it was compiled");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_archive_is_current_until_something_it_packs_is_newer() {
    let dir = scratch("pack");
    let archive = dir.join("content.vault");
    touch(&dir.join("materials/a.keromat"));
    touch(&dir.join("art/a.png"));
    age(&dir.join("materials/a.keromat"), 60);
    assert!(!archive_is_current(&dir, &archive), "no archive yet");
    touch(&archive);
    assert!(
        archive_is_current(&dir, &archive),
        "a newer source that is never packed does not matter"
    );
    touch(&dir.join("materials/b.keromat"));
    age(&archive, 30);
    assert!(!archive_is_current(&dir, &archive));
    let _ = std::fs::remove_dir_all(dir);
}
