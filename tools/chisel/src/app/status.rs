// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The status bar.

use super::*;

impl ChiselApp {
    pub(super) fn status_bar(&mut self, ctx: &Context) {
        use kerosene_math::units;

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&self.status).monospace().size(11.0));

                // Where the map lives, always visible. "Did that save?" is
                // not a question an editor should make anyone guess at, and
                // an unnamed map is worth saying outright rather than showing
                // as `untitled.keromap` as though it were a file.
                ui.separator();
                let (file, colour) = match self.document.path.as_deref() {
                    Some(path) => {
                        let name = files::label(path, &self.content_root);
                        if self.document.is_modified() {
                            (format!("{name} *"), Some(egui::Color32::from_rgb(240, 200, 90)))
                        } else {
                            (name, None)
                        }
                    }
                    None => ("not saved".to_string(), Some(egui::Color32::from_rgb(240, 200, 90))),
                };
                let label = RichText::new(file).monospace().size(11.0);
                let label = match colour { Some(c) => label.color(c), None => label };
                ui.label(label).on_hover_text(match self.document.path.as_deref() {
                    Some(path) => format!(
                        "{}\n\nctrl-S saves. file -> rename... moves it, and takes anything \
                         compiled from it along.",
                        path.display()
                    ),
                    None => "This map has never been saved. ctrl-S will ask for a name."
                        .to_string(),
                });

                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} brushes  {} entities",
                        self.document.map.solid_count(),
                        self.document.map.entities.len(),
                    ))
                    .monospace()
                    .size(11.0),
                );
                ui.separator();
                ui.label(RichText::new(format!("grid {}", units::length_short(self.document.grid.size))).monospace().size(11.0))
                    .on_hover_text(format!(
                        "One grid square is {}.\nDistances in Kerosene are kerosene units: \
                         1 ku is one inch, a player is {} tall and runs at {}.",
                        units::length(self.document.grid.size),
                        units::length(units::PLAYER_HEIGHT),
                        units::speed(units::PLAYER_SPEED),
                    ));
                ui.separator();
                ui.label(RichText::new(self.viewports[self.active].kind.label()).monospace().size(11.0));

                // An editor with no content is nearly useless, and the way it
                // used to fail was silent: three hard-coded class names, flat
                // colours, and no clue why.
                ui.separator();
                let missing = self.textures.problem_count();
                let (text, colour) = if self.schema.is_empty() {
                    ("no entity classes".to_string(), Some(draw::colors::LEAK))
                } else if missing > 0 {
                    (
                        format!("{missing} materials unbuilt"),
                        Some(egui::Color32::from_rgb(240, 200, 90)),
                    )
                } else {
                    (format!("{} classes, {} materials", self.schema.len(), self.materials.len()), None)
                };
                let label = RichText::new(text).monospace().size(11.0);
                let label = match colour {
                    Some(c) => label.color(c),
                    None => label,
                };
                ui.label(label).on_hover_text(if missing > 0 {
                    format!(
                        "{}\n\n{missing} materials have no compiled texture behind them. \
                         Chisel builds the textures on startup and again before every \
                         compile, so these are ones that would not build -- check the log. \
                         view -> reload textures picks up a build done outside.",
                        self.content_note
                    )
                } else {
                    self.content_note.clone()
                });

                if let Some(bounds) = self.document.selection_bounds() {
                    let size = bounds.size();
                    let centre = bounds.center();
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "selection {} x {} x {} ku",
                            kerosene_math::format_float(size.x),
                            kerosene_math::format_float(size.y),
                            kerosene_math::format_float(size.z),
                        ))
                        .monospace()
                        .size(11.0),
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

                if !self.viewports[self.active].kind.is_2d() {
                    ui.separator();
                    ui.label(
                        RichText::new(format!("fly {}", units::length_short(self.fly_speed) + "/s"))
                            .monospace()
                            .size(11.0),
                    )
                    .on_hover_text("WASD to fly, Q and E for down and up, Shift to hurry, Alt to creep. Ctrl-wheel changes the speed.");
                }
            });
        });
    }
}
