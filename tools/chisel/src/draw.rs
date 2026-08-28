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
    /// The leak trace. Deliberately the loudest thing on screen: it is only
    /// ever drawn when the map is broken.
    pub const LEAK: Color32 = Color32::from_rgb(255, 70, 70);
}

/// Every fourth grid line is drawn brighter, so it is possible to count
/// squares at a glance rather than by dragging along them.
const MAJOR_EVERY: i64 = 4;

/// The selection's outline, moved by a drag, as world-space polygons.
///
/// A move used to show only the rubber band between where the drag started
/// and where the pointer is, which says nothing about where the thing being
/// moved will end up. This is the shape it will land in, drawn where it will
/// land -- the answer to the question a person is actually asking while
/// dragging.
///
/// Point entities come back as a small cross so they are not invisible during
/// a move, which is the case where guessing is hardest.
pub fn ghost_outline(document: &Document, delta: Vec3) -> Vec<Vec<Vec3>> {
    let mut polygons = Vec::new();

    for (entity, solid) in document.map.all_solids() {
        let moving = document.selection.solids.contains(&solid.id)
            || document.selection.entities.contains(&entity.id);
        if !moving { continue }
        for (_, winding) in solid.face_windings() {
            polygons.push(winding.points.iter().map(|p| *p + delta).collect());
        }
    }

    for entity in document.map.entities.iter().filter(|e| e.solids.is_empty()) {
        if !document.selection.entities.contains(&entity.id) { continue }
        let at = entity.origin() + delta;
        const ARM: f32 = 8.0;
        for axis in 0..3 {
            let mut a = at;
            let mut b = at;
            a[axis] -= ARM;
            b[axis] += ARM;
            polygons.push(vec![a, b]);
        }
    }

    polygons
}

/// Stroke world-space polygons into a 2D pane.
fn stroke_polygons(
    painter: &Painter,
    rect: Rect,
    viewport: &Viewport,
    polygons: &[Vec<Vec3>],
    stroke: Stroke,
) {
    for polygon in polygons {
        let points: Vec<Pos2> = polygon
            .iter()
            .map(|p| {
                let (x, y) = viewport.world_to_screen(*p);
                Pos2::new(rect.min.x + x, rect.min.y + y)
            })
            .collect();
        // A two-point "polygon" is a line, not a loop: closing it would draw
        // an entity marker's arms twice.
        let last = if points.len() > 2 { points.len() } else { points.len().saturating_sub(1) };
        for i in 0..last {
            painter.line_segment([points[i], points[(i + 1) % points.len()]], stroke);
        }
    }
}

/// Draw one 2D pane.
pub fn draw_2d(
    painter: &Painter,
    rect: Rect,
    viewport: &Viewport,
    document: &Document,
    tool: &Tool,
    leak: &crate::leak::LeakTrace,
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

    draw_leak(painter, rect, viewport, leak);
    draw_tool_preview(painter, rect, viewport, document, tool);
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
fn draw_tool_preview(
    painter: &Painter,
    rect: Rect,
    viewport: &Viewport,
    document: &Document,
    tool: &Tool,
) {
    let Some(drag) = &tool.drag else { return };
    if !drag.is_dragging && tool.kind != ToolKind::Block { return; }

    // A select drag moves the selection. Show where it is going, not the
    // rectangle the pointer swept out -- the rectangle is not a thing that
    // exists after the drag ends.
    if tool.kind == ToolKind::Select {
        let delta = drag.delta();
        let ghost = ghost_outline(document, delta);
        if ghost.is_empty() { return }
        stroke_polygons(painter, rect, viewport, &ghost, Stroke::new(1.5, colors::TOOL_PREVIEW));

        let (h, v, _) = viewport.kind.axes();
        let anchor = viewport.world_to_screen(drag.current);
        painter.text(
            Pos2::new(rect.min.x + anchor.0 + 10.0, rect.min.y + anchor.1 + 10.0),
            egui::Align2::LEFT_TOP,
            format!(
                "{} {}, {} {}",
                axis_name(h),
                void_math::units::length_short(delta[h]),
                axis_name(v),
                void_math::units::length_short(delta[v]),
            ),
            egui::FontId::monospace(11.0),
            colors::TOOL_PREVIEW,
        );
        return;
    }

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

    // The size in void units, which is what a designer is actually reading
    // off the screen while dragging. Without a unit on it the number is just
    // a number.
    let size = bounds.size();
    painter.text(
        preview.right_bottom() + Vec2::new(4.0, 4.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{} x {} vu",
            void_math::format_float(size[h]),
            void_math::format_float(size[v])
        ),
        egui::FontId::monospace(11.0),
        colors::TOOL_PREVIEW,
    );
}

/// Draw the route out of a leaking map.
///
/// A leak file that nothing draws is a list of coordinates, and finding a
/// one-unit gap in a large map from coordinates is not a reasonable thing to
/// ask. Follow the line to the wall it goes through.
fn draw_leak(painter: &Painter, rect: Rect, viewport: &Viewport, leak: &crate::leak::LeakTrace) {
    if leak.is_empty() { return }
    let stroke = Stroke::new(2.0, colors::LEAK);
    let to_screen = |world: Vec3| {
        let (x, y) = viewport.world_to_screen(world);
        Pos2::new(rect.min.x + x, rect.min.y + y)
    };
    for pair in leak.points.windows(2) {
        painter.line_segment([to_screen(pair[0]), to_screen(pair[1])], stroke);
    }
    if let Some(start) = leak.origin() {
        painter.text(
            to_screen(start) + Vec2::new(6.0, -14.0),
            egui::Align2::LEFT_TOP,
            "leak",
            egui::FontId::monospace(11.0),
            colors::LEAK,
        );
    }
}

/// The name of a world axis, for a readout that would otherwise be two
/// unlabelled numbers.
pub fn axis_name(axis: usize) -> &'static str {
    ["x", "y", "z"][axis.min(2)]
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

/// A vertex on its way to the screen: where it is, and what texel is on it.
///
/// The two travel together because clipping has to move both. A polygon cut
/// by the near plane gains vertices, and a new vertex with no texture
/// coordinate would slide its whole face's texture -- visibly, and only at
/// the angles where the clip happens.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FaceVertex {
    /// Camera space: `x` right, `y` up, `z` forward.
    pub position: Vec3,
    /// Texture coordinate, in texels rather than normalised, because that is
    /// what the face's texture axes produce and the texture's size is not
    /// known here.
    pub texel: (f32, f32),
}

impl FaceVertex {
    fn lerp(self, other: FaceVertex, t: f32) -> FaceVertex {
        FaceVertex {
            position: self.position + (other.position - self.position) * t,
            texel: (
                self.texel.0 + (other.texel.0 - self.texel.0) * t,
                self.texel.1 + (other.texel.1 - self.texel.1) * t,
            ),
        }
    }
}

/// The texel coordinate of a world point on a face.
///
/// The same arithmetic Cleave writes into a `TexInfo` and the engine's shader
/// reads, so what Chisel shows and what the compiled map shows are the same
/// thing. A preview that computed this differently would be a preview that
/// lies about texture alignment, which is the one thing it is for.
pub fn texel_for(side: &void_map::Side, point: Vec3) -> (f32, f32) {
    let u = &side.uaxis;
    let v = &side.vaxis;
    (
        point.dot(u.axis) / u.safe_scale() + u.offset,
        point.dot(v.axis) / v.safe_scale() + v.offset,
    )
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
pub fn clip_near(polygon: &[FaceVertex], near: f32) -> Vec<FaceVertex> {
    if polygon.len() < 3 { return Vec::new(); }

    let mut out: Vec<FaceVertex> = Vec::with_capacity(polygon.len() + 2);
    for i in 0..polygon.len() {
        let current = polygon[i];
        let next = polygon[(i + 1) % polygon.len()];
        let current_in = current.position.z >= near;
        let next_in = next.position.z >= near;

        if current_in { out.push(current); }
        // Emit a crossing point whenever the edge changes side.
        if current_in != next_in {
            let span = next.position.z - current.position.z;
            if span.abs() > 1e-9 {
                let t = (near - current.position.z) / span;
                let mut crossing = current.lerp(next, t);
                // Land exactly on the plane rather than a hair off it, so the
                // perspective divide below cannot see a depth of zero.
                crossing.position.z = near;
                // A vertex sitting exactly on the plane is both inside and a
                // crossing, and would otherwise be emitted twice. A repeated
                // point makes a zero-length edge, which is where stroke
                // tessellation used to produce a spike across the screen.
                if out.last().is_none_or(|last| {
                    last.position.distance_squared(crossing.position) > 1e-12
                }) {
                    out.push(crossing);
                }
            }
        }
    }
    // The same again across the wrap: the crossing on the last edge can land
    // exactly on the first vertex.
    if out.len() >= 2
        && out[0].position.distance_squared(out.last().expect("checked").position) <= 1e-12
    {
        out.pop();
    }
    if out.len() < 3 { Vec::new() } else { out }
}

/// Clip a plain camera-space polygon, for callers with no texture on it.
pub fn clip_near_positions(polygon: &[Vec3], near: f32) -> Vec<Vec3> {
    let vertices: Vec<FaceVertex> = polygon
        .iter()
        .map(|p| FaceVertex { position: *p, texel: (0.0, 0.0) })
        .collect();
    clip_near(&vertices, near).into_iter().map(|v| v.position).collect()
}

/// One face of the document, ready to draw in the 3D pane.
#[derive(Clone, Debug)]
pub struct VisibleFace {
    /// The polygon in camera space, clipped to the near plane.
    pub polygon: Vec<FaceVertex>,
    /// Sort key: the depth of the farthest vertex.
    pub depth: f32,
    /// World-space face normal, for flat shading.
    pub normal: Vec3,
    /// Which material is on it.
    pub material: String,
    pub selected: bool,
    /// Whether this face in particular is selected, as against its brush.
    pub face_selected: bool,
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
            let vertices: Vec<FaceVertex> = winding
                .points
                .iter()
                .zip(camera)
                .map(|(world, position)| FaceVertex { position, texel: texel_for(side, *world) })
                .collect();
            let polygon = clip_near(&vertices, NEAR);
            if polygon.len() < 3 { continue; }

            // Sort by the *farthest* vertex, not the average. A small object
            // standing on a large surface has a greater average depth than the
            // surface it sits on, so averaging sorts the surface later and
            // paints it straight over the object.
            let depth = polygon.iter().fold(f32::MIN, |acc, v| acc.max(v.position.z));
            let face_selected = document.selection.faces.contains(&(solid.id, side.id));
            faces.push(VisibleFace {
                polygon,
                depth,
                normal: plane.normal,
                material: side.material.clone(),
                selected,
                face_selected,
            });
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

    // ---- the move ghost --------------------------------------------------

    /// Two boxes in the world, so a move has something to move and something
    /// to leave behind.
    fn two_brushes() -> (Document, u32, u32) {
        let mut document = Document::new();
        document.grid.size = 16.0;
        let a = document.create_block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(64.0, 64.0, 64.0));
        let b = document.create_block(Vec3::new(256.0, 0.0, 0.0), Vec3::new(320.0, 64.0, 64.0));
        document.selection.clear();
        (document, a, b)
    }

    #[test]
    fn nothing_selected_means_no_ghost() {
        let (document, _, _) = two_brushes();
        assert!(ghost_outline(&document, Vec3::new(64.0, 0.0, 0.0)).is_empty());
    }

    #[test]
    fn the_ghost_is_the_selection_where_it_will_land() {
        // The bug: a drag drew the rubber band between where the pointer
        // started and where it is now, which says nothing about where the
        // brush ends up. This is the shape, at the destination.
        let (mut document, a, _) = two_brushes();
        document.selection.solids.insert(a);

        let before = document.find_solid(a).expect("the brush exists").bounds();
        let delta = Vec3::new(64.0, -32.0, 16.0);
        let ghost = ghost_outline(&document, delta);
        assert_eq!(ghost.len(), 6, "six faces of one box");

        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for polygon in &ghost {
            for p in polygon {
                min = min.min(*p);
                max = max.max(*p);
            }
        }
        assert!((min - (before.min + delta)).length() < 1e-3, "ghost is not at the destination");
        assert!((max - (before.max + delta)).length() < 1e-3);
    }

    #[test]
    fn an_unselected_brush_leaves_no_ghost_behind() {
        let (mut document, a, _) = two_brushes();
        document.selection.solids.insert(a);
        let ghost = ghost_outline(&document, Vec3::ZERO);
        assert_eq!(ghost.len(), 6, "only the selected box, not both");
        for polygon in &ghost {
            for p in polygon {
                assert!(p.x <= 64.0 + 1e-3, "a brush that is not selected was drawn as moving");
            }
        }
    }

    #[test]
    fn a_selected_point_entity_ghosts_as_a_marker() {
        // The hardest case to guess at: a point entity has no faces, so
        // without this it simply vanishes for the length of the drag.
        let (mut document, _, _) = two_brushes();
        let id = document.create_entity("light", Vec3::new(96.0, 96.0, 96.0));
        document.selection.clear();
        document.selection.entities.insert(id);

        let delta = Vec3::new(0.0, 0.0, 64.0);
        let ghost = ghost_outline(&document, delta);
        assert_eq!(ghost.len(), 3, "three arms of a cross: {ghost:?}");
        for arm in &ghost {
            assert_eq!(arm.len(), 2, "an arm is a line segment, not a loop");
            let midpoint = (arm[0] + arm[1]) * 0.5;
            assert!(
                (midpoint - Vec3::new(96.0, 96.0, 160.0)).length() < 1e-3,
                "an arm is centred at {midpoint:?}"
            );
        }
    }

    #[test]
    fn selecting_a_brush_entity_ghosts_all_of_its_brushes() {
        let (mut document, a, b) = two_brushes();
        document.selection.solids.insert(a);
        document.selection.solids.insert(b);
        let entity = document.tie_to_entity("func_door").expect("brushes tie to an entity");

        document.selection.clear();
        document.selection.entities.insert(entity);
        let ghost = ghost_outline(&document, Vec3::new(0.0, 0.0, 32.0));
        assert_eq!(ghost.len(), 12, "both brushes of the door move together");
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
        assert_eq!(clip_near_positions(&quad, NEAR), quad);
    }

    #[test]
    fn a_polygon_entirely_behind_is_dropped() {
        let quad = vec![
            Vec3::new(-10.0, -10.0, -100.0),
            Vec3::new(10.0, -10.0, -100.0),
            Vec3::new(10.0, 10.0, -100.0),
            Vec3::new(-10.0, 10.0, -100.0),
        ];
        assert!(clip_near_positions(&quad, NEAR).is_empty());
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
        let clipped = clip_near_positions(&quad, NEAR);
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
        assert_eq!(clip_near_positions(&quad, NEAR).len(), 5);
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
        let clipped = clip_near_positions(&quad, NEAR);
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
        assert!(clip_near_positions(&quad, NEAR).is_empty());
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
            for p in clip_near_positions(&quad, NEAR) {
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
            assert!(face.polygon.iter().all(|v| v.position.z >= NEAR - 1e-4));
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
