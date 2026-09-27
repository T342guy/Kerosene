// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;
use kerosene_math::Pose;

fn shaded<'a>() -> Options<'a> {
    Options {
        shading: Shading::Shaded,
        textures: None,
        models: None,
    }
}

#[test]
fn an_empty_document_is_an_empty_scene() {
    let scene = build(&Document::new(), &[], &mut shaded());
    assert_eq!(scene.triangle_count(), 0);
    assert!(scene.lines.is_empty() && scene.batches.is_empty());
}

#[test]
fn a_block_is_twelve_triangles_and_twelve_edges_each_way() {
    let mut document = Document::new();
    document.create_block(Vec3::splat(-32.0), Vec3::splat(32.0));
    document.selection.clear();
    let scene = build(&document, &[], &mut shaded());
    assert_eq!(scene.triangle_count(), 12);
    // Every face strokes its own outline, so each edge is drawn twice.
    assert_eq!(scene.lines.len() / 2, 24);
    assert!(
        scene.xray.is_empty(),
        "nothing selected, nothing through walls"
    );
}

#[test]
fn a_selected_brush_is_tinted_and_found_through_walls() {
    let mut document = Document::new();
    document.create_block(Vec3::splat(-32.0), Vec3::splat(32.0));
    let scene = build(&document, &[], &mut shaded());
    assert!(scene.triangles.iter().all(|v| v.tint[3] > 0.0));
    assert!(!scene.xray.is_empty());
}

#[test]
fn tool_volumes_go_in_a_translucent_batch_after_the_solid_ones() {
    let mut document = Document::new();
    document.create_block(Vec3::splat(-32.0), Vec3::splat(32.0));
    document.current_material = "tools/trigger".into();
    document.create_block(Vec3::splat(64.0), Vec3::splat(128.0));
    let scene = build(&document, &[], &mut shaded());
    let order: Vec<bool> = scene.batches.iter().map(|b| b.translucent).collect();
    assert_eq!(order, [false, true]);
    let trigger = &scene.batches[1];
    let vertex = scene.triangles[trigger.first as usize];
    assert!(vertex.color[3] < 1.0);
}

#[test]
fn a_point_entity_is_a_box_until_it_has_a_model() {
    let mut document = Document::new();
    let id = document.create_entity("info_target", Vec3::ZERO);
    let scene = build(&document, &[], &mut shaded());
    assert_eq!(scene.triangle_count(), 12, "a marker box");

    let mut model = kerosene_asset::Model::new();
    model.vertices.extend([
        kerosene_asset::Vertex::rigid(Vec3::ZERO, Vec3::Z, [0.0; 2]),
        kerosene_asset::Vertex::rigid(Vec3::X, Vec3::Z, [0.0; 2]),
        kerosene_asset::Vertex::rigid(Vec3::Y, Vec3::Z, [0.0; 2]),
    ]);
    model.indices.extend([0, 1, 2]);
    model.recompute_bounds();
    let model = Arc::new(model);
    let helpers = [Helper::Model {
        owner: Some(id),
        path: "one".into(),
        pose: Pose::new(Vec3::new(0.0, 0.0, 100.0), Default::default()),
        selected: false,
        opacity: 1.0,
    }];
    let mut models = |_: &str| Some(model.clone());
    let scene = build(
        &document,
        &helpers,
        &mut Options {
            shading: Shading::Shaded,
            textures: None,
            models: Some(&mut models),
        },
    );
    assert_eq!(scene.triangle_count(), 1, "the model, and no box");
    assert_eq!(scene.triangles[0].position, [0.0, 0.0, 100.0], "posed");
}

#[test]
fn a_model_that_fails_to_load_leaves_the_marker() {
    let mut document = Document::new();
    let id = document.create_entity("prop_static", Vec3::ZERO);
    let helpers = [Helper::Model {
        owner: Some(id),
        path: "missing".into(),
        pose: Pose::IDENTITY,
        selected: false,
        opacity: 1.0,
    }];
    let mut models = |_: &str| None;
    let scene = build(
        &document,
        &helpers,
        &mut Options {
            shading: Shading::Shaded,
            textures: None,
            models: Some(&mut models),
        },
    );
    assert_eq!(scene.triangle_count(), 12);
}

#[test]
fn a_translucent_fill_is_unshaded_and_two_sided() {
    let helpers = [Helper::Fill {
        triangles: vec![[Vec3::ZERO, Vec3::X, Vec3::Y]],
        color: Color32::from_rgba_unmultiplied(255, 200, 100, 60),
    }];
    let scene = build(&Document::new(), &helpers, &mut shaded());
    let batch = &scene.batches[0];
    assert!(batch.translucent && batch.two_sided);
    assert_eq!(scene.triangles[0].normal, [0.0; 3]);
}

#[test]
fn srgb_is_converted_to_linear() {
    assert_eq!(linear(Color32::WHITE), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(linear(Color32::BLACK)[..3], [0.0, 0.0, 0.0]);
    let mid = linear(Color32::from_gray(128))[0];
    assert!((mid - 0.2158).abs() < 0.001, "{mid}");
}
