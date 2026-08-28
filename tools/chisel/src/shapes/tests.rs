// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;

/// A 256-unit box from the origin, the size most brushwork is drawn at.
fn box_() -> Aabb {
    Aabb::new(Vec3::ZERO, Vec3::splat(256.0))
}

/// Every solid a shape produced, checked as geometry the compiler will take.
fn all_valid(solids: &[Solid]) {
    assert!(!solids.is_empty(), "a shape that produces nothing is a tool that does nothing");
    for (i, solid) in solids.iter().enumerate() {
        assert!(solid.validate().is_ok(), "brush {i}: {:?}", solid.validate());
        let faces = solid.windings().iter().filter(|w| w.is_some()).count();
        assert!(faces >= 4, "brush {i} has only {faces} real faces");
    }
}

#[test]
fn every_shape_produces_valid_geometry_on_every_axis() {
    for shape in Shape::all() {
        for axis in 0..3 {
            let solids = build(shape, box_(), axis, Options::default(), "dev/grid");
            assert!(!solids.is_empty(), "{} on axis {axis}", shape.label());
            all_valid(&solids);
        }
    }
}

#[test]
fn every_shape_stays_inside_the_box_it_was_drawn_in() {
    // Drawing a box and getting something bigger than it is the fastest way
    // to lose trust in a tool.
    for shape in Shape::all() {
        for axis in 0..3 {
            let bounds = box_();
            for solid in build(shape, bounds, axis, Options::default(), "dev/grid") {
                let got = solid.bounds();
                for i in 0..3 {
                    assert!(got.min[i] >= bounds.min[i] - 0.01, "{} escaped: {got:?}", shape.label());
                    assert!(got.max[i] <= bounds.max[i] + 0.01, "{} escaped: {got:?}", shape.label());
                }
            }
        }
    }
}

#[test]
fn every_shape_has_a_label_and_a_reason_to_exist() {
    for shape in Shape::all() {
        assert!(!shape.label().is_empty());
        assert!(!shape.help().is_empty());
    }
}

#[test]
fn a_box_too_flat_to_hold_anything_produces_nothing_rather_than_rubbish() {
    let flat = Aabb::new(Vec3::ZERO, Vec3::new(256.0, 256.0, 0.0));
    for shape in Shape::all() {
        assert!(build(shape, flat, 2, Options::default(), "dev/grid").is_empty(), "{}", shape.label());
    }
}

#[test]
fn a_wedge_is_one_brush_and_slopes() {
    let solids = build(Shape::Wedge, box_(), 2, Options::default(), "dev/grid");
    assert_eq!(solids.len(), 1, "a ramp is one brush");
    all_valid(&solids);

    // Tall at the far end, nothing at the near one.
    let wedge = &solids[0];
    assert!(wedge.contains_point(Vec3::new(200.0, 128.0, 128.0)));
    assert!(!wedge.contains_point(Vec3::new(40.0, 128.0, 128.0)));
}

#[test]
fn a_wedge_is_a_ramp_rather_than_a_triangular_wall_standing_on_end() {
    // The slope has to be in the vertical plane. A triangle standing upright
    // is a corner piece, which is a different and much less wanted thing --
    // and the two look identical from directly above.
    let wedge = &build(Shape::Wedge, box_(), 2, Options::default(), "dev/grid")[0];

    // Full width across the axis it does not slope along...
    assert!(wedge.contains_point(Vec3::new(200.0, 8.0, 32.0)), "solid at one side");
    assert!(wedge.contains_point(Vec3::new(200.0, 248.0, 32.0)), "and at the other");

    // ...and it climbs: low down it reaches back, high up it does not.
    assert!(wedge.contains_point(Vec3::new(140.0, 128.0, 8.0)), "the floor reaches the middle");
    assert!(!wedge.contains_point(Vec3::new(140.0, 128.0, 240.0)), "the ceiling does not");
}

#[test]
fn a_wedge_slopes_the_right_way_whichever_pane_it_was_drawn_in() {
    for axis in 0..3 {
        let wedge = &build(Shape::Wedge, box_(), axis, Options::default(), "dev/grid")[0];
        let mut high = Vec3::splat(128.0);
        let mut low = Vec3::splat(128.0);
        let rise = match axis { 0 => 1, 1 => 0, _ => 0 };

        // Near the top of the sweep axis, only the far end is solid.
        high[axis] = 240.0;
        low[axis] = 240.0;
        high[rise] = 240.0;
        low[rise] = 16.0;
        assert!(wedge.contains_point(high), "axis {axis}: the high end is solid at the top");
        assert!(!wedge.contains_point(low), "axis {axis}: the low end is not");
    }
}

#[test]
fn a_cylinder_is_one_brush_however_many_sides_it_has() {
    // A convex polygon swept along a line is convex, and a convex solid is
    // one brush. Splitting it would cost the compiler faces for nothing.
    for sides in [3u32, 8, 16, 64] {
        let options = Options { sides, ..Options::default() };
        let solids = build(Shape::Cylinder, box_(), 2, options, "dev/grid");
        assert_eq!(solids.len(), 1, "{sides} sides");
        assert_eq!(solids[0].sides.len(), sides as usize + 2);
        all_valid(&solids);
    }
}

#[test]
fn a_cylinder_is_hollow_nowhere_and_round_enough_to_be_worth_it() {
    let solids = build(Shape::Cylinder, box_(), 2, Options::default(), "dev/grid");
    let pillar = &solids[0];

    assert!(pillar.contains_point(Vec3::new(128.0, 128.0, 128.0)), "solid down the middle");
    // The corners of the box are outside a shape inscribed in it.
    assert!(!pillar.contains_point(Vec3::new(4.0, 4.0, 128.0)), "the box corner is cut off");
}

#[test]
fn a_cone_narrows_to_its_apex() {
    let solids = build(Shape::Cone, box_(), 2, Options::default(), "dev/grid");
    assert_eq!(solids.len(), 1);
    all_valid(&solids);

    let cone = &solids[0];
    assert!(cone.contains_point(Vec3::new(128.0, 128.0, 8.0)), "wide at the base");
    assert!(!cone.contains_point(Vec3::new(30.0, 30.0, 240.0)), "and a point at the top");
}

#[test]
fn an_arch_is_a_brush_per_segment() {
    for sides in [4u32, 8, 16] {
        let options = Options { sides, ..Options::default() };
        let solids = build(Shape::Arch, box_(), 2, options, "dev/grid");
        assert_eq!(solids.len(), sides as usize, "{sides} segments");
        all_valid(&solids);
    }
}

#[test]
fn an_arch_has_a_hole_in_it() {
    // Otherwise it is a disc, and nobody walks through a disc.
    let options = Options { arc: 360.0, wall: 32.0, sides: 12 };
    let solids = build(Shape::Arch, box_(), 2, options, "dev/grid");
    let centre = Vec3::new(128.0, 128.0, 128.0);

    assert!(
        solids.iter().all(|s| !s.contains_point(centre)),
        "the middle of a ring is not solid"
    );
    // And the ring itself is: a point just inside the outer edge is in one.
    let on_the_ring = Vec3::new(128.0, 250.0, 128.0);
    assert!(solids.iter().any(|s| s.contains_point(on_the_ring)), "the ring is solid");
}

#[test]
fn an_arch_wall_thicker_than_the_arch_still_leaves_a_hole() {
    // A wall of 10000 is a typo, not a request for a solid disc, and a
    // generator that produced one inside out would be worse than either.
    let options = Options { arc: 360.0, wall: 10_000.0, sides: 8 };
    let solids = build(Shape::Arch, box_(), 2, options, "dev/grid");
    all_valid(&solids);
    assert!(solids.iter().all(|s| !s.contains_point(Vec3::new(128.0, 128.0, 128.0))));
}

#[test]
fn a_half_arch_is_the_top_half_because_that_is_the_bit_you_walk_under() {
    // Sweeping the other way is a bowl. Valid geometry, and not what anybody
    // asking for an arch is asking for.
    for axis in 0..3 {
        let bounds = box_();
        let over = match axis { 0 => 2, 1 => 2, _ => 1 };
        let middle = bounds.center()[over];

        let solids = build(Shape::Arch, bounds, axis, Options { arc: 180.0, ..Default::default() }, "dev/grid");
        assert!(!solids.is_empty());
        for solid in &solids {
            assert!(
                solid.bounds().max[over] > middle,
                "axis {axis}: a segment hangs below the middle: {:?}",
                solid.bounds()
            );
        }
        // And the doorway itself -- under the middle of the span -- is clear.
        let mut under = bounds.center();
        under[over] = bounds.min[over] + bounds.size()[over] * 0.1;
        assert!(solids.iter().all(|s| !s.contains_point(under)), "axis {axis}: the doorway is blocked");
    }
}

#[test]
fn a_half_arch_covers_half_the_ring() {
    let half = build(Shape::Arch, box_(), 2, Options { arc: 180.0, ..Options::default() }, "dev/grid");
    let full = build(Shape::Arch, box_(), 2, Options { arc: 360.0, ..Options::default() }, "dev/grid");

    let extent = |solids: &[Solid]| {
        let mut bounds = Aabb::EMPTY;
        for s in solids { bounds = bounds.union(&s.bounds()); }
        bounds
    };
    assert!(
        extent(&half).size().y < extent(&full).size().y * 0.75,
        "a half arch is not as tall as a whole ring"
    );
}

#[test]
fn stairs_are_a_brush_per_step_and_each_one_reaches_the_floor() {
    let options = Options { sides: 8, ..Options::default() };
    let solids = build(Shape::Stairs, box_(), 2, options, "dev/grid");
    assert_eq!(solids.len(), 8);
    all_valid(&solids);

    for step in &solids {
        assert_eq!(step.bounds().min.z, 0.0, "a step that floats is a shelf");
    }
    // And they climb.
    let heights: Vec<f32> = solids.iter().map(|s| s.bounds().max.z).collect();
    for pair in heights.windows(2) {
        assert!(pair[1] > pair[0], "each step is taller than the last: {heights:?}");
    }
}

#[test]
fn stairs_run_along_the_longer_side_of_the_box_they_are_drawn_in() {
    let long_x = Aabb::new(Vec3::ZERO, Vec3::new(512.0, 128.0, 256.0));
    let solids = build(Shape::Stairs, long_x, 2, Options::default(), "dev/grid");
    let first = solids[0].bounds();
    assert!(first.size().x < first.size().y, "the tread is narrow along the run: {first:?}");
}

#[test]
fn the_options_are_clamped_to_what_can_actually_be_built() {
    let mad = Options { sides: 0, arc: -90.0, wall: -5.0 }.sane();
    assert_eq!(mad.sides, MIN_SIDES);
    assert!(mad.arc > 0.0);
    assert!(mad.wall > 0.0);

    let huge = Options { sides: 10_000, arc: 10_000.0, wall: 1.0 }.sane();
    assert_eq!(huge.sides, MAX_SIDES);
    assert_eq!(huge.arc, 360.0);

    // And a shape built from nonsense options is still valid geometry.
    all_valid(&build(Shape::Cylinder, box_(), 2, Options { sides: 0, arc: 0.0, wall: 0.0 }, "dev/grid"));
}

#[test]
fn a_shape_carries_the_material_it_was_asked_for() {
    for shape in Shape::all() {
        for solid in build(shape, box_(), 2, Options::default(), "tools/clip") {
            assert!(solid.sides.iter().all(|s| s.material == "tools/clip"), "{}", shape.label());
        }
    }
}

#[test]
fn a_shape_swept_along_x_lies_on_its_side() {
    // A cylinder drawn in the front view is a pipe running across the room,
    // which is the whole reason the axis is a parameter.
    let pipe = build(Shape::Cylinder, box_(), 0, Options::default(), "dev/grid");
    let bounds = pipe[0].bounds();
    assert_eq!(bounds.size().x, 256.0, "full length along the sweep axis");
    assert!(bounds.size().y < 256.0, "and inscribed across it: {bounds:?}");
}

#[test]
fn only_the_shapes_that_use_a_setting_say_they_do() {
    // The panel hides the controls a shape ignores, and a control that
    // silently does nothing is worse than one that is not there.
    assert!(!Shape::Wedge.uses_sides());
    assert!(Shape::Cylinder.uses_sides());
    assert!(Shape::Arch.uses_arc() && Shape::Arch.uses_wall());
    assert!(!Shape::Cylinder.uses_arc() && !Shape::Cylinder.uses_wall());
    assert!(Shape::Stairs.uses_sides(), "the step count is the side count");
}
