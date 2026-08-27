//! Drawing the viewports.
//!
//! The 2D panes are drawn with egui's painter: a grid, then brush outlines,
//! then entities, then whatever the current tool is previewing. That is all a
//! 2D view needs, and doing it in immediate mode means there is no scene graph
//! to keep in step with the document.
//!
//! The 3D pane is drawn the same way, as shaded polygons sorted back to front.
//! It is a painter's-algorithm view rather than a GPU one, so it will not show
//! textures or lighting -- that is what the compiled map in the engine is for,
//! one keystroke away. What it does show, accurately, is shape and scale,
//! which is what you are judging while building.

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

/// Draw the 3D pane as shaded polygons.
pub fn draw_3d(painter: &Painter, rect: Rect, viewport: &Viewport, document: &Document) {
    painter.rect_filled(rect, 0.0, colors::BACKGROUND);

    let basis = viewport.angles.vectors();
    let aspect = rect.width() / rect.height().max(1.0);
    let half_y = (void_render::vertical_fov(viewport.fov, aspect) * 0.5).tan().max(1e-4);
    let half_x = half_y * aspect;

    // Project into pane pixels, or `None` if behind the camera.
    let project = |world: Vec3| -> Option<(Pos2, f32)> {
        let relative = world - viewport.eye;
        let depth = relative.dot(basis.forward);
        if depth <= 1.0 { return None; }
        let x = relative.dot(basis.right) / (depth * half_x);
        let y = relative.dot(basis.up) / (depth * half_y);
        Some((
            Pos2::new(
                rect.center().x + x * rect.width() * 0.5,
                rect.center().y - y * rect.height() * 0.5,
            ),
            depth,
        ))
    };

    // Gather faces with a depth, then draw back to front. A painter's
    // algorithm gets the common cases right and is wrong only where faces
    // interpenetrate, which brush geometry rarely does.
    let mut faces: Vec<(f32, Vec<Pos2>, Color32)> = Vec::new();

    for (entity, solid) in document.map.all_solids() {
        let selected = document.selection.solids.contains(&solid.id)
            || document.selection.entities.contains(&entity.id);

        for (side, winding) in solid.face_windings() {
            let Some(plane) = side.plane() else { continue };
            // Back-face cull: a face pointing away is inside the brush.
            if plane.normal.dot(viewport.eye - winding.center()) <= 0.0 { continue; }

            let mut screen = Vec::with_capacity(winding.points.len());
            let mut total_depth = 0.0;
            let mut visible = true;
            for p in &winding.points {
                match project(*p) {
                    Some((pos, depth)) => {
                        screen.push(pos);
                        total_depth += depth;
                    }
                    None => { visible = false; break; }
                }
            }
            if !visible || screen.len() < 3 { continue; }

            // Flat shading from the face normal: enough to read shape without
            // any lighting data, which the editor does not have.
            let light = (plane.normal.dot(Vec3::new(0.4, 0.3, 0.87).normalize()) * 0.5 + 0.5)
                .clamp(0.25, 1.0);
            let base = if selected { colors::SELECTED } else { colors::BRUSH };
            let color = Color32::from_rgb(
                (base.r() as f32 * light) as u8,
                (base.g() as f32 * light) as u8,
                (base.b() as f32 * light) as u8,
            );

            faces.push((total_depth / screen.len() as f32, screen, color));
        }
    }

    faces.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, points, color) in faces {
        painter.add(egui::Shape::convex_polygon(
            points,
            color,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 90)),
        ));
    }

    // Point entities as small markers, so they can be seen in 3D too.
    for entity in document.map.entities.iter().filter(|e| e.solids.is_empty()) {
        let Some((pos, _)) = project(entity.origin()) else { continue };
        let selected = document.selection.entities.contains(&entity.id);
        painter.circle_stroke(
            pos,
            4.0,
            Stroke::new(1.5, if selected { colors::SELECTED } else { colors::ENTITY }),
        );
    }
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
