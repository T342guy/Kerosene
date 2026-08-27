// SPDX-License-Identifier: LGPL-3.0-or-later
//! Drawing the viewports.
//!
//! The 2D panes are drawn with egui's painter: a grid, then brush outlines,
//! then entities, then whatever the current tool is previewing. That is all a
//! 2D view needs, and doing it in immediate mode means there is no scene graph
//! to keep in step with the document.
//!
//! The 3D pane needs an answer to "what is in front of what", which no
//! ordering of whole polygons can give, so the drawing of it lives in
//! [`crate::raster`] behind a depth buffer. What stays here is the geometry
//! side of it: camera space, near-plane clipping, and which faces are worth
//! considering at all. Those are questions with testable answers, and keeping
//! them out of the drawing is what let the two bugs in this file be written
//! down as tests rather than as screenshots.

use crate::document::Document;
use crate::tools::{Tool, ToolAction, ToolKind};
use crate::viewport::Viewport;
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};
use void_map::Solid;
use void_math::{Aabb, Vec3};

/// The editor's colours, in one place so the panes agree.
pub mod colors {
    use egui::Color32;
    pub const BACKGROUND: Color32 = Color32::from_rgb(22, 24, 28);
    pub const GRID_MINOR: Color32 = Color32::from_rgb(38, 41, 47);
    pub const GRID_MAJOR: Color32 = Color32::from_rgb(52, 56, 64);
    pub const AXIS: Color32 = Color32::from_rgb(78, 84, 96);
    pub const BRUSH: Color32 = Color32::from_rgb(170, 178, 190);
    pub const BRUSH_ENTITY: Color32 = Color32::from_rgb(120, 200, 160);
    pub const SELECTED: Color32 = Color32::from_rgb(255, 190, 70);
    pub const ENTITY: Color32 = Color32::from_rgb(120, 170, 255);
    pub const TOOL_PREVIEW: Color32 = Color32::from_rgb(255, 120, 200);
    pub const TEXT: Color32 = Color32::from_rgb(160, 168, 180);
}

/// Every fourth grid line is drawn brighter, so it is possible to count
/// squares at a glance rather than by dragging along them.
const MAJOR_EVERY: i64 = 4;

/// Draw one 2D pane.
pub fn draw_2d(
    painter: &Painter,
    rect: Rect,
    viewport: &Viewport,
    document: &Document,
    tool: &Tool,
) {
    painter.rect_filled(rect, 0.0, colors::BACKGROUND);
    draw_grid(painter, rect, viewport, document);

    let (h, v, _) = viewport.kind.axes();
    let to_screen = |world: Vec3| {
        let (x, y) = viewport.world_to_screen(world);
        Pos2::new(rect.min.x + x, rect.min.y + y)
    };

    // Brushes, world first so entity brushes draw over them.
    for (entity, solid) in document.map.all_solids() {
        let bounds = solid.bounds();
        if !viewport.shows(bounds) { continue; }

        let selected = document.selection.solids.contains(&solid.id)
            || document.selection.entities.contains(&entity.id);
        let color = if selected {
            colors::SELECTED
        } else if entity.is_brush_entity() && entity.classname() != "worldspawn" {
            colors::BRUSH_ENTITY
        } else {
            colors::BRUSH
        };

        draw_solid_outline(painter, solid, viewport, rect, color, selected);
        let _ = (h, v);
    }

    // Point entities.
    for entity in document.map.entities.iter().filter(|e| e.solids.is_empty()) {
        let origin = entity.origin();
        let selected = document.selection.entities.contains(&entity.id);
        let color = if selected { colors::SELECTED } else { colors::ENTITY };
        let center = to_screen(origin);
        let half = 5.0;
        painter.rect_stroke(
            Rect::from_center_size(center, Vec2::splat(half * 2.0)),
            0.0,
            Stroke::new(if selected { 2.0 } else { 1.0 }, color),
            egui::StrokeKind::Middle,
        );
        if viewport.zoom > 0.15 {
            painter.text(
                center + Vec2::new(8.0, -4.0),
                egui::Align2::LEFT_TOP,
                entity.classname(),
                egui::FontId::proportional(10.0),
                color,
            );
        }
    }

    draw_tool_preview(painter, rect, viewport, tool);
}

/// Grid lines, coarsening automatically as the view zooms out.
fn draw_grid(painter: &Painter, rect: Rect, viewport: &Viewport, document: &Document) {
    let Some(spacing) = document.grid.draw_spacing(viewport.zoom) else { return };
    let (h, v, _) = viewport.kind.axes();
    let bounds = viewport.visible_bounds();

    let line = |from: Pos2, to: Pos2, color: Color32| {
        painter.line_segment([from, to], Stroke::new(1.0, color));
    };

    // Vertical lines: constant along the horizontal world axis.
    let first = (bounds.min[h] / spacing).floor() as i64;
    let last = (bounds.max[h] / spacing).ceil() as i64;
    for i in first..=last {
        let world = i as f32 * spacing;
        let (x, _) = viewport.world_to_screen(axis_point(h, world));
        let x = rect.min.x + x;
        if x < rect.min.x || x > rect.max.x { continue; }
        let color = grid_color(i, world);
        line(Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y), color);
    }

    let first = (bounds.min[v] / spacing).floor() as i64;
    let last = (bounds.max[v] / spacing).ceil() as i64;
    for i in first..=last {
        let world = i as f32 * spacing;
        let (_, y) = viewport.world_to_screen(axis_point(v, world));
        let y = rect.min.y + y;
        if y < rect.min.y || y > rect.max.y { continue; }
        let color = grid_color(i, world);
        line(Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y), color);
    }
}

fn grid_color(index: i64, world: f32) -> Color32 {
    if world == 0.0 {
        colors::AXIS
    } else if index % MAJOR_EVERY == 0 {
        colors::GRID_MAJOR
    } else {
        colors::GRID_MINOR
    }
}

fn axis_point(axis: usize, value: f32) -> Vec3 {
    let mut p = Vec3::ZERO;
    p[axis] = value;
    p
}

/// Outline a brush by drawing each of its faces' edges.
fn draw_solid_outline(
    painter: &Painter,
    solid: &Solid,
    viewport: &Viewport,
    rect: Rect,
    color: Color32,
    selected: bool,
) {
    let width = if selected { 2.0 } else { 1.0 };
    let stroke = Stroke::new(width, color);

    for (_, winding) in solid.face_windings() {
        let points: Vec<Pos2> = winding
            .points
            .iter()
            .map(|p| {
                let (x, y) = viewport.world_to_screen(*p);
                Pos2::new(rect.min.x + x, rect.min.y + y)
            })
            .collect();
        for i in 0..points.len() {
            painter.line_segment([points[i], points[(i + 1) % points.len()]], stroke);
        }
    }
}

/// The rubber band or ghost the current tool is showing.
fn draw_tool_preview(painter: &Painter, rect: Rect, viewport: &Viewport, tool: &Tool) {
    let Some(drag) = &tool.drag else { return };
    if !drag.is_dragging && tool.kind != ToolKind::Block { return; }

    let bounds = drag.bounds();
    let (h, v, _) = viewport.kind.axes();
    let (x0, y0) = viewport.world_to_screen(bounds.min);
    let (x1, y1) = viewport.world_to_screen(bounds.max);
    let preview = Rect::from_two_pos(
        Pos2::new(rect.min.x + x0, rect.min.y + y0),
        Pos2::new(rect.min.x + x1, rect.min.y + y1),
    );

    painter.rect_stroke(
        preview,
        0.0,
        Stroke::new(1.0, colors::TOOL_PREVIEW),
        egui::StrokeKind::Middle,
    );

    // The size in world units, which is what a designer is actually reading
    // off the screen while dragging.
    let size = bounds.size();
    painter.text(
        preview.right_bottom() + Vec2::new(4.0, 4.0),
        egui::Align2::LEFT_TOP,
        format!("{:.0} x {:.0}", size[h], size[v]),
        egui::FontId::monospace(11.0),
        colors::TOOL_PREVIEW,
    );
}

/// Near plane distance, in inches.
///
/// Anything closer than this cannot be projected: the perspective divide blows
/// up as depth approaches zero, and behind the camera it flips the sign and
/// smears geometry across the screen.
pub const NEAR: f32 = 1.0;

/// Convert world points into camera space: `x` right, `y` up, `z` forward.
pub fn to_camera_space(points: &[Vec3], eye: Vec3, basis: void_math::Basis) -> Vec<Vec3> {
    points
        .iter()
        .map(|p| {
            let relative = *p - eye;
            Vec3::new(
                relative.dot(basis.right),
                relative.dot(basis.up),
                relative.dot(basis.forward),
            )
        })
        .collect()
}

/// Clip a camera-space polygon against the near plane.
///
/// Returns the part with `z >= near`, which may have more vertices than it
/// started with -- a quad crossing the plane becomes a pentagon.
///
/// This exists because the obvious alternative is wrong in a way that is very
/// visible: dropping any face that has a vertex behind the camera. Standing
/// inside a room, which is the normal case for a level editor, the floor,
/// ceiling and side walls all have corners behind you, so whole walls vanish
/// as the camera turns.
pub fn clip_near(polygon: &[Vec3], near: f32) -> Vec<Vec3> {
    if polygon.len() < 3 { return Vec::new(); }

    let mut out: Vec<Vec3> = Vec::with_capacity(polygon.len() + 2);
    for i in 0..polygon.len() {
        let current = polygon[i];
        let next = polygon[(i + 1) % polygon.len()];
        let current_in = current.z >= near;
        let next_in = next.z >= near;

        if current_in { out.push(current); }
        // Emit a crossing point whenever the edge changes side.
        if current_in != next_in {
            let span = next.z - current.z;
            if span.abs() > 1e-9 {
                let t = (near - current.z) / span;
                let mut crossing = current + (next - current) * t;
                // Land exactly on the plane rather than a hair off it, so the
                // perspective divide below cannot see a depth of zero.
                crossing.z = near;
                // A vertex sitting exactly on the plane is both inside and a
                // crossing, and would otherwise be emitted twice. A repeated
                // point makes a zero-length edge, which is where stroke
                // tessellation used to produce a spike across the screen.
                if out.last().is_none_or(|last| last.distance_squared(crossing) > 1e-12) {
                    out.push(crossing);
                }
            }
        }
    }
    // The same again across the wrap: the crossing on the last edge can land
    // exactly on the first vertex.
    if out.len() >= 2 && out[0].distance_squared(*out.last().expect("checked")) <= 1e-12 {
        out.pop();
    }
    if out.len() < 3 { Vec::new() } else { out }
}

/// One face of the document, ready to draw in the 3D pane.
#[derive(Clone, Debug)]
pub struct VisibleFace {
    /// The polygon in camera space, clipped to the near plane.
    pub polygon: Vec<Vec3>,
    /// Sort key: the depth of the farthest vertex.
    pub depth: f32,
    /// World-space face normal, for flat shading.
    pub normal: Vec3,
    pub selected: bool,
}

/// Every face visible from a viewpoint, clipped and sorted back to front.
///
/// Separated from the drawing so that "what should be on screen" is a question
/// with a testable answer. Both of the bugs this function replaced were
/// invisible to every other test in the editor and obvious the moment you
/// turned the camera.
pub fn visible_faces(document: &Document, eye: Vec3, basis: void_math::Basis) -> Vec<VisibleFace> {
    let mut faces = Vec::new();

    for (entity, solid) in document.map.all_solids() {
        let selected = document.selection.solids.contains(&solid.id)
            || document.selection.entities.contains(&entity.id);

        for (side, winding) in solid.face_windings() {
            let Some(plane) = side.plane() else { continue };
            // Back-face cull: a face pointing away is inside the brush.
            if plane.normal.dot(eye - winding.center()) <= 0.0 { continue; }

            let camera = to_camera_space(&winding.points, eye, basis);
            let polygon = clip_near(&camera, NEAR);
            if polygon.len() < 3 { continue; }

            // Sort by the *farthest* vertex, not the average. A small object
            // standing on a large surface has a greater average depth than the
            // surface it sits on, so averaging sorts the surface later and
            // paints it straight over the object.
            let depth = polygon.iter().fold(f32::MIN, |acc, p| acc.max(p.z));
            faces.push(VisibleFace { polygon, depth, normal: plane.normal, selected });
        }
    }

    faces.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));
    faces
}

/// Apply a finished tool action to the document.
///
/// Kept next to the drawing rather than in the UI so that "what does this
/// gesture do" has one answer, testable without a window.
pub fn apply_action(document: &mut Document, viewport: &Viewport, action: ToolAction) {
    match action {
        ToolAction::CreateBlock(bounds) => {
            document.create_block(bounds.min, bounds.max);
        }
        ToolAction::CreateEntity(class, at) => {
            document.create_entity(&class, at);
        }
        ToolAction::Move(delta) => document.move_selection(delta),
        ToolAction::ApplyMaterialAt(point) => {
            if let Some(id) = crate::tools::pick_solid_2d(document, point, viewport) {
                document.selection.clear();
                document.selection.solids.insert(id);
                document.apply_material();
            }
        }
        ToolAction::PickAt(point, add) => {
            if !add { document.selection.clear(); }

            // Entities take priority: they are drawn on top and are smaller
            // targets, so a click near one almost always means the entity.
            if let Some(id) = crate::tools::pick_entity_2d(document, point, viewport) {
                toggle(&mut document.selection.entities, id, add);
                return;
            }
            if let Some(id) = crate::tools::pick_solid_2d(document, point, viewport) {
                // Clicking a brush that belongs to an entity selects the
                // entity: that is the thing a designer thinks of as the door.
                let owner = document
                    .map
                    .all_solids()
                    .find(|(_, s)| s.id == id)
                    .map(|(e, _)| (e.id, e.is_brush_entity() && e.classname() != "worldspawn"));
                match owner {
                    Some((entity_id, true)) => toggle(&mut document.selection.entities, entity_id, add),
                    _ => toggle(&mut document.selection.solids, id, add),
                }
            }
        }
    }
}

fn toggle(set: &mut std::collections::HashSet<u32>, id: u32, add: bool) {
    if add && !set.insert(id) {
        // Shift-clicking something already selected removes it, which is what
        // every selection model does and what a designer expects.
        set.remove(&id);
    } else {
        set.insert(id);
    }
}

/// Bounds worth framing when the view is reset.
pub fn framing_bounds(document: &Document) -> Aabb {
    document
        .selection_bounds()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| {
            let all = document.map.bounds();
            if all.is_empty() { Aabb::new(Vec3::splat(-512.0), Vec3::splat(512.0)) } else { all }
        })
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
    fn picking_a_brush_selects_it() {
        let (mut document, viewport) = setup();
        let id = document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        document.selection.clear();

        apply_action(&mut document, &viewport, ToolAction::PickAt(Vec3::new(32.0, 32.0, 0.0), false));
        assert_eq!(document.selection.solids.len(), 1);
        assert!(document.selection.solids.contains(&id));
    }

    #[test]
    fn picking_empty_space_clears_the_selection() {
        let (mut document, viewport) = setup();
        document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        apply_action(&mut document, &viewport, ToolAction::PickAt(Vec3::new(900.0, 900.0, 0.0), false));
        assert!(document.selection.is_empty());
    }

    #[test]
    fn shift_clicking_adds_then_removes() {
        let (mut document, viewport) = setup();
        let a = document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        let b = document.create_block(Vec3::new(128.0, 0.0, 0.0), Vec3::new(192.0, 64.0, 64.0));
        document.selection.clear();

        apply_action(&mut document, &viewport, ToolAction::PickAt(Vec3::new(32.0, 32.0, 0.0), false));
        apply_action(&mut document, &viewport, ToolAction::PickAt(Vec3::new(160.0, 32.0, 0.0), true));
        assert_eq!(document.selection.solids.len(), 2);
        assert!(document.selection.solids.contains(&a) && document.selection.solids.contains(&b));

        // Shift-clicking the same thing again removes it.
        apply_action(&mut document, &viewport, ToolAction::PickAt(Vec3::new(160.0, 32.0, 0.0), true));
        assert_eq!(document.selection.solids.len(), 1);
    }

    #[test]
    fn clicking_a_brush_entity_selects_the_entity_not_the_brush() {
        // A designer thinks of the door, not of the brushes it is made of.
        let (mut document, viewport) = setup();
        document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        let door = document.tie_to_entity("func_door").unwrap();
        document.selection.clear();

        apply_action(&mut document, &viewport, ToolAction::PickAt(Vec3::new(32.0, 32.0, 0.0), false));
        assert!(document.selection.entities.contains(&door));
        assert!(document.selection.solids.is_empty());
    }

    #[test]
    fn point_entities_win_over_brushes_underneath_them() {
        let (mut document, viewport) = setup();
        document.create_block(Vec3::ZERO, Vec3::splat(128.0));
        let light = document.create_entity("light", Vec3::new(64.0, 64.0, 64.0));
        document.selection.clear();

        apply_action(&mut document, &viewport, ToolAction::PickAt(Vec3::new(64.0, 64.0, 0.0), false));
        assert!(document.selection.entities.contains(&light));
    }

    #[test]
    fn the_texture_tool_paints_what_it_is_clicked_on() {
        let (mut document, viewport) = setup();
        let id = document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        document.current_material = "dev/wall".into();
        apply_action(&mut document, &viewport, ToolAction::ApplyMaterialAt(Vec3::new(32.0, 32.0, 0.0)));
        assert!(document.find_solid(id).unwrap().sides.iter().all(|s| s.material == "dev/wall"));
    }

    #[test]
    fn framing_falls_back_to_something_sensible_on_an_empty_map() {
        let document = Document::new();
        let bounds = framing_bounds(&document);
        assert!(!bounds.is_empty());
        assert!(bounds.size().length() > 0.0);
    }

    #[test]
    fn framing_prefers_the_selection() {
        let mut document = Document::new();
        document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        document.create_block(Vec3::splat(500.0), Vec3::splat(600.0));
        // Only the first is selected, from create_block.
        document.selection.clear();
        document.selection.solids.insert(document.map.world.solids[0].id);
        let bounds = framing_bounds(&document);
        assert_eq!(bounds.max, Vec3::splat(64.0));
    }
}

#[cfg(test)]
mod view_tests {
    //! Regression tests for the 3D pane.
    //!
    //! Both bugs these cover were angle-dependent: the view looked correct
    //! until the camera turned, and then whole walls disappeared. Nothing else
    //! in the editor's tests could see them, because they lived between the
    //! projection and the sort.

    use super::*;
    use crate::app::starter_document;
    use crate::viewport::ViewportKind;
    use void_math::{Aabb, Angles};

    fn basis_for(yaw: f32, pitch: f32) -> void_math::Basis {
        Angles::new(pitch, yaw, 0.0).vectors()
    }

    // ---- near-plane clipping --------------------------------------------

    #[test]
    fn a_polygon_entirely_in_front_is_untouched() {
        let quad = vec![
            Vec3::new(-10.0, -10.0, 100.0),
            Vec3::new(10.0, -10.0, 100.0),
            Vec3::new(10.0, 10.0, 100.0),
            Vec3::new(-10.0, 10.0, 100.0),
        ];
        assert_eq!(clip_near(&quad, NEAR), quad);
    }

    #[test]
    fn a_polygon_entirely_behind_is_dropped() {
        let quad = vec![
            Vec3::new(-10.0, -10.0, -100.0),
            Vec3::new(10.0, -10.0, -100.0),
            Vec3::new(10.0, 10.0, -100.0),
            Vec3::new(-10.0, 10.0, -100.0),
        ];
        assert!(clip_near(&quad, NEAR).is_empty());
    }

    #[test]
    fn a_polygon_straddling_the_camera_keeps_its_visible_part() {
        // The bug: this used to be discarded entirely, because one vertex was
        // behind the near plane.
        let quad = vec![
            Vec3::new(-10.0, -10.0, -50.0),
            Vec3::new(10.0, -10.0, -50.0),
            Vec3::new(10.0, 10.0, 200.0),
            Vec3::new(-10.0, 10.0, 200.0),
        ];
        let clipped = clip_near(&quad, NEAR);
        assert!(clipped.len() >= 3, "the visible half should survive, got {clipped:?}");
        assert!(
            clipped.iter().all(|p| p.z >= NEAR - 1e-4),
            "nothing may remain behind the near plane: {clipped:?}"
        );
        // The far edge is untouched.
        assert!(clipped.iter().any(|p| (p.z - 200.0).abs() < 1e-3));
        // And the near edge sits exactly on the plane, not a hair off it.
        assert!(clipped.iter().any(|p| (p.z - NEAR).abs() < 1e-4));
    }

    #[test]
    fn clipping_one_corner_of_a_quad_gives_a_pentagon() {
        // A quad with a single vertex behind gains a vertex, not loses one.
        let quad = vec![
            Vec3::new(-10.0, -10.0, -20.0),
            Vec3::new(10.0, -10.0, 100.0),
            Vec3::new(10.0, 10.0, 100.0),
            Vec3::new(-10.0, 10.0, 100.0),
        ];
        assert_eq!(clip_near(&quad, NEAR).len(), 5);
    }

    #[test]
    fn a_vertex_exactly_on_the_near_plane_is_not_emitted_twice() {
        // The stray-line bug: a vertex landing exactly on the plane counts as
        // inside *and* as a crossing, so it went in twice. A repeated point is
        // a zero-length edge, and a zero-length edge has no direction to take
        // a normal from.
        // One corner sits on the plane and one is behind it, so the crossing
        // computed for the closing edge lands exactly on the first vertex.
        let quad = vec![
            Vec3::new(-10.0, -10.0, NEAR),
            Vec3::new(10.0, -10.0, 50.0),
            Vec3::new(10.0, 10.0, 50.0),
            Vec3::new(-10.0, 10.0, 0.5),
        ];
        let clipped = clip_near(&quad, NEAR);
        assert_eq!(clipped.len(), 4, "{clipped:?}");
        for pair in clipped.windows(2) {
            assert!(pair[0].distance_squared(pair[1]) > 1e-12, "a point was repeated: {clipped:?}");
        }
        assert!(
            clipped[0].distance_squared(*clipped.last().expect("not empty")) > 1e-12,
            "the polygon closes on itself: {clipped:?}"
        );
    }

    #[test]
    fn a_polygon_edge_on_at_the_near_plane_has_no_area_and_is_dropped() {
        // Two corners exactly on the plane and two behind it: what survives is
        // a line, not a shape, and a line is not something to draw.
        let quad = vec![
            Vec3::new(-10.0, -10.0, NEAR),
            Vec3::new(10.0, -10.0, NEAR),
            Vec3::new(10.0, 10.0, 0.5),
            Vec3::new(-10.0, 10.0, 0.5),
        ];
        assert!(clip_near(&quad, NEAR).is_empty());
    }

    #[test]
    fn a_clipped_polygon_never_divides_by_zero() {
        // The reason to clip at all: a depth at or near zero blows the
        // perspective divide up and smears the face across the screen.
        for z in [-1000.0f32, -1.0, 0.0, 0.5, 1.0] {
            let quad = vec![
                Vec3::new(-10.0, -10.0, z),
                Vec3::new(10.0, -10.0, 500.0),
                Vec3::new(10.0, 10.0, 500.0),
                Vec3::new(-10.0, 10.0, z),
            ];
            for p in clip_near(&quad, NEAR) {
                assert!(p.z >= NEAR - 1e-4, "z = {} slipped through for start {z}", p.z);
                assert!((p.x / p.z).is_finite() && (p.y / p.z).is_finite());
            }
        }
    }

    // ---- the reported symptom -------------------------------------------

    #[test]
    fn standing_in_a_room_shows_walls_from_every_angle() {
        // The bug as reported: geometry disappearing at certain angles. Inside
        // a room the floor, ceiling and side walls all have corners behind the
        // camera, so rejecting any face with a vertex behind it made whole
        // walls pop out of existence as the view turned.
        let document = starter_document();
        let eye = Vec3::new(256.0, 256.0, 64.0);

        for yaw in (0..360).step_by(15) {
            for pitch in [-45.0f32, 0.0, 45.0] {
                let basis = basis_for(yaw as f32, pitch);
                let faces = visible_faces(&document, eye, basis);
                assert!(
                    !faces.is_empty(),
                    "nothing visible at yaw {yaw}, pitch {pitch} -- the room vanished"
                );
            }
        }
    }

    #[test]
    fn a_wall_the_camera_is_close_to_still_draws() {
        // Standing right against a wall puts most of it behind the near plane.
        // It should still fill the view rather than disappearing.
        let document = starter_document();
        // The starter room spans 0..512; stand a few inches off the -X wall.
        let eye = Vec3::new(4.0, 256.0, 64.0);
        let basis = basis_for(180.0, 0.0);

        let faces = visible_faces(&document, eye, basis);
        assert!(!faces.is_empty(), "the wall in front of the camera vanished");
        for face in &faces {
            assert!(face.polygon.iter().all(|p| p.z >= NEAR - 1e-4));
        }
    }

    #[test]
    fn turning_on_the_spot_never_empties_the_view() {
        // A stronger version of the same thing: sweep every degree and check
        // the face count never collapses.
        let document = starter_document();
        let eye = Vec3::new(128.0, 128.0, 64.0);

        let mut fewest = usize::MAX;
        for yaw in 0..360 {
            let faces = visible_faces(&document, eye, basis_for(yaw as f32, 0.0));
            fewest = fewest.min(faces.len());
        }
        assert!(fewest >= 3, "only {fewest} faces visible at the worst angle");
    }

    // ---- draw order ------------------------------------------------------

    #[test]
    fn a_small_object_is_drawn_over_the_surface_it_stands_on() {
        // The second bug: sorting by *average* depth. A long surface running
        // away from the camera has a lower average depth than a small object
        // standing in the far half of it, so the surface sorted later and
        // painted straight over the object.
        //
        // The numbers are chosen so the two orderings genuinely disagree.
        // Looking down +Y from y = -300 with no pitch, depth is exactly the
        // distance in Y:
        //
        //   floor top:  y -100..900  ->  depth  200..1200, average 700, max 1200
        //   crate top:  y  400..440  ->  depth  700.. 740, average 720, max  740
        //
        // By average the crate (720) sorts behind the floor (700) and is
        // covered. By farthest vertex the floor (1200) is correctly first.
        let mut document = Document::new();
        document.create_block(
            Vec3::new(-512.0, -100.0, -16.0),
            Vec3::new(512.0, 900.0, 0.0),
        );
        let crate_id = document.create_block(
            Vec3::new(-32.0, 400.0, 0.0),
            Vec3::new(32.0, 440.0, 64.0),
        );
        document.selection.clear();
        document.selection.solids.insert(crate_id);

        let eye = Vec3::new(0.0, -300.0, 96.0);
        let faces = visible_faces(&document, eye, basis_for(90.0, 0.0));

        // The floor's top face: unselected, pointing up.
        let floor_top = faces
            .iter()
            .position(|f| !f.selected && f.normal.z > 0.9)
            .expect("the floor's top face should be visible");
        let first_crate = faces
            .iter()
            .position(|f| f.selected)
            .expect("the crate should be visible");

        assert!(
            floor_top < first_crate,
            "the floor's top face is painted at index {floor_top}, after the crate at \
             {first_crate}, so it covers it"
        );
    }

    #[test]
    fn faces_come_back_sorted_back_to_front() {
        let document = starter_document();
        let faces = visible_faces(
            &document,
            Vec3::new(256.0, 256.0, 128.0),
            basis_for(45.0, 10.0),
        );
        for pair in faces.windows(2) {
            assert!(
                pair[0].depth >= pair[1].depth,
                "{} came before {}",
                pair[0].depth,
                pair[1].depth
            );
        }
    }

    // ---- culling still works --------------------------------------------

    #[test]
    fn faces_pointing_away_are_still_culled() {
        // Outside a lone box, at most three of its six faces can be seen.
        let mut document = Document::new();
        document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        let faces = visible_faces(&document, Vec3::new(-300.0, -300.0, 200.0), basis_for(45.0, 20.0));
        assert!(faces.len() <= 3, "{} faces visible on a cube", faces.len());
        assert!(!faces.is_empty());
    }

    #[test]
    fn geometry_behind_the_camera_is_not_drawn() {
        let mut document = Document::new();
        document.create_block(Vec3::new(500.0, -32.0, -32.0), Vec3::new(564.0, 32.0, 32.0));
        // Facing away from it.
        let faces = visible_faces(&document, Vec3::ZERO, basis_for(180.0, 0.0));
        assert!(faces.is_empty(), "{} faces drawn from behind", faces.len());
    }

    #[test]
    fn an_empty_document_draws_nothing() {
        assert!(visible_faces(&Document::new(), Vec3::ZERO, basis_for(0.0, 0.0)).is_empty());
    }

    #[test]
    fn camera_space_puts_forward_on_z() {
        let basis = basis_for(0.0, 0.0);
        // Yaw 0 looks down +X, and +Y is left.
        let points = to_camera_space(
            &[Vec3::new(100.0, 0.0, 0.0), Vec3::new(0.0, 50.0, 0.0), Vec3::new(0.0, 0.0, 50.0)],
            Vec3::ZERO,
            basis,
        );
        assert!((points[0].z - 100.0).abs() < 1e-4, "forward should land on +z: {:?}", points[0]);
        assert!(points[1].x < 0.0, "world +Y is to the left, so camera -x: {:?}", points[1]);
        assert!((points[2].y - 50.0).abs() < 1e-4, "world +Z is up: {:?}", points[2]);
    }

    #[test]
    fn the_viewport_default_camera_can_see_the_starter_room() {
        // A fresh editor should open on something, not on an empty pane.
        let document = starter_document();
        let mut viewport = Viewport::new(ViewportKind::Perspective);
        viewport.focus_on(Aabb::new(Vec3::ZERO, Vec3::splat(512.0)));
        let faces = visible_faces(&document, viewport.eye, viewport.angles.vectors());
        assert!(!faces.is_empty(), "the default 3D view shows nothing");
    }
}
