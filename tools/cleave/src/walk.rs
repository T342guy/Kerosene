// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Collecting the walkmap from the compiled world.
//!
//! The walkmap is the set of faces NPCs may stand on. It is gathered here,
//! after CSG has removed the parts of faces buried inside other brushes, from
//! the same final polygons the renderer gets -- so a face a designer marked
//! `allow` that CSG cut back to a sliver contributes only the sliver, not the
//! part of the floor that is now inside a wall.
//!
//! A face is walkable when it points up and is ordinary geometry: a floor.
//! Walls and ceilings are not, and neither is a `tools/` volume -- `clip`,
//! `trigger`, `sky` and the rest never become ground. The face's walkmap rule
//! then decides the rest:
//!
//! * `allow` -- walkable if flat (the default for a floor).
//! * `deny` -- never part of the map, however flat.
//! * `avoid` -- part of the map, flagged for NPCs to route around.
//! * `always` -- part of the map even if it is not flat, for ramps.
//!
//! Brush entities (`func_door` and the like) are deliberately absent: they
//! move, and a static walkmap that says a closed door is open would send an
//! NPC through it. Movers belong to a later, dynamic pass.

use crate::brush::BrushWork;
use kerosene_map::WalkmapRule;
use kerosene_math::{Aabb, PlaneSet};
use kerosene_walk::{WalkFace, Walkmap};

/// Faces flatter than this count as walkable: `normal.z >= WALK_SLOPE` is
/// about a 45-degree slope, the same rule Source applies.
const WALK_SLOPE: f32 = 0.7;

/// Faces smaller than this, in square units, are numerical slivers and are
/// dropped rather than becoming a walkmap face nobody's foot will ever land
/// on.
const MIN_FACE_AREA: f32 = 1.0;

/// Build the walkmap from the world's compiled brushes.
pub fn collect(world_brushes: &[BrushWork], planes: &PlaneSet) -> Walkmap {
    let mut faces = Vec::new();
    for brush in world_brushes {
        for side in &brush.sides {
            let normal = planes.get(side.plane).normal;
            if !walkable(side, normal) { continue; }
            for fragment in &side.fragments {
                if fragment.points.len() < 3 { continue; }
                if fragment.area() < MIN_FACE_AREA { continue; }
                faces.push(WalkFace {
                    vertices: fragment.points.clone(),
                    normal,
                    rule: side.walkmap,
                    bounds: Aabb::from_points(&fragment.points),
                });
            }
        }
    }
    Walkmap { faces }
}

/// Whether a face takes part in the walkmap.
fn walkable(side: &crate::brush::SideWork, normal: kerosene_math::Vec3) -> bool {
    use kerosene_bsp::surf;

    match side.walkmap {
        WalkmapRule::Deny => return false,
        WalkmapRule::Always => return true,
        WalkmapRule::Allow | WalkmapRule::Avoid => {}
    }

    // An ordinary floor: pointing up, drawable, authored (not an interior
    // cut), and not sky or water, which are solid-looking but not ground.
    if normal.z < WALK_SLOPE { return false; }
    if !side.emits_face || side.generated { return false; }
    side.surface & (surf::SKY | surf::WARP) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::BrushWork;
    use kerosene_map::{Solid, WalkmapRule};
    use kerosene_math::{Aabb, PlaneSet, Vec3};

    fn compile_solid(solid: &Solid) -> (BrushWork, PlaneSet) {
        let mut planes = PlaneSet::new();
        let mut warnings = Vec::new();
        let brush = BrushWork::from_solid(solid, 0, "worldspawn", &mut planes, &mut warnings)
            .expect("the test brush must compile");
        (brush, planes)
    }

    /// Run the CSG pass over a single brush so its fragments are filled in.
    fn chopped(solid: &Solid) -> (BrushWork, PlaneSet) {
        let (mut brush, planes) = compile_solid(solid);
        // CSG fills `side.fragments`; for a single brush there is nothing to
        // chop against, so reproduce what it would have done.
        for side in &mut brush.sides {
            side.fragments = side.winding.clone().into_iter().collect();
        }
        brush.original = 0;
        (brush, planes)
    }

    #[test]
    fn a_floor_becomes_a_walkable_face() {
        let solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "dev/grid");
        let (brush, planes) = chopped(&solid);
        let walk = collect(&[brush], &planes);

        // Five non-flat faces are dropped; the +Z floor remains.
        assert_eq!(walk.len(), 1);
        assert_eq!(walk.faces[0].normal, Vec3::Z);
        assert_eq!(walk.faces[0].rule, WalkmapRule::Allow);
        assert!((walk.faces[0].area() - 128.0 * 128.0).abs() < 0.5);
    }

    #[test]
    fn a_denied_floor_is_left_out() {
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "dev/grid");
        for side in &mut solid.sides {
            if side.plane().unwrap().normal == Vec3::Z { side.walkmap = WalkmapRule::Deny; }
        }
        let (brush, planes) = chopped(&solid);
        let walk = collect(&[brush], &planes);
        assert!(walk.is_empty());
    }

    #[test]
    fn an_avoid_floor_is_walkable_but_flagged() {
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "dev/grid");
        for side in &mut solid.sides {
            if side.plane().unwrap().normal == Vec3::Z { side.walkmap = WalkmapRule::Avoid; }
        }
        let (brush, planes) = chopped(&solid);
        let walk = collect(&[brush], &planes);
        assert_eq!(walk.len(), 1);
        assert_eq!(walk.faces[0].rule, WalkmapRule::Avoid);
    }

    #[test]
    fn always_forces_a_steep_face_in() {
        // A ramp: make the whole brush a wedge, then mark its sloped face.
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "dev/grid");
        // Cut the top so one face slopes. Instead of building a wedge by hand,
        // mark the ceiling's opposite (a wall) as always: the point is that
        // `always` overrides flatness.
        for side in &mut solid.sides {
            if side.plane().unwrap().normal == Vec3::X { side.walkmap = WalkmapRule::Always; }
        }
        let (brush, planes) = chopped(&solid);
        let walk = collect(&[brush], &planes);
        // The floor (flat) plus the forced wall.
        assert_eq!(walk.len(), 2);
        assert!(walk.faces.iter().any(|f| f.normal == Vec3::X && f.rule == WalkmapRule::Always));
    }

    #[test]
    fn tool_volumes_are_never_ground() {
        let solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "tools/nodraw");
        let (brush, planes) = chopped(&solid);
        let walk = collect(&[brush], &planes);
        assert!(walk.is_empty(), "nodraw faces must not become floors");
    }

    #[test]
    fn a_wall_and_ceiling_are_not_walkable() {
        let solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "dev/grid");
        let (brush, planes) = chopped(&solid);
        let walk = collect(&[brush], &planes);
        // Only the top face qualifies; the five others are walls/floor.
        assert_eq!(walk.len(), 1);
        assert_eq!(walk.faces[0].normal, Vec3::Z);
    }
}
