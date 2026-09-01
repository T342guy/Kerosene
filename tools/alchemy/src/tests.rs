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
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).unwrap(); }
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
    assert_eq!(report.compiled, 0, "nothing changed, so nothing should recompile");
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
    assert!(build.dev_art.changed > 0, "the developer set is generated, not required to exist");
    assert!(build.textures.compiled > 0, "and then compiled");

    // The generator's own materials must survive the batch that follows it:
    // the sky is not a lit surface, and only the generator knows that.
    let sky = std::fs::read_to_string(dir.join("materials/dev/sky_kero.keromat")).unwrap();
    assert!(sky.starts_with("sky"), "expected the sky shader, got {sky:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_texture_build_has_nothing_to_do() {
    let dir = scratch("build-textures-again");
    build_textures(&dir).unwrap();

    let build = build_textures(&dir).unwrap();
    assert!(!build.did_anything(), "a build with nothing to do should do nothing");
    assert_eq!(build.dev_art.changed, 0);
    assert_eq!(build.textures.compiled, 0);
    assert!(build.textures.skipped > 0);
    assert_eq!(build.to_string(), format!("textures already built ({} up to date)", build.textures.skipped));

    let _ = std::fs::remove_dir_all(&dir);
}
