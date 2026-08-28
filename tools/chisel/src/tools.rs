// SPDX-License-Identifier: LGPL-3.0-or-later
//! The editing tools: select, block, entity, texture.
//!
//! Each tool is a small state machine over a drag. Keeping them here, away
//! from the UI, means "what does dragging in the block tool do" is a question
//! with a testable answer rather than something you find out by clicking.

use crate::document::Document;
use crate::viewport::{Viewport, ray_box};
use void_math::{Aabb, Vec3, Winding};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolKind {
    /// Click to select, drag to move, shift-click to add.
    #[default]
    Select,
    /// Drag out a box brush.
    Block,
    /// Click to place a point entity.
    Entity,
    /// Click a face to apply the current material.
    Texture,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Select => "select",
            ToolKind::Block => "block",
            ToolKind::Entity => "entity",
            ToolKind::Texture => "texture",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            ToolKind::Select => "1",
            ToolKind::Block => "2",
            ToolKind::Entity => "3",
            ToolKind::Texture => "4",
        }
    }

    pub fn all() -> [ToolKind; 4] {
        [ToolKind::Select, ToolKind::Block, ToolKind::Entity, ToolKind::Texture]
    }
}

/// A drag in progress.
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    pub start: Vec3,
    pub current: Vec3,
    /// Whether the drag has moved far enough to count as a drag rather than a
    /// click. Without a threshold, every click nudges what it selects.
    pub is_dragging: bool,
}

impl Drag {
    pub fn bounds(&self) -> Aabb {
        Aabb::new(self.start.min(self.current), self.start.max(self.current))
    }

    pub fn delta(&self) -> Vec3 { self.current - self.start }
}

/// Pixels a pointer must travel before a click becomes a drag.
pub const DRAG_THRESHOLD: f32 = 3.0;

/// The active tool and its drag state.
#[derive(Default)]
pub struct Tool {
    pub kind: ToolKind,
    pub drag: Option<Drag>,
    /// Where the drag started on screen, for the click-versus-drag test.
    drag_origin_px: (f32, f32),
    /// Class placed by the entity tool.
    pub entity_class: String,
}

impl Tool {
    pub fn new() -> Tool {
        Tool { entity_class: "info_player_start".to_string(), ..Default::default() }
    }

    pub fn set_kind(&mut self, kind: ToolKind) {
        // Switching tools mid-drag would apply the new tool to the old drag.
        self.drag = None;
        self.kind = kind;
    }

    /// Pointer pressed in a viewport.
    pub fn press(&mut self, document: &Document, viewport: &Viewport, x: f32, y: f32) {
        let depth = default_depth(document, viewport);
        let world = document.grid.snap_point(viewport.screen_to_world(x, y, depth));
        self.drag_origin_px = (x, y);
        self.drag = Some(Drag { start: world, current: world, is_dragging: false });
    }

    /// Pointer moved while held.
    pub fn drag_to(&mut self, document: &Document, viewport: &Viewport, x: f32, y: f32) {
        let Some(drag) = &mut self.drag else { return };
        let depth = drag.start[viewport.kind.axes().2];
        drag.current = document.grid.snap_point(viewport.screen_to_world(x, y, depth));

        let moved = ((x - self.drag_origin_px.0).powi(2) + (y - self.drag_origin_px.1).powi(2)).sqrt();
        if moved > DRAG_THRESHOLD { drag.is_dragging = true; }
    }

    /// Pointer released. Returns what the caller should do.
    pub fn release(&mut self, add_to_selection: bool) -> Option<ToolAction> {
        let drag = self.drag.take()?;
        Some(match self.kind {
            ToolKind::Block => {
                if !drag.is_dragging { return None; }
                ToolAction::CreateBlock(drag.bounds())
            }
            ToolKind::Entity => ToolAction::CreateEntity(self.entity_class.clone(), drag.start),
            ToolKind::Texture => ToolAction::ApplyMaterialAt(drag.start),
            ToolKind::Select => {
                if drag.is_dragging {
                    ToolAction::Move(drag.delta())
                } else {
                    ToolAction::PickAt(drag.start, add_to_selection)
                }
            }
        })
    }

    pub fn cancel(&mut self) { self.drag = None; }
}

/// What a finished drag asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolAction {
    CreateBlock(Aabb),
    CreateEntity(String, Vec3),
    ApplyMaterialAt(Vec3),
    Move(Vec3),
    /// Select whatever is at this point; `true` adds to the selection.
    PickAt(Vec3, bool),
}

/// Depth to place new geometry at along a 2D view's hidden axis.
///
/// The current selection, if there is one, so that a brush drawn next to
/// another lands beside it rather than at the origin. That is the behaviour
/// that makes building in 2D views practical.
fn default_depth(document: &Document, viewport: &Viewport) -> f32 {
    if !viewport.kind.is_2d() { return 0.0; }
    let axis = viewport.kind.axes().2;
    match document.selection_bounds() {
        Some(bounds) => bounds.min[axis],
        None => 0.0,
    }
}

/// The solid nearest a point in a 2D view, if any.
///
/// Ties break toward the smallest brush: a small detail brush inside a large
/// room brush is the one you meant to click.
pub fn pick_solid_2d(document: &Document, point: Vec3, viewport: &Viewport) -> Option<u32> {
    let (h, v, _) = viewport.kind.axes();
    let mut best: Option<(f32, u32)> = None;

    for (_, solid) in document.map.all_solids() {
        let bounds = solid.bounds();
        // Only the two axes the view shows: the third is depth, and a 2D view
        // selects through the whole level.
        if point[h] < bounds.min[h] || point[h] > bounds.max[h] { continue; }
        if point[v] < bounds.min[v] || point[v] > bounds.max[v] { continue; }

        let area = (bounds.size()[h] * bounds.size()[v]).max(1.0);
        if best.is_none_or(|(best_area, _)| area < best_area) {
            best = Some((area, solid.id));
        }
    }
    best.map(|(_, id)| id)
}

/// The entity nearest a point in a 2D view.
pub fn pick_entity_2d(document: &Document, point: Vec3, viewport: &Viewport) -> Option<u32> {
    let (h, v, _) = viewport.kind.axes();
    // Point entities are drawn as a small box; clicking near one picks it.
    let reach = 16.0 / viewport.zoom.max(0.001);

    let mut best: Option<(f32, u32)> = None;
    for entity in document.map.entities.iter().filter(|e| e.solids.is_empty()) {
        let origin = entity.origin();
        let distance =
            ((origin[h] - point[h]).powi(2) + (origin[v] - point[v]).powi(2)).sqrt();
        if distance > reach { continue; }
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, entity.id));
        }
    }
    best.map(|(_, id)| id)
}

/// The solid a 3D pick ray hits first.
pub fn pick_solid_3d(document: &Document, origin: Vec3, direction: Vec3) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for (_, solid) in document.map.all_solids() {
        let Some(distance) = ray_box(origin, direction, solid.bounds()) else { continue };
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, solid.id));
        }
    }
    best.map(|(_, id)| id)
}

/// The face a 3D pick ray hits first, as `(solid id, side id)`.
///
/// Against the actual face polygons rather than the brush's bounding box:
/// picking a *face* is what the texture tool does, and a box hit says nothing
/// about which of six faces you meant. Back-facing polygons are skipped, so
/// clicking a wall from inside a room picks the wall you can see and not the
/// one behind you.
pub fn pick_face_3d(
    document: &Document,
    origin: Vec3,
    direction: Vec3,
) -> Option<(u32, u32)> {
    let mut best: Option<(f32, (u32, u32))> = None;
    for (_, solid) in document.map.all_solids() {
        for (side, winding) in solid.face_windings() {
            let Some(plane) = side.plane() else { continue };
            let facing = plane.normal.dot(direction);
            // Only faces turned towards the ray, and never one it runs along.
            if facing >= -1e-6 { continue }

            let distance = -(plane.normal.dot(origin) - plane.dist) / facing;
            if distance < 0.0 { continue }
            let hit = origin + direction * distance;
            if !winding_contains(&winding, plane.normal, hit) { continue }
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, (solid.id, side.id)));
            }
        }
    }
    best.map(|(_, ids)| ids)
}

/// Whether a point on a face's plane is inside the face.
///
/// The winding is convex, so the point is inside when it falls on the same
/// side of every edge. Which side that is depends on the winding order, and
/// this project's is clockwise seen from the front -- so rather than encode
/// that here and be wrong the day it changes, the first edge that says
/// anything decides and the rest must agree.
///
/// The epsilon lets a click landing exactly on an edge count for one of the
/// two faces rather than for neither.
fn winding_contains(winding: &Winding, normal: Vec3, point: Vec3) -> bool {
    let n = winding.points.len();
    if n < 3 { return false }

    let mut sign = 0.0f32;
    for i in 0..n {
        let a = winding.points[i];
        let b = winding.points[(i + 1) % n];
        let side = (b - a).cross(point - a).dot(normal);
        if side.abs() <= 0.05 { continue }
        if sign == 0.0 {
            sign = side.signum();
        } else if side.signum() != sign {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::ViewportKind;

    fn setup() -> (Document, Viewport) {
        let mut document = Document::new();
        document.grid.size = 16.0;
        let viewport = Viewport { size: (800.0, 600.0), zoom: 1.0, ..Viewport::new(ViewportKind::Top) };
        (document, viewport)
    }

    #[test]
    fn dragging_the_block_tool_asks_for_a_brush() {
        let (document, viewport) = setup();
        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 500.0, 200.0);

        match tool.release(false) {
            Some(ToolAction::CreateBlock(bounds)) => {
                assert!(bounds.size().x > 0.0 && bounds.size().y > 0.0);
            }
            other => panic!("expected a block, got {other:?}"),
        }
    }

    #[test]
    fn a_click_with_the_block_tool_creates_nothing() {
        // Otherwise every stray click leaves a one-grid-unit brush behind.
        let (document, viewport) = setup();
        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 401.0, 300.0);
        assert_eq!(tool.release(false), None);
    }

    #[test]
    fn a_click_with_the_select_tool_picks_rather_than_moving() {
        let (document, viewport) = setup();
        let mut tool = Tool::new();
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 401.0, 301.0);
        assert!(matches!(tool.release(false), Some(ToolAction::PickAt(_, false))));
    }

    #[test]
    fn dragging_with_the_select_tool_moves() {
        let (document, viewport) = setup();
        let mut tool = Tool::new();
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 464.0, 300.0);
        match tool.release(false) {
            Some(ToolAction::Move(delta)) => assert_eq!(delta.x, 64.0),
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn shift_click_adds_to_the_selection() {
        let (document, viewport) = setup();
        let mut tool = Tool::new();
        tool.press(&document, &viewport, 400.0, 300.0);
        assert!(matches!(tool.release(true), Some(ToolAction::PickAt(_, true))));
    }

    #[test]
    fn switching_tools_abandons_the_drag_in_progress() {
        let (document, viewport) = setup();
        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.set_kind(ToolKind::Select);
        assert!(tool.drag.is_none());
        assert_eq!(tool.release(false), None);
    }

    #[test]
    fn new_geometry_lands_beside_the_selection_not_at_the_origin() {
        let (mut document, viewport) = setup();
        document.create_block(Vec3::new(0.0, 0.0, 128.0), Vec3::new(64.0, 64.0, 192.0));

        let mut tool = Tool { kind: ToolKind::Block, ..Tool::new() };
        tool.press(&document, &viewport, 400.0, 300.0);
        tool.drag_to(&document, &viewport, 500.0, 200.0);
        match tool.release(false) {
            // The top view hides Z; the new brush should sit at the selected
            // brush's Z rather than back at zero.
            Some(ToolAction::CreateBlock(bounds)) => assert_eq!(bounds.min.z, 128.0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn picking_in_2d_reaches_through_the_depth_axis() {
        // A top view must select a brush at any height, not only at z = 0.
        let (mut document, viewport) = setup();
        let id = document.create_block(Vec3::new(0.0, 0.0, 500.0), Vec3::new(64.0, 64.0, 564.0));
        let hit = pick_solid_2d(&document, Vec3::new(32.0, 32.0, 0.0), &viewport);
        assert_eq!(hit, Some(id));
    }

    #[test]
    fn picking_prefers_the_smaller_brush() {
        // A detail brush inside a room brush is the one you meant to click.
        let (mut document, viewport) = setup();
        document.create_block(Vec3::new(-256.0, -256.0, 0.0), Vec3::new(256.0, 256.0, 128.0));
        let small = document.create_block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(32.0, 32.0, 32.0));
        assert_eq!(pick_solid_2d(&document, Vec3::new(16.0, 16.0, 0.0), &viewport), Some(small));
    }

    #[test]
    fn clicking_empty_space_picks_nothing() {
        let (mut document, viewport) = setup();
        document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        assert_eq!(pick_solid_2d(&document, Vec3::new(1000.0, 1000.0, 0.0), &viewport), None);
    }

    #[test]
    fn point_entities_can_be_picked_near_their_origin() {
        let (mut document, viewport) = setup();
        let id = document.create_entity("light", Vec3::new(100.0, 100.0, 0.0));
        assert_eq!(pick_entity_2d(&document, Vec3::new(104.0, 104.0, 0.0), &viewport), Some(id));
        assert_eq!(pick_entity_2d(&document, Vec3::new(400.0, 400.0, 0.0), &viewport), None);
    }

    #[test]
    fn the_3d_ray_picks_the_nearest_brush() {
        let mut document = Document::new();
        let near = document.create_block(Vec3::new(100.0, -32.0, -32.0), Vec3::new(164.0, 32.0, 32.0));
        document.create_block(Vec3::new(400.0, -32.0, -32.0), Vec3::new(464.0, 32.0, 32.0));
        assert_eq!(pick_solid_3d(&document, Vec3::ZERO, Vec3::X), Some(near));
        assert_eq!(pick_solid_3d(&document, Vec3::ZERO, -Vec3::X), None);
    }

    #[test]
    fn every_tool_has_a_label_and_a_shortcut() {
        for kind in ToolKind::all() {
            assert!(!kind.label().is_empty());
            assert!(!kind.shortcut().is_empty());
        }
    }
}

#[cfg(test)]
mod face_picking_tests {
    use super::*;
    use void_map::Solid;
    use void_math::Aabb;

    fn room() -> (Document, u32) {
        let mut document = Document::new();
        document.map.world.solids.clear();
        let id = document.map.next_id();
        let mut solid = Solid::cube(Aabb::new(Vec3::ZERO, Vec3::splat(128.0)), "dev/grid");
        solid.id = id;
        document.map.world.solids.push(solid);
        (document, id)
    }

    #[test]
    fn a_ray_picks_the_face_it_points_at_not_just_the_brush() {
        // The texture tool needs a face; a bounding-box hit says nothing about
        // which of six faces you meant.
        let (document, id) = room();
        let hit = pick_face_3d(&document, Vec3::new(-100.0, 64.0, 64.0), Vec3::X)
            .expect("the ray hits the cube");
        assert_eq!(hit.0, id);

        let side = document
            .find_solid(id)
            .unwrap()
            .sides
            .iter()
            .find(|s| s.id == hit.1)
            .expect("the side exists");
        assert!(
            side.plane().unwrap().normal.x < -0.9,
            "picked the wrong face: {:?}",
            side.plane().unwrap().normal
        );
    }

    #[test]
    fn the_nearest_face_wins() {
        let (mut document, first) = room();
        let id = document.map.next_id();
        let mut nearer = Solid::cube(
            Aabb::new(Vec3::new(-200.0, 0.0, 0.0), Vec3::new(-150.0, 128.0, 128.0)),
            "dev/wall",
        );
        nearer.id = id;
        document.map.world.solids.push(nearer);

        let hit = pick_face_3d(&document, Vec3::new(-400.0, 64.0, 64.0), Vec3::X).unwrap();
        assert_eq!(hit.0, id, "the far brush was picked over the near one");
        let _ = first;
    }

    #[test]
    fn a_ray_pointing_away_hits_nothing() {
        let (document, _) = room();
        assert!(pick_face_3d(&document, Vec3::new(-100.0, 64.0, 64.0), -Vec3::X).is_none());
    }

    #[test]
    fn a_ray_missing_the_face_hits_nothing_even_on_its_plane() {
        // On the plane of the -X face, but past the end of it.
        let (document, _) = room();
        assert!(pick_face_3d(&document, Vec3::new(-100.0, 900.0, 64.0), Vec3::X).is_none());
    }

    #[test]
    fn standing_in_a_room_picks_the_wall_you_are_looking_at() {
        // The face whose normal points back at the ray, not the one behind
        // the camera that the ray would also cross.
        let document = crate::app::starter_document();
        let hit = pick_face_3d(&document, Vec3::new(256.0, 256.0, 64.0), Vec3::X)
            .expect("a sealed room has a wall in every direction");

        let solid = document.find_solid(hit.0).expect("the brush exists");
        let side = solid.sides.iter().find(|s| s.id == hit.1).expect("the side exists");
        let normal = side.plane().unwrap().normal;
        assert!(normal.x < -0.9, "picked a face turned away from the camera: {normal:?}");
    }

    #[test]
    fn a_camera_buried_in_solid_rock_picks_nothing() {
        // Every face of the brush around it is either behind it or turned
        // away. Reporting a hit would mean texturing a face you cannot see.
        let (document, _) = room();
        assert!(pick_face_3d(&document, Vec3::splat(64.0), Vec3::X).is_none());
    }

    #[test]
    fn every_face_of_a_cube_is_reachable() {
        let (document, id) = room();
        let mut picked = std::collections::HashSet::new();
        for (from, direction) in [
            (Vec3::new(-200.0, 64.0, 64.0), Vec3::X),
            (Vec3::new(300.0, 64.0, 64.0), -Vec3::X),
            (Vec3::new(64.0, -200.0, 64.0), Vec3::Y),
            (Vec3::new(64.0, 300.0, 64.0), -Vec3::Y),
            (Vec3::new(64.0, 64.0, -200.0), Vec3::Z),
            (Vec3::new(64.0, 64.0, 300.0), -Vec3::Z),
        ] {
            let hit = pick_face_3d(&document, from, direction).expect("hits");
            assert_eq!(hit.0, id);
            picked.insert(hit.1);
        }
        assert_eq!(picked.len(), 6, "some faces are unreachable: {picked:?}");
    }
}
