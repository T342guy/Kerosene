// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The tool strip and the toolbar row.

use super::*;

impl ChiselApp {
    pub(super) fn toolbar(&mut self, ctx: &Context) {
        egui::SidePanel::left("tools")
            .exact_width(120.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("tools").strong());
                for kind in ToolKind::all() {
                    let selected = self.tool.kind == kind;
                    let label = format!("{}  [{}]", kind.label(), kind.shortcut());
                    if ui.selectable_label(selected, label).clicked() {
                        self.tool.set_kind(kind);
                    }
                }

                if self.tool.kind == ToolKind::Texture {
                    ui.separator();
                    ui.label(RichText::new("texture").strong());

                    ui.label(RichText::new("select").size(10.0).weak());
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

                    ui.label(RichText::new("apply").size(10.0).weak());
                    for mode in TextureMode::all() {
                        let selected = self.tool.texture_mode == mode;
                        if ui
                            .selectable_label(selected, mode.label())
                            .on_hover_text(mode.describe())
                            .clicked()
                        {
                            self.tool.texture_mode = mode;
                            self.status = format!("texture tool: {}", mode.label());
                        }
                    }
                }

                ui.separator();
                ui.label(RichText::new("grid").strong());
                ui.horizontal(|ui| {
                    if ui.small_button("[").clicked() {
                        self.document.grid.finer();
                    }
                    ui.label(
                        RichText::new(kerosene_math::units::length_short(self.document.grid.size))
                            .monospace(),
                    )
                    .on_hover_text(kerosene_math::units::length(self.document.grid.size));
                    if ui.small_button("]").clicked() {
                        self.document.grid.coarser();
                    }
                });

                ui.separator();
                self.material_browser(ui);

                if self.tool.kind == ToolKind::Shape {
                    ui.separator();
                    self.shape_panel(ui);
                }

                if self.tool.kind == ToolKind::Entity {
                    ui.separator();
                    ui.label(RichText::new("entity").strong());
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            for class in self.point_classes() {
                                let selected = self.tool.entity_class == class;
                                let help = self
                                    .schema
                                    .get(&class)
                                    .map(|s| s.help.clone())
                                    .unwrap_or_default();
                                let kind = crate::icons::Kind::of(&class);

                                // The same icon the viewport will draw, so the list
                                // and the map read as the same thing.
                                let item = ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(16.0, 16.0),
                                        egui::Sense::hover(),
                                    );
                                    crate::icons::draw(
                                        ui.painter(),
                                        rect.center(),
                                        6.0,
                                        kind,
                                        kind.colour(),
                                    );
                                    ui.selectable_label(selected, &class)
                                });
                                let item = item.inner;
                                let item = if help.is_empty() {
                                    item
                                } else {
                                    item.on_hover_text(help)
                                };
                                if item.clicked() {
                                    self.tool.entity_class = class;
                                }
                            }
                        });
                }
            });
    }

    /// What the shape tool will draw, and how many pieces of it.
    ///
    /// Only the settings the chosen shape actually uses are shown. A slider
    /// that does nothing is worse than no slider: it makes you wonder what
    /// you did wrong.
    pub(super) fn shape_panel(&mut self, ui: &mut egui::Ui) {
        use crate::shapes::{MAX_SIDES, MIN_SIDES, Shape};

        ui.label(RichText::new("shape").strong());
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
        if shape.uses_sides() {
            let label = if shape == Shape::Stairs {
                "steps"
            } else {
                "sides"
            };
            ui.add(egui::Slider::new(&mut options.sides, MIN_SIDES..=MAX_SIDES).text(label))
                .on_hover_text(
                    "More segments read as smoother and cost the compiler more \
                     faces. Eight is round enough for a pillar you walk past.",
                );
        }
        if shape.uses_arc() {
            ui.add(egui::Slider::new(&mut options.arc, 15.0..=360.0).text("arc"))
                .on_hover_text("Degrees. 180 is a doorway, 360 a ring.");
        }
        if shape.uses_wall() {
            ui.add(egui::Slider::new(&mut options.wall, 4.0..=256.0).text("wall"))
                .on_hover_text("How thick the arch is, in kerosene units.");
        }

        ui.label(
            RichText::new("drag a box in a 2D pane; the pane decides which way it stands")
                .size(10.0)
                .weak(),
        );
    }

    // ---- the property inspector -----------------------------------------
}
