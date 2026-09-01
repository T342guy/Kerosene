// SPDX-License-Identifier: MPL-2.0
//! The compiler's working representation of a brush.
//!
//! A `.keromap` solid is a list of planes. Cleave needs more: interned plane
//! indices, computed face polygons, resolved contents and surface flags, and
//! the ability to be cut in half by an arbitrary plane while staying convex.

use crate::material;
use kerosene_map::{Solid, TextureAxis, WalkmapRule};
use kerosene_math::{Aabb, ON_EPSILON, Plane, PlaneSet, PlaneSide, Vec3, Winding};

/// One face of a working brush.
#[derive(Clone, Debug)]
pub struct SideWork {
    /// Index into the compile's [`PlaneSet`]. `plane ^ 1` is the inverse.
    pub plane: u32,
    pub material: String,
    /// The face polygon, or `None` if this plane bounds nothing.
    pub winding: Option<Winding>,
    pub surface: u32,
    /// Whether this face can become a drawable face in the output.
    pub emits_face: bool,
    pub uaxis: TextureAxis,
    pub vaxis: TextureAxis,
    pub lightmap_scale: f32,
    pub smoothing_groups: u32,
    /// How this face participates in the NPC walkmap.
    pub walkmap: WalkmapRule,
    /// `side` id from the source map, for error messages that point back at
    /// something the designer can click on.
    pub map_side_id: u32,
    /// Pieces of this face left after CSG removed the parts buried inside
    /// other brushes. Filled in by the CSG pass.
    pub fragments: Vec<Winding>,
    /// True for planes Cleave introduced while splitting the brush. These are
    /// interior cuts: they bound collision but must never render.
    pub generated: bool,
    /// Set once this side's plane has been used as a BSP node plane on the
    /// path down to here. A solid brush whose every side has been used has
    /// been fully carved out of space, which is how a leaf learns it is solid.
    pub used_as_node: bool,
}

impl SideWork {
    pub fn is_visible_surface(&self) -> bool {
        self.emits_face && !self.generated
    }
}

/// A convex brush being compiled.
#[derive(Clone, Debug)]
pub struct BrushWork {
    /// `solid` id from the source map.
    pub map_id: u32,
    /// Index of the brush this one was cut from, into the compile's original
    /// brush list. Splitting preserves it, so a fragment sitting in a leaf can
    /// still name the whole brush for the collision lumps.
    pub original: usize,
    /// Index into the compile's entity list; 0 is `worldspawn`.
    pub entity: usize,
    pub sides: Vec<SideWork>,
    pub contents: u32,
    pub bounds: Aabb,
}

/// Something the compiler wants the designer to know about.
#[derive(Clone, Debug)]
pub struct Warning {
    pub brush_id: u32,
    pub message: String,
}

impl BrushWork {
    /// Convert a map solid, interning its planes.
    ///
    /// Returns `None` if the solid encloses no volume -- an authoring mistake
    /// that would otherwise become an invisible, uncollidable ghost.
    pub fn from_solid(
        solid: &Solid,
        entity: usize,
        classname: &str,
        planes: &mut PlaneSet,
        warnings: &mut Vec<Warning>,
    ) -> Option<BrushWork> {
        let mut sides = Vec::with_capacity(solid.sides.len());
        let mut face_contents: Vec<u32> = Vec::new();

        for side in &solid.sides {
            let Some(plane) = side.plane() else {
                warnings.push(Warning {
                    brush_id: solid.id,
                    message: format!("face {} has a degenerate plane and was dropped", side.id),
                });
                continue;
            };
            if !material::is_known_tool(&side.material) {
                warnings.push(Warning {
                    brush_id: solid.id,
                    message: format!(
                        "face {} uses unknown tool material '{}'; treated as world geometry",
                        side.id, side.material
                    ),
                });
            }
            let flags = material::flags_for(&side.material);
            face_contents.push(flags.contents);
            sides.push(SideWork {
                plane: planes.insert(plane),
                material: side.material.clone(),
                winding: None,
                surface: flags.surface,
                emits_face: flags.emits_face,
                uaxis: side.uaxis,
                vaxis: side.vaxis,
                lightmap_scale: side.lightmap_scale,
                smoothing_groups: side.smoothing_groups,
                walkmap: side.walkmap,
                map_side_id: side.id,
                fragments: Vec::new(),
                generated: false,
                used_as_node: false,
            });
        }

        if sides.len() < 4 {
            warnings.push(Warning {
                brush_id: solid.id,
                message: format!("has only {} usable faces and was dropped", sides.len()),
            });
            return None;
        }

        let contents = match material::contents_for_classname(classname) {
            // The entity's class wins: a trigger_multiple is a trigger no
            // matter what its faces are textured with.
            Some(c) => c,
            None => resolve_contents(&face_contents),
        };

        let mut brush = BrushWork {
            map_id: solid.id,
            original: usize::MAX,
            entity,
            sides,
            contents,
            bounds: Aabb::EMPTY,
        };
        brush.recompute_windings(planes);

        if brush.face_count() < 4 {
            warnings.push(Warning {
                brush_id: solid.id,
                message: "encloses no volume and was dropped".to_string(),
            });
            return None;
        }
        Some(brush)
    }

    /// Recompute every face polygon from the plane set.
    ///
    /// Each face starts as the whole of its plane and is cut back by every
    /// other face turned inward. Faces sharing a plane index are skipped
    /// against each other: clipping a plane by its own inverse would delete it.
    pub fn recompute_windings(&mut self, planes: &PlaneSet) {
        let ps: Vec<Plane> = self.sides.iter().map(|s| planes.get(s.plane)).collect();
        let indices: Vec<u32> = self.sides.iter().map(|s| s.plane).collect();

        for i in 0..self.sides.len() {
            let mut w = Winding::base_for_plane(&ps[i]);
            let mut alive = true;
            for j in 0..self.sides.len() {
                if i == j || indices[j] == indices[i] { continue; }
                match w.clipped(&ps[j].flipped(), ON_EPSILON) {
                    Some(next) => w = next,
                    None => { alive = false; break; }
                }
            }
            self.sides[i].winding = if alive {
                w.remove_collinear();
                (!w.is_tiny()).then_some(w)
            } else {
                None
            };
        }
        self.recompute_bounds();
    }

    pub fn recompute_bounds(&mut self) {
        let mut b = Aabb::EMPTY;
        for side in &self.sides {
            if let Some(w) = &side.winding {
                for p in &w.points { b.add_point(*p); }
            }
        }
        self.bounds = b;
    }

    /// How many faces actually bound the volume.
    pub fn face_count(&self) -> usize {
        self.sides.iter().filter(|s| s.winding.is_some()).count()
    }

    pub fn is_detail(&self) -> bool {
        self.contents & kerosene_bsp::contents::DETAIL != 0
    }

    /// Whether this brush splits the world tree.
    ///
    /// Detail brushes and non-solid volumes (triggers, clips, water) stay out
    /// of the structural tree. That is the single biggest lever on compile
    /// time: a handrail modelled from thirty brushes would otherwise carve the
    /// room into thirty slivers, each of which the vis compile then has to
    /// consider.
    pub fn is_structural(&self) -> bool {
        use kerosene_bsp::contents as c;
        if self.is_detail() { return false; }
        self.contents & (c::SOLID | c::WINDOW | c::GRATE | c::OPAQUE) != 0
    }

    pub fn contains_point(&self, p: Vec3, planes: &PlaneSet) -> bool {
        self.sides.iter().all(|s| planes.get(s.plane).distance_to(p) <= ON_EPSILON)
    }

    /// Which side of a plane this brush is on.
    pub fn classify(&self, plane: &Plane) -> PlaneSide {
        // The bounds test settles the common case without touching windings.
        match self.bounds.classify(plane) {
            PlaneSide::Cross => {}
            decided => return decided,
        }
        let (mut front, mut back) = (false, false);
        for side in &self.sides {
            let Some(w) = &side.winding else { continue };
            for &p in &w.points {
                let d = plane.distance_to(p);
                if d > ON_EPSILON { front = true; }
                else if d < -ON_EPSILON { back = true; }
                if front && back { return PlaneSide::Cross; }
            }
        }
        match (front, back) {
            (true, false) => PlaneSide::Front,
            (false, true) => PlaneSide::Back,
            (false, false) => PlaneSide::On,
            (true, true) => PlaneSide::Cross,
        }
    }

    /// Cut the brush by a plane, returning the front and back pieces.
    ///
    /// Both halves stay convex because each simply gains one more half-space.
    /// The new face is marked `generated` so it can bound collision without
    /// ever being drawn -- it is an interior cut, not a surface anyone can see.
    pub fn split(&self, plane_index: u32, planes: &PlaneSet) -> (Option<BrushWork>, Option<BrushWork>) {
        let plane = planes.get(plane_index);
        match self.classify(&plane) {
            PlaneSide::Front => return (Some(self.clone()), None),
            PlaneSide::Back | PlaneSide::On => return (None, Some(self.clone())),
            PlaneSide::Cross => {}
        }

        // The cross-section of the brush by the cutting plane. If there is
        // none, the classification above was float noise and the brush really
        // does lie on one side.
        let mut mid = Winding::base_for_plane(&plane);
        for side in &self.sides {
            if side.plane == plane_index || side.plane == (plane_index ^ 1) { continue; }
            match mid.clipped(&planes.get(side.plane).flipped(), ON_EPSILON) {
                Some(next) => mid = next,
                None => return (Some(self.clone()), None),
            }
        }
        if mid.is_tiny() { return (Some(self.clone()), None); }

        let make = |extra_plane: u32| -> Option<BrushWork> {
            let mut b = self.clone();
            b.sides.push(SideWork {
                plane: extra_plane,
                material: "tools/nodraw".to_string(),
                winding: None,
                surface: kerosene_bsp::surf::NODRAW,
                emits_face: false,
                uaxis: TextureAxis::default(),
                vaxis: TextureAxis::default(),
                lightmap_scale: kerosene_map::DEFAULT_LIGHTMAP_SCALE,
                smoothing_groups: 0,
                walkmap: WalkmapRule::Allow,
                map_side_id: 0,
                fragments: Vec::new(),
                generated: true,
                used_as_node: false,
            });
            b.recompute_windings(planes);
            (b.face_count() >= 4).then_some(b)
        };

        // The front piece is bounded below by the plane facing *back*, and
        // vice versa -- a brush's own faces always point outward.
        (make(plane_index ^ 1), make(plane_index))
    }
}

/// Decide a brush's contents from its faces' materials.
///
/// A brush is usually uniform. When it is not -- a clip material on one face
/// of an otherwise solid brush -- the non-default contents wins, because that
/// is what the designer was reaching for; painting one face of a block with
/// `tools/clip` is how you say "this whole block is a clip brush".
pub fn resolve_contents(face_contents: &[u32]) -> u32 {
    use kerosene_bsp::contents as c;
    let mut combined = 0u32;
    for &f in face_contents {
        if f != c::SOLID { combined |= f; }
    }
    if combined == 0 { c::SOLID } else { combined }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_map::Solid;

    fn cube(min: f32, max: f32, material: &str) -> Solid {
        Solid::cube(Aabb::new(Vec3::splat(min), Vec3::splat(max)), material)
    }

    fn build(solid: &Solid) -> (BrushWork, PlaneSet, Vec<Warning>) {
        let mut planes = PlaneSet::new();
        let mut warnings = Vec::new();
        let b = BrushWork::from_solid(solid, 0, "worldspawn", &mut planes, &mut warnings)
            .expect("cube should compile");
        (b, planes, warnings)
    }

    #[test]
    fn a_cube_becomes_six_faces() {
        let (b, _, warnings) = build(&cube(0.0, 64.0, "dev/grid"));
        assert_eq!(b.face_count(), 6);
        assert!(warnings.is_empty());
        assert_eq!(b.contents, kerosene_bsp::contents::SOLID);
        assert_eq!(b.bounds.min, Vec3::ZERO);
        assert_eq!(b.bounds.max, Vec3::splat(64.0));
    }

    #[test]
    fn opposite_faces_intern_to_paired_planes() {
        // The +Z and -Z faces of a box are the same plane flipped only if they
        // are at the same distance; here they are not, so we check the
        // pairing property instead: every plane index has its inverse present.
        let (b, planes, _) = build(&cube(0.0, 64.0, "dev/grid"));
        for s in &b.sides {
            assert!(planes.get(s.plane).flipped().approx_eq(&planes.get(s.plane ^ 1)));
        }
    }

    #[test]
    fn splitting_a_cube_halves_it() {
        let (b, mut planes, _) = build(&cube(0.0, 64.0, "dev/grid"));
        let cut = planes.insert(Plane::new(Vec3::X, 32.0));
        let (front, back) = b.split(cut, &planes);
        let (front, back) = (front.expect("front half"), back.expect("back half"));

        assert_eq!(front.bounds.min.x, 32.0);
        assert_eq!(front.bounds.max.x, 64.0);
        assert_eq!(back.bounds.min.x, 0.0);
        assert_eq!(back.bounds.max.x, 32.0);
        assert_eq!(front.face_count(), 6);
        assert_eq!(back.face_count(), 6);
    }

    #[test]
    fn a_split_introduces_a_face_that_never_renders() {
        let (b, mut planes, _) = build(&cube(0.0, 64.0, "dev/grid"));
        let cut = planes.insert(Plane::new(Vec3::X, 32.0));
        let (front, _) = b.split(cut, &planes);
        let front = front.unwrap();
        let generated: Vec<_> = front.sides.iter().filter(|s| s.generated).collect();
        assert_eq!(generated.len(), 1);
        assert!(!generated[0].is_visible_surface(), "an interior cut must not draw");
    }

    #[test]
    fn a_plane_that_misses_does_not_split() {
        let (b, mut planes, _) = build(&cube(0.0, 64.0, "dev/grid"));
        let outside = planes.insert(Plane::new(Vec3::X, 500.0));
        let (front, back) = b.split(outside, &planes);
        assert!(front.is_none() && back.is_some(), "the brush is entirely behind");

        let outside = planes.insert(Plane::new(Vec3::X, -500.0));
        let (front, back) = b.split(outside, &planes);
        assert!(front.is_some() && back.is_none());
    }

    #[test]
    fn splitting_exactly_on_a_face_does_not_produce_a_sliver() {
        let (b, mut planes, _) = build(&cube(0.0, 64.0, "dev/grid"));
        let on_face = planes.insert(Plane::new(Vec3::X, 64.0));
        let (front, back) = b.split(on_face, &planes);
        assert!(front.is_none(), "nothing lies in front of the brush's own boundary");
        assert!(back.is_some());
    }

    #[test]
    fn repeated_splits_conserve_volume() {
        // Cutting a cube four ways should leave pieces whose bounds still tile
        // the original -- a check that splitting is not leaking geometry.
        let (b, mut planes, _) = build(&cube(0.0, 64.0, "dev/grid"));
        let mut pieces = vec![b];
        for d in [16.0f32, 32.0, 48.0] {
            let cut = planes.insert(Plane::new(Vec3::X, d));
            let mut next = Vec::new();
            for p in &pieces {
                let (f, bk) = p.split(cut, &planes);
                next.extend(f);
                next.extend(bk);
            }
            pieces = next;
        }
        assert_eq!(pieces.len(), 4);
        let total: f32 = pieces.iter().map(|p| p.bounds.size().x).sum();
        assert!((total - 64.0).abs() < 1e-3, "widths summed to {total}");
        for p in &pieces {
            assert_eq!(p.face_count(), 6);
        }
    }

    #[test]
    fn clip_material_makes_the_whole_brush_a_clip() {
        let (b, _, _) = build(&cube(0.0, 64.0, "tools/clip"));
        assert_eq!(b.contents, kerosene_bsp::contents::PLAYER_CLIP);
        assert!(!b.is_structural(), "a clip brush must not split the world tree");
    }

    #[test]
    fn one_clip_face_converts_the_brush() {
        let mut solid = cube(0.0, 64.0, "dev/grid");
        solid.sides[0].material = "tools/clip".to_string();
        let (b, _, _) = build(&solid);
        assert!(b.contents & kerosene_bsp::contents::PLAYER_CLIP != 0);
    }

    #[test]
    fn detail_brushes_stay_out_of_the_tree() {
        let mut planes = PlaneSet::new();
        let mut w = Vec::new();
        let solid = cube(0.0, 64.0, "dev/grid");
        let b = BrushWork::from_solid(&solid, 1, "func_detail", &mut planes, &mut w).unwrap();
        assert!(b.is_detail());
        assert!(!b.is_structural());
    }

    #[test]
    fn a_brush_with_too_few_faces_is_dropped_with_a_warning() {
        let mut solid = cube(0.0, 64.0, "dev/grid");
        solid.sides.truncate(3);
        let mut planes = PlaneSet::new();
        let mut warnings = Vec::new();
        assert!(BrushWork::from_solid(&solid, 0, "worldspawn", &mut planes, &mut warnings).is_none());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].brush_id, solid.id);
    }

    #[test]
    fn an_inside_out_brush_is_dropped() {
        // Faces that enclose nothing: the compile must reject it rather than
        // emit a brush with no volume.
        let mut solid = cube(0.0, 64.0, "dev/grid");
        for side in &mut solid.sides {
            side.plane_points.swap(0, 2); // flip every face inward
        }
        let mut planes = PlaneSet::new();
        let mut warnings = Vec::new();
        assert!(BrushWork::from_solid(&solid, 0, "worldspawn", &mut planes, &mut warnings).is_none());
        assert!(warnings.iter().any(|w| w.message.contains("encloses no volume")));
    }

    #[test]
    fn an_unknown_tool_material_warns() {
        let (_, _, warnings) = build(&cube(0.0, 64.0, "tools/clpi"));
        assert!(warnings.iter().any(|w| w.message.contains("unknown tool material")));
    }

    #[test]
    fn contains_point_matches_the_box() {
        let (b, planes, _) = build(&cube(0.0, 64.0, "dev/grid"));
        assert!(b.contains_point(Vec3::splat(32.0), &planes));
        assert!(!b.contains_point(Vec3::splat(-8.0), &planes));
    }
}
