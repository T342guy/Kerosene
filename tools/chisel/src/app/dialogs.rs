// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The modal questions: a name for the map, and whether to throw work away.

use super::*;

impl ChiselApp {
    /// The name field, and the "this will lose work" question.
    pub(super) fn file_windows(&mut self, ctx: &Context) {
        if let Some(prompt) = &mut self.prompt {
            let (kind, mut cancel, mut confirm) = (prompt.kind, false, false);
            let fresh = std::mem::take(&mut prompt.fresh);
            let mut name = std::mem::take(&mut prompt.name);
            let error = prompt.error.clone();

            // A modal rather than a floating window: it dims what is behind
            // it and swallows the clicks, so nobody draws half a brush into a
            // map that is mid-way through being renamed.
            let modal = egui::Modal::new(egui::Id::new("chisel-name-prompt")).show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading(kind.title());
                ui.add_space(2.0);
                ui.label(RichText::new("name").size(11.0).weak());
                let mut output = egui::TextEdit::singleline(&mut name)
                    .desired_width(f32::INFINITY)
                    .hint_text("arena")
                    .show(ui);
                let field = &output.response;
                if fresh {
                    field.request_focus();
                    // And with the whole name selected, so typing
                    // replaces it. The field is filled in with the
                    // current name because that is usually what is being
                    // changed -- which makes "delete it first" the most
                    // common thing the dialog asks of anyone.
                    let all = egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(name.chars().count()),
                    );
                    output.state.cursor.set_char_range(Some(all));
                    output.state.clone().store(ui.ctx(), output.response.id);
                }
                if output.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    confirm = true;
                }

                // What the name is about to mean, before it means it.
                match files::resolve(&name, &self.content_root) {
                    Ok(path) => {
                        let label = files::label(&path, &self.content_root);
                        let exists = path.exists();
                        let note = if exists && kind == PromptKind::SaveAs {
                            RichText::new(format!("{label}  -- overwrites the map already there"))
                                .size(11.0)
                                .color(egui::Color32::from_rgb(220, 170, 90))
                        } else {
                            RichText::new(label).size(11.0).weak()
                        };
                        ui.label(note);
                    }
                    Err(e) => {
                        ui.label(
                            RichText::new(e)
                                .size(11.0)
                                .color(egui::Color32::from_rgb(220, 110, 110)),
                        );
                    }
                }
                if let Some(error) = &error {
                    ui.label(RichText::new(error).color(egui::Color32::from_rgb(220, 110, 110)));
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(kind.verb()).clicked() {
                        confirm = true
                    }
                    if ui.button("cancel").clicked() {
                        cancel = true
                    }
                });
            });

            if let Some(prompt) = &mut self.prompt {
                prompt.name = name;
            }
            // Escape, or a click on the dimmed background.
            if modal.should_close() {
                cancel = true
            }
            if cancel {
                self.prompt = None;
            } else if confirm {
                self.confirm_prompt();
            }
        }

        if let Some(what) = self.discarding.clone() {
            let mut decided = None;
            let modal = egui::Modal::new(egui::Id::new("chisel-unsaved")).show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.heading("unsaved changes");
                ui.add_space(2.0);
                ui.label(format!(
                    "{} has changes that have not been saved.",
                    self.document.title()
                ));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("save first").clicked() {
                        decided = Some(Decision::Save)
                    }
                    if ui.button("discard them").clicked() {
                        decided = Some(Decision::Discard)
                    }
                    if ui.button("cancel").clicked() {
                        decided = Some(Decision::Cancel)
                    }
                });
            });
            // Escape and a click outside both mean "no", which is the answer
            // that keeps the work.
            if modal.should_close() {
                decided = Some(Decision::Cancel)
            }

            match decided {
                Some(Decision::Save) => {
                    self.discarding = None;
                    // Only on a save that happened. With no path the save
                    // turned into a name prompt, and a save that failed -- a
                    // read-only file, a full disk -- put its reason in the
                    // status bar; going ahead in either case would throw
                    // away the very work the question was about.
                    if self.save(None) {
                        self.discard_now(what)
                    }
                }
                Some(Decision::Discard) => {
                    self.discarding = None;
                    self.discard_now(what);
                }
                Some(Decision::Cancel) => self.discarding = None,
                None => {}
            }
        }
    }
}
