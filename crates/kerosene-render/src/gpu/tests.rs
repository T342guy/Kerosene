// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Shader validation without a GPU.
//!
//! `naga` is the same compiler wgpu uses internally, so parsing and
//! validating the WGSL here catches exactly the errors that would
//! otherwise only surface at pipeline creation on a machine with a
//! display -- which is a slow way to find a typo.

const WORLD_WGSL: &str = include_str!("../shaders/world.wgsl");
const MODEL_WGSL: &str = include_str!("../shaders/model.wgsl");
const LINE_WGSL: &str = include_str!("../shaders/line.wgsl");
const TONEMAP_WGSL: &str = include_str!("../shaders/tonemap.wgsl");
const SHADOW_WGSL: &str = include_str!("../shaders/shadow.wgsl");
const UI_WGSL: &str = include_str!("../shaders/ui.wgsl");
const PANEL_WGSL: &str = include_str!("../shaders/panel.wgsl");

#[test]
fn the_ui_shaders_compile() {
    validate("ui.wgsl", UI_WGSL);
    validate("panel.wgsl", PANEL_WGSL);
}

#[test]
fn the_ui_quad_matches_what_the_shader_declares() {
    // Nine attributes: six vec4s, a uvec4 and two vec3s.
    assert_eq!(std::mem::size_of::<crate::ui::GpuQuad>(), 7 * 16 + 2 * 12);
}

fn validate(name: &str, source: &str) -> naga::valid::ModuleInfo {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{name} failed to parse:\n{}", e.emit_to_string(source)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{name} failed validation: {e:?}"))
}

#[test]
fn the_world_shader_compiles() {
    validate("world.wgsl", WORLD_WGSL);
}

#[test]
fn the_model_shader_compiles() {
    validate("model.wgsl", MODEL_WGSL);
}

#[test]
fn the_line_shader_compiles() {
    validate("line.wgsl", LINE_WGSL);
}

#[test]
fn the_shadow_shader_compiles() {
    let module = naga::front::wgsl::parse_str(SHADOW_WGSL).expect("parses");
    assert!(module.entry_points.iter().any(|e| e.name == "vs_shadow"));
    validate("shadow.wgsl", SHADOW_WGSL);
}

#[test]
fn the_tonemap_shader_compiles() {
    let module = naga::front::wgsl::parse_str(TONEMAP_WGSL).expect("parses");
    let names: Vec<&str> = module
        .entry_points
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(names.contains(&"vs_fullscreen") && names.contains(&"fs_tonemap"));
    validate("tonemap.wgsl", TONEMAP_WGSL);
}

#[test]
fn every_entry_point_the_pipelines_ask_for_exists() {
    let module = naga::front::wgsl::parse_str(WORLD_WGSL).expect("parses");
    let names: Vec<&str> = module
        .entry_points
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    for wanted in ["vs_main", "fs_world", "fs_sky", "fs_unlit"] {
        assert!(
            names.contains(&wanted),
            "missing entry point {wanted}; have {names:?}"
        );
    }
    let module = naga::front::wgsl::parse_str(MODEL_WGSL).expect("parses");
    let names: Vec<&str> = module
        .entry_points
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    for wanted in ["vs_model", "fs_model"] {
        assert!(
            names.contains(&wanted),
            "missing entry point {wanted}; have {names:?}"
        );
    }
    let module = naga::front::wgsl::parse_str(LINE_WGSL).expect("parses");
    let names: Vec<&str> = module
        .entry_points
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    for wanted in ["vs_line", "fs_line"] {
        assert!(
            names.contains(&wanted),
            "missing entry point {wanted}; have {names:?}"
        );
    }
}

#[test]
fn the_camera_uniform_matches_what_the_shader_declares() {
    // A mismatch here writes the wrong bytes into the wrong fields and
    // produces a picture that is subtly, inexplicably wrong.
    assert_eq!(
        std::mem::size_of::<super::CameraUniform>(),
        64 + 16 + 16 + 16 + 16
    );
}

#[test]
fn the_vertex_layout_matches_the_mesh_vertex() {
    use crate::mesh::WorldVertex;
    assert_eq!(std::mem::size_of::<WorldVertex>(), 60);
    // The attribute offsets in `Renderer::new` assume this layout.
    assert_eq!(std::mem::offset_of!(WorldVertex, position), 0);
    assert_eq!(std::mem::offset_of!(WorldVertex, normal), 12);
    assert_eq!(std::mem::offset_of!(WorldVertex, uv), 24);
    assert_eq!(std::mem::offset_of!(WorldVertex, lightmap_uv), 32);
    assert_eq!(std::mem::offset_of!(WorldVertex, tangent), 40);
    assert_eq!(std::mem::offset_of!(WorldVertex, probe), 56);
}

#[test]
fn the_material_uniform_matches_what_the_shader_declares() {
    // Both shaders declare this struct; getting it wrong here binds
    // `normal_strength` where `present` should be and turns every map off
    // at once, or on at once, depending on the float.
    assert_eq!(std::mem::size_of::<super::MaterialUniform>(), 32);
    assert_eq!(std::mem::offset_of!(super::MaterialUniform, present), 0);
    assert_eq!(
        std::mem::offset_of!(super::MaterialUniform, emissive_strength),
        4
    );
    assert_eq!(
        std::mem::offset_of!(super::MaterialUniform, normal_strength),
        8
    );
    assert_eq!(
        std::mem::offset_of!(super::MaterialUniform, specular_strength),
        12
    );
    assert_eq!(std::mem::offset_of!(super::MaterialUniform, metalness), 16);
    assert_eq!(
        std::mem::offset_of!(super::MaterialUniform, roughness_factor),
        20
    );
}

#[test]
fn the_model_uniform_matches_what_the_shaders_declare() {
    assert_eq!(std::mem::size_of::<super::ModelUniform>(), 80);
    assert_eq!(std::mem::offset_of!(super::ModelUniform, probe), 64);
    assert_eq!(super::ModelUniform::default().probe[0], crate::NO_PROBE);
}

#[test]
fn the_tonemap_uniform_matches_what_the_shader_declares() {
    assert_eq!(std::mem::size_of::<super::ToneMapUniform>(), 16);
    assert_eq!(std::mem::offset_of!(super::ToneMapUniform, exposure), 0);
    assert_eq!(std::mem::offset_of!(super::ToneMapUniform, curve), 4);
}

#[test]
fn msaa_is_off_or_four_samples() {
    assert_eq!(super::msaa_samples_for(0), 1);
    assert_eq!(super::msaa_samples_for(1), 1);
    assert_eq!(super::msaa_samples_for(2), super::MSAA_SAMPLES);
    assert_eq!(super::msaa_samples_for(16), super::MSAA_SAMPLES);
}

#[test]
fn tonemap_operators_come_from_the_convar_by_index() {
    use super::ToneMapOperator;
    assert_eq!(ToneMapOperator::from_index(0), ToneMapOperator::None);
    assert_eq!(ToneMapOperator::from_index(1), ToneMapOperator::Reinhard);
    assert_eq!(ToneMapOperator::from_index(2), ToneMapOperator::Aces);
    assert_eq!(ToneMapOperator::from_index(99), ToneMapOperator::Aces);
    assert_eq!(ToneMapOperator::default(), ToneMapOperator::Aces);
}

#[test]
fn the_renderers_map_order_matches_the_asset_crates() {
    // The shader indexes maps by bit position, so these two lists being
    // in different orders would bind roughness where the emissive map
    // should be -- and look like an art bug, not a code one.
    assert_eq!(super::MAP_COUNT, kerosene_asset::MapKind::ALL.len());
    assert_eq!(super::MAP_KINDS, kerosene_asset::MapKind::ALL);
}

#[test]
fn a_material_with_no_maps_turns_every_optional_path_off() {
    // The regression that matters most: an albedo-only material -- which
    // is every material written before texture sets existed -- must take
    // none of the new branches.
    let uniform = super::MaterialUniform::default();
    assert_eq!(uniform.present, 0);
    for slot in 0..super::MAP_COUNT {
        assert_eq!(uniform.present & (1 << slot), 0);
    }
}

#[test]
fn the_model_vertex_layout_matches_what_the_pipeline_declares() {
    use super::ModelVertex;
    assert_eq!(std::mem::size_of::<ModelVertex>(), 40);
    assert_eq!(
        std::mem::size_of::<ModelVertex>(),
        std::mem::size_of::<kerosene_asset::Vertex>(),
        "the same layout as the file, so upload is a copy"
    );
    assert_eq!(std::mem::offset_of!(ModelVertex, bone_indices), 32);
    assert_eq!(std::mem::offset_of!(ModelVertex, bone_weights), 36);
    assert_eq!(std::mem::offset_of!(ModelVertex, position), 0);
    assert_eq!(std::mem::offset_of!(ModelVertex, normal), 12);
    assert_eq!(std::mem::offset_of!(ModelVertex, uv), 24);
}

#[test]
fn the_line_vertex_layout_matches_what_the_pipeline_declares() {
    use super::LineVertex;
    assert_eq!(std::mem::size_of::<LineVertex>(), 24);
    assert_eq!(std::mem::offset_of!(LineVertex, position), 0);
    assert_eq!(std::mem::offset_of!(LineVertex, color), 12);
}
