// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The tool strip down the left and the toolbar row under the menu.
//!
//! Hammer's arrangement: the tools are a column of icons on the left edge,
//! and the settings that apply to whatever you are doing -- the grid, snap,
//! how the 3D panes draw, the texture tool's modes -- are a row across the
//! top. What used to sit in the same 120-point column as the tools (the
//! entity classes, the shape settings, a keyhole of a material picker) has
//! moved to the inspector's *Tool* and *Materials* tabs, where there is room
//! for it.

use super::*;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

impl ToolKind {
    /// The icon on the tool strip.
    pub(super) fn glyph(self) -> &'static str {
        match self {
            ToolKind::Select => icons::CURSOR,
            ToolKind::Block => icons::SQUARE,
            ToolKind::Entity => icons::LIGHTBULB,
            ToolKind::Texture => icons::PAINT_BUCKET,
            ToolKind::Shape => icons::SHAPES,
            ToolKind::Clip => icons::SCISSORS,
        }
    }

    /// One line on what the tool does, for the tooltip and the Tool tab.
    pub(super) fn describe(self) -> &'static str {
        match self {
            ToolKind::Select => "Click to select; drag to move. Shift adds to the selection.",
            ToolKind::Block => {
                "Drag a box in a 2D pane to make a brush, snapped outward to the grid."
            }
            ToolKind::Entity => "Click in a pane to place the chosen entity class.",
            ToolKind::Texture => {
                "Click a face in the 3D pane to select it; the inspector edits how its material sits."
            }
            ToolKind::Shape => {
                "Drag a box in a 2D pane and fill it with a wedge, cylinder, cone, arch or stairs."
            }
            ToolKind::Clip => {
                "Drag a line across the selection in a 2D pane; Enter cuts along it. \
                 6 again cycles which side is kept."
            }
        }
    }
}

impl ChiselApp {
    /// The column of tool icons on the left edge.
    pub(super) fn tool_strip(&mut self, ctx: &Context) {
        egui::SidePanel::left("tools")
            .exact_width(46.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(5, 6)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for kind in ToolKind::all() {
                    let selected = self.tool.kind == kind;
                    let name = format!("{} tool", capitalise(kind.label()));
                    let response = widgets::tool_button(
                        ui,
                        kind.glyph(),
                        &name,
                        Some(kind.shortcut()),
                        selected,
                    )
                    .on_hover_text(kind.describe());
                    if response.clicked() {
                        self.tool.set_kind(kind);
                        self.status = format!("{}: {}", kind.label(), kind.describe());
                    }
                }
            });
    }

    /// The row of settings under the menu.
    pub(super) fn toolbar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("toolbar")
            .exact_height(32.0)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // Undo and redo, because they are the two things pressed
                    // most and the two most worth seeing the state of.
                    let can_undo = self.document.undo_depth() > 0;
                    let can_redo = self.document.redo_depth() > 0;
                    let undo_tip = match self.document.undo_label() {
                        Some(label) => format!("undo {label}  (ctrl-Z)"),
                        None => "nothing to undo".to_string(),
                    };
                    if ui
                        .add_enabled_ui(can_undo, |ui| {
                            widgets::icon_button(ui, icons::ARROW_COUNTER_CLOCKWISE, &undo_tip)
                        })
                        .inner
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled_ui(can_redo, |ui| {
                            widgets::icon_button(ui, icons::ARROW_CLOCKWISE, "redo  (ctrl-shift-Z)")
                        })
                        .inner
                        .clicked()
                    {
                        self.redo();
                    }

                    toolbar_gap(ui);

                    // The grid.
                    ui.label(theme::icon(icons::GRID_FOUR).color(colors::TEXT_MUTED))
                        .on_hover_text("The grid. [ and ] make it finer and coarser.");
                    if widgets::icon_button(ui, icons::MINUS, "finer grid  ([)").clicked() {
                        self.document.grid.finer();
                    }
                    ui.label(
                        theme::mono(kerosene_math::units::length_short(self.document.grid.size))
                            .color(colors::TEXT),
                    )
                    .on_hover_text(kerosene_math::units::length(self.document.grid.size));
                    if widgets::icon_button(ui, icons::PLUS, "coarser grid  (])").clicked() {
                        self.document.grid.coarser();
                    }
                    widgets::icon_toggle(
                        ui,
                        icons::MAGNET,
                        "snap to grid",
                        &mut self.document.grid.snap,
                    );
                    widgets::icon_toggle(
                        ui,
                        icons::EYE,
                        "show the grid",
                        &mut self.document.grid.visible,
                    );

                    toolbar_gap(ui);

                    // Groups and the cordon: what a click takes hold of, and
                    // how much of the map is in play.
                    let mut select_groups = !self.document.ignore_groups;
                    if widgets::icon_toggle(
                        ui,
                        icons::SELECTION_ALL,
                        "select whole groups  (off: pick one member at a time)",
                        &mut select_groups,
                    )
                    .clicked()
                    {
                        self.document.ignore_groups = !select_groups;
                    }
                    let mut cordon = self.document.cordon_active();
                    if widgets::icon_toggle(
                        ui,
                        icons::BOUNDING_BOX,
                        "cordon: show and compile only what is inside the box",
                        &mut cordon,
                    )
                    .clicked()
                    {
                        self.toggle_cordon();
                    }
                    if self.document.map.cordon.is_some() {
                        widgets::icon_toggle(
                            ui,
                            icons::ARROWS_OUT_CARDINAL,
                            "edit the cordon: drag its grips in a 2D pane",
                            &mut self.document.editing_cordon,
                        );
                    }

                    toolbar_gap(ui);

                    // How the 3D panes draw.
                    ui.label(theme::icon(icons::CUBE).color(colors::TEXT_MUTED))
                        .on_hover_text("How the 3D panes draw.");
                    let before = self.shading;
                    egui::ComboBox::from_id_salt("shading")
                        .selected_text(RichText::new(self.shading.label()).size(12.0))
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for mode in Shading::all() {
                                ui.selectable_value(&mut self.shading, mode, mode.label());
                            }
                        });
                    if self.shading != before {
                        self.status = format!("3D panes: {}", self.shading.label());
                    }

                    // The gizmo, only while the select tool is active: a
                    // brush or a shape tool drag has nothing selected to put
                    // handles on yet.
                    if self.tool.kind == ToolKind::Select {
                        toolbar_gap(ui);
                        use crate::gizmo::GizmoMode;
                        for (mode, glyph, tip) in [
                            (
                                Some(GizmoMode::Move),
                                icons::ARROWS_OUT_CARDINAL,
                                "move gizmo",
                            ),
                            (
                                Some(GizmoMode::Rotate),
                                icons::ARROWS_CLOCKWISE,
                                "rotate gizmo",
                            ),
                        ] {
                            let mut on = self.gizmo_mode == mode;
                            if widgets::icon_toggle(ui, glyph, tip, &mut on).clicked() {
                                self.gizmo_mode = if on { mode } else { None };
                            }
                        }
                    }

                    // The texture tool's two settings, only while it is the tool.
                    if self.tool.kind == ToolKind::Texture {
                        toolbar_gap(ui);
                        ui.label(theme::caption("select"));
                        for target in TextureTarget::all() {
                            let selected = self.tool.texture_target == target;
                            if ui
                                .selectable_label(
                                    selected,
                                    RichText::new(target.label()).size(12.0),
                                )
                                .on_hover_text(target.describe())
                                .clicked()
                            {
                                self.tool.texture_target = target;
                                self.status = format!("texture tool: {}", target.label());
                            }
                        }
                        ui.add_space(6.0);
                        ui.label(theme::caption("apply"));
                        for mode in TextureMode::all() {
                            let selected = self.tool.texture_mode == mode;
                            if ui
                                .selectable_label(selected, RichText::new(mode.label()).size(12.0))
                                .on_hover_text(format!("{}\n\nT cycles these.", mode.describe()))
                                .clicked()
                            {
                                self.tool.texture_mode = mode;
                                self.status = format!("texture tool: {}", mode.label());
                            }
                        }
                    }

                    // The right end: compile, and the pane layout.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let compiling = self.compile.as_ref().is_some_and(|j| !j.finished);
                        let label = if compiling {
                            format!("{}  compiling", icons::CIRCLE_NOTCH)
                        } else {
                            format!("{}  compile", icons::PLAY)
                        };
                        let button = ui
                            .add_enabled(
                                !compiling,
                                egui::Button::new(
                                    RichText::new(label).size(12.0).color(colors::ON_ACCENT),
                                )
                                .fill(colors::ACCENT)
                                .stroke(egui::Stroke::NONE),
                            )
                            .on_hover_text(
                                "Compile (fast) and run.  F9\nmap -> compile... for the settings.",
                            );
                        if button.clicked() {
                            self.compile_now(Quality::Fast);
                        }
                        if widgets::icon_button(ui, icons::GEAR, "compile settings...").clicked() {
                            self.show_compile = true;
                        }

                        toolbar_gap(ui);

                        let maximised = self.maximised.is_some();
                        let glyph = if maximised {
                            icons::CORNERS_IN
                        } else {
                            icons::CORNERS_OUT
                        };
                        let tip = if maximised {
                            "show four panes  (shift-space)"
                        } else {
                            "maximise the active pane  (shift-space)"
                        };
                        if widgets::icon_button(ui, glyph, tip).clicked() {
                            self.toggle_maximised();
                        }
                        if widgets::icon_button(ui, icons::CROSSHAIR, "frame everything").clicked()
                        {
                            self.frame_all();
                        }
                    });
                });
            });
    }

    /// One pane full size, or four again.
    pub(super) fn toggle_maximised(&mut self) {
        self.maximised = match self.maximised {
            Some(_) => None,
            None => Some(self.active),
        };
        self.status = match self.maximised {
            Some(index) => format!("pane {} maximised", index + 1),
            None => "four panes".to_string(),
        };
    }

    /// Undo, the way the menu and the toolbar both do it.
    ///
    /// Pending property edits are committed first, so a half-typed value
    /// becomes its own undo step rather than the *next* one.
    pub(super) fn undo(&mut self) {
        self.commit_properties();
        self.commit_property_window();
        if let Some(label) = self.document.undo() {
            self.status = format!("undid {label}");
        }
    }

    pub(super) fn redo(&mut self) {
        self.commit_properties();
        self.commit_property_window();
        if let Some(label) = self.document.redo() {
            self.status = format!("redid {label}");
        }
    }

    /// What the shape tool will draw, and how many pieces of it.
    ///
    /// Only the settings the chosen shape actually uses are shown. A slider
    /// that does nothing is worse than no slider: it makes you wonder what
    /// you did wrong.
    pub(super) fn shape_panel(&mut self, ui: &mut egui::Ui) {
        use crate::shapes::{MAX_SIDES, MIN_SIDES, Shape};

        widgets::section(ui, "shape", |ui| {
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
            ui.label(theme::caption(self.tool.shape.help()));
        });

        let shape = self.tool.shape;
        let options = &mut self.tool.shape_options;
        if shape.uses_sides() || shape.uses_arc() || shape.uses_wall() {
            widgets::section(ui, "settings", |ui| {
                if shape.uses_sides() {
                    let label = if shape == Shape::Stairs {
                        "steps"
                    } else {
                        "sides"
                    };
                    ui.add(
                        egui::Slider::new(&mut options.sides, MIN_SIDES..=MAX_SIDES).text(label),
                    )
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
            });
        }

        ui.add_space(4.0);
        ui.label(theme::caption(
            "Drag a box in a 2D pane; the pane you draw in decides which way the shape stands.",
        ));
    }

    /// The entity tool's class list, with a search box.
    pub(super) fn entity_panel(&mut self, ui: &mut egui::Ui) {
        widgets::section(ui, "entity class", |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::icon(icons::MAGNIFYING_GLASS).color(colors::TEXT_MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut self.entity_filter)
                        .desired_width(f32::INFINITY)
                        .hint_text("filter classes"),
                );
            });
            ui.add_space(2.0);

            let filter = self.entity_filter.to_ascii_lowercase();
            let classes: Vec<String> = self
                .point_classes()
                .into_iter()
                .filter(|c| filter.is_empty() || c.to_ascii_lowercase().contains(&filter))
                .collect();
            if classes.is_empty() {
                ui.label(theme::caption("nothing matches"));
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for class in classes {
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
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            crate::icons::draw(
                                ui.painter(),
                                rect.center(),
                                6.0,
                                kind,
                                kind.colour(),
                            );
                            ui.selectable_label(selected, theme::mono(&class))
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
        });
    }
}

/// A gap between two groups on the toolbar, with a hairline in it.
fn toolbar_gap(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, colors::BORDER),
    );
    ui.add_space(6.0);
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
