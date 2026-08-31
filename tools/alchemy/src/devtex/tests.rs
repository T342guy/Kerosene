// SPDX-License-Identifier: MPL-2.0
use super::*;
use std::collections::HashSet;

#[test]
fn the_set_covers_every_tool_material_the_compiler_knows() {
    // The gap this closes: `tools/nodraw` and friends were understood by
    // Cleave and offered by Chisel, and no such texture existed. Picking one
    // gave a missing texture.
    let names: HashSet<&str> = set().iter().map(|t| t.name).collect();
    for tool in [
        "nodraw", "clip", "playerclip", "npcclip", "trigger", "skybox", "hint", "skip",
        "blocklight", "grate", "water",
    ] {
        let name = format!("tools/{tool}");
        assert!(names.contains(name.as_str()), "{name} is missing from the set");
    }
}

#[test]
fn every_tool_texture_in_the_set_is_one_the_compiler_understands() {
    // The other direction: offering a tool material the compiler treats as
    // world geometry would silently wall off a doorway.
    for texture in set() {
        if !texture.name.starts_with("tools/") { continue }
        assert!(
            cleave::material::is_known_tool(texture.name),
            "{} is not a tool the compiler knows",
            texture.name
        );
    }
}

#[test]
fn names_are_unique() {
    let mut seen = HashSet::new();
    for texture in set() {
        assert!(seen.insert(texture.name), "{} appears twice", texture.name);
    }
}

#[test]
fn a_sky_is_not_lit_and_a_tool_texture_is_not_shaded() {
    for texture in set() {
        let expected = match texture.name {
            "dev/sky_kero" | "tools/skybox" => "sky",
            n if n.starts_with("tools/") && n != "tools/grate" && n != "tools/water" => "unlit",
            _ => "lit",
        };
        assert_eq!(texture.shader, expected, "{}", texture.name);
    }
}

// ---- what the pictures actually look like ---------------------------------

#[test]
fn a_measurement_texture_is_square_and_the_size_the_scale_assumes() {
    let canvas = measure([200, 200, 200], [100, 100, 100], Some("X"));
    assert_eq!((canvas.width, canvas.height), (DEV_SIZE, DEV_SIZE));
    assert_eq!(canvas.pixels.len(), (DEV_SIZE * DEV_SIZE * 3) as usize);
    // 256 texels at 0.25 units per texel is 64 units, in 16-unit cells.
    assert_eq!(DEV_SPAN / DEV_CELL, 4);
}

#[test]
fn a_checkerboard_actually_alternates() {
    // Without this a stretched texture is invisible, which is the one thing a
    // measurement texture exists to show.
    let canvas = measure([220, 220, 220], [60, 60, 60], None);
    let cell = (DEV_SIZE / (DEV_SPAN / DEV_CELL)) as i32;
    let sample = |cx: i32, cy: i32| canvas.get(cx * cell + cell / 2, cy * cell + cell / 4);
    assert_ne!(sample(0, 0), sample(1, 0), "neighbouring cells are the same shade");
    assert_eq!(sample(0, 0), sample(1, 1), "the diagonal should match");
}

#[test]
fn a_tool_texture_is_hatched_rather_than_flat() {
    // A flat colour can pass for a wall edge-on; hatching cannot.
    let canvas = tool([180, 60, 60], "CLIP");
    let mut shades = HashSet::new();
    for x in 20..40 {
        shades.insert(canvas.get(x, 60));
    }
    assert!(shades.len() > 1, "no hatching: {shades:?}");
}

#[test]
fn only_the_measurement_texture_carries_a_label() {
    // A world-aligned projection mirrors on opposite walls, so text in a
    // tiling texture reads backwards on half a room. Worth it on the one
    // texture that exists to be counted; litter on the four you build with.
    let plain = measure([200, 200, 200], [100, 100, 100], None);
    let labelled = measure([200, 200, 200], [100, 100, 100], Some("16 KU"));
    assert_ne!(plain.pixels, labelled.pixels, "the label was not drawn");

    let bright = |c: &Canvas| c.pixels.chunks(3).filter(|p| p[0] > 240 && p[1] > 240).count();
    assert!(bright(&labelled) > bright(&plain) + 50, "the label is not legible");
}

#[test]
fn a_label_is_drawn_somewhere_light() {
    let plain = tool([100, 100, 100], "");
    let labelled = tool([100, 100, 100], "NODRAW");
    assert_ne!(plain.pixels, labelled.pixels, "the label was not drawn");

    // ...and it is legible: something near white exists on it.
    let bright = labelled.pixels.chunks(3).filter(|p| p[0] > 240 && p[1] > 240).count();
    assert!(bright > 50, "only {bright} bright pixels");
}

#[test]
fn a_sky_gets_darker_towards_the_top() {
    let canvas = sky([40, 50, 80], [140, 150, 180]);
    let top = canvas.get(128, 2);
    let bottom = canvas.get(128, DEV_SIZE as i32 - 3);
    assert!(bottom[2] > top[2], "the gradient runs the wrong way: {top:?} to {bottom:?}");
}

#[test]
fn a_flat_texture_is_not_perfectly_flat_but_is_deterministic() {
    // Noise so a large surface does not band; deterministic so two builds
    // produce the same bytes.
    let a = flat([128, 128, 128], None);
    let b = flat([128, 128, 128], None);
    assert_eq!(a.pixels, b.pixels, "the same texture came out differently twice");

    let unique: HashSet<Rgb> = (0..64).map(|x| a.get(x, 7)).collect();
    assert!(unique.len() > 1, "no variation at all");
}

// ---- the canvas -----------------------------------------------------------

#[test]
fn drawing_outside_the_canvas_is_ignored_rather_than_a_panic() {
    // Text is centred, and a long label on a small texture runs off the edge.
    let mut canvas = Canvas::new(8, 8, [0, 0, 0]);
    canvas.set(-5, -5, [255, 255, 255]);
    canvas.set(100, 100, [255, 255, 255]);
    canvas.blend(-1, 3, [255, 255, 255], 1.0);
    canvas.text("A VERY LONG LABEL INDEED", (4, 4), 3, [255, 255, 255]);
    assert_eq!(canvas.pixels.len(), 8 * 8 * 3);
}

#[test]
fn blending_moves_a_colour_towards_the_new_one() {
    let mut canvas = Canvas::new(4, 4, [0, 0, 0]);
    canvas.blend(1, 1, [255, 255, 255], 0.5);
    let [r, _, _] = canvas.get(1, 1);
    assert!((126..=129).contains(&r), "{r}");
    canvas.blend(1, 1, [255, 255, 255], 0.0);
    assert!((126..=129).contains(&canvas.get(1, 1)[0]), "an alpha of zero changed something");
}

#[test]
fn the_whole_set_writes_and_reads_back() {
    let dir = std::env::temp_dir().join(format!("alchemy-devtex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let written = write_all(&dir).expect("the set writes");
    assert_eq!(written.total(), set().len());
    assert_eq!(written.changed, set().len(), "nothing was there before");
    for texture in set() {
        let path = dir.join(format!("{}.png", texture.name));
        assert!(path.exists(), "{} was not written", path.display());
        let image = image::open(&path).expect("it is a readable PNG");
        assert!(image.width() >= TOOL_SIZE);
    }

    let materials = dir.join("materials");
    let written = write_materials(&materials).expect("the materials write");
    assert_eq!(written.changed, set().len());
    let text = std::fs::read_to_string(materials.join("tools/nodraw.keromat")).unwrap();
    assert!(text.contains("unlit"), "{text}");
    assert!(text.contains("tools/nodraw"), "{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn running_it_again_changes_nothing() {
    // The set is committed to the repository *and* regenerated by the build.
    // That only works if the generator is byte-deterministic: otherwise every
    // build produces a diff, and a diff that is always there is one nobody
    // reads.
    let dir = std::env::temp_dir().join(format!("alchemy-devtex-again-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    write_all(&dir).expect("first run");
    write_materials(&dir.join("materials")).expect("first run");

    let again = write_all(&dir).expect("second run");
    assert_eq!(again.changed, 0, "{} textures came out different", again.changed);
    assert_eq!(again.unchanged, set().len());

    let materials = write_materials(&dir.join("materials")).expect("second run");
    assert_eq!(materials.changed, 0, "{} materials came out different", materials.changed);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_texture_someone_deleted_comes_back() {
    // Filling gaps is the other half of the job: a clone that lost a file, or
    // a set that grew a new entry, should not need a special command.
    let dir = std::env::temp_dir().join(format!("alchemy-devtex-gap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    write_all(&dir).expect("first run");

    std::fs::remove_file(dir.join("tools/nodraw.png")).expect("delete one");
    let again = write_all(&dir).expect("second run");
    assert_eq!(again.changed, 1, "exactly the missing one");
    assert!(dir.join("tools/nodraw.png").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_report_says_what_happened_in_words() {
    assert_eq!(Written { changed: 0, unchanged: 20 }.to_string(), "20 already up to date");
    assert_eq!(
        Written { changed: 3, unchanged: 17 }.to_string(),
        "3 written, 17 already up to date"
    );
}
