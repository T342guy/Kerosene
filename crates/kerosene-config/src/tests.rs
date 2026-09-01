// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

#[test]
fn the_defaults_are_vulkan_and_a_reasonable_window() {
    let conf = EngineConf::default();
    assert_eq!(conf.renderer, Renderer::Vulkan);
    assert_eq!(conf.width, 1280);
    assert_eq!(conf.height, 720);
    assert!(conf.vsync);
}

#[test]
fn a_document_round_trips() {
    let conf = EngineConf {
        renderer: Renderer::Metal,
        width: 800,
        height: 600,
        vsync: false,
    };
    let reparsed = EngineConf::parse(&conf.to_document());
    assert_eq!(reparsed, conf);
}

#[test]
fn every_key_defaults_when_absent() {
    let conf = EngineConf::parse("engineconf\n{\n}\n");
    assert_eq!(conf, EngineConf::default());
}

#[test]
fn an_empty_file_is_the_defaults() {
    assert_eq!(EngineConf::parse(""), EngineConf::default());
}

#[test]
fn a_file_without_a_block_is_read_from_the_root() {
    let conf = EngineConf::parse("\"renderer\" \"gl\"\n\"vsync\" \"0\"\n");
    assert_eq!(conf.renderer, Renderer::Gl);
    assert!(!conf.vsync);
    assert_eq!(conf.width, DEFAULT_WIDTH);
}

#[test]
fn renderer_names_take_friendly_aliases() {
    assert_eq!(Renderer::from_str("vulkan"), Some(Renderer::Vulkan));
    assert_eq!(Renderer::from_str("VK"), Some(Renderer::Vulkan));
    assert_eq!(Renderer::from_str("DirectX12"), Some(Renderer::Dx12));
    assert_eq!(Renderer::from_str("OpenGL"), Some(Renderer::Gl));
    assert_eq!(Renderer::from_str("auto"), Some(Renderer::Auto));
    assert_eq!(Renderer::from_str("nonsense"), None);
}

#[test]
fn an_unknown_renderer_falls_back_to_vulkan() {
    let conf = EngineConf::parse("engineconf\n{\n\t\"renderer\" \"ps2\"\n}\n");
    assert_eq!(conf.renderer, Renderer::Vulkan);
}

#[test]
fn a_malformed_number_falls_back_and_the_rest_survives() {
    let conf = EngineConf::parse("engineconf\n{\n\t\"width\" \"wide\"\n\t\"height\" \"900\"\n}\n");
    assert_eq!(conf.width, DEFAULT_WIDTH);
    assert_eq!(conf.height, 900);
}

#[test]
fn booleans_accept_the_same_words_the_engine_always_has() {
    for (text, expected) in [("1", true), ("0", false), ("true", true), ("off", false), ("yes", true)] {
        let conf = EngineConf::parse(&format!("engineconf\n{{\n\t\"vsync\" \"{text}\"\n}}\n"));
        assert_eq!(conf.vsync, expected, "vsync {text:?}");
    }
}

#[test]
fn load_or_create_writes_the_defaults_when_the_file_is_missing() {
    let dir = std::env::temp_dir().join(format!("kerosene-config-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let conf = EngineConf::load_or_create(&dir);
    assert_eq!(conf, EngineConf::default());

    let path = dir.join(FILENAME);
    assert!(path.is_file(), "the config should have been generated");
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert_eq!(EngineConf::parse(&on_disk), EngineConf::default());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_or_create_reads_an_existing_file_without_overwriting_it() {
    let dir = std::env::temp_dir().join(format!("kerosene-config-existing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let path = dir.join(FILENAME);
    std::fs::write(&path, "engineconf\n{\n\t\"renderer\" \"dx12\"\n}\n").unwrap();

    let conf = EngineConf::load_or_create(&dir);
    assert_eq!(conf.renderer, Renderer::Dx12);

    // The custom file is still the custom file: not rewritten over it.
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("\"renderer\" \"dx12\""));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_renderer_maps_to_a_distinct_backend() {
    // Auto is everything; the named ones are exactly one backend each, so a
    // config that says "vulkan" cannot silently draw with metal.
    assert_eq!(Renderer::Auto.wgpu_backends(), wgpu::Backends::all());
    assert_eq!(Renderer::Vulkan.wgpu_backends(), wgpu::Backends::VULKAN);
    assert_eq!(Renderer::Metal.wgpu_backends(), wgpu::Backends::METAL);
    assert_eq!(Renderer::Dx12.wgpu_backends(), wgpu::Backends::DX12);
    assert_eq!(Renderer::Gl.wgpu_backends(), wgpu::Backends::GL);
}
