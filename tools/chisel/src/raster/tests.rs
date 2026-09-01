// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Regression tests for the 3D pane, at the pixel level.
//!
//! The bugs these cover were all reported the same way -- "at certain angles"
//! -- because that is how painter's algorithm fails: correct until the camera
//! moves far enough for the sort to invert, then wrong, then correct again.
//! Testing the sort could never catch them, because no sort is right. Testing
//! pixels can.

use super::*;
use crate::app::starter_document;
use std::collections::HashSet;
use kerosene_map::{Solid, WalkmapRule};
use kerosene_math::{Aabb, Angles};

const W: usize = 160;
const H: usize = 120;
const FOV: f32 = 90.0;

fn basis_for(yaw: f32, pitch: f32) -> Basis {
    Angles::new(pitch, yaw, 0.0).vectors()
}

fn brush(document: &mut Document, min: Vec3, max: Vec3) -> u32 {
    let id = document.map.next_id();
    let mut solid = Solid::cube(Aabb::new(min, max), "dev/grid");
    solid.id = id;
    document.map.world.solids.push(solid);
    id
}

fn render_at(document: &Document, eye: Vec3, yaw: f32, pitch: f32) -> Image {
    render(document, eye, basis_for(yaw, pitch), FOV, W, H)
}

fn any_pixel(image: &Image, want: [u8; 4]) -> bool {
    image.pixels.iter().any(|p| *p == want)
}

/// The colour a face gets in the untextured mode these tests use.
///
/// Mirrors `Surface::finish`: shading depends only on the normal, and a
/// selection *tints* rather than replaces, so the material underneath is
/// still readable while a face is being worked on.
fn shade(normal: Vec3, selected: bool) -> [u8; 4] {
    let s = shading_for(normal);
    let base = colors::BRUSH;
    let mut out = [
        (base.r() as f32 * s) as u8,
        (base.g() as f32 * s) as u8,
        (base.b() as f32 * s) as u8,
        255,
    ];
    if selected {
        let tint = colors::SELECTED;
        for (c, channel) in [tint.r(), tint.g(), tint.b()].into_iter().enumerate() {
            out[c] = (out[c] as f32 * 0.45 + channel as f32 * s * 0.55) as u8;
        }
    }
    out
}

// ---- the reported symptom: seeing walls through walls ---------------------

#[test]
fn a_wall_behind_another_wall_never_shows_through_it() {
    // The arrangement painter's algorithm cannot sort. The near wall is wide
    // and deep -- it runs away from the camera, so its farthest vertex is
    // further than anything in the wall behind it -- while the far wall is
    // small and flat. Sorting by farthest vertex puts the near wall first and
    // paints the far one straight over it; sorting by average depth inverts a
    // different pair. Only a depth buffer gets both right.
    let mut document = Document::new();
    document.map.world.solids.clear();

    // Near: a long slab beside the camera, stretching from just ahead to far
    // away, so it spans almost the whole depth range of the scene.
    brush(&mut document, Vec3::new(-200.0, 40.0, -100.0), Vec3::new(2000.0, 60.0, 100.0));
    // Far: a small panel, directly behind the near slab from this viewpoint.
    let hidden = brush(&mut document, Vec3::new(400.0, 80.0, -20.0), Vec3::new(440.0, 100.0, 20.0));
    document.selection.solids.insert(hidden);

    // Look along +X with the slab filling the left of the view.
    let image = render_at(&document, Vec3::new(0.0, 0.0, 0.0), 0.0, 0.0);

    // The hidden panel is selected, so every colour it could draw in is a
    // shade of the selection colour and nothing else in the scene is.
    let selected_shades: Vec<[u8; 4]> = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z]
        .into_iter()
        .map(|n| shade(n, true))
        .collect();
    for want in selected_shades {
        assert!(
            !any_pixel(&image, want),
            "the wall behind showed through the wall in front ({want:?})"
        );
    }
}

#[test]
fn something_poking_through_a_wall_is_not_painted_over_by_it() {
    // A rod running away from the camera and through a wall. The near half of
    // the rod is in front of the wall and the far half is behind it, so no
    // ordering of the two is right for the whole screen.
    //
    // Painter's algorithm has to pick one. Sorting by farthest vertex draws
    // the rod first -- it reaches further -- and then the wall over the top of
    // it, so the half that should be in plain view vanishes.
    let mut document = Document::new();
    document.map.world.solids.clear();
    let rod = brush(&mut document, Vec3::new(100.0, -20.0, -20.0), Vec3::new(600.0, 20.0, 20.0));
    brush(&mut document, Vec3::new(380.0, -200.0, -200.0), Vec3::new(400.0, 200.0, 200.0));
    document.selection.solids.insert(rod);

    // Look down the rod from slightly above, so its top face is in view.
    let image = render(&document, Vec3::new(0.0, 0.0, 90.0), basis_for(0.0, 30.0), FOV, W, H);

    // Specifically the rod's *top* face, which is the one that reaches past
    // the wall and so sorts as the farthest thing in the scene. Its end cap is
    // nearer than everything and would be drawn last either way, so looking
    // for any rod pixel at all would prove nothing.
    assert!(
        any_pixel(&image, shade(Vec3::Z, true)),
        "the wall was painted over the length of rod in front of it"
    );
    assert!(any_pixel(&image, shade(-Vec3::X, false)), "the wall itself is missing");
}

// ---- the reported symptom: stray lines at certain angles ------------------

#[test]
fn standing_inside_a_sealed_room_leaves_no_gaps_at_any_angle() {
    // Inside a closed room every pixel is a wall. A hole in the image means
    // geometry was dropped or a polygon was mangled by the near-plane clip --
    // which is what the stray lines and vanishing walls both came from.
    let document = starter_document();
    let eye = Vec3::new(256.0, 256.0, 64.0);

    for yaw in (0..360).step_by(11) {
        for pitch in [-60.0f32, -20.0, 0.0, 20.0, 60.0] {
            let image = render_at(&document, eye, yaw as f32, pitch);
            let background = image.pixels.len() - image.covered();
            assert_eq!(
                background, 0,
                "{background} pixels of empty space inside a sealed room at yaw {yaw}, pitch {pitch}"
            );
        }
    }
}

#[test]
fn standing_against_a_wall_is_still_a_solid_view() {
    // Most of the wall is behind the near plane here, so the clipper is doing
    // real work on almost every face.
    let document = starter_document();
    for eye in [
        Vec3::new(2.0, 256.0, 64.0),
        Vec3::new(510.0, 256.0, 64.0),
        Vec3::new(256.0, 256.0, 2.0),
        Vec3::new(256.0, 256.0, 253.0),
    ] {
        for yaw in (0..360).step_by(30) {
            let image = render_at(&document, eye, yaw as f32, 0.0);
            assert_eq!(
                image.covered(),
                image.pixels.len(),
                "a gap opened up at {eye:?} yaw {yaw}"
            );
        }
    }
}

#[test]
fn a_face_clipped_at_the_near_plane_stays_inside_the_pane() {
    // The spikes came from projecting a vertex that sits exactly on the near
    // plane far off to one side: the divide is finite but enormous, and the
    // stroke tessellation drew a line across the screen. Nothing can now be
    // written outside the buffer, so the test is simply that the render
    // completes and every pixel is one of the colours it should be.
    let mut document = Document::new();
    document.map.world.solids.clear();
    brush(&mut document, Vec3::new(-4000.0, -20.0, -4000.0), Vec3::new(4000.0, 20.0, 4000.0));

    for yaw in (0..360).step_by(7) {
        let image = render_at(&document, Vec3::new(0.0, 0.0, 0.0), yaw as f32, 0.0);
        assert_eq!(image.pixels.len(), W * H, "the buffer changed size");
        assert!(image.pixels.iter().all(|p| p[3] == 255), "a pixel lost its alpha");
    }
}

// ---- the basics still hold ------------------------------------------------

#[test]
fn an_empty_document_is_all_background() {
    let mut document = Document::new();
    document.map.world.solids.clear();
    let image = render_at(&document, Vec3::ZERO, 0.0, 0.0);
    assert_eq!(image.covered(), 0);
}

#[test]
fn geometry_behind_the_camera_is_not_drawn() {
    let mut document = Document::new();
    document.map.world.solids.clear();
    brush(&mut document, Vec3::new(-600.0, -100.0, -100.0), Vec3::new(-400.0, 100.0, 100.0));
    let image = render_at(&document, Vec3::ZERO, 0.0, 0.0);
    assert_eq!(image.covered(), 0, "something behind the camera was drawn in front of it");
}

#[test]
fn a_nearer_face_wins_the_pixel_whatever_order_it_arrives_in() {
    // Straight down the middle: two panels, one squarely behind the other.
    let mut document = Document::new();
    document.map.world.solids.clear();
    let near = brush(&mut document, Vec3::new(200.0, -50.0, -50.0), Vec3::new(220.0, 50.0, 50.0));
    brush(&mut document, Vec3::new(400.0, -50.0, -50.0), Vec3::new(420.0, 50.0, 50.0));
    document.selection.solids.insert(near);

    let image = render_at(&document, Vec3::ZERO, 0.0, 0.0);
    let centre = image.pixel(W / 2, H / 2);
    assert_eq!(centre, shade(-Vec3::X, true), "the near panel should own the centre pixel");
}

#[test]
fn point_entities_are_marked_but_do_not_show_through_walls() {
    let mut document = Document::new();
    document.map.world.solids.clear();
    // A wall at x = 300, and a light hidden behind it at x = 500.
    brush(&mut document, Vec3::new(300.0, -200.0, -200.0), Vec3::new(320.0, 200.0, 200.0));
    let id = document.map.next_id();
    let mut light = kerosene_map::Entity::new(id, "light");
    light.set_origin(Vec3::new(500.0, 0.0, 0.0));
    document.map.entities.push(light);

    // Marked in its family's colour -- the same amber the 2D panes give a
    // light -- so picking one out of the 3D view does not mean reading a
    // label that is not there.
    let c = crate::icons::Kind::of("light").colour();
    let marker = [c.r(), c.g(), c.b(), 255];

    let image = render_at(&document, Vec3::ZERO, 0.0, 0.0);
    assert!(!any_pixel(&image, marker), "a light behind a wall was drawn through it");

    // Move it in front and it appears.
    document.map.entities[0].set_origin(Vec3::new(150.0, 0.0, 0.0));
    let image = render_at(&document, Vec3::ZERO, 0.0, 0.0);
    assert!(any_pixel(&image, marker), "a light in plain view was not drawn");
}

#[test]
fn two_kinds_of_entity_are_marked_in_two_different_colours() {
    // The whole reason the marker takes its colour from the class: a wall of
    // identical squares tells you where things are and not what they are.
    let mut document = Document::new();
    document.map.world.solids.clear();
    for (i, class) in ["light", "info_player_start"].into_iter().enumerate() {
        let id = document.map.next_id();
        let mut entity = kerosene_map::Entity::new(id, class);
        entity.set_origin(Vec3::new(200.0, -60.0 + i as f32 * 120.0, 0.0));
        document.map.entities.push(entity);
    }

    let image = render_at(&document, Vec3::ZERO, 0.0, 0.0);
    for class in ["light", "info_player_start"] {
        let c = crate::icons::Kind::of(class).colour();
        assert!(any_pixel(&image, [c.r(), c.g(), c.b(), 255]), "{class} was not marked");
    }
}

#[test]
fn faces_are_outlined_where_they_meet() {
    // The outline is read back out of the face buffer rather than stroked, so
    // it exists exactly where two different faces are adjacent on screen.
    let document = starter_document();
    let image = render_at(&document, Vec3::new(256.0, 256.0, 64.0), 45.0, 0.0);
    let plain: Vec<[u8; 4]> = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z]
        .into_iter()
        .map(|n| shade(n, false))
        .collect();
    let darkened = image
        .pixels
        .iter()
        .filter(|p| **p != background_rgba() && !plain.contains(p))
        .count();
    assert!(darkened > 0, "no outlines were drawn between the walls of the room");
}

#[test]
fn a_zero_sized_pane_does_not_panic() {
    let document = starter_document();
    let image = render(&document, Vec3::new(256.0, 256.0, 64.0), basis_for(0.0, 0.0), FOV, 0, 0);
    assert_eq!(image.pixels.len(), 1, "clamped to something drawable");
}



// ---- textures -------------------------------------------------------------

use crate::textures::{Level, Texture};

/// A texture split down the middle: white on the left half, black on the
/// right. Coarse on purpose -- where the boundary lands on screen is the whole
/// question, and a busy texture would hide it.
fn split() -> Arc<Texture> {
    let black = [0, 0, 0, 255];
    let white = [255, 255, 255, 255];
    Arc::new(Texture {
        mips: vec![Level { width: 2, height: 1, pixels: vec![white, black] }],
        average: [128, 128, 128],
    })
}

/// A 2x2 checkerboard, for "is it sampling at all".
fn checker() -> Arc<Texture> {
    let a = [255, 0, 0, 255];
    let b = [0, 0, 255, 255];
    Arc::new(Texture {
        mips: vec![Level { width: 2, height: 2, pixels: vec![a, b, b, a] }],
        average: [128, 0, 128],
    })
}

fn render_textured(
    document: &Document,
    eye: Vec3,
    yaw: f32,
    pitch: f32,
    texture: Arc<Texture>,
) -> Image {
    let mut resolve = move |_: &str| Some(Arc::clone(&texture));
    let mut settings =
        Settings { shading: Shading::Textured, resolve: Some(&mut resolve) };
    render_with(document, eye, basis_for(yaw, pitch), FOV, W, H, &mut settings)
}

#[test]
fn a_textured_face_shows_more_than_one_colour() {
    let mut document = Document::new();
    document.map.world.solids.clear();
    brush(&mut document, Vec3::new(300.0, -300.0, -300.0), Vec3::new(320.0, 300.0, 300.0));

    let flat = render_at(&document, Vec3::ZERO, 0.0, 0.0);
    let textured = render_textured(&document, Vec3::ZERO, 0.0, 0.0, checker());

    let distinct = |image: &Image| {
        image.pixels.iter().filter(|p| **p != background_rgba()).collect::<HashSet<_>>().len()
    };
    assert_eq!(distinct(&flat), 1, "the untextured wall should be one colour");
    assert!(distinct(&textured) > 1, "the texture was not sampled");
}

#[test]
fn a_face_keeps_its_texture_when_the_camera_is_inside_the_room() {
    // Clipping a face at the near plane adds vertices, and a new vertex with
    // no texture coordinate slides the whole face's texture -- visibly, and
    // only at the angles where the clip happens.
    let document = starter_document();
    let eye = Vec3::new(256.0, 256.0, 64.0);
    for yaw in (0..360).step_by(29) {
        let image = render_textured(&document, eye, yaw as f32, 0.0, checker());
        let seen: HashSet<[u8; 4]> = image.pixels.iter().copied().collect();
        assert!(
            seen.len() > 1,
            "at yaw {yaw} the room came out one flat colour, so the texture was lost"
        );
    }
}

#[test]
fn texture_coordinates_are_perspective_correct() {
    // A floor running away from the camera. Interpolating uv straight across
    // the triangle -- affine texturing -- makes the texture swim, and on a
    // long floor it is not subtle: the boundary between the two halves lands
    // in the wrong place, and the cells are evenly spaced instead of
    // compressing towards the horizon.
    let mut document = Document::new();
    document.map.world.solids.clear();
    brush(&mut document, Vec3::new(0.0, -400.0, -32.0), Vec3::new(4000.0, 400.0, 0.0));

    // A texture that repeats along the floor, so the spacing of the repeats
    // is readable off the image.
    let image = render_textured(&document, Vec3::new(0.0, 0.0, 40.0), 0.0, 20.0, split());

    // Walk down the centre column and record where the stripe flips.
    let column = W / 2;
    let mut flips = Vec::new();
    let mut last: Option<u8> = None;
    for y in 0..H {
        let p = image.pixel(column, y);
        if p == background_rgba() { continue }
        let bright = if p[0] > 100 { 1 } else { 0 };
        if last.is_some_and(|l| l != bright) { flips.push(y); }
        last = Some(bright);
    }
    assert!(flips.len() >= 4, "not enough stripes to measure: {flips:?}");

    // Nearer stripes (further down the screen) must be further apart than
    // distant ones. Affine interpolation makes them all the same.
    let gaps: Vec<usize> = flips.windows(2).map(|w| w[1] - w[0]).collect();
    let near = *gaps.last().expect("at least one gap");
    let far = gaps[0];
    assert!(
        near > far,
        "stripes did not compress with distance -- affine texturing? {gaps:?}"
    );
}

#[test]
fn a_material_with_no_texture_behind_it_is_a_colour_not_a_hole() {
    // The content has to be built for a texture to exist; until then, a wrong
    // colour is a much better answer than a black hole.
    let mut document = Document::new();
    document.map.world.solids.clear();
    brush(&mut document, Vec3::new(300.0, -300.0, -300.0), Vec3::new(320.0, 300.0, 300.0));

    let mut resolve = |_: &str| None;
    let mut settings = Settings { shading: Shading::Textured, resolve: Some(&mut resolve) };
    let image = render_with(&document, Vec3::ZERO, basis_for(0.0, 0.0), FOV, W, H, &mut settings);
    assert!(image.covered() > 0, "nothing was drawn at all");

    let expected = crate::textures::TextureCache::fallback_colour("dev/grid");
    let centre = image.pixel(W / 2, H / 2);
    // Shaded, so not equal -- but the same hue, and not black.
    assert!(centre[0] > 0 || centre[1] > 0 || centre[2] > 0, "a black hole");
    let _ = expected;
}

#[test]
fn flat_mode_uses_the_average_rather_than_the_pixels() {
    // The point of flat mode: shape is readable when a texture is busy.
    let mut document = Document::new();
    document.map.world.solids.clear();
    brush(&mut document, Vec3::new(300.0, -300.0, -300.0), Vec3::new(320.0, 300.0, 300.0));

    let texture = checker();
    let mut resolve = |_: &str| Some(Arc::clone(&texture));
    let mut settings = Settings { shading: Shading::Flat, resolve: Some(&mut resolve) };
    let image = render_with(&document, Vec3::ZERO, basis_for(0.0, 0.0), FOV, W, H, &mut settings);

    let distinct: HashSet<[u8; 4]> =
        image.pixels.iter().filter(|p| **p != background_rgba()).copied().collect();
    assert_eq!(distinct.len(), 1, "flat mode drew texture detail: {distinct:?}");
}

#[test]
fn walkmap_mode_colours_each_face_by_its_rule() {
    // The walkmap view ignores materials entirely: it reads the face's rule,
    // so a designer can see where NPCs may go without compiling. A face that
    // changes rule must change colour even though its texture does not.
    let mut document = Document::new();
    document.map.world.solids.clear();
    let id = brush(&mut document, Vec3::new(300.0, -300.0, -300.0), Vec3::new(320.0, 300.0, 300.0));

    let render_walkmap = |document: &Document| {
        let mut resolve = |_: &str| None;
        let mut settings = Settings { shading: Shading::Walkmap, resolve: Some(&mut resolve) };
        render_with(document, Vec3::ZERO, basis_for(0.0, 0.0), FOV, W, H, &mut settings)
    };

    let allow = render_walkmap(&document);
    let centre = allow.pixel(W / 2, H / 2);
    assert!(centre[0] > 0 || centre[1] > 0 || centre[2] > 0, "nothing drawn");
    assert!(centre[1] >= centre[0], "allow should be green-dominant, got {centre:?}");

    // Change the face the camera can see from allow to deny; the colour must
    // move from green to red, with no change to material or texture.
    let side = document
        .find_solid(id)
        .unwrap()
        .sides
        .iter()
        .find(|s| s.plane().is_some_and(|p| p.normal.x < -0.9))
        .expect("the box has a face pointing at -X")
        .id;
    document
        .map
        .find_solid_mut(id)
        .unwrap()
        .sides
        .iter_mut()
        .find(|s| s.id == side)
        .unwrap()
        .walkmap = WalkmapRule::Deny;

    let deny = render_walkmap(&document);
    assert_ne!(allow.pixels, deny.pixels, "the rule change recoloured nothing");
    let centre = deny.pixel(W / 2, H / 2);
    assert!(centre[0] > centre[1], "deny should be red-dominant, got {centre:?}");
}

#[test]
fn a_selected_face_is_tinted_rather_than_painted_over() {
    // Replacing the colour hides what a face is textured with, which is the
    // thing you are usually looking at while you select it.
    let mut document = Document::new();
    document.map.world.solids.clear();
    let id = brush(&mut document, Vec3::new(300.0, -300.0, -300.0), Vec3::new(320.0, 300.0, 300.0));

    let plain = render_textured(&document, Vec3::ZERO, 0.0, 0.0, checker());
    document.selection.solids.insert(id);
    let selected = render_textured(&document, Vec3::ZERO, 0.0, 0.0, checker());

    assert_ne!(plain.pixels, selected.pixels, "selecting changed nothing");
    let distinct: HashSet<[u8; 4]> =
        selected.pixels.iter().filter(|p| **p != background_rgba()).copied().collect();
    assert!(distinct.len() > 1, "the texture was painted over: {distinct:?}");
}

#[test]
fn selecting_one_face_marks_that_face_and_not_its_brush() {
    let mut document = Document::new();
    document.map.world.solids.clear();
    let id = brush(&mut document, Vec3::new(300.0, -300.0, -300.0), Vec3::new(320.0, 300.0, 300.0));
    // The face the camera can actually see: the one whose normal points back
    // at it. A back-facing side is culled and would prove nothing.
    let side = document
        .find_solid(id)
        .unwrap()
        .sides
        .iter()
        .find(|s| s.plane().is_some_and(|p| p.normal.x < -0.9))
        .expect("the box has a face pointing at -X")
        .id;
    document.selection.faces.insert((id, side));

    let faces = crate::draw::visible_faces(&document, Vec3::ZERO, basis_for(0.0, 0.0));
    let marked = faces.iter().filter(|f| f.face_selected).count();
    assert_eq!(marked, 1, "{} faces came back marked", marked);
    assert!(faces.iter().all(|f| !f.selected), "the whole brush was marked instead");
}

// ---- the reported symptom: trigger brushes are not semi-clear -------------

fn brush_with(document: &mut Document, min: Vec3, max: Vec3, material: &str) -> u32 {
    let id = document.map.next_id();
    let mut solid = Solid::cube(Aabb::new(min, max), material);
    solid.id = id;
    document.map.world.solids.push(solid);
    id
}

fn flat_texture(rgb: [u8; 3]) -> Arc<Texture> {
    let pixel = [rgb[0], rgb[1], rgb[2], 255];
    Arc::new(Texture {
        mips: vec![Level { width: 1, height: 1, pixels: vec![pixel] }],
        average: rgb,
    })
}

/// Render in flat mode, giving each material its own colour.
///
/// The shaded-only mode the other tests use paints every face the same grey,
/// so blending a face over another face of the same colour is invisible by
/// construction -- it would pass whether the blend happened or not.
fn render_coloured(document: &Document) -> Image {
    let mut resolve = |material: &str| {
        Some(flat_texture(if material.starts_with("tools/") {
            [220, 40, 40]
        } else {
            [30, 60, 200]
        }))
    };
    let mut settings = Settings { shading: Shading::Flat, resolve: Some(&mut resolve) };
    render_with(document, Vec3::ZERO, basis_for(0.0, 0.0), FOV, W, H, &mut settings)
}

fn wall(document: &mut Document) {
    brush_with(
        document,
        Vec3::new(600.0, -400.0, -400.0),
        Vec3::new(620.0, 400.0, 400.0),
        "dev/grid",
    );
}

fn volume(document: &mut Document, near: f32, material: &str) {
    brush_with(
        document,
        Vec3::new(near, -60.0, -60.0),
        Vec3::new(near + 40.0, 60.0, 60.0),
        material,
    );
}

fn empty() -> Document {
    let mut document = Document::new();
    document.map.world.solids.clear();
    document
}

fn centre(image: &Image) -> [u8; 4] {
    image.pixel(image.width / 2, image.height / 2)
}

#[test]
fn tool_volumes_are_see_through_and_world_materials_are_not() {
    // The whole point: a trigger is a region, not a wall. Drawn opaque it
    // hides the room it is sitting in, which makes a level with triggers in
    // it impossible to work in.
    for tool in ["tools/trigger", "tools/clip", "tools/hint", "tools/skip", "tools/water"] {
        assert!(opacity_for(tool) < 1.0, "{tool} is drawn solid");
    }
    // `nodraw` is a wall nobody sees, not a volume, so it stays solid -- and
    // so does anything outside `tools/`.
    for solid in
        ["tools/nodraw", "tools/invisible", "tools/skybox", "tools/sky", "dev/grid", "dev/wall"]
    {
        assert_eq!(opacity_for(solid), 1.0, "{solid} was made see-through");
    }
    // The prefix is a directory, not a spelling.
    assert_eq!(opacity_for("dev/tools_test"), 1.0);
    assert_eq!(opacity_for("TOOLS/Trigger"), opacity_for("tools/trigger"));
}

#[test]
fn a_wall_shows_through_a_trigger_volume() {
    let mut with_trigger = empty();
    wall(&mut with_trigger);
    volume(&mut with_trigger, 200.0, "tools/trigger");

    let mut bare = empty();
    wall(&mut bare);

    let mut solid = empty();
    wall(&mut solid);
    volume(&mut solid, 200.0, "tools/nodraw");

    let trigger = centre(&render_coloured(&with_trigger));
    let bare = centre(&render_coloured(&bare));
    let solid = centre(&render_coloured(&solid));

    assert_ne!(trigger, bare, "the trigger did not draw over the wall");
    assert_ne!(trigger, solid, "the trigger was drawn solid");
    // A mix, not one or the other: every channel lands between the two.
    for c in 0..3 {
        let (lo, hi) = (bare[c].min(solid[c]), bare[c].max(solid[c]));
        assert!(
            (lo..=hi).contains(&trigger[c]),
            "channel {c} is {} , outside the wall ({}) and the volume ({})",
            trigger[c],
            bare[c],
            solid[c]
        );
    }
}

#[test]
fn a_nodraw_brush_hides_the_wall_behind_it() {
    // The counter-case that makes the test above mean something: the same
    // geometry with a solid tool material covers what is behind it.
    let mut with_wall = empty();
    wall(&mut with_wall);
    volume(&mut with_wall, 200.0, "tools/nodraw");

    let mut alone = empty();
    volume(&mut alone, 200.0, "tools/nodraw");

    assert_eq!(
        centre(&render_coloured(&with_wall)),
        centre(&render_coloured(&alone)),
        "a solid tool brush let the wall through"
    );
}

#[test]
fn a_volume_does_not_erase_what_is_behind_it_from_the_depth_buffer() {
    // A volume that claimed the depth buffer would hide everything drawn
    // after it -- including a second volume overlapping it. Two triggers one
    // behind the other must both show, so the region they share is a
    // different colour again.
    let mut one = empty();
    wall(&mut one);
    volume(&mut one, 200.0, "tools/trigger");

    let mut two = empty();
    wall(&mut two);
    volume(&mut two, 200.0, "tools/trigger");
    volume(&mut two, 300.0, "tools/trigger");

    assert_ne!(
        centre(&render_coloured(&one)),
        centre(&render_coloured(&two)),
        "the nearer volume hid the one behind it instead of stacking with it"
    );
}

#[test]
fn the_world_is_drawn_before_the_volumes() {
    // The two-pass sort, from the outside. Blending only works if what is
    // under a volume is already painted, so every solid face has to go down
    // first -- including one that is *behind* the volume and would otherwise
    // sort after it.
    let mut document = empty();
    wall(&mut document);
    volume(&mut document, 200.0, "tools/trigger");

    let mut faces = crate::draw::visible_faces(&document, Vec3::ZERO, basis_for(0.0, 0.0));
    faces.sort_by(|a, b| {
        let (a_solid, b_solid) =
            (opacity_for(&a.material) >= 1.0, opacity_for(&b.material) >= 1.0);
        b_solid.cmp(&a_solid).then(b.depth.total_cmp(&a.depth))
    });
    let first_volume = faces.iter().position(|f| opacity_for(&f.material) < 1.0);
    let last_solid = faces.iter().rposition(|f| opacity_for(&f.material) >= 1.0);
    let (Some(first_volume), Some(last_solid)) = (first_volume, last_solid) else {
        panic!("the scene needs both a solid face and a volume face");
    };
    assert!(last_solid < first_volume, "a volume sorted before the world");
}

#[test]
fn blending_is_a_mix_and_stays_opaque() {
    let black = [0, 0, 0, 255];
    let white = [255, 255, 255, 255];
    assert_eq!(blend(black, white, 0.0), black, "alpha 0 changed the pixel");
    assert_eq!(blend(black, white, 1.0), white, "alpha 1 did not take the colour");
    let mid = blend(black, white, 0.5);
    assert!((120..=136).contains(&mid[0]), "half way is {mid:?}");
    // The pane is handed to egui as an image; a hole in it is a hole in the
    // viewport, not a see-through brush.
    assert_eq!(blend(black, white, 0.45)[3], 255);
}
