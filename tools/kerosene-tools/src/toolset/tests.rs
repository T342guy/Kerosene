// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;
use kerosene_toolui::App;

/// A toolset over an empty directory: no project, no content.
fn toolset_in(name: &str) -> (Toolset, PathBuf) {
    let root = std::env::temp_dir().join(format!("kerosene-toolset-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let toolset = Toolset::open(Launch {
        tab: Tab::Home,
        content: Some(root.clone()),
        ..Default::default()
    })
    .unwrap();
    (toolset, root)
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
    egui::RawInput {
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
        ..Default::default()
    }
}

#[test]
fn a_games_schema_and_runtime_reach_the_editor() {
    let (_, root) = toolset_in("game-schema");
    let toolset = Toolset::open(Launch {
        tab: Tab::Editor,
        content: Some(root.clone()),
        schema: vec![r#"class { "name" "item_pickup" "base" "Entity" "base" "Point" }"#],
        runtime: Some(kerosene_vfs::toolchain::Runtime::Binary(PathBuf::from(
            "/x/mygame",
        ))),
        ..Default::default()
    })
    .unwrap();
    let spec = toolset
        .editor
        .schema
        .get("item_pickup")
        .expect("the game's class");
    assert!(spec.key("origin").is_some(), "with the engine's bases");
    assert!(toolset.editor.schema.get("door").is_some());
    assert_eq!(
        toolset.editor.compile_settings.runtime,
        kerosene_vfs::toolchain::Runtime::Binary(PathBuf::from("/x/mygame"))
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_project_naming_a_game_package_is_what_f9_launches() {
    let (_, root) = toolset_in("game-key");
    std::fs::write(
        root.join("mine.kproj"),
        "project { \"name\" \"Mine\" \"content\" \".\" \"game\" \"my-game\" }",
    )
    .unwrap();
    // Found through the map, the way a double-clicked map is.
    let map = root.join("maps").join("mine.kmap");
    std::fs::create_dir_all(map.parent().unwrap()).unwrap();
    std::fs::write(&map, chisel::app::starter_document().map.to_text()).unwrap();
    let toolset = Toolset::open(Launch {
        tab: Tab::Editor,
        map: Some(map),
        ..Default::default()
    })
    .unwrap();
    match &toolset.editor.compile_settings.runtime {
        kerosene_vfs::toolchain::Runtime::Package { name, .. } => assert_eq!(name, "my-game"),
        other => panic!("expected the project's package, got {other:?}"),
    }
    assert_eq!(toolset.info.game.as_deref(), Some("my-game"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_tab_draws_a_frame_without_a_content_tree() {
    let (mut toolset, root) = toolset_in("tabs");
    let ctx = egui::Context::default();
    kerosene_toolui::theme::install(&ctx);
    for tab in Tab::ALL {
        toolset.tab = tab;
        toolset.output.open = true;
        let output = ctx.run(egui::RawInput::default(), |ctx| toolset.ui(ctx));
        assert!(!output.shapes.is_empty(), "{tab:?}");
        assert!(!toolset.window_title().is_empty());
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_tab_draws_over_the_shipped_content() {
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let mut toolset = Toolset::open(Launch {
        content: Some(content),
        ..Default::default()
    })
    .unwrap();
    assert!(!toolset.showing_start);
    let ctx = egui::Context::default();
    kerosene_toolui::theme::install(&ctx);
    for tab in Tab::ALL {
        toolset.tab = tab;
        let output = ctx.run(egui::RawInput::default(), |ctx| toolset.ui(ctx));
        assert!(!output.shapes.is_empty(), "{tab:?}");
    }
}

#[test]
fn the_window_opens_on_the_home_page() {
    let (toolset, root) = toolset_in("default-tab");
    assert_eq!(toolset.tab, Tab::Home);
    assert_eq!(Tab::default(), Tab::Home);
    assert!(!toolset.showing_start, "a content tree was named");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_tab_has_its_own_name_glyph_and_key() {
    use std::collections::HashSet;
    let names: HashSet<_> = Tab::ALL.iter().map(|t| t.name()).collect();
    let keys: HashSet<_> = Tab::ALL.iter().map(|t| t.info().key).collect();
    let shortcuts: HashSet<_> = Tab::ALL.iter().map(|t| t.shortcut()).collect();
    assert_eq!(names.len(), Tab::ALL.len());
    assert_eq!(keys.len(), Tab::ALL.len());
    assert_eq!(shortcuts.len(), Tab::ALL.len());
}

#[test]
fn ctrl_and_a_digit_switch_tabs() {
    let (mut toolset, root) = toolset_in("keys");
    let ctx = egui::Context::default();
    let _ = ctx.run(key(egui::Key::Num6, egui::Modifiers::CTRL), |ctx| {
        toolset.ui(ctx)
    });
    assert_eq!(toolset.tab, Tab::Build);
    let _ = ctx.run(key(egui::Key::Num4, egui::Modifiers::CTRL), |ctx| {
        toolset.ui(ctx)
    });
    assert_eq!(toolset.tab, Tab::Models);
    let _ = ctx.run(key(egui::Key::Num2, egui::Modifiers::CTRL), |ctx| {
        toolset.ui(ctx)
    });
    assert_eq!(toolset.tab, Tab::Assets);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ctrl_p_opens_the_palette_and_a_command_runs() {
    let (mut toolset, root) = toolset_in("palette");
    let ctx = egui::Context::default();
    kerosene_toolui::theme::install(&ctx);
    let _ = ctx.run(key(egui::Key::P, egui::Modifiers::CTRL), |ctx| {
        toolset.ui(ctx)
    });
    assert!(toolset.palette.is_open());

    toolset.palette.set_query("go to archive");
    let _ = ctx.run(key(egui::Key::Enter, egui::Modifiers::NONE), |ctx| {
        toolset.ui(ctx)
    });
    assert!(!toolset.palette.is_open());
    assert_eq!(toolset.tab, Tab::Archive);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_palette_lists_the_contents_maps() {
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let mut toolset = Toolset::open(Launch {
        content: Some(content.clone()),
        ..Default::default()
    })
    .unwrap();
    toolset.palette.open();
    let commands = toolset.commands();
    let room = commands
        .iter()
        .find(|c| c.title == "kerosene_room.kmap")
        .expect("the shipped map is offered");
    assert_eq!(
        commands::action_for(&room.id),
        Some(Action::OpenMap(content.join("maps/kerosene_room.kmap")))
    );
    assert!(commands.iter().any(|c| c.id == "act:build"));
    assert!(commands.iter().any(|c| c.group == "Model"));
}

#[test]
fn opening_a_map_from_the_home_page_goes_to_the_editor() {
    let (mut toolset, root) = toolset_in("open-map");
    let map = root.join("maps").join("arena.kmap");
    std::fs::create_dir_all(map.parent().unwrap()).unwrap();
    toolset.editor.document = chisel::app::starter_document();
    assert!(toolset.editor.save(Some(map.clone())));
    toolset.act(Action::OpenMap(map.clone()));
    assert_eq!(toolset.tab, Tab::Editor);
    assert_eq!(toolset.editor.document.path.as_deref(), Some(map.as_path()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_stat_tile_opens_the_asset_browser_on_its_category() {
    let (mut toolset, root) = toolset_in("browse");
    toolset.act(Action::BrowseAssets(Some(Category::Models)));
    assert_eq!(toolset.tab, Tab::Assets);
    assert_eq!(toolset.assets.category, Some(Category::Models));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn switching_projects_opens_the_other_one_and_remembers_it() {
    let (mut toolset, first) = toolset_in("switch-a");
    let (_, second) = toolset_in("switch-b");
    toolset.act(Action::SwitchProject(second.clone()));
    assert_eq!(toolset.info.content, second);
    assert_eq!(toolset.tab, Tab::Home);
    assert_eq!(toolset.recent.entries[0].content, second);
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}

#[test]
fn an_unsaved_map_holds_a_switch_until_it_is_answered() {
    let (mut toolset, first) = toolset_in("switch-unsaved");
    let (_, second) = toolset_in("switch-unsaved-b");
    toolset.editor.document = chisel::app::starter_document();
    toolset.editor.document.apply("touch", |_| ());
    assert!(toolset.editor.document.is_modified());
    toolset.act(Action::SwitchProject(second.clone()));
    assert_eq!(
        toolset.pending_close,
        Some(AfterClose::Switch(second.clone()))
    );
    assert_eq!(
        toolset.info.content, first,
        "nothing is thrown away unasked"
    );
    let ctx = egui::Context::default();
    kerosene_toolui::theme::install(&ctx);
    let output = ctx.run(egui::RawInput::default(), |ctx| toolset.ui(ctx));
    assert!(!output.shapes.is_empty(), "the question is drawn");
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}

#[test]
fn the_start_page_can_be_shown_and_left() {
    let (mut toolset, root) = toolset_in("start");
    toolset.act(Action::ShowStart);
    assert!(toolset.showing_start);
    assert_eq!(toolset.window_title(), "Kerosene");
    let ctx = egui::Context::default();
    kerosene_toolui::theme::install(&ctx);
    let output = ctx.run(egui::RawInput::default(), |ctx| toolset.ui(ctx));
    assert!(!output.shapes.is_empty());
    toolset.act(Action::Goto(Tab::Build));
    assert!(!toolset.showing_start);
    assert_eq!(toolset.tab, Tab::Build);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn new_or_open_on_an_unsaved_map_asks_rather_than_doing_nothing() {
    let (mut toolset, dir) = toolset_in("new-unsaved");
    toolset.editor.document = chisel::app::starter_document();
    toolset.editor.document.apply("touch", |_| ());
    toolset.act(Action::NewMap);
    assert_eq!(toolset.pending_close, Some(AfterClose::NewMap));
    assert!(toolset.editor.document.is_modified(), "kept until answered");
    toolset.act(Action::OpenMap(dir.join("maps/other.kmap")));
    assert!(matches!(
        toolset.pending_close,
        Some(AfterClose::OpenMap(_))
    ));
    assert!(toolset.editor.document.is_modified());

    // With nothing unsaved, both simply happen.
    toolset.pending_close = None;
    toolset.editor.document = chisel::app::starter_document();
    toolset.act(Action::NewMap);
    assert_eq!(toolset.pending_close, None);
    assert_eq!(toolset.tab, Tab::Editor);
    let _ = std::fs::remove_dir_all(&dir);
}
