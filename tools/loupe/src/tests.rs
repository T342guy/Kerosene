// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
use super::*;
use kerosene_toolui::App as _;

/// A content tree with the shipped crate model copied into it, so `LoupeApp`
/// can find it the way it would a real one -- through the VFS and
/// `scan_models`, not by loading bytes directly.
fn tree_with_crate(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "loupe-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let models = root.join("models/props");
    std::fs::create_dir_all(&models).unwrap();

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/models/props/crate.keromdl");
    std::fs::copy(&fixture, models.join("crate.keromdl")).expect("the shipped model exists");
    root
}

fn draw_a_frame(app: &mut LoupeApp) -> egui::FullOutput {
    let ctx = egui::Context::default();
    ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx))
}

#[test]
fn an_empty_content_tree_lists_nothing_rather_than_failing() {
    let root = std::env::temp_dir().join(format!(
        "loupe-empty-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let app = LoupeApp::open(root.clone());
    assert!(app.names.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn scanning_finds_the_shipped_model() {
    let root = tree_with_crate("scan");
    let app = LoupeApp::open(root.clone());
    assert!(
        app.names.iter().any(|n| n == "props/crate"),
        "{:?}",
        app.names
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn selecting_a_model_loads_it_and_it_matches_its_own_validation() {
    let root = tree_with_crate("select");
    let mut app = LoupeApp::open(root.clone());
    app.select("props/crate");

    let model = app.model.as_ref().expect("the shipped model should load");
    assert!(
        model.validate().is_ok(),
        "the shipped model should validate"
    );
    assert!(!model.meshes.is_empty());
    assert!(app.load_error.is_none());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn selecting_a_model_that_is_not_there_reports_why_instead_of_going_quiet() {
    let root = tree_with_crate("missing");
    let mut app = LoupeApp::open(root.clone());
    app.select("props/does_not_exist");

    assert!(app.model.is_none());
    assert!(app.load_error.is_some(), "a failed load should say why");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_viewer_renders_the_shipped_model_without_falling_over() {
    let root = tree_with_crate("frame");
    let mut app = LoupeApp::open(root.clone());
    app.select("props/crate");

    let output = draw_a_frame(&mut app);
    assert!(!output.shapes.is_empty());
    assert!(
        app.render.is_some(),
        "selecting a model should have rasterised a view of it"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_frame_with_nothing_selected_still_draws() {
    let root = tree_with_crate("idle");
    let mut app = LoupeApp::open(root.clone());
    let output = draw_a_frame(&mut app);
    assert!(!output.shapes.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refresh_picks_up_a_model_added_after_opening() {
    let root = std::env::temp_dir().join(format!(
        "loupe-refresh-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let mut app = LoupeApp::open(root.clone());
    assert!(app.names.is_empty());

    let models = root.join("models/props");
    std::fs::create_dir_all(&models).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/models/props/crate.keromdl");
    std::fs::copy(&fixture, models.join("crate.keromdl")).unwrap();

    app.refresh();
    assert!(app.names.iter().any(|n| n == "props/crate"));

    let _ = std::fs::remove_dir_all(&root);
}
