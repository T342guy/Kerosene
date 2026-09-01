// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

fn close(a: Vec3, b: Vec3) -> bool { (a - b).length() < 1e-3 }

#[test]
fn an_unrotated_pose_is_just_a_translation() {
    let pose = Pose::at(Vec3::new(10.0, 20.0, 30.0));
    assert!(!pose.is_rotated());
    assert!(close(pose.to_world(Vec3::X), Vec3::new(11.0, 20.0, 30.0)));
}

#[test]
fn local_and_world_are_inverses_of_each_other() {
    // The property the whole type exists for: the renderer transforms one way
    // and the collision code transforms the other, and a mismatch would put
    // what you see and what you walk into in different places.
    let pose = Pose::new(Vec3::new(-40.0, 12.0, 96.0), Angles::new(20.0, 35.0, -15.0));
    for point in [Vec3::ZERO, Vec3::X * 64.0, Vec3::new(3.0, -7.0, 11.0)] {
        let round_tripped = pose.to_local(pose.to_world(point));
        assert!(close(round_tripped, point), "{point:?} became {round_tripped:?}");
    }
}

#[test]
fn yaw_turns_local_x_toward_local_y() {
    // Yaw 90 looks down +Y, so a model's nose ends up pointing that way.
    let pose = Pose::new(Vec3::ZERO, Angles::new(0.0, 90.0, 0.0));
    assert!(close(pose.to_world(Vec3::X), Vec3::Y), "{:?}", pose.to_world(Vec3::X));
}

#[test]
fn a_direction_is_turned_but_not_moved() {
    // A plane normal carried through the origin would point somewhere
    // meaningless, which is why this is a separate operation.
    let pose = Pose::new(Vec3::new(500.0, 0.0, 0.0), Angles::new(0.0, 90.0, 0.0));
    assert!(close(pose.direction_to_world(Vec3::X), Vec3::Y));
}

#[test]
fn rotating_a_box_takes_every_corner_not_just_two() {
    // Rotating min and max gives two points that are the extremes of nothing.
    // A 45-degree yaw on a unit-ish box has to grow the bounds by root two.
    let local = Aabb::new(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 4.0));
    let pose = Pose::new(Vec3::ZERO, Angles::new(0.0, 45.0, 0.0));
    let bounds = pose.bounds_of(local);

    let expected = 10.0 * 2.0f32.sqrt();
    assert!((bounds.max.x - expected).abs() < 0.01, "{bounds:?}");
    assert!((bounds.min.x + expected).abs() < 0.01, "{bounds:?}");
    assert!((bounds.max.z - 4.0).abs() < 0.01, "the axis turned about is unchanged: {bounds:?}");
}

#[test]
fn an_unrotated_box_is_moved_and_not_grown() {
    let local = Aabb::new(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 4.0));
    let bounds = Pose::at(Vec3::new(5.0, 0.0, 0.0)).bounds_of(local);
    assert_eq!(bounds, Aabb::new(Vec3::new(-5.0, -10.0, 0.0), Vec3::new(15.0, 10.0, 4.0)));
}

#[test]
fn an_empty_box_stays_empty_however_it_is_posed() {
    // A model with no geometry has empty bounds, and growing them from eight
    // rotated infinities would produce a box that contains everything.
    let posed = Pose::new(Vec3::new(9.0, 9.0, 9.0), Angles::new(10.0, 20.0, 30.0));
    assert!(posed.bounds_of(Aabb::EMPTY).is_empty());
}

#[test]
fn the_matrix_agrees_with_the_point_transform() {
    // The shader uses the matrix and the collision code uses to_world; if the
    // two disagreed, a rotating door would be drawn somewhere other than where
    // it blocks.
    let pose = Pose::new(Vec3::new(3.0, -4.0, 5.0), Angles::new(15.0, 70.0, 25.0));
    let point = Vec3::new(12.0, -3.0, 8.0);
    let by_matrix = pose.to_mat4() * point.extend(1.0);
    assert!(close(by_matrix.truncate(), pose.to_world(point)));
}

// ---- pivots ---------------------------------------------------------------

#[test]
fn a_pivot_turns_a_body_in_place_instead_of_around_the_world_origin() {
    // The failure this prevents is spectacular: a brush model is compiled in
    // world coordinates, so turning it about its origin flings it around the
    // map instead of spinning it where it stands.
    let far_away = Vec3::new(1000.0, 0.0, 32.0);
    let pose = Pose::about(Vec3::ZERO, Angles::new(0.0, 180.0, 0.0), far_away);

    // The pivot itself never moves, whatever the angle.
    assert!(close(pose.to_world(far_away), far_away));
    // And a point beside it swings to the other side of it, not to the origin.
    let beside = far_away + Vec3::X * 16.0;
    assert!(close(pose.to_world(beside), far_away - Vec3::X * 16.0), "{:?}", pose.to_world(beside));
}

#[test]
fn local_and_world_still_invert_with_a_pivot() {
    let pose = Pose::about(
        Vec3::new(4.0, -9.0, 2.0),
        Angles::new(12.0, 47.0, -8.0),
        Vec3::new(600.0, 300.0, 64.0),
    );
    for point in [Vec3::ZERO, Vec3::new(600.0, 300.0, 64.0), Vec3::new(-11.0, 5.0, 2.0)] {
        assert!(close(pose.to_local(pose.to_world(point)), point));
    }
}

#[test]
fn the_matrix_still_agrees_with_the_point_transform_when_there_is_a_pivot() {
    let pose = Pose::about(
        Vec3::new(1.0, 2.0, 3.0),
        Angles::new(0.0, 30.0, 0.0),
        Vec3::new(256.0, 256.0, 0.0),
    );
    let point = Vec3::new(300.0, 200.0, 48.0);
    let by_matrix = pose.to_mat4() * point.extend(1.0);
    assert!(close(by_matrix.truncate(), pose.to_world(point)), "{by_matrix:?}");
}

#[test]
fn a_pivot_is_ignored_when_nothing_is_turning() {
    // A door slides; it should cost a vector add whatever its pivot says.
    let pose = Pose::about(Vec3::new(0.0, 0.0, 120.0), Angles::ZERO, Vec3::splat(999.0));
    assert!(close(pose.to_world(Vec3::ZERO), Vec3::new(0.0, 0.0, 120.0)));
}
