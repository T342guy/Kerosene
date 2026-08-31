// SPDX-License-Identifier: MPL-2.0
use super::*;
use crate::types::{Brush, BrushSide, BspPlane, Leaf, Model, Node, TexData, TexInfo, encode_leaf};

/// A world consisting of one open leaf holding a single 64-unit cube brush.
///
/// Enough to exercise brush clipping directly, without a tree in the way.
fn cube_world(brush_contents: u32) -> Bsp {
    let mut bsp = Bsp::new();
    let mut planes = kerosene_math::PlaneSet::new();

    let faces = [
        (Vec3::X, 64.0), (-Vec3::X, 0.0),
        (Vec3::Y, 64.0), (-Vec3::Y, 0.0),
        (Vec3::Z, 64.0), (-Vec3::Z, 0.0),
    ];
    let name = bsp.intern_texdata_string("dev/grid");
    bsp.texdata.push(TexData { name_offset: name, ..Default::default() });
    bsp.texinfo.push(TexInfo { texdata: 0, ..Default::default() });

    for (normal, dist) in faces {
        let index = planes.insert(Plane::new(normal, dist));
        bsp.brushsides.push(BrushSide { plane: index, texinfo: 0, bevel: 0 });
    }
    bsp.planes = planes.planes().iter().map(BspPlane::from_plane).collect();
    bsp.brushes.push(Brush { first_side: 0, num_sides: 6, contents: brush_contents });
    bsp.leafbrushes.push(0);

    bsp.leaves.push(Leaf {
        contents: content_flags::EMPTY,
        first_leafbrush: 0,
        num_leafbrushes: 1,
        cluster: 0,
        mins: [-512; 3],
        maxs: [512; 3],
        ..Default::default()
    });
    bsp.models.push(Model {
        mins: [-512.0; 3],
        maxs: [512.0; 3],
        origin: [0.0; 3],
        head_node: encode_leaf(0),
        first_face: 0,
        num_faces: 0,
    });
    bsp.validate().expect("fixture is well formed");
    bsp
}

fn solid_cube() -> Bsp { cube_world(content_flags::SOLID) }

#[test]
fn a_ray_stops_at_the_near_face() {
    let bsp = solid_cube();
    let t = bsp.trace_ray(
        Vec3::new(-100.0, 32.0, 32.0),
        Vec3::new(100.0, 32.0, 32.0),
        content_flags::MASK_SOLID,
    );
    assert!(t.hit());
    // The cube starts at x = 0, half way along a 200-unit path.
    assert!((t.fraction - 0.5).abs() < 0.01, "fraction {}", t.fraction);
    assert!((t.endpos.x - 0.0).abs() < 0.1, "stopped at {:?}", t.endpos);
    assert_eq!(t.plane.unwrap().normal, -Vec3::X, "should hit the -X face");
    assert!(!t.start_solid && !t.all_solid);
}

#[test]
fn a_ray_that_misses_reaches_the_end() {
    let bsp = solid_cube();
    let end = Vec3::new(100.0, 200.0, 32.0);
    let t = bsp.trace_ray(Vec3::new(-100.0, 200.0, 32.0), end, content_flags::MASK_SOLID);
    assert!(!t.hit());
    assert_eq!(t.fraction, 1.0);
    assert_eq!(t.endpos, end);
    assert!(t.plane.is_none());
}

#[test]
fn a_ray_just_clear_of_a_face_does_not_catch_on_it() {
    // Sliding along just above the +Z face must not snag. This is the case
    // that produces phantom collisions when the epsilons are wrong, and it is
    // why movement keeps the player a hair off the ground rather than exactly
    // on it -- a ray *exactly* coplanar with a face does touch the brush, and
    // is reported as a hit.
    let bsp = solid_cube();
    let clear = bsp.trace_ray(
        Vec3::new(-100.0, 32.0, 64.0 + kerosene_math::DIST_EPSILON * 2.0),
        Vec3::new(100.0, 32.0, 64.0 + kerosene_math::DIST_EPSILON * 2.0),
        content_flags::MASK_SOLID,
    );
    assert!(!clear.hit(), "a ray above the surface must pass");

    let exactly_on = bsp.trace_ray(
        Vec3::new(-100.0, 32.0, 64.0),
        Vec3::new(100.0, 32.0, 64.0),
        content_flags::MASK_SOLID,
    );
    assert!(exactly_on.hit(), "a ray in the surface plane is touching the brush");
}

#[test]
fn a_box_stops_further_out_than_a_ray() {
    let bsp = solid_cube();
    let (start, end) = (Vec3::new(-100.0, 32.0, 32.0), Vec3::new(100.0, 32.0, 32.0));
    let ray = bsp.trace_ray(start, end, content_flags::MASK_SOLID);

    let half = Vec3::splat(16.0);
    let boxed = bsp.trace_box(start, end, -half, half, content_flags::MASK_SOLID);
    assert!(boxed.hit());
    assert!(
        boxed.fraction < ray.fraction,
        "a 32-wide box should stop 16 units earlier: box {} vs ray {}",
        boxed.fraction, ray.fraction
    );
    // Its centre should end 16 units short of the face.
    assert!((boxed.endpos.x + 16.0).abs() < 0.2, "box centre at {:?}", boxed.endpos);
}

#[test]
fn a_box_that_fits_past_the_corner_gets_through() {
    let bsp = solid_cube();
    let half = Vec3::splat(8.0);
    // Passing well clear of the cube's +Y side.
    let t = bsp.trace_box(
        Vec3::new(-100.0, 100.0, 32.0),
        Vec3::new(100.0, 100.0, 32.0),
        -half,
        half,
        content_flags::MASK_SOLID,
    );
    assert!(!t.hit());
}

#[test]
fn starting_inside_a_brush_is_reported() {
    let bsp = solid_cube();
    let t = bsp.trace_ray(
        Vec3::splat(32.0),
        Vec3::new(200.0, 32.0, 32.0),
        content_flags::MASK_SOLID,
    );
    assert!(t.start_solid, "a trace beginning inside solid must say so");
    assert!(!t.all_solid, "it does leave the brush");
}

#[test]
fn a_trace_entirely_inside_solid_is_all_solid() {
    let bsp = solid_cube();
    let t = bsp.trace_ray(
        Vec3::new(16.0, 32.0, 32.0),
        Vec3::new(48.0, 32.0, 32.0),
        content_flags::MASK_SOLID,
    );
    assert!(t.start_solid && t.all_solid);
    assert_eq!(t.fraction, 0.0);
}

#[test]
fn the_mask_decides_what_stops_a_trace() {
    // A player clip blocks players but not bullets.
    let bsp = cube_world(content_flags::PLAYER_CLIP);
    let (start, end) = (Vec3::new(-100.0, 32.0, 32.0), Vec3::new(100.0, 32.0, 32.0));

    let player = bsp.trace_ray(start, end, content_flags::MASK_PLAYER_SOLID);
    assert!(player.hit(), "a player should be stopped by a clip brush");

    let shot = bsp.trace_ray(start, end, content_flags::MASK_SHOT);
    assert!(!shot.hit(), "a bullet should pass through a clip brush");
}

#[test]
fn a_grate_blocks_movement_but_not_sight() {
    let bsp = cube_world(content_flags::GRATE);
    let (start, end) = (Vec3::new(-100.0, 32.0, 32.0), Vec3::new(100.0, 32.0, 32.0));
    assert!(bsp.trace_ray(start, end, content_flags::MASK_PLAYER_SOLID).hit());
    assert!(
        bsp.is_visible_between(start, end, content_flags::MASK_OPAQUE),
        "you can see through a grate"
    );
}

#[test]
fn the_hit_surface_reports_its_flags() {
    let mut bsp = solid_cube();
    bsp.texinfo[0].flags = crate::surf::SKY;
    let t = bsp.trace_ray(
        Vec3::new(-100.0, 32.0, 32.0),
        Vec3::new(100.0, 32.0, 32.0),
        content_flags::MASK_SOLID,
    );
    assert_eq!(t.surface_flags, crate::surf::SKY, "a shadow ray needs to know it hit sky");
}

#[test]
fn traces_are_reversible() {
    // Firing the other way should stop at the far face.
    let bsp = solid_cube();
    let forward = bsp.trace_ray(
        Vec3::new(-100.0, 32.0, 32.0),
        Vec3::new(100.0, 32.0, 32.0),
        content_flags::MASK_SOLID,
    );
    let backward = bsp.trace_ray(
        Vec3::new(100.0, 32.0, 32.0),
        Vec3::new(-100.0, 32.0, 32.0),
        content_flags::MASK_SOLID,
    );
    assert!((forward.endpos.x - 0.0).abs() < 0.1);
    assert!((backward.endpos.x - 64.0).abs() < 0.1);
}

#[test]
fn a_zero_length_trace_does_not_divide_by_zero() {
    let bsp = solid_cube();
    let p = Vec3::new(-100.0, 32.0, 32.0);
    let t = bsp.trace_ray(p, p, content_flags::MASK_SOLID);
    assert!(t.fraction.is_finite());
    assert!(!t.hit());
}

#[test]
fn point_contents_finds_brushes_inside_open_leaves() {
    // A detail or clip brush sits inside an *empty* leaf, so the leaf's own
    // contents say nothing about it.
    let bsp = cube_world(content_flags::WATER);
    assert_eq!(bsp.point_contents(Vec3::splat(32.0)), content_flags::EMPTY);
    assert!(
        bsp.point_contents_brushes(Vec3::splat(32.0)) & content_flags::WATER != 0,
        "standing in water should be detectable"
    );
    assert_eq!(bsp.point_contents_brushes(Vec3::splat(200.0)) & content_flags::WATER, 0);
}

#[test]
fn traces_walk_a_real_tree() {
    // The single-leaf fixture skips node traversal entirely; this exercises it.
    let mut bsp = solid_cube();
    let empty = bsp.leaves.len();
    bsp.leaves.push(Leaf {
        contents: content_flags::EMPTY,
        cluster: 1,
        mins: [-512; 3],
        maxs: [512; 3],
        ..Default::default()
    });
    let plane = bsp.planes.len() as u32;
    bsp.planes.push(BspPlane::from_plane(&Plane::new(Vec3::X, -32.0)));
    bsp.planes.push(BspPlane::from_plane(&Plane::new(-Vec3::X, 32.0)));
    bsp.nodes.push(Node {
        plane,
        // In front of x = -32 is the cube's leaf; behind it is empty space.
        children: [encode_leaf(0), encode_leaf(empty)],
        mins: [-512; 3],
        maxs: [512; 3],
        first_face: 0,
        num_faces: 0,
        area: 0,
    });
    bsp.models[0].head_node = 0;
    bsp.validate().unwrap();

    let t = bsp.trace_ray(
        Vec3::new(-100.0, 32.0, 32.0),
        Vec3::new(100.0, 32.0, 32.0),
        content_flags::MASK_SOLID,
    );
    assert!(t.hit());
    assert!((t.endpos.x - 0.0).abs() < 0.2, "stopped at {:?}", t.endpos);
}
