// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Picking out and dragging a brush's corners, edges and faces.
//!
//! The select tool's modes, as Hammer 5 has them: *object* picks whole
//! brushes and entities; *vertex*, *edge* and *face* put handles on the
//! selected brushes and let one be dragged. The geometry is in
//! [`crate::brush_edit`] and tested there; this is the handles, the drag
//! and the undo step.
//!
//! Handles are dragged in the flat views, where a drag has an unambiguous
//! plane to move in -- the same reason the rest of the editor moves
//! geometry there. In the 3D view they can be picked, so a corner can be
//! found by looking at it and then moved in whichever flat view suits.

use super::*;
use crate::brush_edit;
use kerosene_map::Solid;
use kerosene_toolui::theme::colors;

/// What a click in the select tool takes hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectMode {
    /// Whole brushes and entities.
    #[default]
    Object,
    /// Brush corners.
    Vertex,
    /// Brush edges.
    Edge,
    /// Brush faces.
    Face,
}

impl SelectMode {
    pub fn all() -> [SelectMode; 4] {
        [
            SelectMode::Object,
            SelectMode::Vertex,
            SelectMode::Edge,
            SelectMode::Face,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            SelectMode::Object => "object",
            SelectMode::Vertex => "vertex",
            SelectMode::Edge => "edge",
            SelectMode::Face => "face",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            SelectMode::Object => "Pick whole brushes and entities.  shift-1",
            SelectMode::Vertex => {
                "Handles on the selected brushes' corners: click to pick, drag in a flat view to move.  shift-2"
            }
            SelectMode::Edge => {
                "Handles on the selected brushes' edges: drag one to move both its corners.  shift-3"
            }
            SelectMode::Face => {
                "Handles on the selected brushes' faces: drag to push or pull a face; extrude grows a new brush off it.  shift-4"
            }
        }
    }
}

/// A picked corner or edge, by where it is: a corner's index changes every
/// time its brush does, its position only when it is the one moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element {
    Vertex { solid: u32, at: Vec3 },
    Edge { solid: u32, a: Vec3, b: Vec3 },
}

/// One handle on the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Handle {
    Element(Element),
    Face { solid: u32, side: u32 },
}

/// A handle drag in a flat pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ElementDrag {
    pub pane: usize,
    /// Where the grabbed handle was when the drag began.
    pub grabbed: Vec3,
    /// How far it has gone, snapped.
    pub delta: Vec3,
}

/// How close, in pixels, a click must be to a handle to take it.
const REACH: f32 = 8.0;

impl ChiselApp {
    /// Every handle on the selected brushes in the current mode, with where
    /// it is and whether it is picked.
    pub(super) fn handles(&self) -> Vec<(Handle, Vec3, bool)> {
        let mut out = Vec::new();
        if self.tool.kind != ToolKind::Select || self.select_mode == SelectMode::Object {
            return out;
        }
        for id in self.document.selected_solid_ids() {
            let Some(solid) = self.document.find_solid(id) else {
                continue;
            };
            let topology = brush_edit::topology(solid);
            match self.select_mode {
                SelectMode::Object => {}
                SelectMode::Vertex => {
                    for &at in &topology.vertices {
                        let element = Element::Vertex { solid: id, at };
                        let picked = self.elements.iter().any(|e| same(e, &element));
                        out.push((Handle::Element(element), at, picked));
                    }
                }
                SelectMode::Edge => {
                    for (i, &(a, b)) in topology.edges.iter().enumerate() {
                        let element = Element::Edge {
                            solid: id,
                            a: topology.vertices[a],
                            b: topology.vertices[b],
                        };
                        let picked = self.elements.iter().any(|e| same(e, &element));
                        out.push((Handle::Element(element), topology.edge_midpoint(i), picked));
                    }
                }
                SelectMode::Face => {
                    for (i, (side, _)) in topology.faces.iter().enumerate() {
                        let picked = self.document.selection.faces.contains(&(id, *side));
                        out.push((
                            Handle::Face {
                                solid: id,
                                side: *side,
                            },
                            topology.face_centre(i),
                            picked,
                        ));
                    }
                }
            }
        }
        out
    }

    /// Where a world point lands in a pane, in screen points.
    pub(super) fn project(&self, index: usize, rect: egui::Rect, p: Vec3) -> Option<egui::Pos2> {
        let viewport = &self.viewports[index];
        if viewport.kind.is_2d() {
            let (x, y) = viewport.world_to_screen(p);
            return Some(egui::pos2(rect.min.x + x, rect.min.y + y));
        }
        let m = crate::gpu::view_projection(
            viewport.eye,
            viewport.angles.vectors(),
            viewport.fov,
            rect.width() / rect.height().max(1.0),
        );
        let clip = m * p.extend(1.0);
        if clip.w < crate::draw::NEAR {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(egui::pos2(
            rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(),
            rect.min.y + (0.5 - ndc.y * 0.5) * rect.height(),
        ))
    }

    /// The handles, and while one is dragged, the brushes as they would be.
    pub(super) fn draw_handles(&self, painter: &egui::Painter, index: usize, rect: egui::Rect) {
        let handles = self.handles();
        if handles.is_empty() {
            return;
        }
        let hovered = painter
            .ctx()
            .pointer_hover_pos()
            .filter(|p| rect.contains(*p))
            .and_then(|p| self.handle_at(index, rect, p));

        // The picked edges, drawn along their length.
        for element in &self.elements {
            if let Element::Edge { a, b, .. } = element
                && let (Some(a), Some(b)) =
                    (self.project(index, rect, *a), self.project(index, rect, *b))
            {
                painter.line_segment([a, b], egui::Stroke::new(2.5_f32, colors::ACCENT));
            }
        }

        for (handle, at, picked) in &handles {
            let Some(pos) = self.project(index, rect, *at) else {
                continue;
            };
            let hot = hovered.as_ref().is_some_and(|(h, _)| h == handle);
            let fill = if *picked {
                colors::ACCENT
            } else if hot {
                colors::TEXT
            } else {
                colors::BG_FIELD
            };
            let stroke = egui::Stroke::new(
                1.0_f32,
                if *picked {
                    colors::ON_ACCENT
                } else {
                    colors::TEXT
                },
            );
            let r = if hot { 4.5 } else { 3.5 };
            match handle {
                Handle::Element(Element::Vertex { .. }) => {
                    let square = egui::Rect::from_center_size(pos, egui::vec2(r * 2.0, r * 2.0));
                    painter.rect_filled(square, 0.0, fill);
                    painter.rect_stroke(square, 0.0, stroke, egui::StrokeKind::Middle);
                }
                Handle::Element(Element::Edge { .. }) => {
                    let points = vec![
                        pos + egui::vec2(0.0, -r - 1.0),
                        pos + egui::vec2(r + 1.0, 0.0),
                        pos + egui::vec2(0.0, r + 1.0),
                        pos + egui::vec2(-r - 1.0, 0.0),
                    ];
                    painter.add(egui::Shape::convex_polygon(points, fill, stroke));
                }
                Handle::Face { .. } => {
                    painter.circle(pos, r, fill, stroke);
                }
            }
        }

        // The drag's result, outlined where it will land.
        if let Some(drag) = self.element_drag {
            let color = match self.element_edit(drag.delta) {
                Ok(_) => colors::ACCENT,
                Err(_) => colors::ERR,
            };
            let solids = self.element_edit(drag.delta).unwrap_or_default();
            for solid in &solids {
                for (_, winding) in solid.face_windings() {
                    let points: Vec<egui::Pos2> = winding
                        .points
                        .iter()
                        .filter_map(|p| self.project(index, rect, *p))
                        .collect();
                    if points.len() == winding.points.len() {
                        for i in 0..points.len() {
                            painter.line_segment(
                                [points[i], points[(i + 1) % points.len()]],
                                egui::Stroke::new(1.5_f32, color),
                            );
                        }
                    }
                }
            }
            if let Err(why) = self.element_edit(drag.delta) {
                painter.text(
                    rect.left_bottom() + egui::vec2(8.0, -8.0),
                    egui::Align2::LEFT_BOTTOM,
                    why.to_string(),
                    egui::FontId::proportional(12.0),
                    colors::ERR,
                );
            }
        }
    }

    /// The handle under a screen point, if one is within reach.
    pub(super) fn handle_at(
        &self,
        index: usize,
        rect: egui::Rect,
        pointer: egui::Pos2,
    ) -> Option<(Handle, Vec3)> {
        self.handles()
            .into_iter()
            .filter_map(|(handle, at, _)| {
                let pos = self.project(index, rect, at)?;
                let d = pos.distance(pointer);
                (d <= REACH).then_some((handle, at, d))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(handle, at, _)| (handle, at))
    }

    /// Pick a handle, or add it to what is picked.
    fn pick_handle(&mut self, handle: Handle, add: bool) {
        match handle {
            Handle::Element(element) => {
                if !add {
                    self.elements.clear();
                }
                if let Some(at) = self.elements.iter().position(|e| same(e, &element)) {
                    if add {
                        self.elements.remove(at);
                    }
                } else {
                    self.elements.push(element);
                }
            }
            Handle::Face { solid, side } => {
                if !add {
                    self.document.selection.faces.clear();
                }
                if !self.document.selection.faces.insert((solid, side)) && add {
                    self.document.selection.faces.remove(&(solid, side));
                }
            }
        }
    }

    /// What the picked elements would make of their brushes, moved by
    /// `delta`: every brush with something picked, edited, or why not.
    pub(super) fn element_edit(&self, delta: Vec3) -> Result<Vec<Solid>, brush_edit::EditError> {
        let mut out = Vec::new();
        for id in self.document.selected_solid_ids() {
            let Some(solid) = self.document.find_solid(id) else {
                continue;
            };
            let edited = match self.select_mode {
                SelectMode::Face => {
                    let faces: Vec<u32> = self
                        .document
                        .selection
                        .faces
                        .iter()
                        .filter(|(s, _)| *s == id)
                        .map(|(_, side)| *side)
                        .collect();
                    if faces.is_empty() {
                        continue;
                    }
                    let topology = brush_edit::topology(solid);
                    let corners: Vec<usize> = topology
                        .faces
                        .iter()
                        .filter(|(side, _)| faces.contains(side))
                        .flat_map(|(_, c)| c.iter().copied())
                        .collect();
                    brush_edit::move_vertices(solid, &corners, delta)?
                }
                _ => {
                    let points: Vec<Vec3> = self
                        .elements
                        .iter()
                        .flat_map(|e| match *e {
                            Element::Vertex { solid, at } if solid == id => vec![at],
                            Element::Edge { solid, a, b } if solid == id => vec![a, b],
                            _ => Vec::new(),
                        })
                        .collect();
                    if points.is_empty() {
                        continue;
                    }
                    brush_edit::move_points(solid, &points, delta)?
                }
            };
            out.push(edited);
        }
        if out.is_empty() {
            return Err(brush_edit::EditError::Nothing);
        }
        Ok(out)
    }

    /// Apply a move of the picked elements as one undo step, and keep them
    /// picked where they went. Returns whether it was applied.
    pub fn move_elements(&mut self, delta: Vec3) -> bool {
        if delta == Vec3::ZERO {
            return false;
        }
        match self.element_edit(delta) {
            Ok(solids) => {
                let label = format!("move {}", self.select_mode.label());
                self.document.replace_solids(label, solids);
                for element in &mut self.elements {
                    match element {
                        Element::Vertex { at, .. } => *at += delta,
                        Element::Edge { a, b, .. } => {
                            *a += delta;
                            *b += delta;
                        }
                    }
                }
                self.status = format!("moved the {}", self.select_mode.label());
                true
            }
            Err(why) => {
                self.status = why.to_string();
                false
            }
        }
    }

    /// Merge the selected brushes, or say why not.
    pub fn merge_brushes(&mut self) {
        self.status = match self.document.merge_selected_solids() {
            Ok(_) => "merged into one brush".into(),
            Err(why) => why.to_string(),
        };
    }

    /// Grow a new brush off each picked face, `distance` out.
    pub fn extrude_faces(&mut self, distance: f32) -> usize {
        let mut grown = Vec::new();
        for &(solid, side) in &self.document.selection.faces {
            if let Some(brush) = self.document.find_solid(solid)
                && let Ok(new) = brush_edit::extrude_face(brush, side, distance)
            {
                grown.push(new);
            }
        }
        let n = grown.len();
        if n > 0 {
            self.document.create_shape(grown, "extrude");
            self.status = format!("extruded {n} face{}", if n == 1 { "" } else { "s" });
        } else {
            self.status = "pick a face to extrude first".into();
        }
        n
    }

    /// A click or drag in a pane, while the select tool is in an element
    /// mode. Returns whether it was taken: a click that hits no handle falls
    /// through to the ordinary select tool, so a brush can still be picked.
    pub(super) fn element_input(
        &mut self,
        index: usize,
        rect: egui::Rect,
        response: &egui::Response,
        ui: &egui::Ui,
    ) -> bool {
        if self.tool.kind != ToolKind::Select || self.select_mode == SelectMode::Object {
            self.element_drag = None;
            return false;
        }
        let add = ui.input(|i| i.modifiers.shift);
        let kind = self.viewports[index].kind;

        // A drag under way, in this pane.
        if let Some(mut drag) = self.element_drag.filter(|d| d.pane == index) {
            if let Some(pos) = response.interact_pointer_pos() {
                let (h, v, depth) = kind.axes();
                let viewport = &self.viewports[index];
                let world = viewport.screen_to_world(
                    pos.x - rect.min.x,
                    pos.y - rect.min.y,
                    drag.grabbed[depth],
                );
                let target = self.document.grid.snap_point(world);
                let mut delta = Vec3::ZERO;
                delta[h] = target[h] - drag.grabbed[h];
                delta[v] = target[v] - drag.grabbed[v];
                drag.delta = delta;
                self.element_drag = Some(drag);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary) || !response.dragged() {
                self.element_drag = None;
                self.move_elements(drag.delta);
            }
            return true;
        }

        let Some(pointer) = response.interact_pointer_pos() else {
            return false;
        };
        let pressed = response.drag_started_by(egui::PointerButton::Primary) || response.clicked();
        if !pressed {
            return false;
        }
        let Some((handle, at)) = self.handle_at(index, rect, pointer) else {
            // An element mode with nothing hit: let go of what is picked,
            // and let the click select a brush as usual.
            if response.clicked() && !add {
                self.elements.clear();
                if self.select_mode == SelectMode::Face {
                    self.document.selection.faces.clear();
                }
            }
            return false;
        };

        let already = match handle {
            Handle::Element(e) => self.elements.iter().any(|x| same(x, &e)),
            Handle::Face { solid, side } => self.document.selection.faces.contains(&(solid, side)),
        };
        // Dragging something already picked keeps the rest of what is
        // picked; clicking picks this one alone, or adds it with shift.
        if !(already && response.drag_started()) {
            self.pick_handle(handle, add);
        }
        if response.drag_started_by(egui::PointerButton::Primary) && kind.is_2d() {
            self.element_drag = Some(ElementDrag {
                pane: index,
                grabbed: at,
                delta: Vec3::ZERO,
            });
        } else if response.drag_started() && !kind.is_2d() {
            self.status = "drag a handle in a flat view to move it".into();
        }
        true
    }
}

/// The same element, give or take float noise.
fn same(a: &Element, b: &Element) -> bool {
    const CLOSE: f32 = 0.05;
    match (a, b) {
        (Element::Vertex { solid: s, at: p }, Element::Vertex { solid: t, at: q }) => {
            s == t && p.distance(*q) < CLOSE
        }
        (
            Element::Edge {
                solid: s,
                a: a1,
                b: b1,
            },
            Element::Edge {
                solid: t,
                a: a2,
                b: b2,
            },
        ) => {
            s == t
                && ((a1.distance(*a2) < CLOSE && b1.distance(*b2) < CLOSE)
                    || (a1.distance(*b2) < CLOSE && b1.distance(*a2) < CLOSE))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_cube() -> (ChiselApp, u32) {
        let mut app = ChiselApp::new(std::path::PathBuf::from("/nonexistent"));
        app.document = Document::new();
        let id = app.document.create_block(Vec3::ZERO, Vec3::splat(64.0));
        app.tool.set_kind(ToolKind::Select);
        (app, id)
    }

    #[test]
    fn object_mode_has_no_handles_and_vertex_mode_has_one_per_corner() {
        let (mut app, _) = app_with_cube();
        assert!(app.handles().is_empty());
        app.select_mode = SelectMode::Vertex;
        assert_eq!(app.handles().len(), 8);
        app.select_mode = SelectMode::Edge;
        assert_eq!(app.handles().len(), 12);
        app.select_mode = SelectMode::Face;
        assert_eq!(app.handles().len(), 6);
    }

    #[test]
    fn a_picked_corner_moves_as_one_undo_step_and_stays_picked() {
        let (mut app, id) = app_with_cube();
        app.select_mode = SelectMode::Vertex;
        app.elements.push(Element::Vertex {
            solid: id,
            at: Vec3::splat(64.0),
        });
        let depth = app.document.undo_depth();
        assert!(app.move_elements(Vec3::new(0.0, 0.0, 32.0)));
        assert_eq!(app.document.undo_depth(), depth + 1);
        assert_eq!(app.document.find_solid(id).unwrap().bounds().max.z, 96.0);
        assert!(
            matches!(app.elements[0], Element::Vertex { at, .. } if at == Vec3::new(64.0, 64.0, 96.0))
        );
        // And moving it again moves the same corner.
        assert!(app.move_elements(Vec3::new(0.0, 0.0, 32.0)));
        assert_eq!(app.document.find_solid(id).unwrap().bounds().max.z, 128.0);
    }

    #[test]
    fn a_dent_is_refused_and_leaves_the_map_alone() {
        let (mut app, id) = app_with_cube();
        app.select_mode = SelectMode::Vertex;
        app.elements.push(Element::Vertex {
            solid: id,
            at: Vec3::splat(64.0),
        });
        let before = app.document.find_solid(id).unwrap().clone();
        let depth = app.document.undo_depth();
        assert!(!app.move_elements(Vec3::splat(-40.0)));
        assert_eq!(app.document.undo_depth(), depth);
        assert_eq!(app.document.find_solid(id).unwrap(), &before);
        assert!(app.status.contains("convex"), "{}", app.status);
    }

    #[test]
    fn a_picked_face_is_pushed_and_extruded() {
        let (mut app, id) = app_with_cube();
        app.select_mode = SelectMode::Face;
        let top = app
            .document
            .find_solid(id)
            .unwrap()
            .sides
            .iter()
            .find(|s| s.plane().unwrap().normal.z > 0.9)
            .unwrap()
            .id;
        app.document.selection.faces.insert((id, top));
        assert!(app.move_elements(Vec3::new(0.0, 0.0, 16.0)));
        assert_eq!(app.document.find_solid(id).unwrap().bounds().max.z, 80.0);
        let before = app.document.map.world.solids.len();
        assert_eq!(app.extrude_faces(32.0), 1);
        assert_eq!(app.document.map.world.solids.len(), before + 1);
    }

    #[test]
    fn a_3d_point_projects_into_its_pane() {
        let (mut app, _) = app_with_cube();
        let index = app.viewports.iter().position(|v| !v.kind.is_2d()).unwrap();
        app.viewports[index].eye = Vec3::new(-200.0, 32.0, 32.0);
        app.viewports[index].angles = kerosene_math::Angles::ZERO;
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(400.0, 300.0));
        let centre = app
            .project(index, rect, Vec3::new(0.0, 32.0, 32.0))
            .unwrap();
        assert!((centre - rect.center()).length() < 0.5, "{centre:?}");
        assert!(
            app.project(index, rect, Vec3::new(-400.0, 32.0, 32.0))
                .is_none(),
            "behind"
        );
    }
}
