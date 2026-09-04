// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;
use kerosene_asset::model::Vertex;
use kerosene_math::Aabb;

/// A unit cube, centred on the origin.
fn cube() -> Model {
    let mut model = Model::new();
    let h = 32.0;
    let corners = [
        Vec3::new(-h, -h, -h), Vec3::new(h, -h, -h), Vec3::new(h, h, -h), Vec3::new(-h, h, -h),
        Vec3::new(-h, -h, h), Vec3::new(h, -h, h), Vec3::new(h, h, h), Vec3::new(-h, h, h),
    ];
    for c in corners {
        model.vertices.push(Vertex::rigid(c, c.normalize(), [0.0, 0.0]));
    }
    // Wound the way `.keromdl` stores triangles: counter-clockwise seen from
    // the front, so the raw cross product of two edges points *out* of the
    // model. A fixture wound the other way is a fixture that cannot tell a
    // correct renderer from one showing the inside of everything.
    let faces: [[u32; 4]; 6] = [
        [3, 2, 1, 0], // -Z
        [5, 6, 7, 4], // +Z
        [1, 5, 4, 0], // -Y
        [3, 7, 6, 2], // +Y
        [2, 6, 5, 1], // +X
        [4, 7, 3, 0], // -X
    ];
    for f in faces {
        model.indices.extend([f[0], f[1], f[2], f[0], f[2], f[3]]);
    }
    model.bounds = Aabb::new(Vec3::splat(-h), Vec3::splat(h));
    model
}

fn drawn(image: &Image) -> usize {
    image.pixels.iter().filter(|p| **p != BACKGROUND).count()
}

#[test]
fn a_model_renders_something() {
    let image = model(&cube(), 64, 30.0, -20.0);
    assert_eq!(image.width, 64);
    assert_eq!(image.height, 64);
    assert!(drawn(&image) > 200, "only {} pixels drawn", drawn(&image));
}

#[test]
fn a_model_is_framed_inside_its_picture() {
    // Framing is what makes one call work for a doorframe and a teacup. A
    // model that runs off the edge has been drawn at the wrong distance.
    let image = model(&cube(), 64, 30.0, -20.0);
    let edge_pixels: usize = (0..64)
        .filter(|&i| {
            image.pixel(i, 0) != BACKGROUND
                || image.pixel(i, 63) != BACKGROUND
                || image.pixel(0, i) != BACKGROUND
                || image.pixel(63, i) != BACKGROUND
        })
        .count();
    assert_eq!(edge_pixels, 0, "the model touches the edge of its box");
}

#[test]
fn a_model_fills_enough_of_the_picture_to_be_worth_looking_at() {
    // Framed too far away is as useless as running off the edge.
    let image = model(&cube(), 64, 30.0, -20.0);
    let fraction = drawn(&image) as f32 / (64.0 * 64.0);
    assert!(fraction > 0.10, "only {:.0}% of the picture is model", fraction * 100.0);
}

#[test]
fn turning_it_changes_the_picture() {
    // Otherwise the turntable in the picker does nothing.
    let a = model(&cube(), 48, 0.0, 0.0);
    let b = model(&cube(), 48, 45.0, -25.0);
    assert_ne!(a.pixels, b.pixels);
}

#[test]
fn a_model_that_is_a_long_way_from_the_origin_is_still_framed() {
    // Models are authored wherever the artist left them, and a previewer that
    // assumed the origin would show an empty box for half of them.
    let mut far = cube();
    for v in &mut far.vertices {
        for i in 0..3 { v.position[i] += 4000.0 }
    }
    far.bounds = Aabb::new(Vec3::splat(4000.0 - 32.0), Vec3::splat(4000.0 + 32.0));

    let image = model(&far, 64, 30.0, -20.0);
    assert!(drawn(&image) > 200, "a model away from the origin vanished");
}

#[test]
fn nearer_surfaces_win() {
    // Without a depth test the back of a model paints over the front, which
    // looks like a hole rather than like a mistake.
    let image = model(&cube(), 64, 30.0, -20.0);
    let centre = image.pixel(32, 32);
    assert_ne!(centre, BACKGROUND);

    // The lit top of the cube is brighter than its shadowed sides, and would
    // not be if a back face had painted over it.
    let brightest = image.pixels.iter().filter(|p| **p != BACKGROUND).map(|p| p[0]).max().unwrap();
    let darkest = image.pixels.iter().filter(|p| **p != BACKGROUND).map(|p| p[0]).min().unwrap();
    assert!(brightest > darkest, "nothing is shaded: {brightest} vs {darkest}");
}

#[test]
fn an_empty_model_draws_a_blank_rather_than_panicking() {
    let image = model(&Model::new(), 32, 0.0, 0.0);
    assert_eq!(drawn(&image), 0);
}

#[test]
fn a_zero_sized_picture_is_empty_rather_than_a_panic() {
    let image = model(&cube(), 0, 0.0, 0.0);
    assert!(image.pixels.is_empty());
}

#[test]
fn the_shipped_model_renders() {
    // The real thing, not a fixture: a format change that broke previews
    // should fail here rather than in a screenshot nobody takes.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/models/props/crate.keromdl");
    let Ok(bytes) = std::fs::read(&path) else { return };
    let crate_model = Model::from_bytes(&bytes).expect("the shipped model loads");

    let image = model(&crate_model, 96, 35.0, -20.0);
    assert!(drawn(&image) > 500, "the crate rendered {} pixels", drawn(&image));
}

// ---- which way round a triangle is ----------------------------------------

/// A triangle in the plane x = 0, facing the camera at yaw 0.
///
/// A triangle in the plane x = 0, facing the camera at yaw 0.
///
/// At yaw 0 the camera sits on the -X side looking toward +X, so a triangle
/// whose outward normal is -X is the one facing it. With `reversed` set the
/// winding is flipped to point the other way, producing the same triangle
/// seen from behind.
fn facing_triangle(reversed: bool) -> Model {
    let h = 32.0;
    let corners = [
        Vec3::new(0.0, -h, -h),
        Vec3::new(0.0, h, -h),
        Vec3::new(0.0, -h, h),
    ];
    let mut model = Model::new();
    for c in corners {
        model.vertices.push(Vertex::rigid(c, Vec3::NEG_X, [0.0, 0.0]));
    }
    // Counter-clockwise from the front (outward normal -X) unless reversed.
    model.indices = if reversed { vec![0, 1, 2] } else { vec![0, 2, 1] };
    model.bounds = Aabb::new(Vec3::new(-1.0, -h, -h), Vec3::new(1.0, h, h));
    model
}

#[test]
fn a_triangle_facing_the_camera_is_drawn() {
    let image = model(&facing_triangle(false), 64, 0.0, 0.0);
    assert!(drawn(&image) > 100, "a front face was culled: {} pixels", drawn(&image));
}

#[test]
fn a_triangle_facing_away_is_not() {
    // Getting this backwards renders the inside of everything, which on a
    // crate reads as a shapeless lump rather than as a mistake.
    let image = model(&facing_triangle(true), 64, 0.0, 0.0);
    assert_eq!(drawn(&image), 0, "a back face was drawn");
}

#[test]
fn the_fixture_is_wound_the_way_the_real_format_is() {
    // `.keromdl` stores triangles counter-clockwise as seen from the front,
    // which is also the winding the GPU renderer culls by. A fixture wound
    // the opposite way would pass whether the renderer is right or inside
    // out, which is how the crate came to be rendered from within for as
    // long as it was.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/models/props/crate.keromdl");
    let Ok(bytes) = std::fs::read(&path) else { return };
    let real = Model::from_bytes(&bytes).unwrap();

    assert_eq!(raw_normals_point_outward(&real), 12, "the format stores them counter-clockwise");
    assert_eq!(raw_normals_point_outward(&cube()), 12, "and so must the fixture");
}

/// How many triangles have `(b-a) x (c-a)` pointing away from the centre.
fn raw_normals_point_outward(model: &Model) -> usize {
    let centre = model.bounds.center();
    model
        .indices
        .chunks_exact(3)
        .filter(|t| {
            let p: Vec<Vec3> = t
                .iter()
                .map(|i| Vec3::from_array(model.vertices[*i as usize].position))
                .collect();
            let raw = (p[1] - p[0]).cross(p[2] - p[0]);
            raw.dot(p[0] - centre) > 0.0
        })
        .count()
}

#[test]
fn a_solid_model_shows_three_shades_from_a_corner() {
    // Three faces meeting at a corner, lit from one side, must come out as
    // three different greys -- that is what makes a preview readable as a
    // shape rather than as a silhouette.
    let image = model(&cube(), 96, 35.0, -20.0);
    let mut shades: Vec<u8> = image
        .pixels
        .iter()
        .filter(|p| **p != BACKGROUND)
        .map(|p| p[0])
        .collect();
    shades.sort_unstable();
    shades.dedup();
    assert!(shades.len() >= 3, "only {} shades: {shades:?}", shades.len());
}
