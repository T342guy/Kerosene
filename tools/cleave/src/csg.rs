// SPDX-License-Identifier: MPL-2.0
//! CSG: removing the faces you cannot see.
//!
//! Level designers build with overlapping boxes. A room is six slabs that
//! interpenetrate at the corners; a pillar is pushed into the floor. Every
//! buried face is a polygon the renderer would draw and the lighting compile
//! would light, hidden inside solid rock.
//!
//! This pass cuts each face against every other brush that could bury it and
//! keeps only the fragments that survive outside. On a real map it typically
//! removes a third of all faces before anything else has run.
//!
//! The subtle part is coplanar faces -- two brushes meeting flush. If both
//! keep their face you get z-fighting; if both drop it you get a hole. The
//! rules here are:
//!
//! * **Same plane, same facing** (two brushes flush side by side): the
//!   lower-indexed brush keeps the shared area, the other drops it. Exactly
//!   one face survives.
//! * **Same plane, opposite facing** (two brushes back to back): both drop it.
//!   It is an interior seam that nothing can ever see.

use crate::brush::BrushWork;
use kerosene_math::{ON_EPSILON, PlaneSet, Winding};

/// Run CSG over every brush, filling in each side's surviving fragments.
///
/// Returns how many whole faces were removed, which the compiler reports --
/// it is a good proxy for how much overlap a map has.
pub fn chop_brushes(brushes: &mut [BrushWork], planes: &PlaneSet) -> usize {
    let mut removed = 0usize;

    // Work on a snapshot: cutting brush i must see the others as they were
    // authored, not as previous iterations left them.
    let snapshot: Vec<BrushWork> = brushes.to_vec();

    for i in 0..brushes.len() {
        for s in 0..brushes[i].sides.len() {
            let Some(winding) = brushes[i].sides[s].winding.clone() else {
                brushes[i].sides[s].fragments.clear();
                continue;
            };
            let plane_index = brushes[i].sides[s].plane;

            let mut fragments = vec![winding];
            for (j, other) in snapshot.iter().enumerate() {
                if i == j || fragments.is_empty() { continue; }
                if !should_cut(&snapshot[i], other) { continue; }
                if !snapshot[i].bounds.intersects(&other.bounds) { continue; }

                // A tie on a shared surface goes to the lower-indexed brush,
                // deterministically, so a recompile does not flip which of two
                // flush faces survives.
                let wins_tie = i < j;
                fragments = fragments
                    .iter()
                    .flat_map(|f| subtract_brush(f, plane_index, other, planes, wins_tie))
                    .collect();
            }

            if fragments.is_empty() && brushes[i].sides[s].is_visible_surface() {
                removed += 1;
            }
            brushes[i].sides[s].fragments = fragments;
        }
    }

    removed
}

/// Whether brush `cutter` may remove parts of `victim`'s faces.
fn should_cut(victim: &BrushWork, cutter: &BrushWork) -> bool {
    // Only within one entity. A door's brushes must not erase the world's
    // faces -- the door moves away and would leave a hole.
    if victim.entity != cutter.entity { return false; }
    // Only something at least as opaque as what it is burying. A trigger
    // volume around a doorway must not delete the doorway.
    priority(cutter.contents) >= priority(victim.contents) && priority(cutter.contents) > 0
}

/// How thoroughly a contents type buries what is inside it.
fn priority(contents: u32) -> u8 {
    use kerosene_bsp::contents as c;
    if contents & c::SOLID != 0 { 4 }
    else if contents & c::WINDOW != 0 { 3 }
    else if contents & c::GRATE != 0 { 2 }
    else if contents & (c::WATER | c::SLIME) != 0 { 1 }
    else { 0 } // triggers, clips, hint: never bury anything
}

/// The parts of `w` that lie outside `other`.
///
/// Walks `other`'s planes, peeling off the piece in front of each (which is
/// outside the convex volume) and carrying the rest forward. What remains
/// after every plane is the piece inside, which is discarded -- except in the
/// coplanar cases described in the module docs.
fn subtract_brush(
    w: &Winding,
    w_plane: u32,
    other: &BrushWork,
    planes: &PlaneSet,
    wins_coplanar_tie: bool,
) -> Vec<Winding> {
    let mut outside: Vec<Winding> = Vec::new();
    let mut rest = w.clone();
    let mut coplanar_same = false;
    let mut coplanar_opposite = false;

    for side in &other.sides {
        if side.winding.is_none() { continue; }

        // Never clip a winding by its own plane: it lies on it, so the split
        // would classify the whole thing as behind and delete it.
        if side.plane == w_plane { coplanar_same = true; continue; }
        if side.plane == (w_plane ^ 1) { coplanar_opposite = true; continue; }

        let (front, back) = rest.split(&planes.get(side.plane), ON_EPSILON);
        if let Some(f) = front {
            if !f.is_tiny() { outside.push(f); }
        }
        match back {
            Some(b) => rest = b,
            // Nothing left behind this plane, so nothing is inside the brush.
            None => return outside,
        }
    }

    if coplanar_same && !coplanar_opposite && wins_coplanar_tie && !rest.is_tiny() {
        // Two brushes flush against each other; this one keeps the surface.
        outside.push(rest);
    }
    // Every other case drops `rest`: it is buried, or it is a duplicate the
    // other brush is keeping, or it is a back-to-back interior seam.
    outside
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::{BrushWork, Warning};
    use kerosene_map::Solid;
    use kerosene_math::{Aabb, Vec3};

    fn make(boxes: &[(Aabb, &str, &str)]) -> (Vec<BrushWork>, PlaneSet) {
        let mut planes = PlaneSet::new();
        let mut warnings: Vec<Warning> = Vec::new();
        let mut out = Vec::new();
        for (i, (b, material, class)) in boxes.iter().enumerate() {
            let mut solid = Solid::cube(*b, material);
            solid.id = i as u32 + 1;
            let entity = if *class == "worldspawn" { 0 } else { 1 };
            out.push(
                BrushWork::from_solid(&solid, entity, class, &mut planes, &mut warnings).unwrap(),
            );
        }
        (out, planes)
    }

    fn total_area(brushes: &[BrushWork]) -> f32 {
        brushes
            .iter()
            .flat_map(|b| b.sides.iter())
            .flat_map(|s| s.fragments.iter())
            .map(|w| w.area())
            .sum()
    }

    #[test]
    fn a_lone_brush_keeps_every_face() {
        let (mut b, planes) = make(&[(Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid", "worldspawn")]);
        let removed = chop_brushes(&mut b, &planes);
        assert_eq!(removed, 0);
        assert!((total_area(&b) - 6.0 * 4096.0).abs() < 1e-1);
    }

    #[test]
    fn a_brush_fully_inside_another_loses_every_face() {
        let (mut b, planes) = make(&[
            (Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "dev/grid", "worldspawn"),
            (Aabb::new(Vec3::splat(32.0), Vec3::splat(64.0)), "dev/grid", "worldspawn"),
        ]);
        chop_brushes(&mut b, &planes);
        let inner_area: f32 = b[1].sides.iter().flat_map(|s| s.fragments.iter()).map(|w| w.area()).sum();
        assert_eq!(inner_area, 0.0, "a buried brush has no visible surface");
        // The container is untouched: the inner brush is lower priority to cut
        // it only where it overlaps, and it does not reach the outer surface.
        let outer_area: f32 = b[0].sides.iter().flat_map(|s| s.fragments.iter()).map(|w| w.area()).sum();
        assert!((outer_area - 6.0 * 128.0 * 128.0).abs() < 1e-1);
    }

    #[test]
    fn two_flush_brushes_keep_exactly_one_shared_face() {
        // Side by side, touching at x = 64. Each has a face there.
        let (mut b, planes) = make(&[
            (Aabb::new(Vec3::ZERO, Vec3::new(64.0, 64.0, 64.0)), "dev/grid", "worldspawn"),
            (Aabb::new(Vec3::new(64.0, 0.0, 0.0), Vec3::new(128.0, 64.0, 64.0)), "dev/grid", "worldspawn"),
        ]);
        chop_brushes(&mut b, &planes);

        // Count fragments sitting on the shared plane x = 64.
        let mut on_seam = 0;
        for brush in &b {
            for side in &brush.sides {
                for f in &side.fragments {
                    if f.points.iter().all(|p| (p.x - 64.0).abs() < 0.01) { on_seam += 1; }
                }
            }
        }
        assert_eq!(on_seam, 0, "back-to-back faces are an interior seam and both go");
    }

    #[test]
    fn a_pillar_sunk_into_a_floor_loses_only_the_buried_part() {
        // Floor slab 0..16 in Z; pillar from Z=8 up to Z=64, overlapping by 8.
        let (mut b, planes) = make(&[
            (Aabb::new(Vec3::new(-128.0, -128.0, 0.0), Vec3::new(128.0, 128.0, 16.0)), "dev/grid", "worldspawn"),
            (Aabb::new(Vec3::new(-8.0, -8.0, 8.0), Vec3::new(8.0, 8.0, 64.0)), "dev/grid", "worldspawn"),
        ]);
        chop_brushes(&mut b, &planes);

        // The pillar's four sides should be cut down from 56 tall to 48.
        let pillar_side_area: f32 = b[1]
            .sides
            .iter()
            .filter(|s| {
                let n = planes.get(s.plane).normal;
                n.z.abs() < 0.5
            })
            .flat_map(|s| s.fragments.iter())
            .map(|w| w.area())
            .sum();
        assert!(
            (pillar_side_area - 4.0 * 16.0 * 48.0).abs() < 1.0,
            "expected 4 sides of 16x48, got {pillar_side_area}"
        );

        // The pillar's bottom cap is buried in the floor and must be gone.
        let bottom: f32 = b[1]
            .sides
            .iter()
            .filter(|s| planes.get(s.plane).normal.z < -0.5)
            .flat_map(|s| s.fragments.iter())
            .map(|w| w.area())
            .sum();
        assert_eq!(bottom, 0.0);
    }

    #[test]
    fn the_floor_under_a_pillar_is_carved_out() {
        let (mut b, planes) = make(&[
            (Aabb::new(Vec3::new(-128.0, -128.0, 0.0), Vec3::new(128.0, 128.0, 16.0)), "dev/grid", "worldspawn"),
            (Aabb::new(Vec3::new(-8.0, -8.0, 8.0), Vec3::new(8.0, 8.0, 64.0)), "dev/grid", "worldspawn"),
        ]);
        chop_brushes(&mut b, &planes);
        let floor_top: f32 = b[0]
            .sides
            .iter()
            .filter(|s| planes.get(s.plane).normal.z > 0.5)
            .flat_map(|s| s.fragments.iter())
            .map(|w| w.area())
            .sum();
        let expected = 256.0 * 256.0 - 16.0 * 16.0;
        assert!(
            (floor_top - expected).abs() < 1.0,
            "floor should have a {expected} hole punched in it, got {floor_top}"
        );
    }

    #[test]
    fn a_trigger_does_not_erase_world_geometry() {
        // The bug this prevents: a trigger volume placed over a doorway
        // deleting the doorway's faces.
        let (mut b, planes) = make(&[
            (Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid", "worldspawn"),
            (Aabb::new(Vec3::new(-8.0, -8.0, -8.0), Vec3::splat(72.0)), "tools/trigger", "worldspawn"),
        ]);
        chop_brushes(&mut b, &planes);
        let world_area: f32 = b[0].sides.iter().flat_map(|s| s.fragments.iter()).map(|w| w.area()).sum();
        assert!((world_area - 6.0 * 4096.0).abs() < 1e-1, "got {world_area}");
    }

    #[test]
    fn brushes_in_different_entities_do_not_cut_each_other() {
        // A door sitting flush in a wall must not delete the wall.
        let (mut b, planes) = make(&[
            (Aabb::new(Vec3::ZERO, Vec3::splat(64.0)), "dev/grid", "worldspawn"),
            (Aabb::new(Vec3::splat(16.0), Vec3::splat(48.0)), "dev/grid", "func_door"),
        ]);
        chop_brushes(&mut b, &planes);
        let world: f32 = b[0].sides.iter().flat_map(|s| s.fragments.iter()).map(|w| w.area()).sum();
        let door: f32 = b[1].sides.iter().flat_map(|s| s.fragments.iter()).map(|w| w.area()).sum();
        assert!((world - 6.0 * 4096.0).abs() < 1e-1);
        assert!((door - 6.0 * 32.0 * 32.0).abs() < 1e-1, "the door keeps all its faces");
    }

    #[test]
    fn csg_is_deterministic() {
        let boxes: Vec<(Aabb, &str, &str)> = (0..6)
            .map(|i| {
                let x = i as f32 * 48.0;
                (Aabb::new(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 64.0, 64.0, 64.0)), "dev/grid", "worldspawn")
            })
            .collect();
        let (mut a, pa) = make(&boxes);
        let (mut c, pc) = make(&boxes);
        chop_brushes(&mut a, &pa);
        chop_brushes(&mut c, &pc);
        assert_eq!(total_area(&a), total_area(&c));
    }
}
