// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The tool's options, in a strip under the toolbar.
//!
//! Hammer 5 keeps what applies to everything -- the grid, how the views
//! draw -- in one row, and what applies to the tool in hand in a row of its
//! own that changes with the tool. The editor used to mix the two in one
//! toolbar that grew and shrank as tools changed, and put the rest of the
//! tool's settings in an inspector tab you had to go and find. Now the
//! answer to "how do I change what this tool does" is always the same
//! strip, directly above where you are working.

use super::*;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

/// How tall the strip is.
pub(super) const OPTIONS_HEIGHT: f32 = 30.0;

impl ChiselApp {
    pub(super) fn tool_options(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("tool-options")
            .exact_height(OPTIONS_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_HEADER)
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let kind = self.tool.kind;
                    ui.label(theme::icon(kind.glyph()).color(colors::ACCENT));
                    ui.label(
                        RichText::new(capitalise(kind.label()))
                            .strong()
                            .color(colors::TEXT),
                    );
                    options_gap(ui);

                    match kind {
                        ToolKind::Select => self.select_options(ui),
                        ToolKind::Block => self.block_options(ui, ctx),
                        ToolKind::Shape => self.shape_options(ui),
                        ToolKind::Entity => self.entity_options(ui),
                        ToolKind::Texture => self.texture_options(ui),
                        ToolKind::Clip => self.clip_options(ui),
                    }

                    // How to use it, at the far end, for whoever has not.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(theme::caption(kind.describe())).truncate());
                    });
                });
            });
    }

    fn select_options(&mut self, ui: &mut egui::Ui) {
        use crate::gizmo::GizmoMode;
        ui.label(theme::caption("pick"));
        for mode in SelectMode::all() {
            if ui
                .selectable_label(self.select_mode == mode, mode.label())
                .on_hover_text(mode.describe())
                .clicked()
            {
                self.select_mode = mode;
            }
        }
        options_gap(ui);
        match self.select_mode {
            SelectMode::Face => {
                ui.label(theme::caption("extrude"));
                ui.add(
                    egui::DragValue::new(&mut self.extrude_distance)
                        .range(1.0..=4096.0)
                        .speed(self.document.grid.size.max(1.0) * 0.1),
                );
                let n = self.document.selection.faces.len();
                if ui
                    .add_enabled(n > 0, egui::Button::new("extrude"))
                    .on_hover_text("Grow a new brush off each picked face, this far out.")
                    .on_disabled_hover_text("Pick a face first.")
                    .clicked()
                {
                    let distance = self.extrude_distance;
                    self.extrude_faces(distance);
                }
                options_gap(ui);
            }
            SelectMode::Vertex | SelectMode::Edge => {
                ui.label(theme::caption(format!("{} picked", self.elements.len())));
                options_gap(ui);
            }
            SelectMode::Object => {
                if self.document.selected_solid_ids().len() > 1
                    && ui
                        .button("merge")
                        .on_hover_text(
                            "Make the selected brushes one, when together they are convex.",
                        )
                        .clicked()
                {
                    self.merge_brushes();
                }
            }
        }
        ui.label(theme::caption("gizmo"));
        for (mode, glyph, label, tip) in [
            (
                None,
                icons::CURSOR,
                "none",
                "No handles: drag in a 2D pane to move.",
            ),
            (
                Some(GizmoMode::Move),
                icons::ARROWS_OUT_CARDINAL,
                "move",
                "Arrows on the selection in the 3D pane, one per axis.",
            ),
            (
                Some(GizmoMode::Rotate),
                icons::ARROWS_CLOCKWISE,
                "rotate",
                "Rings on the selection in the 3D pane, one per axis.",
            ),
        ] {
            let on = self.gizmo_mode == mode;
            if ui
                .selectable_label(on, format!("{glyph} {label}"))
                .on_hover_text(tip)
                .clicked()
            {
                self.gizmo_mode = mode;
            }
        }
        options_gap(ui);
        let mut select_groups = !self.document.ignore_groups;
        if ui
            .checkbox(&mut select_groups, "whole groups")
            .on_hover_text("A click takes the whole group. Off: one member at a time.")
            .changed()
        {
            self.document.ignore_groups = !select_groups;
        }
    }

    fn block_options(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        ui.label(theme::caption("material"));
        self.material_chip(ui, ctx);
        options_gap(ui);
        ui.label(theme::caption(format!(
            "snaps outward to the {} grid",
            kerosene_math::units::length_short(self.document.grid.size)
        )));
    }

    /// The current material as a swatch and a name; a click opens the
    /// browser.
    pub(super) fn material_chip(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        let material = self.document.current_material.clone();
        let thumbnail = self.thumbnail(ctx, &material);
        let clicked = ui
            .add(egui::ImageButton::new(
                egui::Image::new(&thumbnail).fit_to_exact_size(egui::vec2(20.0, 20.0)),
            ))
            .on_hover_text("New brushes wear this. Click to pick another  (M)")
            .clicked();
        let named = ui
            .add(
                egui::Label::new(theme::mono(&material).color(colors::ACCENT))
                    .sense(egui::Sense::click()),
            )
            .on_hover_text("New brushes wear this. Click to pick another  (M)")
            .clicked();
        if clicked || named {
            self.show_assets = true;
            self.assets_tab = AssetsTab::Materials;
        }
    }

    fn shape_options(&mut self, ui: &mut egui::Ui) {
        use crate::shapes::{MAX_SIDES, MIN_SIDES, Shape};
        for shape in Shape::all() {
            let selected = self.tool.shape == shape;
            if ui
                .selectable_label(selected, shape.label())
                .on_hover_text(shape.help())
                .clicked()
            {
                self.tool.shape = shape;
                self.status = format!("{}: {}", shape.label(), shape.help());
            }
        }
        let shape = self.tool.shape;
        let options = &mut self.tool.shape_options;
        if shape.uses_sides() || shape.uses_arc() || shape.uses_wall() {
            options_gap(ui);
        }
        if shape.uses_sides() {
            let label = if shape == Shape::Stairs {
                "steps"
            } else {
                "sides"
            };
            ui.label(theme::caption(label));
            ui.add(egui::DragValue::new(&mut options.sides).range(MIN_SIDES..=MAX_SIDES))
                .on_hover_text(
                    "More segments read as smoother and cost the compiler more faces. \
                     Eight is round enough for a pillar you walk past.",
                );
        }
        if shape.uses_arc() {
            ui.label(theme::caption("arc"));
            ui.add(
                egui::DragValue::new(&mut options.arc)
                    .range(15.0..=360.0)
                    .suffix(" deg"),
            )
            .on_hover_text("Degrees. 180 is a doorway, 360 a ring.");
        }
        if shape.uses_wall() {
            ui.label(theme::caption("wall"));
            ui.add(egui::DragValue::new(&mut options.wall).range(4.0..=256.0))
                .on_hover_text("How thick the arch is, in kerosene units.");
        }
    }

    fn entity_options(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::caption("class"));
        let current = self.tool.entity_class.clone();
        let kind = crate::icons::Kind::of(&current);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        crate::icons::draw(ui.painter(), rect.center(), 5.5, kind, kind.colour());

        let mut chosen = None;
        let schema = &self.schema;
        let help = |class: &str| {
            schema
                .get(class)
                .map(|s| s.help.clone())
                .unwrap_or_default()
        };
        let classes = self.point_classes();
        let entity_filter = &mut self.entity_filter;
        let response = egui::ComboBox::from_id_salt("entity-class")
            .selected_text(theme::mono(&current))
            .width(200.0)
            .height(420.0)
            .show_ui(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(icons::MAGNIFYING_GLASS).color(colors::TEXT_MUTED));
                    let filter = ui.add(
                        egui::TextEdit::singleline(entity_filter)
                            .desired_width(200.0)
                            .hint_text("search classes"),
                    );
                    filter.request_focus();
                });
                let filter = entity_filter.to_ascii_lowercase();
                // Grouped by family, the way you look for one: "a light",
                // not "something starting with l".
                for family in crate::icons::Kind::all() {
                    let members: Vec<&String> = classes
                        .iter()
                        .filter(|c| crate::icons::Kind::of(c) == family)
                        .filter(|c| filter.is_empty() || c.to_ascii_lowercase().contains(&filter))
                        .collect();
                    if members.is_empty() {
                        continue;
                    }
                    ui.label(theme::section_title(family.label()));
                    for class in members {
                        let item = ui.horizontal(|ui| {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                            crate::icons::draw(
                                ui.painter(),
                                rect.center(),
                                5.0,
                                family,
                                family.colour(),
                            );
                            ui.selectable_label(*class == current, theme::mono(class))
                        });
                        let text = help(class);
                        let item = if text.is_empty() {
                            item.inner
                        } else {
                            item.inner.on_hover_text(text)
                        };
                        if item.clicked() {
                            chosen = Some(class.clone());
                        }
                    }
                }
            });
        let text = help(&current);
        if !text.is_empty() {
            response.response.on_hover_text(text);
        }
        if let Some(class) = chosen {
            self.tool.entity_class = class;
            self.tool.entity_keys.clear();
        }

        // A model picked from the asset browser rides along with the class.
        if let Some((_, model)) = self
            .tool
            .entity_keys
            .iter()
            .find(|(k, _)| k == "model")
            .cloned()
        {
            options_gap(ui);
            ui.label(theme::caption("model"));
            ui.label(theme::mono(&model).color(colors::ACCENT));
            if widgets::icon_button(ui, icons::X, "place the class's own model instead").clicked() {
                self.tool.entity_keys.retain(|(k, _)| k != "model");
            }
        }
    }

    fn texture_options(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::caption("select"));
        for target in TextureTarget::all() {
            let selected = self.tool.texture_target == target;
            if ui
                .selectable_label(selected, target.label())
                .on_hover_text(target.describe())
                .clicked()
            {
                self.tool.texture_target = target;
                self.status = format!("texture tool: {}", target.label());
            }
        }
        options_gap(ui);
        ui.label(theme::caption("click"));
        for mode in TextureMode::all() {
            let selected = self.tool.texture_mode == mode;
            if ui
                .selectable_label(selected, mode.label())
                .on_hover_text(format!(
                    "{}\n\nT cycles these. Shift always just selects.",
                    mode.describe()
                ))
                .clicked()
            {
                self.tool.texture_mode = mode;
                self.status = format!("texture tool: {}", mode.label());
            }
        }
        options_gap(ui);
        ui.label(theme::caption("applies"));
        let ctx = ui.ctx().clone();
        self.material_chip(ui, &ctx);
    }

    fn clip_options(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::caption("keep"));
        for mode in [ClipMode::Both, ClipMode::Front, ClipMode::Back] {
            let selected = self.tool.clip_mode == mode;
            if ui
                .selectable_label(selected, mode.label())
                .on_hover_text("Front is the side the cut's arrow points to. 6 cycles these.")
                .clicked()
            {
                self.tool.clip_mode = mode;
            }
        }
        options_gap(ui);
        let ready = self.tool.clip_line.is_some() && !self.document.selection.is_empty();
        if ui
            .add_enabled(ready, egui::Button::new("cut  (enter)"))
            .on_disabled_hover_text("Drag a line across the selection in a 2D pane first.")
            .clicked()
        {
            self.apply_clip();
        }
    }
}

/// A hairline between two groups of options.
fn options_gap(ui: &mut egui::Ui) {
    ui.add_space(2.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 16.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, colors::BORDER),
    );
    ui.add_space(2.0);
}

pub(super) fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
