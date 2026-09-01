// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

/// The shipped content tree, which has to be built for any of this to work.
fn content() -> Vfs {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let mut vfs = Vfs::new();
    vfs.add_directory(&root, "GAME");
    vfs
}

fn built() -> bool {
    content().exists(&kerosene_asset::texture_path("dev/grid"))
}

#[test]
fn a_shipped_material_loads_with_its_mip_chain() {
    if !built() {
        // The content is generated, not committed. Skipping is honest; making
        // the test fail on a fresh clone would be noise.
        eprintln!("content not built; skipping");
        return;
    }
    let vfs = content();
    let mut cache = TextureCache::new();
    let texture = cache.get(&vfs, "dev/grid").expect("dev/grid loads");
    assert_eq!(texture.width(), 256);
    assert_eq!(texture.height(), 256);
    assert!(texture.mips.len() > 1, "no mip chain: {} levels", texture.mips.len());

    // Each level is half the last, and its pixels match its dimensions.
    for pair in texture.mips.windows(2) {
        assert!(pair[1].width <= pair[0].width);
        assert_eq!(pair[1].pixels.len(), (pair[1].width * pair[1].height) as usize);
    }
}

#[test]
fn a_texture_is_loaded_once_and_kept() {
    if !built() { return }
    let vfs = content();
    let mut cache = TextureCache::new();
    let first = cache.get(&vfs, "dev/grid").unwrap();
    let second = cache.get(&vfs, "dev/grid").unwrap();
    assert!(Arc::ptr_eq(&first, &second), "it was loaded twice");
    assert_eq!(cache.len(), 1);
}

#[test]
fn names_are_matched_without_regard_to_case_or_a_leading_slash() {
    if !built() { return }
    let vfs = content();
    let mut cache = TextureCache::new();
    cache.get(&vfs, "dev/grid").unwrap();
    assert!(cache.get(&vfs, "DEV/Grid").is_some());
    assert!(cache.get(&vfs, "/dev/grid").is_some());
    assert_eq!(cache.len(), 1, "the same texture under three names");
}

#[test]
fn a_material_that_is_not_there_fails_once_and_says_why() {
    // Retrying every frame would cost a filesystem miss sixty times a second
    // and fill the log with one line.
    let vfs = content();
    let mut cache = TextureCache::new();
    assert!(cache.get(&vfs, "nothing/at/all").is_none());
    assert!(cache.problem("nothing/at/all").is_some());
    assert_eq!(cache.problem_count(), 1);

    assert!(cache.get(&vfs, "nothing/at/all").is_none());
    assert_eq!(cache.problem_count(), 1, "it was tried again");
}

#[test]
fn clearing_makes_a_rebuilt_texture_show_up() {
    if !built() { return }
    let vfs = content();
    let mut cache = TextureCache::new();
    cache.get(&vfs, "dev/grid").unwrap();
    assert_eq!(cache.len(), 1);
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.problem_count(), 0);
}

// ---- sampling -------------------------------------------------------------

fn checker() -> Texture {
    // Two by two: white, black / black, white.
    let w = [255, 255, 255, 255];
    let b = [0, 0, 0, 255];
    Texture {
        mips: vec![Level { width: 2, height: 2, pixels: vec![w, b, b, w] }],
        average: [128, 128, 128],
    }
}

#[test]
fn sampling_reads_the_texel_under_the_coordinate() {
    let texture = checker();
    assert_eq!(texture.sample(0.25, 0.25, 0), [255, 255, 255, 255]);
    assert_eq!(texture.sample(0.75, 0.25, 0), [0, 0, 0, 255]);
    assert_eq!(texture.sample(0.25, 0.75, 0), [0, 0, 0, 255]);
    assert_eq!(texture.sample(0.75, 0.75, 0), [255, 255, 255, 255]);
}

#[test]
fn coordinates_wrap_rather_than_clamping() {
    // A wall wider than its texture repeats it, and counting the repeats is
    // what a measurement texture is for.
    let texture = checker();
    assert_eq!(texture.sample(1.25, 0.25, 0), texture.sample(0.25, 0.25, 0));
    assert_eq!(texture.sample(-0.75, 0.25, 0), texture.sample(0.25, 0.25, 0));
    assert_eq!(texture.sample(7.75, 5.75, 0), texture.sample(0.75, 0.75, 0));
}

#[test]
fn asking_for_a_mip_that_is_not_there_gives_the_smallest_one() {
    let texture = checker();
    assert_eq!(texture.level(99).width, 2);
    assert_eq!(texture.sample(0.25, 0.25, 99), [255, 255, 255, 255]);
}

// ---- the fallback ---------------------------------------------------------

#[test]
fn a_missing_texture_gets_a_colour_derived_from_its_name() {
    let a = TextureCache::fallback_colour("dev/grid");
    let b = TextureCache::fallback_colour("dev/wall");
    assert_ne!(a, b, "two materials, one colour");
    assert_eq!(a, TextureCache::fallback_colour("dev/grid"), "not stable");
}

#[test]
fn a_fallback_colour_is_never_mistakeable_for_a_lighting_bug() {
    // Nearly black or nearly white reads as broken lighting rather than as a
    // missing texture.
    for name in ["a", "dev/grid", "tools/nodraw", "props/crate_wood", "", "zzzz/zzzz"] {
        let [r, g, b] = TextureCache::fallback_colour(name);
        for channel in [r, g, b] {
            assert!((96..=160).contains(&channel), "{name} gave {r},{g},{b}");
        }
    }
}

#[test]
fn the_average_of_a_material_falls_back_when_it_cannot_be_loaded() {
    let vfs = content();
    let mut cache = TextureCache::new();
    assert_eq!(
        cache.average(&vfs, "nothing/here"),
        TextureCache::fallback_colour("nothing/here")
    );
}

#[test]
fn the_average_of_a_real_texture_is_a_real_average() {
    if !built() { return }
    let vfs = content();
    let mut cache = TextureCache::new();
    // dev/grid is a grey checkerboard: its mean is grey and not extreme.
    let [r, g, b] = cache.average(&vfs, "dev/grid");
    assert!((60..=200).contains(&r), "{r},{g},{b}");
    assert!(r.abs_diff(g) < 24 && g.abs_diff(b) < 24, "not grey: {r},{g},{b}");
}

// ---- choosing a mip to draw at a known size --------------------------------

/// A texture with a full mip chain from `top` pixels down to one.
fn chain(top: u32) -> Texture {
    let mut mips = Vec::new();
    let mut size = top;
    loop {
        mips.push(Level { width: size, height: size, pixels: vec![[0; 4]; (size * size) as usize] });
        if size == 1 { break }
        size /= 2;
    }
    Texture { mips, average: [0; 3] }
}

#[test]
fn a_swatch_is_drawn_from_a_mip_at_least_as_big_as_itself() {
    // The bug this replaces: picking two from the end of the chain gave a
    // 2x2 image for every 256-pixel texture, so every material in the browser
    // was the same flat smudge.
    let texture = chain(256);
    assert_eq!(texture.level_for_size(128).width, 128);
    assert_eq!(texture.level_for_size(64).width, 64);
    assert_eq!(texture.level_for_size(48).width, 64, "just big enough, not just too small");
}

#[test]
fn asking_for_more_than_the_texture_has_gives_the_biggest_there_is() {
    let texture = chain(64);
    assert_eq!(texture.level_for_size(512).width, 64);
}

#[test]
fn asking_for_something_tiny_gives_the_smallest() {
    let texture = chain(64);
    assert_eq!(texture.level_for_size(1).width, 1);
}

#[test]
fn a_size_of_zero_is_not_a_division_by_zero_or_a_panic() {
    let texture = chain(64);
    assert!(texture.level_for_size(0).width >= 1);
}

#[test]
fn a_texture_with_one_level_always_gives_that_level() {
    let texture = chain(1);
    for size in [0, 1, 64, 4096] {
        assert_eq!(texture.level_for_size(size).width, 1);
    }
}
