// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The status bar.
//!
//! One line of facts, each with an icon so the eye can find it without
//! reading: the last thing that happened, the file, the pointer's place in
//! the world, what is selected, the grid, the content. Colour is reserved
//! for the two things that want it -- an unsaved map, and content that will
//! not look right.

use super::*;
use kerosene_ui::theme::{self, colors, icons};

impl ChiselApp {
    pub(super) fn status_bar(&mut self, ctx: &Context) {
        use kerosene_math::units;

        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_HEADER)
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    // Where the map lives, always visible. "Did that save?" is
                    // not a question an editor should make anyone guess at, and
                    // an unnamed map is worth saying outright rather than showing
                    // as `untitled.keromap` as though it were a file.
                    let (file, colour) = match self.document.path.as_deref() {
                        Some(path) => {
                            let name = files::label(path, &self.content_root);
                            if self.document.is_modified() {
                                (format!("{name} *"), colors::WARN)
                            } else {
                                (name, colors::TEXT)
                            }
                        }
                        None => ("not saved".to_string(), colors::WARN),
                    };
                    segment(ui, icons::FILE, &file, colour).on_hover_text(
                        match self.document.path.as_deref() {
                            Some(path) => format!(
                                "{}\n\nctrl-S saves. File -> Rename... moves it, and takes \
                                 anything compiled from it along.",
                                path.display()
                            ),
                            None => "This map has never been saved. ctrl-S will ask for a name."
                                .to_string(),
                        },
                    );

                    divider(ui);
                    segment(
                        ui,
                        icons::CUBE,
                        &format!(
                            "{} brushes  {} entities",
                            self.document.map.solid_count(),
                            self.document.map.entities.len(),
                        ),
                        colors::TEXT_MUTED,
                    );

                    if let Some(bounds) = self.document.selection_bounds() {
                        let size = bounds.size();
                        let centre = bounds.center();
                        divider(ui);
                        segment(
                            ui,
                            icons::SELECTION,
                            &format!(
                                "{} x {} x {} ku",
                                kerosene_math::format_float(size.x),
                                kerosene_math::format_float(size.y),
                                kerosene_math::format_float(size.z),
                            ),
                            colors::ACCENT,
                        )
                        .on_hover_text(format!(
                            "{}\nheight {}\ncentred at {} {} {} ku",
                            units::size(size.x, size.y, size.z),
                            units::in_players(size.z),
                            kerosene_math::format_float(centre.x),
                            kerosene_math::format_float(centre.y),
                            kerosene_math::format_float(centre.z),
                        ));
                    }

                    // The pointer, in the world. The one thing a Hammer user
                    // looks down for.
                    if let Some((_, world)) = self.pointer_world {
                        divider(ui);
                        segment(
                            ui,
                            icons::CROSSHAIR,
                            &format!(
                                "{} {} {}",
                                kerosene_math::format_float(world.x),
                                kerosene_math::format_float(world.y),
                                kerosene_math::format_float(world.z),
                            ),
                            colors::TEXT,
                        )
                        .on_hover_text(
                            "Where the pointer is, in kerosene units. The axis the pane \
                             cannot see is taken from the selection.",
                        );
                    }

                    divider(ui);
                    segment(
                        ui,
                        icons::GRID_FOUR,
                        &units::length_short(self.document.grid.size),
                        colors::TEXT_MUTED,
                    )
                    .on_hover_text(format!(
                        "One grid square is {}.\nDistances in Kerosene are kerosene units: \
                         1 ku is one inch, a player is {} tall and runs at {}.",
                        units::length(self.document.grid.size),
                        units::length(units::PLAYER_HEIGHT),
                        units::speed(units::PLAYER_SPEED),
                    ));

                    // The last thing that happened, in whatever room is left,
                    // and the content on the far right.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;

                        // An editor with no content is nearly useless, and the
                        // way it used to fail was silent: three hard-coded class
                        // names, flat colours, and no clue why.
                        let missing = self.textures.problem_count();
                        let (glyph, text, colour) = if self.schema.is_empty() {
                            (icons::WARNING, "no entity classes".to_string(), colors::ERR)
                        } else if missing > 0 {
                            (
                                icons::WARNING,
                                format!("{missing} materials unbuilt"),
                                colors::WARN,
                            )
                        } else {
                            (
                                icons::FOLDER_OPEN,
                                format!(
                                    "{} classes, {} materials",
                                    self.schema.len(),
                                    self.materials.len()
                                ),
                                colors::TEXT_MUTED,
                            )
                        };
                        segment(ui, glyph, &text, colour).on_hover_text(if missing > 0 {
                            format!(
                                "{}\n\n{missing} materials have no compiled texture behind \
                                 them. Chisel builds the textures on startup and again before \
                                 every compile, so these are ones that would not build -- \
                                 check the log. View -> Reload textures picks up a build done \
                                 outside.",
                                self.content_note
                            )
                        } else {
                            self.content_note.clone()
                        });

                        divider(ui);
                        let compiling = self.compile.as_ref().is_some_and(|j| !j.finished);
                        if compiling {
                            ui.add(egui::Spinner::new().size(11.0).color(colors::ACCENT));
                        }
                        ui.label(
                            theme::mono(&self.status)
                                .color(colors::TEXT)
                                .size(11.0),
                        )
                        .on_hover_text(&self.status);
                    });
                });
            });
    }
}

/// An icon and a short text, as one hoverable thing.
fn segment(ui: &mut egui::Ui, glyph: &str, text: &str, colour: egui::Color32) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(theme::icon(glyph).size(12.0).color(colour.gamma_multiply(0.8)));
        ui.label(theme::mono(text).size(11.0).color(colour));
    })
    .response
}

/// A hairline between segments.
fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 14.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, colors::BORDER),
    );
}
