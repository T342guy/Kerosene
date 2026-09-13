// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

/// A scratch directory, cleaned up by the caller.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "alchemy-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write a small solid PNG at `path`.
fn png(path: &Path, size: u32) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let image = image::RgbImage::from_pixel(size, size, image::Rgb([40, 90, 160]));
    image.save(path).unwrap();
}

#[test]
fn a_batch_compiles_what_is_there_and_writes_a_material_for_it() {
    let dir = scratch("batch-first-run");
    png(&dir.join("art/dev/thing.png"), 16);

    let report = batch(&dir.join("art"), &dir.join("materials"), true).unwrap();
    assert_eq!(report.compiled, 1);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.materials, 1);
    assert!(dir.join("materials/dev/thing.kerotex").is_file());
    assert!(dir.join("materials/dev/thing.keromat").is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_batch_skips_everything_and_keeps_the_material() {
    let dir = scratch("batch-second-run");
    png(&dir.join("art/dev/thing.png"), 16);
    batch(&dir.join("art"), &dir.join("materials"), true).unwrap();

    let report = batch(&dir.join("art"), &dir.join("materials"), true).unwrap();
    assert_eq!(
        report.compiled, 0,
        "nothing changed, so nothing should recompile"
    );
    assert_eq!(report.skipped, 1);
    assert_eq!(report.materials, 0);
    assert_eq!(report.kept, 1, "an authored material is never clobbered");
    assert!(!report.did_anything());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn touching_the_source_makes_it_compile_again() {
    let dir = scratch("batch-touch");
    let source = dir.join("art/dev/thing.png");
    png(&source, 16);
    batch(&dir.join("art"), &dir.join("materials"), true).unwrap();

    // A source newer than its output is out of date, whatever it now contains.
    png(&source, 32);
    filetime_forward(&source);

    let report = batch(&dir.join("art"), &dir.join("materials"), true).unwrap();
    assert_eq!(report.compiled, 1);
    assert_eq!(report.skipped, 0);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Push a file's modification time a second into the future.
///
/// Filesystem timestamps are coarse enough that a rewrite within the same
/// second can land on the same stamp, which would make the test depend on how
/// fast the machine is.
fn filetime_forward(path: &Path) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    file.set_modified(later).unwrap();
}

#[test]
fn a_missing_output_is_out_of_date() {
    let dir = scratch("uptodate");
    let source = dir.join("thing.png");
    png(&source, 8);
    assert!(!is_up_to_date(&source, &dir.join("nothing-here.kerotex")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_texture_build_populates_an_empty_content_tree() {
    let dir = scratch("build-textures");

    let build = build_textures(&dir).unwrap();
    assert!(build.did_anything());
    assert!(
        build.dev_art.changed > 0,
        "the developer set is generated, not required to exist"
    );
    assert!(build.textures.compiled > 0, "and then compiled");

    // The generator's own materials must survive the batch that follows it:
    // the sky is not a lit surface, and only the generator knows that.
    let sky = std::fs::read_to_string(dir.join("materials/dev/sky_kero.keromat")).unwrap();
    assert!(
        sky.starts_with("sky"),
        "expected the sky shader, got {sky:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_texture_build_has_nothing_to_do() {
    let dir = scratch("build-textures-again");
    build_textures(&dir).unwrap();

    let build = build_textures(&dir).unwrap();
    assert!(
        !build.did_anything(),
        "a build with nothing to do should do nothing"
    );
    assert_eq!(build.dev_art.changed, 0);
    assert_eq!(build.textures.compiled, 0);
    assert!(build.textures.skipped > 0);
    assert_eq!(
        build.to_string(),
        format!(
            "textures already built ({} up to date)",
            build.textures.skipped
        )
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---- texture sets -----------------------------------------------------------

#[test]
fn a_set_compiles_every_map_it_has_and_writes_one_material() {
    let dir = scratch("set-compiles");
    let set_dir = dir.join("textures/Walls/v1");
    png(&set_dir.join("basecolor.png"), 16);
    png(&set_dir.join("normal.png"), 16);
    png(&set_dir.join("roughness.png"), 16);

    let report = batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();
    assert_eq!(report.compiled, 3);
    assert_eq!(report.materials, 1);

    for name in ["Walls_v1", "Walls_v1_normal", "Walls_v1_rough"] {
        assert!(
            dir.join(format!("materials/{name}.kerotex")).is_file(),
            "{name}.kerotex should have been compiled"
        );
    }
    assert!(dir.join("materials/Walls_v1.keromat").is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_material_a_set_writes_names_every_map_it_compiled() {
    let dir = scratch("set-material");
    let set_dir = dir.join("textures/brick");
    png(&set_dir.join("basecolor.png"), 16);
    png(&set_dir.join("normal.png"), 16);
    png(&set_dir.join("ao.png"), 16);

    batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();

    let text = std::fs::read_to_string(dir.join("materials/brick.keromat")).unwrap();
    let material = Material::parse(&text).unwrap();
    assert_eq!(material.base_texture(), Some("brick"));
    assert_eq!(material.bump_map(), Some("brick_normal"));
    assert_eq!(material.ao_map(), Some("brick_ao"));
    // Nothing was authored for roughness, so nothing should claim it exists.
    assert_eq!(material.roughness_map(), None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_build_compiles_nothing_and_keeps_the_material() {
    // This runs on the way into the editor. A build with nothing to do has to
    // cost nothing, or opening Chisel gets slower with every texture added.
    let dir = scratch("set-idempotent");
    let set_dir = dir.join("textures/Walls/v1");
    png(&set_dir.join("basecolor.png"), 16);
    png(&set_dir.join("normal.png"), 16);

    let first = batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();
    assert_eq!(first.compiled, 2);
    assert_eq!(first.materials, 1);

    let second = batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();
    assert_eq!(second.compiled, 0);
    assert_eq!(second.skipped, 2);
    assert_eq!(second.materials, 0);
    assert_eq!(second.kept, 1);
    assert!(!second.did_anything());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_authored_material_survives_a_rebuild() {
    let dir = scratch("set-keeps-material");
    let set_dir = dir.join("textures/brick");
    png(&set_dir.join("basecolor.png"), 16);
    batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();

    // Somebody sets the surface property by hand, as they are meant to.
    let path = dir.join("materials/brick.keromat");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut material = Material::parse(&text).unwrap();
    material.set("$surfaceprop", "brick");
    std::fs::write(&path, material.to_text()).unwrap();

    batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();

    let reread = Material::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(reread.surface_property(), "brick");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn each_map_carries_the_flags_its_kind_calls_for() {
    let dir = scratch("set-flags");
    let set_dir = dir.join("textures/brick");
    png(&set_dir.join("basecolor.png"), 16);
    png(&set_dir.join("normal.png"), 16);
    png(&set_dir.join("roughness.png"), 16);
    batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();

    let read = |name: &str| {
        let bytes = std::fs::read(dir.join(format!("materials/{name}.kerotex"))).unwrap();
        Texture::from_bytes(&bytes).unwrap().flags
    };

    assert!(read("brick").is_color());
    assert!(read("brick_normal").contains(TextureFlags::NORMAL_MAP));
    assert!(read("brick_rough").contains(TextureFlags::DATA));
    // Both mean "not colour", which is what the upload path branches on.
    assert!(!read("brick_normal").is_color());
    assert!(!read("brick_rough").is_color());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_with_no_base_colour_is_not_built() {
    let dir = scratch("set-no-base");
    png(&dir.join("textures/notes/normal.png"), 16);

    let report = batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();
    assert_eq!(report.compiled, 0);
    assert!(!dir.join("materials/notes_normal.kerotex").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_content_tree_with_no_textures_folder_still_builds() {
    let dir = scratch("set-absent");
    let report = batch_sets(&dir.join("textures"), &dir.join("materials")).unwrap();
    assert_eq!(report, Batch::default());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_full_build_compiles_loose_art_and_sets_together() {
    let dir = scratch("build-both");
    png(&dir.join("art/props/crate.png"), 16);
    png(&dir.join("textures/Walls/v1/basecolor.png"), 16);
    png(&dir.join("textures/Walls/v1/normal.png"), 16);

    let build = build_textures(&dir).unwrap();
    assert!(build.did_anything());
    assert_eq!(build.sets.compiled, 2);
    assert!(dir.join("materials/props/crate.kerotex").is_file());
    assert!(dir.join("materials/Walls_v1.kerotex").is_file());
    assert!(dir.join("materials/Walls_v1_normal.kerotex").is_file());

    // And a second pass changes nothing at all.
    let again = build_textures(&dir).unwrap();
    assert!(!again.did_anything());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_texture_lands_where_a_build_will_find_it() {
    let dir = scratch("new-texture");
    let art = dir.join("incoming");
    png(&art.join("my_brick.png"), 16);
    png(&art.join("my_brick_bumps.png"), 16);

    new_texture(
        "Walls/brick",
        &dir,
        &art.join("my_brick.png"),
        Some(&art.join("my_brick_bumps.png")),
        None,
        None,
        None,
        "lit",
        "brick",
        false,
    )
    .unwrap();

    // The images are copied in under their canonical stems, so the folder
    // reads the same whatever the originals were called.
    let set_dir = dir.join("textures/Walls/brick");
    assert!(set_dir.join("basecolor.png").is_file());
    assert!(set_dir.join("normal.png").is_file());
    assert!(set_dir.join("texture.kconfig").is_file());
    // The artist's originals are untouched.
    assert!(art.join("my_brick.png").is_file());

    // And the ordinary build picks it up with no further help.
    let build = build_textures(&dir).unwrap();
    assert_eq!(build.sets.compiled, 2);
    assert!(dir.join("materials/Walls_brick.kerotex").is_file());
    assert!(dir.join("materials/Walls_brick_normal.kerotex").is_file());

    let text = std::fs::read_to_string(dir.join("materials/Walls_brick.keromat")).unwrap();
    assert_eq!(
        Material::parse(&text).unwrap().surface_property(),
        "brick",
        "the surfaceprop given at creation should reach the material"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_texture_refuses_to_overwrite_an_existing_one() {
    let dir = scratch("new-texture-clash");
    png(&dir.join("incoming/a.png"), 16);
    std::fs::create_dir_all(dir.join("textures/brick")).unwrap();

    let result = new_texture(
        "brick",
        &dir,
        &dir.join("incoming/a.png"),
        None,
        None,
        None,
        None,
        "lit",
        "default",
        false,
    );
    assert!(result.is_err(), "an existing folder should not be clobbered");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_texture_checks_every_image_before_creating_anything() {
    // A typo in the last argument should not leave half a texture behind.
    let dir = scratch("new-texture-atomic");
    png(&dir.join("incoming/a.png"), 16);

    let result = new_texture(
        "brick",
        &dir,
        &dir.join("incoming/a.png"),
        Some(&dir.join("incoming/nope.png")),
        None,
        None,
        None,
        "lit",
        "default",
        false,
    );
    assert!(result.is_err());
    assert!(
        !dir.join("textures/brick").exists(),
        "nothing should have been created"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
