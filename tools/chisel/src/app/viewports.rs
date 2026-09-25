// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The four panes: layout, painting, and input.

use super::*;
use kerosene_math::Quat;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

/// The height of the strip across the top of each pane, in points.
const PANE_HEADER: f32 = 22.0;

impl ChiselApp {
    pub(super) fn viewports_panel(&mut self, ctx: &Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_APP)
                    .inner_margin(egui::Margin::same(3)),
            )
            .show(ctx, |ui| {
                let available = ui.available_rect_before_wrap();

                if let Some(index) = self.maximised {
                    self.viewport_ui(ui, index, available);
                    return;
                }

                // Panes divide at a draggable fraction rather than at the middle.
                // Half of laying out a level is looking at one view closely and
                // the others only for reference.
                self.split.x = self.split.x.clamp(0.1, 0.9);
                self.split.y = self.split.y.clamp(0.1, 0.9);
                let cut = egui::pos2(
                    available.min.x + available.width() * self.split.x,
                    available.min.y + available.height() * self.split.y,
                );
                let half = SPLITTER * 0.5;
                let (l, r) = (available.min.x, available.max.x);
                let (t, b) = (available.min.y, available.max.y);
                let rects = [
                    egui::Rect::from_min_max(
                        egui::pos2(l, t),
                        egui::pos2(cut.x - half, cut.y - half),
                    ),
                    egui::Rect::from_min_max(
                        egui::pos2(cut.x + half, t),
                        egui::pos2(r, cut.y - half),
                    ),
                    egui::Rect::from_min_max(
                        egui::pos2(l, cut.y + half),
                        egui::pos2(cut.x - half, b),
                    ),
                    egui::Rect::from_min_max(
                        egui::pos2(cut.x + half, cut.y + half),
                        egui::pos2(r, b),
                    ),
                ];
                for (index, rect) in rects.into_iter().enumerate() {
                    self.viewport_ui(ui, index, rect);
                }

                // Registered after the panes so they take the pointer first: a
                // splitter a viewport can steal the drag from is one that only
                // works some of the time.
                let vertical = egui::Rect::from_min_max(
                    egui::pos2(cut.x - half, t),
                    egui::pos2(cut.x + half, b),
                );
                let horizontal = egui::Rect::from_min_max(
                    egui::pos2(l, cut.y - half),
                    egui::pos2(r, cut.y + half),
                );
                for (bar, axis) in [(vertical, 0usize), (horizontal, 1usize)] {
                    let response =
                        ui.interact(bar, ui.id().with(("splitter", axis)), egui::Sense::drag());
                    if response.hovered() || response.dragged() {
                        ui.ctx().set_cursor_icon(if axis == 0 {
                            egui::CursorIcon::ResizeHorizontal
                        } else {
                            egui::CursorIcon::ResizeVertical
                        });
                    }
                    if response.dragged() {
                        let (delta, extent) = if axis == 0 {
                            (response.drag_delta().x, available.width())
                        } else {
                            (response.drag_delta().y, available.height())
                        };
                        self.split[axis] =
                            (self.split[axis] + delta / extent.max(1.0)).clamp(0.1, 0.9);
                    }
                    let lit = response.hovered() || response.dragged();
                    ui.painter().rect_filled(
                        bar,
                        0.0,
                        if lit { colors::ACCENT } else { colors::BORDER },
                    );
                }
            });
    }

    pub(super) fn viewport_ui(&mut self, ui: &mut egui::Ui, index: usize, rect: egui::Rect) {
        // A header strip across the top of the pane, and the view under it.
        let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), PANE_HEADER));
        let body =
            egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + PANE_HEADER), rect.max);

        let response = ui.allocate_rect(body, egui::Sense::click_and_drag());
        self.viewports[index].size = (body.width(), body.height());

        let painter = ui.painter_at(body);
        let kind = self.viewports[index].kind;

        // Where the entity tool would place its class if clicked right now:
        // every pane can answer this, unlike the status bar's plain pointer
        // position, because placement follows a well-defined rule (a ray hit
        // or a snapped 2D point) rather than needing a depth guess. Updated
        // like `pointer_world` below: only the hovered pane sets it, and only
        // that same pane clears it again, so the other three passes through
        // this loop each frame do not clobber it. Computed before drawing, so
        // the preview this frame draws is the one this frame just found.
        if self.tool.kind == ToolKind::Entity
            && let Some(pos) = response.hover_pos()
        {
            let (x, y) = (pos.x - body.min.x, pos.y - body.min.y);
            let viewport = &self.viewports[index];
            let point = if kind.is_2d() {
                crate::tools::entity_placement_point(&self.document, viewport, x, y)
            } else {
                let (origin, direction) = viewport.pick_ray(x, y);
                self.document.grid.snap_point(crate::tools::pick_point_3d(
                    &self.document,
                    origin,
                    direction,
                ))
            };
            self.entity_hover = Some((index, point));
        } else if self.entity_hover.is_some_and(|(i, _)| i == index) && !response.hovered() {
            self.entity_hover = None;
        }

        if kind.is_2d() {
            let entity_ghost = self
                .entity_hover
                .filter(|(i, _)| *i == index)
                .map(|(_, point)| (point, self.ghost_model().map(|m| m.bounds)));
            draw::draw_2d(
                &painter,
                body,
                &self.viewports[index],
                &self.document,
                &self.tool,
                &self.leak,
                entity_ghost,
            );
        } else {
            self.draw_preview(ui, &painter, index, body);
        }

        self.pane_header(ui, index, header);

        let active = self.active == index;
        ui.painter_at(rect).rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(
                1.0_f32,
                if active {
                    colors::ACCENT
                } else {
                    colors::BORDER
                },
            ),
            egui::StrokeKind::Inside,
        );

        if response.hovered() {
            self.active = index;
        }

        // Where the pointer is in the world, for the status bar. Only a flat
        // view can say; a 3D pane's pointer is a ray, not a point.
        if kind.is_2d()
            && let Some(pos) = response.hover_pos()
        {
            let depth = self
                .document
                .selection_bounds()
                .map(|b| b.center()[kind.axes().2])
                .unwrap_or(0.0);
            let world = self.viewports[index].screen_to_world(
                pos.x - body.min.x,
                pos.y - body.min.y,
                depth,
            );
            self.pointer_world = Some((index, world));
        } else if self.pointer_world.is_some_and(|(i, _)| i == index) && !response.hovered() {
            self.pointer_world = None;
        }

        self.viewport_input(index, body, &response, ui);
    }

    /// The strip along the top of a pane: which view it shows, how far in
    /// it is, and a button to make it the only pane.
    ///
    /// The view's name is a menu: any pane can show any view. Six flat views
    /// exist, and a layout that could only ever reach three of them was the
    /// reason to add the other three.
    fn pane_header(&mut self, ui: &mut egui::Ui, index: usize, header: egui::Rect) {
        let kind = self.viewports[index].kind;
        let active = self.active == index;

        let strip = ui.interact(
            header,
            ui.id().with(("pane-header", index)),
            egui::Sense::click(),
        );
        if strip.double_clicked() {
            self.active = index;
            self.toggle_maximised();
        }
        ui.painter_at(header).rect_filled(
            header,
            0.0,
            if active {
                colors::BG_HEADER
            } else {
                colors::BG_PANEL
            },
        );

        let mut chosen = None;
        let mut maximise = false;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(header.shrink2(egui::vec2(4.0, 1.0))),
            |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.spacing_mut().button_padding = egui::vec2(6.0, 1.0);
                    egui::ComboBox::from_id_salt(("view", index))
                        .selected_text(RichText::new(kind.label()).size(11.5).color(if active {
                            colors::ACCENT
                        } else {
                            colors::TEXT
                        }))
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for option in crate::viewport::ViewportKind::all() {
                                if ui
                                    .selectable_label(option == kind, option.label())
                                    .clicked()
                                {
                                    chosen = Some(option);
                                }
                            }
                        });

                    // How far in: a scale for a flat view, a speed for the 3D one.
                    let detail = if kind.is_2d() {
                        let zoom = self.viewports[index].zoom;
                        let per_square =
                            kerosene_math::units::length_short(self.document.grid.size);
                        format!("{per_square} grid   {:.2} px/ku", zoom)
                    } else {
                        format!(
                            "fly {}/s",
                            kerosene_math::units::length_short(self.fly_speed)
                        )
                    };
                    ui.label(theme::caption(detail));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let maximised = self.maximised == Some(index);
                        let (glyph, tip) = if maximised {
                            (icons::CORNERS_IN, "show four panes  (shift-space)")
                        } else {
                            (icons::CORNERS_OUT, "maximise this pane  (shift-space)")
                        };
                        if widgets::icon_button(ui, glyph, tip).clicked() {
                            maximise = true;
                        }
                    });
                });
            },
        );

        if let Some(option) = chosen {
            self.viewports[index].set_kind(option);
            self.status = format!("pane {} shows {}", index + 1, option.label());
        }
        if maximise {
            self.active = index;
            self.toggle_maximised();
        }
    }

    /// A click in a 3D pane.
    ///
    /// What it picks depends on the tool, because "the thing under the
    /// pointer" means different things: the select tool wants the brush or the
    /// entity that owns it, and the texture tool wants the one face you are
    /// looking at. Picking a brush when someone meant a face is how a whole
    /// room ends up wearing the same texture.
    pub(super) fn pick_in_3d(&mut self, index: usize, x: f32, y: f32, ui: &egui::Ui, double: bool) {
        let (origin, direction) = self.viewports[index].pick_ray(x, y);
        let (add, sample) = ui.input(|i| (i.modifiers.shift, i.modifiers.ctrl));

        if self.tool.kind == ToolKind::Texture {
            // Whether a plain click applies depends on the apply mode. Shift
            // always just selects, for building up a selection; the mode
            // decides what an unshifted click does.
            let apply = !add
                && match self.tool.texture_mode {
                    TextureMode::Selection => false,
                    TextureMode::ApplyDoubleClick => double,
                    TextureMode::AlwaysApply => true,
                };

            match self.tool.texture_target {
                TextureTarget::SingleFace => {
                    let Some((solid, side)) =
                        crate::tools::pick_face_3d(&self.document, origin, direction)
                    else {
                        if !add {
                            self.document.selection.faces.clear();
                        }
                        return;
                    };

                    // Ctrl samples: the eyedropper every texture tool has, and
                    // worth having now that the browser shows what you picked up.
                    if sample {
                        if let Some(material) = self
                            .document
                            .find_solid(solid)
                            .and_then(|s| s.sides.iter().find(|s| s.id == side))
                            .map(|s| s.material.clone())
                        {
                            self.status = format!("picked up {material}");
                            self.document.current_material = material;
                        }
                        return;
                    }

                    if !add {
                        self.document.selection.clear();
                    }
                    self.document.selection.faces.insert((solid, side));

                    if apply {
                        let material = self.document.current_material.clone();
                        self.document.apply_material();
                        self.status = format!("{material} on 1 face");
                    }
                }
                TextureTarget::WholeBrush => {
                    let Some(solid) =
                        crate::tools::pick_solid_3d(&self.document, origin, direction)
                    else {
                        if !add {
                            self.document.selection.clear();
                        }
                        return;
                    };

                    if !add {
                        self.document.selection.clear();
                    }
                    // A brush that belongs to an entity selects the entity:
                    // that is the thing a designer thinks of as the door.
                    let owner = self
                        .document
                        .map
                        .all_solids()
                        .find(|(_, s)| s.id == solid)
                        .map(|(e, _)| (e.id, e.is_brush_entity() && e.classname() != "worldspawn"));
                    match owner {
                        Some((entity, true)) => {
                            self.document.selection.entities.insert(entity);
                        }
                        _ => {
                            self.document.selection.solids.insert(solid);
                        }
                    }

                    if apply {
                        let material = self.document.current_material.clone();
                        let changed = self.document.apply_material();
                        self.status = format!("{material} on {changed} faces");
                    }
                }
            }
            return;
        }

        if !add {
            self.document.selection.clear();
        }
        let picked = crate::tools::pick_3d(&self.document, origin, direction);
        if let Some(crate::tools::Picked::Mesh(id)) = picked {
            self.document.selection.meshes.insert(id);
            self.document.expand_selection_groups();
            return;
        }
        if let Some(crate::tools::Picked::Solid(id)) = picked {
            // Clicking a brush that belongs to an entity selects the entity:
            // that is the thing a designer thinks of as the door. Same rule
            // the 2D views follow.
            let owner = self
                .document
                .map
                .all_solids()
                .find(|(_, s)| s.id == id)
                .map(|(e, _)| (e.id, e.is_brush_entity() && e.classname() != "worldspawn"));
            match owner {
                Some((entity, true)) => {
                    self.document.selection.entities.insert(entity);
                }
                _ => {
                    self.document.selection.solids.insert(id);
                }
            }
            self.document.expand_selection_groups();
        }
    }

    /// Fly the 3D camera with the keyboard.
    ///
    /// WASD along the view, Q and E straight up and down, Shift to hurry and
    /// Alt to creep. Movement is per second rather than per frame, so it does
    /// not depend on how fast the pane happens to be redrawing.
    ///
    /// Only the pane under the pointer moves, and only while nothing is being
    /// typed into -- otherwise naming an entity `wasd_door` would fly the
    /// camera across the level.
    pub(super) fn fly(&mut self, index: usize, response: &egui::Response, ui: &egui::Ui) {
        if ui.ctx().wants_keyboard_input() {
            return;
        }
        if !(response.hovered() || response.dragged()) {
            return;
        }

        let (forward, side, up, fast, slow, dt) = ui.input(|i| {
            let held = |k: Key| i.key_down(k);
            (
                (held(Key::W) as i32 - held(Key::S) as i32) as f32,
                (held(Key::D) as i32 - held(Key::A) as i32) as f32,
                (held(Key::E) as i32 - held(Key::Q) as i32) as f32,
                i.modifiers.shift,
                i.modifiers.alt,
                // Clamped: a frame that took a second (a compile finishing, a
                // window being dragged) must not teleport the camera.
                i.stable_dt.min(0.1),
            )
        });
        if forward == 0.0 && side == 0.0 && up == 0.0 {
            return;
        }

        let speed = self.fly_speed * if fast { 2.5 } else { 1.0 } * if slow { 0.25 } else { 1.0 };
        let viewport = &mut self.viewports[index];
        viewport.eye += viewport.fly_step(forward, side, up, speed * dt);

        // Held keys produce no events, so without this the view moves one
        // frame and stops until the pointer twitches.
        ui.ctx().request_repaint();
    }

    /// Everything the rasterised 3D pane depends on, hashed.
    ///
    /// When this key is unchanged the pane is redrawn from the cached image
    /// rather than rasterised again. The selection is part of the key because
    /// it changes colours without editing the map -- and faces in particular,
    /// because picking one face tints it while its brush stays unselected.
    pub(super) fn preview_key(&self, index: usize, width: usize, height: usize) -> u64 {
        use std::hash::{Hash, Hasher};

        let viewport = &self.viewports[index];
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.document.revision().hash(&mut hasher);
        let mut selected: Vec<u32> = self.document.selection.solids.iter().copied().collect();
        selected.extend(self.document.selection.entities.iter().copied());
        selected.sort_unstable();
        selected.hash(&mut hasher);
        let mut faces: Vec<(u32, u32)> = self.document.selection.faces.iter().copied().collect();
        faces.sort_unstable();
        faces.hash(&mut hasher);
        for f in [
            viewport.eye.x,
            viewport.eye.y,
            viewport.eye.z,
            viewport.angles.pitch,
            viewport.angles.yaw,
            viewport.angles.roll,
            viewport.fov,
        ] {
            f.to_bits().hash(&mut hasher);
        }
        (width, height).hash(&mut hasher);
        (self.shading as u8).hash(&mut hasher);
        // A texture arriving after a failed load changes the picture.
        self.textures.len().hash(&mut hasher);
        // The placement ghost moves every frame the pointer does, with
        // nothing else above changing -- without this, the cache would show
        // it frozen at wherever it last happened to redraw for some other
        // reason.
        if let Some((hover_index, point)) = self.entity_hover
            && hover_index == index
        {
            self.tool.entity_class.hash(&mut hasher);
            for c in [point.x, point.y, point.z] {
                c.to_bits().hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// Draw a 3D pane, rasterising it again only if it would look different.
    ///
    /// A software rasteriser is cheap but not free, and an editor spends most
    /// of its frames showing exactly what it showed last frame. Hashing what
    /// the image depends on turns a still view into a texture blit.
    pub(super) fn draw_preview(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        index: usize,
        rect: egui::Rect,
    ) {
        // Render at device resolution so the pane is not soft on a high-DPI
        // screen, but cap it: past a point this is work nobody can see.
        const MAX_EDGE: f32 = 1920.0;
        let scale = ui.ctx().pixels_per_point();
        let width = (rect.width() * scale).round().clamp(1.0, MAX_EDGE) as usize;
        let height = (rect.height() * scale).round().clamp(1.0, MAX_EDGE) as usize;

        // Ahead of the borrow below: loading the ghost model needs `&mut
        // self`, and a plain `&self.viewports[index]` held across that call
        // would conflict with it.
        let ghost_point = self
            .entity_hover
            .filter(|(i, _)| *i == index)
            .map(|(_, p)| p);
        let ghost_model = ghost_point.and_then(|_| self.ghost_model());

        let viewport = &self.viewports[index];
        let key = self.preview_key(index, width, height);

        let stale = self.previews[index].as_ref().is_none_or(|p| p.key != key);
        if stale {
            let viewport = viewport.clone();
            let vfs = &self.vfs;
            let cache = &mut self.textures;
            let mut resolve = move |material: &str| cache.get(vfs, material);
            let ghost = match (ghost_point, &ghost_model) {
                (Some(point), Some(model)) => Some(raster::Ghost {
                    model: model.as_ref(),
                    pose: kerosene_math::Pose::new(point, kerosene_math::Angles::ZERO),
                    // Half-solid: enough to read the shape, not so much it is
                    // mistaken for a brush that has already been placed.
                    opacity: 0.5,
                }),
                _ => None,
            };
            let mut settings = raster::Settings {
                shading: self.shading,
                resolve: Some(&mut resolve),
                ghost,
            };
            let image = raster::render_with(
                &self.document,
                viewport.eye,
                viewport.angles.vectors(),
                viewport.fov,
                width,
                height,
                &mut settings,
            );
            let pixels: Vec<egui::Color32> = image
                .pixels
                .iter()
                .map(|p| egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
                .collect();
            let color_image = egui::ColorImage {
                size: [image.width, image.height],
                pixels,
                source_size: egui::vec2(image.width as f32, image.height as f32),
            };
            match self.previews[index].as_mut() {
                Some(preview) => {
                    preview
                        .texture
                        .set(color_image, egui::TextureOptions::LINEAR);
                    preview.key = key;
                }
                None => {
                    let texture = ui.ctx().load_texture(
                        format!("chisel-3d-{index}"),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.previews[index] = Some(Preview { texture, key });
                }
            }
        }

        if let Some(preview) = &self.previews[index] {
            painter.image(
                preview.texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        self.draw_drag_ghost(painter, index, rect);
        self.draw_gizmo(painter, index, rect);
        self.draw_leak_3d(painter, index, rect);
    }

    /// The move/rotate gizmo over the selection, in the 3D pane.
    pub(super) fn draw_gizmo(&self, painter: &egui::Painter, index: usize, rect: egui::Rect) {
        let Some(mode) = self.gizmo_mode else { return };
        if self.tool.kind != ToolKind::Select {
            return;
        }
        let Some(pivot) = self.document.selection_centre() else {
            return;
        };
        let viewport = &self.viewports[index];
        let radius = crate::gizmo::world_radius(viewport, pivot, crate::gizmo::TARGET_PIXELS);

        // While dragging, show where the selection will land -- the same
        // live preview a plain move or resize already gets.
        if let Some(update) = self.gizmo_preview {
            let polygons = match update {
                crate::gizmo::GizmoUpdate::Move(delta) => {
                    draw::ghost_outline(&self.document, delta)
                }
                crate::gizmo::GizmoUpdate::Rotate(pivot, rotation) => {
                    draw::transformed_outline(&self.document, |p| pivot + rotation * (p - pivot))
                }
            };
            draw::stroke_polygons_3d(
                painter,
                viewport,
                rect,
                &polygons,
                egui::Stroke::new(1.5_f32, draw::colors::TOOL_PREVIEW),
            );
        }

        let active = self.gizmo_drag.map(|d| d.axis);
        let to_screen = |world: Vec3| -> Option<egui::Pos2> {
            crate::gizmo::project(viewport, world)
                .map(|(x, y)| egui::pos2(rect.min.x + x, rect.min.y + y))
        };

        for axis in crate::gizmo::GizmoAxis::all() {
            let color = if active == Some(axis) {
                draw::colors::GIZMO_ACTIVE
            } else {
                match axis {
                    crate::gizmo::GizmoAxis::X => draw::colors::GIZMO_X,
                    crate::gizmo::GizmoAxis::Y => draw::colors::GIZMO_Y,
                    crate::gizmo::GizmoAxis::Z => draw::colors::GIZMO_Z,
                }
            };
            let stroke = egui::Stroke::new(2.0_f32, color);
            match mode {
                crate::gizmo::GizmoMode::Move => {
                    let (Some(from), Some(to)) =
                        (to_screen(pivot), to_screen(pivot + axis.vector() * radius))
                    else {
                        continue;
                    };
                    draw::draw_arrow(painter, from, to, stroke, None);
                }
                crate::gizmo::GizmoMode::Rotate => {
                    let (u, v) = axis.plane_basis();
                    let points: Vec<Option<egui::Pos2>> = (0..=crate::gizmo::RING_SEGMENTS)
                        .map(|i| {
                            let theta = (i as f32 / crate::gizmo::RING_SEGMENTS as f32)
                                * std::f32::consts::TAU;
                            let world = pivot + (u * theta.cos() + v * theta.sin()) * radius;
                            to_screen(world)
                        })
                        .collect();
                    for pair in points.windows(2) {
                        if let (Some(a), Some(b)) = (pair[0], pair[1]) {
                            painter.line_segment([a, b], stroke);
                        }
                    }
                }
            }
        }
    }

    /// Move/rotate gizmo interaction: hit-test on press, preview while
    /// dragging, commit as one undo step on release. Returns whether the
    /// gizmo took this press, so a fall-through pick does not also fire.
    pub(super) fn gizmo_input(
        &mut self,
        index: usize,
        rect: egui::Rect,
        response: &egui::Response,
        ui: &egui::Ui,
    ) -> bool {
        let Some(mode) = self.gizmo_mode else {
            return false;
        };
        if self.tool.kind != ToolKind::Select {
            return false;
        }
        let Some(pivot) = self.document.selection_centre() else {
            return false;
        };
        let local = |pos: egui::Pos2| (pos.x - rect.min.x, pos.y - rect.min.y);
        let viewport = self.viewports[index].clone();

        if self.gizmo_drag.is_none()
            && ui.input(|i| i.pointer.primary_pressed())
            && response.is_pointer_button_down_on()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (x, y) = local(pos);
            let radius = crate::gizmo::world_radius(&viewport, pivot, crate::gizmo::TARGET_PIXELS);
            if let Some(axis) = crate::gizmo::hit(mode, pivot, radius, &viewport, x, y) {
                let (origin, direction) = viewport.pick_ray(x, y);
                self.gizmo_drag =
                    crate::gizmo::GizmoDrag::begin(mode, axis, pivot, origin, direction);
            }
        }

        let Some(drag) = self.gizmo_drag else {
            return false;
        };

        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            let (x, y) = local(pos);
            let (origin, direction) = viewport.pick_ray(x, y);
            if let Some(update) = drag.update(origin, direction) {
                self.gizmo_preview = Some(update);
            }
        }

        if !ui.input(|i| i.pointer.primary_down()) {
            if let Some(update) = self.gizmo_preview.take() {
                match update {
                    crate::gizmo::GizmoUpdate::Move(delta) => {
                        if delta.length() > 1e-4 {
                            self.document.move_selection(delta);
                            self.status = format!(
                                "moved {} {} {}",
                                kerosene_math::format_float(delta.x),
                                kerosene_math::format_float(delta.y),
                                kerosene_math::format_float(delta.z)
                            );
                        }
                    }
                    crate::gizmo::GizmoUpdate::Rotate(pivot, rotation) => {
                        if rotation != Quat::IDENTITY {
                            self.document.rotate_selection(pivot, rotation);
                            let (_, angle) = rotation.to_axis_angle();
                            self.status = format!("rotated {:.1} degrees", angle.to_degrees());
                        }
                    }
                }
            }
            self.gizmo_drag = None;
        }

        true
    }

    /// The leak trace, over the 3D image.
    ///
    /// Not depth-tested on purpose: the whole point is to follow it *through*
    /// the wall it escapes by.
    pub(super) fn draw_leak_3d(&self, painter: &egui::Painter, index: usize, rect: egui::Rect) {
        if self.leak.is_empty() {
            return;
        }
        let viewport = &self.viewports[index];
        let basis = viewport.angles.vectors();
        let aspect = rect.width() / rect.height().max(1.0);
        let half_y = (kerosene_render::vertical_fov(viewport.fov, aspect) * 0.5)
            .tan()
            .max(1e-4);
        let half_x = half_y * aspect;

        let camera = draw::to_camera_space(&self.leak.points, viewport.eye, basis);
        let stroke = egui::Stroke::new(2.0_f32, draw::colors::LEAK);
        for pair in camera.windows(2) {
            // Clip the segment to the near plane rather than dropping it: the
            // camera is usually inside the room the leak starts in.
            let (mut a, mut b) = (pair[0], pair[1]);
            if a.z < draw::NEAR && b.z < draw::NEAR {
                continue;
            }
            if a.z < draw::NEAR {
                a = a + (b - a) * ((draw::NEAR - a.z) / (b.z - a.z));
            } else if b.z < draw::NEAR {
                b = b + (a - b) * ((draw::NEAR - b.z) / (a.z - b.z));
            }
            let project = |c: Vec3| {
                egui::pos2(
                    rect.center().x + (c.x / (c.z * half_x)) * rect.width() * 0.5,
                    rect.center().y - (c.y / (c.z * half_y)) * rect.height() * 0.5,
                )
            };
            painter.line_segment([project(a), project(b)], stroke);
        }
    }

    /// Outline where a dragged selection will land, over the 3D image.
    ///
    /// A move shows the selection translated; a resize shows it scaled about
    /// the far side, the same shape the 2D pane that owns the drag previews.
    /// Stroked on top rather than rasterised into the pane, for two reasons:
    /// the cached image does not have to be thrown away on every mouse move,
    /// and a ghost that is hidden by the wall you are dragging something
    /// behind is a ghost that is no use.
    pub(super) fn draw_drag_ghost(&self, painter: &egui::Painter, index: usize, rect: egui::Rect) {
        let Some(drag) = &self.tool.drag else { return };
        if self.tool.kind != ToolKind::Select || !drag.is_dragging {
            return;
        }

        // A grip drag is a resize: show the scaled shape, computed the same
        // way the 2D pane that owns the drag computes it, rather than a move
        // of the whole selection by the pointer delta.
        let polygons = match (drag.grip, drag.from, drag.axes) {
            (Some(grip), Some(from), Some(axes)) => {
                let minimum = self.document.grid.size;
                let Some((anchor, factor)) =
                    crate::tools::resize_factor(from, axes, grip, drag.current, minimum)
                else {
                    return;
                };
                draw::resize_outline(&self.document, anchor, factor)
            }
            _ => draw::ghost_outline(&self.document, drag.delta()),
        };

        draw::stroke_polygons_3d(
            painter,
            &self.viewports[index],
            rect,
            &polygons,
            egui::Stroke::new(1.5_f32, draw::colors::TOOL_PREVIEW),
        );
    }

    pub(super) fn viewport_input(
        &mut self,
        index: usize,
        rect: egui::Rect,
        response: &egui::Response,
        ui: &egui::Ui,
    ) {
        let local = |pos: egui::Pos2| (pos.x - rect.min.x, pos.y - rect.min.y);
        let kind = self.viewports[index].kind;

        // A right-click picks the thing under the pointer and offers the
        // context menu Hammer opens here: object properties, brush type, and
        // so on. (Right-*drag* in the 3D pane still looks around; a click is
        // a click.)
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (x, y) = local(pos);
            self.select_at(index, x, y);
        }
        response.context_menu(|ui| self.context_menu(ui));

        // Scroll zooms a 2D pane and moves the 3D camera forward.
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if kind.is_2d() {
                    if let Some(pos) = response.hover_pos() {
                        let (x, y) = local(pos);
                        self.viewports[index].zoom_at(1.0 + scroll * 0.002, x, y);
                    }
                } else if ui.input(|i| i.modifiers.ctrl) {
                    // Ctrl-wheel sets how fast the camera flies, the way it
                    // does in every 3D application.
                    self.fly_speed = (self.fly_speed * (1.0 + scroll * 0.004)).clamp(16.0, 8192.0);
                    self.status =
                        format!("fly speed {}", kerosene_math::units::speed(self.fly_speed));
                } else {
                    let forward = self.viewports[index].angles.forward();
                    self.viewports[index].eye += forward * scroll * 2.0;
                }
            }
        }

        // Middle-drag pans; right-drag looks around in 3D.
        if response.dragged_by(egui::PointerButton::Middle) {
            let delta = response.drag_delta();
            if kind.is_2d() {
                self.viewports[index].pan(delta.x, delta.y);
            } else {
                let viewport = &mut self.viewports[index];
                let basis = viewport.angles.vectors();
                viewport.eye += basis.right * -delta.x + basis.up * delta.y;
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary) && !kind.is_2d() {
            let delta = response.drag_delta();
            let viewport = &mut self.viewports[index];
            viewport.angles.yaw -= delta.x * 0.25;
            viewport.angles.pitch += delta.y * 0.25;
            viewport.angles = viewport.angles.clamped_view();
        }

        if !kind.is_2d() {
            self.fly(index, response, ui);
        }

        if !kind.is_2d() {
            // The 3D pane otherwise picks but does not drag geometry: a free
            // drag has no unambiguous depth. An axis or a rotation plane
            // removes exactly that ambiguity, which is what makes the gizmo
            // possible here where a plain drag is not -- see `gizmo_input`.
            if self.gizmo_input(index, rect, response, ui) {
                return;
            }
            if self.tool.kind == ToolKind::Entity {
                if response.clicked()
                    && let Some((hover_index, point)) = self.entity_hover
                    && hover_index == index
                {
                    let class = self.tool.entity_class.clone();
                    self.document.create_entity(&class, point);
                    self.status = format!("placed {class}");
                }
                return;
            }
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let (x, y) = local(pos);
                self.pick_in_3d(index, x, y, ui, response.double_clicked());
            }
            return;
        }

        // Press the geometry on the frame the button goes down, not on the
        // frame egui first calls it a drag. egui delays a drag until the
        // pointer has moved its threshold, and by then the point in hand is
        // no longer where the user pressed -- a resize grip hit-tested there
        // is why grabbing a corner sometimes fell through to a move and
        // relocated the brush.
        if ui.input(|i| i.pointer.primary_pressed())
            && response.is_pointer_button_down_on()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (x, y) = local(pos);
            self.tool
                .press(&self.document, &self.viewports[index], x, y);
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (x, y) = local(pos);
            self.tool
                .drag_to(&self.document, &self.viewports[index], x, y);
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) || response.clicked() {
            if self.tool.drag.is_none()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let (x, y) = local(pos);
                self.tool
                    .press(&self.document, &self.viewports[index], x, y);
            }
            let add = ui.input(|i| i.modifiers.shift);
            if let Some(action) = self.tool.release(add) {
                let viewport = self.viewports[index].clone();
                draw::apply_action(&mut self.document, &viewport, action);
            }
        }
    }
}
