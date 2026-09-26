// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kerosene-new-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_title_becomes_a_package_a_type_and_a_map() {
    let names = Names::from_title("Orbital Drift!");
    assert_eq!(names.package, "orbital-drift");
    assert_eq!(names.type_name, "OrbitalDrift");
    assert_eq!(names.map, "orbital_drift_start");
    assert_eq!(Names::from_title("3 Body").package, "game-3-body");
    assert_eq!(Names::from_title("3 Body").type_name, "Game3Body");
    assert_eq!(Names::from_title("game").type_name, "TheGame");
    assert_eq!(Names::from_title("???").package, "game");
}

#[test]
fn a_new_game_is_a_package_a_project_and_a_map_that_all_agree() {
    let dir = scratch("game");
    let names = Names::from_title("Orbital Drift");
    let source = Source::Version("1.2.3".into());
    make_game(&dir, &names, &source).unwrap();

    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("name = \"orbital-drift\""));
    assert!(cargo.contains(r#"kerosene = { version = "1.2.3", default-features = false }"#));
    assert!(cargo.contains("name = \"orbital-drift-tools\""));
    assert!(!cargo.contains('@'), "every hole filled: {cargo}");
    for file in [
        "src/main.rs",
        "src/tools.rs",
        "src/game.rs",
        ".cargo/config.toml",
        "README.md",
    ] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        assert!(!text.contains('@') || file == "README.md", "{file}: {text}");
    }
    let aliases = std::fs::read_to_string(dir.join(".cargo/config.toml")).unwrap();
    assert!(aliases.contains("--bin orbital-drift-tools -- play"));

    let project = kerosene_vfs::Project::read(&dir.join("orbital-drift.keroproj")).unwrap();
    assert_eq!(project.name, "Orbital Drift");
    assert_eq!(project.game.as_deref(), Some("orbital-drift"));
    assert_eq!(project.start_map.as_deref(), Some("orbital_drift_start"));
    assert!(kerosene_vfs::root::is_content_root(&project.content));

    // The map parses, and wires the trigger to the game's own class.
    let text =
        std::fs::read_to_string(project.content.join("maps/orbital_drift_start.keromap")).unwrap();
    let map = Map::parse(&text).unwrap();
    assert!(map.entities.iter().any(|e| e.classname() == "item_pickup"));
    assert!(map.validate().is_empty());

    // The game's schema is valid and names the class the map uses.
    let game = std::fs::read_to_string(dir.join("src/game.rs")).unwrap();
    let schema = game
        .split("r#\"")
        .nth(1)
        .unwrap()
        .split("\"#")
        .next()
        .unwrap();
    let parsed = kerosene_entity_schema(schema);
    assert!(parsed.contains("item_pickup"), "{parsed}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The schema's class names, by parsing it as KeyValues the way the editor
/// does.
fn kerosene_entity_schema(text: &str) -> String {
    let kv = kerosene_kv::KeyValues::parse(text).expect("the schema is valid KeyValues");
    format!("{kv:?}")
}

#[test]
fn new_refuses_a_directory_with_something_in_it() {
    let dir = scratch("full");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "mine").unwrap();
    let err = run(vec![dir.display().to_string()]).unwrap_err();
    assert!(err.to_string().contains("already has files"), "{err}");
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "mine"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_content_only_project_has_no_rust_and_no_game_class_in_its_map() {
    let dir = scratch("content");
    run(vec![
        dir.display().to_string(),
        "--content-only".into(),
        "--name".into(),
        "Mod".into(),
    ])
    .unwrap();
    assert!(!dir.join("Cargo.toml").exists());
    let project = kerosene_vfs::Project::read(&dir.join("mod.keroproj")).unwrap();
    assert_eq!(project.game, None);
    let text = std::fs::read_to_string(project.content.join("maps/mod_start.keromap")).unwrap();
    let map = Map::parse(&text).unwrap();
    assert!(map.entities.iter().all(|e| e.classname() != "item_pickup"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_dependency_is_spelled_as_cargo_wants_it() {
    assert_eq!(
        Source::Path(PathBuf::from(r"C:\src\kerosene\crates\kerosene")).to_toml(),
        r#"{ path = "C:/src/kerosene/crates/kerosene", default-features = false }"#
    );
    assert_eq!(
        Source::Path(PathBuf::from(r"\\?\C:\src\k")).to_toml(),
        r#"{ path = "C:/src/k", default-features = false }"#,
        "the verbatim prefix Windows' canonicalize adds is not Cargo's"
    );
    assert_eq!(
        Source::Git("https://example.com/k".into()).to_toml(),
        r#"{ git = "https://example.com/k", default-features = false }"#
    );
}

#[test]
fn a_toolset_from_a_cache_never_points_a_game_into_the_cache() {
    let git = Source::for_toolset_at(Path::new(
        "/home/me/.cargo/git/checkouts/kerosene-abc/1234/tools/kerosene-tools",
    ));
    assert_eq!(
        git,
        Source::GitTag(
            env!("CARGO_PKG_REPOSITORY").to_string(),
            env!("CARGO_PKG_VERSION").to_string()
        ),
        "pinned to the release the toolset is"
    );
    assert!(git.to_toml().contains(r#"tag = ""#));
    let registry = Source::for_toolset_at(Path::new(
        "/home/me/.cargo/registry/src/index.crates.io-x/kerosene-tools-1.0.0",
    ));
    assert_eq!(
        registry,
        Source::Version(env!("CARGO_PKG_VERSION").to_string())
    );
    // This checkout: the path to its own game crate.
    match Source::for_toolset_at(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        Source::Path(p) => assert!(p.join("Cargo.toml").is_file(), "{}", p.display()),
        other => panic!("{other:?}"),
    }
}
