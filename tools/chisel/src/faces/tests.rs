// SPDX-License-Identifier: MPL-2.0
use super::*;
use kerosene_math::Aabb;

/// A 64-unit cube, and the side of it facing the given direction.
fn cube_face(normal: Vec3) -> (Solid, u32, Plane, Winding) {
    let solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
    let side = solid
        .face_windings()
        .into_iter()
        .find(|(s, _)| s.plane().is_some_and(|p| p.normal.dot(normal) > 0.9))
        .map(|(s, _)| s.id)
        .expect("the cube has a face that way");
    let (plane, winding) = winding_of(&solid, side).expect("it has a winding");
    (solid, side, plane, winding)
}

fn side_mut(solid: &mut Solid, id: u32) -> &mut Side {
    solid.sides.iter_mut().find(|s| s.id == id).expect("the side exists")
}

// ---- scale ---------------------------------------------------------------

#[test]
fn scaling_up_stretches_the_texture() {
    let (mut solid, id, _, winding) = cube_face(Vec3::Z);
    let before = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();
    scale_by(side_mut(&mut solid, id), 2.0, 2.0);
    let after = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();

    let span = |b: ((f32, f32), (f32, f32))| (b.1.0 - b.0.0, b.1.1 - b.0.1);
    let (bu, bv) = span(before);
    let (au, av) = span(after);
    assert!((au - bu / 2.0).abs() < 1e-3, "{au} vs {bu}");
    assert!((av - bv / 2.0).abs() < 1e-3, "{av} vs {bv}");
}

#[test]
fn a_zero_scale_is_refused_rather_than_poisoning_the_compile() {
    // Dividing by it produces infinite texture coordinates, which reach the
    // lightmap packer several stages later as a much stranger bug.
    let (mut solid, id, _, _) = cube_face(Vec3::Z);
    set_scale(side_mut(&mut solid, id), 0.0, f32::NAN);
    let side = side_mut(&mut solid, id);
    assert!(side.uaxis.scale.abs() > 1e-4, "{}", side.uaxis.scale);
    assert!(side.vaxis.scale.is_finite() && side.vaxis.scale.abs() > 1e-4);
}

#[test]
fn a_scale_is_clamped_to_something_a_map_can_hold() {
    let (mut solid, id, _, _) = cube_face(Vec3::Z);
    set_scale(side_mut(&mut solid, id), 1e9, -1e9);
    let side = side_mut(&mut solid, id);
    assert!(side.uaxis.scale <= 1024.0);
    assert!(side.vaxis.scale >= -1024.0);
}

// ---- shift ---------------------------------------------------------------

#[test]
fn shifting_moves_the_texture_by_exactly_that_many_texels() {
    let (mut solid, id, _, winding) = cube_face(Vec3::Z);
    let before = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();
    shift_by(side_mut(&mut solid, id), 16.0, -8.0);
    let after = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();
    assert!((after.0.0 - before.0.0 - 16.0).abs() < 1e-3);
    assert!((after.0.1 - before.0.1 + 8.0).abs() < 1e-3);
}

#[test]
fn a_shift_that_is_not_a_number_is_ignored() {
    let (mut solid, id, _, _) = cube_face(Vec3::Z);
    set_shift(side_mut(&mut solid, id), f32::NAN, f32::INFINITY);
    let side = side_mut(&mut solid, id);
    assert_eq!((side.uaxis.offset, side.vaxis.offset), (0.0, 0.0));
}

// ---- rotate --------------------------------------------------------------

#[test]
fn rotating_turns_the_axes_within_the_face() {
    let (mut solid, id, plane, winding) = cube_face(Vec3::Z);
    let before = side_mut(&mut solid, id).uaxis.axis;
    rotate_by(side_mut(&mut solid, id), &plane, &winding, 90.0);
    let after = side_mut(&mut solid, id).uaxis.axis;

    assert!(before.dot(after).abs() < 1e-3, "not a quarter turn: {before:?} to {after:?}");
    // Still in the face's plane: a texture axis with a normal component
    // projects the texture through the surface.
    assert!(after.dot(plane.normal).abs() < 1e-4, "the axis left the plane");
    assert!((side_mut(&mut solid, id).rotation - 90.0).abs() < 1e-3);
}

#[test]
fn rotation_pivots_about_the_face_not_the_world_origin() {
    // A face a long way from the origin would otherwise fling its texture
    // somewhere unreachable the moment it was rotated.
    let mut solid = Solid::cube(
        Aabb::new(Vec3::new(4000.0, 4000.0, 0.0), Vec3::new(4064.0, 4064.0, 64.0)),
        "dev/grid",
    );
    let id = solid
        .face_windings()
        .into_iter()
        .find(|(s, _)| s.plane().is_some_and(|p| p.normal.z > 0.9))
        .map(|(s, _)| s.id)
        .unwrap();
    let (plane, winding) = winding_of(&solid, id).unwrap();

    let centre = winding.center();
    let before = {
        let s = side_mut(&mut solid, id);
        (
            centre.dot(s.uaxis.axis) / s.uaxis.safe_scale() + s.uaxis.offset,
            centre.dot(s.vaxis.axis) / s.vaxis.safe_scale() + s.vaxis.offset,
        )
    };
    rotate_by(side_mut(&mut solid, id), &plane, &winding, 37.0);
    let after = {
        let s = side_mut(&mut solid, id);
        (
            centre.dot(s.uaxis.axis) / s.uaxis.safe_scale() + s.uaxis.offset,
            centre.dot(s.vaxis.axis) / s.vaxis.safe_scale() + s.vaxis.offset,
        )
    };
    assert!((after.0 - before.0).abs() < 1e-2, "u moved: {before:?} to {after:?}");
    assert!((after.1 - before.1).abs() < 1e-2, "v moved: {before:?} to {after:?}");
}

#[test]
fn four_quarter_turns_come_back_to_where_it_started() {
    let (mut solid, id, plane, winding) = cube_face(Vec3::X);
    let before = side_mut(&mut solid, id).uaxis.axis;
    for _ in 0..4 {
        rotate_by(side_mut(&mut solid, id), &plane, &winding, 90.0);
    }
    let after = side_mut(&mut solid, id).uaxis.axis;
    assert!((after - before).length() < 1e-3, "{before:?} to {after:?}");
    assert!(side_mut(&mut solid, id).rotation.abs() < 1e-3, "the angle did not wrap");
}

// ---- alignment -----------------------------------------------------------

#[test]
fn aligning_to_the_world_restores_the_default_projection() {
    let (mut solid, id, plane, winding) = cube_face(Vec3::X);
    rotate_by(side_mut(&mut solid, id), &plane, &winding, 33.0);
    shift_by(side_mut(&mut solid, id), 40.0, 12.0);
    align_to_world(side_mut(&mut solid, id), &plane);

    let (expected_u, expected_v) = kerosene_map::texture::default_axes_for_plane(&plane, 0.25);
    let side = side_mut(&mut solid, id);
    assert!((side.uaxis.axis - expected_u.axis).length() < 1e-4);
    assert!((side.vaxis.axis - expected_v.axis).length() < 1e-4);
    assert_eq!((side.uaxis.offset, side.vaxis.offset), (0.0, 0.0));
    assert_eq!(side.rotation, 0.0);
}

#[test]
fn aligning_to_the_world_keeps_the_scale_someone_chose() {
    let (mut solid, id, plane, _) = cube_face(Vec3::X);
    set_scale(side_mut(&mut solid, id), 0.5, 0.5);
    align_to_world(side_mut(&mut solid, id), &plane);
    assert_eq!(side_mut(&mut solid, id).uaxis.scale, 0.5);
}

#[test]
fn aligning_to_the_face_puts_both_axes_in_its_plane() {
    // On a slope the world projection foreshortens the texture; this is the
    // other option, and it has to actually lie in the surface.
    let plane = Plane::new(Vec3::new(0.6, 0.0, 0.8).normalize(), 0.0);
    let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
    let id = solid.sides[0].id;
    align_to_face(side_mut(&mut solid, id), &plane);

    let side = side_mut(&mut solid, id);
    assert!(side.uaxis.axis.dot(plane.normal).abs() < 1e-4, "u left the plane");
    assert!(side.vaxis.axis.dot(plane.normal).abs() < 1e-4, "v left the plane");
    assert!(side.uaxis.axis.dot(side.vaxis.axis).abs() < 1e-4, "the axes are not square");
    assert!((side.uaxis.axis.length() - 1.0).abs() < 1e-4);
}

#[test]
fn aligning_to_the_face_works_for_every_facing_including_straight_up() {
    // The helper vector has to be chosen so the cross product never vanishes.
    for normal in [Vec3::Z, -Vec3::Z, Vec3::X, -Vec3::Y, Vec3::new(1.0, 1.0, 1.0).normalize()] {
        let plane = Plane::new(normal, 0.0);
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
        let id = solid.sides[0].id;
        align_to_face(side_mut(&mut solid, id), &plane);
        let side = side_mut(&mut solid, id);
        assert!(
            (side.uaxis.axis.length() - 1.0).abs() < 1e-3,
            "degenerate axes facing {normal:?}"
        );
    }
}

// ---- justify -------------------------------------------------------------

#[test]
fn fitting_makes_the_texture_span_the_face_exactly_once() {
    let (mut solid, id, _, winding) = cube_face(Vec3::Z);
    justify(side_mut(&mut solid, id), &winding, Justify::Fit, (256, 256));

    let (min, max) = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();
    assert!(min.0.abs() < 1e-2 && min.1.abs() < 1e-2, "not at the corner: {min:?}");
    assert!((max.0 - 256.0).abs() < 1e-1, "u spans {} texels", max.0 - min.0);
    assert!((max.1 - 256.0).abs() < 1e-1, "v spans {} texels", max.1 - min.1);
}

#[test]
fn justifying_left_puts_the_texture_at_the_faces_edge() {
    let (mut solid, id, _, winding) = cube_face(Vec3::Z);
    shift_by(side_mut(&mut solid, id), 137.0, 0.0);
    justify(side_mut(&mut solid, id), &winding, Justify::Left, (256, 256));
    let (min, _) = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();
    assert!(min.0.abs() < 1e-2, "{min:?}");
}

#[test]
fn justifying_right_puts_its_far_edge_on_the_textures() {
    let (mut solid, id, _, winding) = cube_face(Vec3::Z);
    justify(side_mut(&mut solid, id), &winding, Justify::Right, (256, 256));
    let (_, max) = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();
    assert!((max.0 - 256.0).abs() < 1e-2, "{max:?}");
}

#[test]
fn justifying_to_the_centre_leaves_equal_margins() {
    let (mut solid, id, _, winding) = cube_face(Vec3::Z);
    justify(side_mut(&mut solid, id), &winding, Justify::Centre, (256, 256));
    let (min, max) = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();
    let left = min.0;
    let right = 256.0 - max.0;
    assert!((left - right).abs() < 1e-2, "left {left}, right {right}");
}

#[test]
fn justifying_only_touches_the_axis_it_is_about() {
    // Pushing a texture to the left edge should not move it vertically.
    let (mut solid, id, _, winding) = cube_face(Vec3::Z);
    shift_by(side_mut(&mut solid, id), 0.0, 21.0);
    let before = side_mut(&mut solid, id).vaxis.offset;
    justify(side_mut(&mut solid, id), &winding, Justify::Left, (256, 256));
    assert_eq!(side_mut(&mut solid, id).vaxis.offset, before);
}

#[test]
fn fitting_a_face_that_is_not_square_still_fits_both_ways() {
    let mut solid = Solid::cube(
        Aabb::new(Vec3::ZERO, Vec3::new(256.0, 32.0, 64.0)),
        "dev/grid",
    );
    let id = solid
        .face_windings()
        .into_iter()
        .find(|(s, _)| s.plane().is_some_and(|p| p.normal.z > 0.9))
        .map(|(s, _)| s.id)
        .unwrap();
    let (_, winding) = winding_of(&solid, id).unwrap();
    justify(side_mut(&mut solid, id), &winding, Justify::Fit, (256, 256));

    let (min, max) = texel_bounds(side_mut(&mut solid, id), &winding).unwrap();
    assert!((max.0 - min.0 - 256.0).abs() < 1e-1, "u span {}", max.0 - min.0);
    assert!((max.1 - min.1 - 256.0).abs() < 1e-1, "v span {}", max.1 - min.1);
}

// ---- the shape of a face -------------------------------------------------

#[test]
fn a_face_with_no_winding_is_not_an_error() {
    let solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
    assert!(winding_of(&solid, 9999).is_none());
}

#[test]
fn texel_bounds_of_an_empty_winding_is_nothing() {
    let (mut solid, id, _, _) = cube_face(Vec3::Z);
    let empty = Winding::new(Vec::new());
    assert!(texel_bounds(side_mut(&mut solid, id), &empty).is_none());
}
