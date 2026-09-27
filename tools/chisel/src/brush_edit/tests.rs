// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;
use kerosene_math::Aabb;

fn cube() -> Solid {
    let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid");
    solid.id = 7;
    solid
}

fn corner(solid: &Solid, at: Vec3) -> usize {
    topology(solid)
        .vertex_near(at, 0.5)
        .expect("a corner there")
}

#[test]
fn a_cube_has_eight_corners_twelve_edges_and_six_faces() {
    let t = topology(&cube());
    assert_eq!(t.vertices.len(), 8);
    assert_eq!(t.edges.len(), 12);
    assert_eq!(t.faces.len(), 6);
    assert!(t.faces.iter().all(|(_, c)| c.len() == 4));
}

#[test]
fn pulling_a_top_corner_up_makes_a_sloped_top_and_keeps_the_rest() {
    let solid = cube();
    let v = corner(&solid, Vec3::new(64.0, 64.0, 64.0));
    let edited = move_vertices(&solid, &[v], Vec3::new(0.0, 0.0, 32.0)).unwrap();
    assert!(edited.validate().is_ok());
    assert_eq!(edited.id, 7, "still the same brush");
    let t = topology(&edited);
    assert!(t.vertex_near(Vec3::new(64.0, 64.0, 96.0), 0.5).is_some());
    // The bottom is untouched, and keeps its side's id and material.
    let bottom = cube()
        .sides
        .iter()
        .find(|s| s.plane().unwrap().normal.z < -0.9)
        .unwrap()
        .id;
    let kept = edited
        .sides
        .iter()
        .find(|s| s.id == bottom)
        .expect("bottom kept");
    assert_eq!(kept.material, "dev/grid");
    assert!(edited.volume() > cube().volume());
}

#[test]
fn pushing_a_corner_in_a_little_bevels_it() {
    // Still outside the plane through its three neighbours, so the brush
    // stays convex: the corner becomes a shallow point.
    let solid = cube();
    let v = corner(&solid, Vec3::new(64.0, 64.0, 64.0));
    let bevelled = move_vertices(&solid, &[v], Vec3::splat(-16.0)).unwrap();
    assert!(bevelled.volume() < cube().volume());
    assert!(
        topology(&bevelled)
            .vertex_near(Vec3::splat(48.0), 0.5)
            .is_some()
    );
}

#[test]
fn pushing_a_corner_in_past_its_neighbours_is_refused_as_a_dent() {
    let solid = cube();
    let v = corner(&solid, Vec3::new(64.0, 64.0, 64.0));
    let err = move_vertices(&solid, &[v], Vec3::splat(-32.0)).unwrap_err();
    assert_eq!(err, EditError::Concave);
    assert!(err.to_string().contains("convex"));
}

#[test]
fn dragging_a_top_edge_down_to_the_bottom_makes_a_wedge() {
    let solid = cube();
    let a = corner(&solid, Vec3::new(64.0, 0.0, 64.0));
    let b = corner(&solid, Vec3::new(64.0, 64.0, 64.0));
    let wedge = move_vertices(&solid, &[a, b], Vec3::new(0.0, 0.0, -64.0)).unwrap();
    let t = topology(&wedge);
    assert_eq!(t.vertices.len(), 6, "two corners welded onto two others");
    assert_eq!(t.faces.len(), 5);
    assert!((wedge.volume() - 64.0 * 64.0 * 64.0 / 2.0).abs() < 1.0);
}

#[test]
fn collapsing_a_brush_flat_is_refused() {
    let solid = cube();
    let top: Vec<usize> = topology(&solid)
        .vertices
        .iter()
        .enumerate()
        .filter(|(_, p)| p.z > 32.0)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        move_vertices(&solid, &top, Vec3::new(0.0, 0.0, -64.0)).unwrap_err(),
        EditError::Flat
    );
}

#[test]
fn moving_a_face_out_grows_the_brush_that_way() {
    let solid = cube();
    let top = solid
        .sides
        .iter()
        .find(|s| s.plane().unwrap().normal.z > 0.9)
        .unwrap()
        .id;
    let taller = move_face(&solid, top, Vec3::new(0.0, 0.0, 32.0)).unwrap();
    let b = taller.bounds();
    assert_eq!(b.max.z, 96.0);
    assert_eq!(b.min.z, 0.0);
    assert_eq!(taller.sides.len(), 6);
    assert!(
        taller.sides.iter().any(|s| s.id == top),
        "the face is still that face"
    );
}

#[test]
fn a_selection_by_position_survives_the_brush_changing() {
    let solid = cube();
    let moved = move_points(
        &solid,
        &[Vec3::new(64.0, 64.0, 64.0)],
        Vec3::new(0.0, 0.0, 16.0),
    )
    .unwrap();
    // The moved corner is found again where it went.
    let again = move_points(
        &moved,
        &[Vec3::new(64.0, 64.0, 80.0)],
        Vec3::new(0.0, 0.0, 16.0),
    )
    .unwrap();
    assert!(
        topology(&again)
            .vertex_near(Vec3::new(64.0, 64.0, 96.0), 0.5)
            .is_some()
    );
}

#[test]
fn extruding_a_face_grows_a_new_brush_off_it() {
    let solid = cube();
    let side = solid
        .sides
        .iter()
        .find(|s| s.plane().unwrap().normal.x > 0.9)
        .unwrap()
        .id;
    let grown = extrude_face(&solid, side, 32.0).unwrap();
    let b = grown.bounds();
    assert_eq!((b.min.x, b.max.x), (64.0, 96.0));
    assert_eq!((b.min.y, b.max.y), (0.0, 64.0));
    assert!(grown.validate().is_ok());
    assert!(!grown.overlaps(&solid), "beside it, not inside it");
}

#[test]
fn two_halves_of_a_block_merge_and_two_apart_do_not() {
    let a = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::new(32.0, 64.0, 64.0)), "a");
    let b = Solid::cube(Aabb::new(Vec3::new(32.0, 0.0, 0.0), Vec3::splat(64.0)), "b");
    let merged = merge(&[&a, &b]).unwrap();
    assert!((merged.volume() - 64.0f32.powi(3)).abs() < 1.0);
    assert_eq!(merged.sides.len(), 6);

    let far = Solid::cube(Aabb::new(Vec3::splat(128.0), Vec3::splat(160.0)), "c");
    assert_eq!(merge(&[&a, &far]).unwrap_err(), EditError::NotConvex);
    assert_eq!(merge(&[&a]).unwrap_err(), EditError::Nothing);
}

#[test]
fn corners_moved_on_the_grid_stay_on_it_however_many_times() {
    let mut solid = cube();
    let mut at = Vec3::new(64.0, 64.0, 64.0);
    for _ in 0..10 {
        solid = move_points(&solid, &[at], Vec3::new(8.0, 0.0, 16.0)).unwrap();
        at += Vec3::new(8.0, 0.0, 16.0);
    }
    for p in topology(&solid).vertices {
        for c in p.to_array() {
            assert!((c - c.round()).abs() < 1e-3, "{p} drifted off the grid");
        }
    }
}
