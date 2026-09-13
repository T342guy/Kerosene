// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

/// A scratch directory that cleans itself up.
///
/// The set rules are all about what is on disk, so testing them against a
/// mock filesystem would be testing the mock.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let path = std::env::temp_dir().join(format!(
            "kerosene-textureset-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Dir(path)
    }

    fn image(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Contents do not matter: discovery works on names and extensions.
        std::fs::write(&path, b"not really a png").unwrap();
        path
    }

    fn file(&self, relative: &str, body: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_nested_folder_is_named_for_its_whole_path() {
    let root = Path::new("/content/textures");
    assert_eq!(
        TextureSet::name_from_path(&root.join("Walltextures/variant1"), root),
        "Walltextures_variant1"
    );
}

#[test]
fn a_folder_directly_in_the_root_keeps_its_own_name() {
    let root = Path::new("/content/textures");
    assert_eq!(TextureSet::name_from_path(&root.join("brick"), root), "brick");
}

#[test]
fn a_deep_folder_joins_every_level() {
    let root = Path::new("/content/textures");
    assert_eq!(
        TextureSet::name_from_path(&root.join("a/b/c/d"), root),
        "a_b_c_d"
    );
}

#[test]
fn each_map_gets_its_own_texture_name() {
    let dir = Dir::new("names");
    dir.image("Walls/v1/basecolor.png");
    dir.image("Walls/v1/normal.png");
    let set = TextureSet::discover(&dir.0.join("Walls/v1"), &dir.0).unwrap();

    assert_eq!(set.name, "Walls_v1");
    assert_eq!(set.texture_name(MapKind::Base), "Walls_v1");
    assert_eq!(set.texture_name(MapKind::Normal), "Walls_v1_normal");
    assert_eq!(set.texture_name(MapKind::Roughness), "Walls_v1_rough");
}

#[test]
fn every_kind_is_found_by_each_of_its_aliases() {
    for kind in MapKind::ALL {
        for alias in kind.aliases() {
            let dir = Dir::new(&format!("alias-{}-{alias}", kind.suffix()));
            dir.image("set/basecolor.png");
            dir.image(&format!("set/{alias}.png"));
            let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
            // `basecolor` is itself a Base alias, so a Base alias test just
            // re-finds the file already there.
            assert!(
                set.maps.contains_key(&kind),
                "{alias}.png should have been read as {kind:?}"
            );
        }
    }
}

#[test]
fn a_folder_with_no_images_is_not_a_set() {
    let dir = Dir::new("empty");
    std::fs::create_dir_all(dir.0.join("nothing")).unwrap();
    assert_eq!(TextureSet::discover(&dir.0.join("nothing"), &dir.0), None);
}

#[test]
fn a_folder_with_no_base_colour_is_not_a_set() {
    // Bumps and roughness modulate a colour. With nothing to modulate the
    // folder would compile into a material that draws the missing-texture
    // checkerboard, which is worse than not being a texture at all.
    let dir = Dir::new("nobase");
    dir.image("set/normal.png");
    dir.image("set/roughness.png");
    assert_eq!(TextureSet::discover(&dir.0.join("set"), &dir.0), None);
}

#[test]
fn unrecognised_image_names_are_ignored() {
    let dir = Dir::new("stray");
    dir.image("set/basecolor.png");
    dir.image("set/reference_photo.png");
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
    assert_eq!(set.maps.len(), 1);
}

#[test]
fn the_config_renames_the_set() {
    let dir = Dir::new("rename");
    dir.image("Walls/v1/basecolor.png");
    dir.file(
        &format!("Walls/v1/{CONFIG_FILENAME}"),
        "texture { \"name\" \"brick_red\" }",
    );
    let set = TextureSet::discover(&dir.0.join("Walls/v1"), &dir.0).unwrap();
    assert_eq!(set.name, "brick_red");
}

#[test]
fn the_config_names_images_discovery_would_not_find() {
    let dir = Dir::new("explicit");
    dir.image("set/basecolor.png");
    dir.image("set/weird_bumps.png");
    dir.file(
        &format!("set/{CONFIG_FILENAME}"),
        "texture { \"normal\" \"weird_bumps.png\" }",
    );
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
    assert_eq!(
        set.maps.get(&MapKind::Normal).unwrap().file_name().unwrap(),
        "weird_bumps.png"
    );
}

#[test]
fn an_explicit_name_beats_a_discovered_one() {
    let dir = Dir::new("beats");
    dir.image("set/basecolor.png");
    dir.image("set/normal.png");
    dir.image("set/other.png");
    dir.file(
        &format!("set/{CONFIG_FILENAME}"),
        "texture { \"normal\" \"other.png\" }",
    );
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
    assert_eq!(
        set.maps.get(&MapKind::Normal).unwrap().file_name().unwrap(),
        "other.png"
    );
}

#[test]
fn a_config_naming_a_missing_file_falls_back_to_discovery() {
    // A typo in a config should cost the one map it names, not the texture.
    let dir = Dir::new("typo");
    dir.image("set/basecolor.png");
    dir.image("set/normal.png");
    dir.file(
        &format!("set/{CONFIG_FILENAME}"),
        "texture { \"normal\" \"nope.png\" }",
    );
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
    assert_eq!(
        set.maps.get(&MapKind::Normal).unwrap().file_name().unwrap(),
        "normal.png"
    );
}

#[test]
fn a_config_that_does_not_parse_leaves_the_folder_usable() {
    let dir = Dir::new("broken");
    dir.image("set/basecolor.png");
    dir.file(&format!("set/{CONFIG_FILENAME}"), "texture { \"name\" ");
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
    assert_eq!(set.name, "set");
}

#[test]
fn the_config_carries_material_fields() {
    let dir = Dir::new("matfields");
    dir.image("set/basecolor.png");
    dir.file(
        &format!("set/{CONFIG_FILENAME}"),
        "texture { \"shader\" \"unlit\" \"surfaceprop\" \"brick\" \"clamp\" \"1\" }",
    );
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
    assert_eq!(set.shader, Shader::Unlit);
    assert_eq!(set.surface_prop, "brick");
    assert!(set.clamp);
    assert!(set.flags_for(MapKind::Base).contains(TextureFlags::CLAMP));
}

#[test]
fn an_unknown_shader_falls_back_to_lit() {
    let dir = Dir::new("badshader");
    dir.image("set/basecolor.png");
    dir.file(
        &format!("set/{CONFIG_FILENAME}"),
        "texture { \"shader\" \"raytraced\" }",
    );
    assert_eq!(
        TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap().shader,
        Shader::Lit
    );
}

#[test]
fn data_maps_are_not_colour_and_colour_maps_are() {
    assert!(MapKind::Base.flags().is_color());
    assert!(MapKind::Emissive.flags().is_color());
    assert!(!MapKind::Normal.flags().is_color());
    assert!(!MapKind::Roughness.flags().is_color());
    assert!(!MapKind::Ao.flags().is_color());
}

#[test]
fn the_material_wires_up_only_the_maps_that_exist() {
    let dir = Dir::new("material");
    dir.image("set/basecolor.png");
    dir.image("set/normal.png");
    dir.image("set/roughness.png");
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
    let material = set.to_material();

    assert_eq!(material.base_texture(), Some("set"));
    assert_eq!(material.bump_map(), Some("set_normal"));
    assert_eq!(material.roughness_map(), Some("set_rough"));
    // Nothing was authored for these, so nothing should claim they exist.
    assert_eq!(material.emissive_map(), None);
    assert_eq!(material.ao_map(), None);
}

#[test]
fn every_map_a_set_has_is_packed_with_it() {
    // Vault packs what `referenced_textures` reports. A map the material
    // names but the packer does not know about ships broken.
    let dir = Dir::new("packed");
    dir.image("set/basecolor.png");
    dir.image("set/normal.png");
    dir.image("set/roughness.png");
    dir.image("set/emissive.png");
    dir.image("set/ao.png");
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();
    let material = set.to_material();
    let referenced = material.referenced_textures();

    for kind in MapKind::ALL {
        let name = set.texture_name(kind);
        assert!(
            referenced.contains(&name.as_str()),
            "{name} is in the material but would not be packed"
        );
    }
}

#[test]
fn walking_finds_sets_and_skips_the_folders_above_them() {
    let dir = Dir::new("walk");
    dir.image("Walls/v1/basecolor.png");
    dir.image("Walls/v2/basecolor.png");
    dir.image("Floors/tile/basecolor.png");
    // An intermediate folder with no images of its own.
    std::fs::create_dir_all(dir.0.join("Walls/notes")).unwrap();

    let names: Vec<String> = walk(&dir.0).into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["Floors_tile", "Walls_v1", "Walls_v2"]);
}

#[test]
fn a_config_round_trips_through_discovery() {
    // What `new-texture` writes has to be what `discover` reads back, or the
    // generated file documents a format the reader does not accept.
    let dir = Dir::new("roundtrip");
    dir.image("set/basecolor.png");
    dir.image("set/normal.png");
    let set = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();

    dir.file(&format!("set/{CONFIG_FILENAME}"), &set.to_config());
    let reread = TextureSet::discover(&dir.0.join("set"), &dir.0).unwrap();

    assert_eq!(reread.name, set.name);
    assert_eq!(reread.maps, set.maps);
    assert_eq!(reread.shader, set.shader);
    assert_eq!(reread.surface_prop, set.surface_prop);
}
