// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The modal questions: a name for the map, and whether to throw work away.

use super::*;
use kerosene_ui::theme::{self, colors, icons};
use kerosene_ui::widgets;

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
            let modal = widgets::dialog(
                ctx,
                "chisel-name-prompt",
                kind.title(),
                420.0,
                |ui| {
                    ui.label(theme::caption("name"));
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
                    let entered =
                        output.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));

                    // What the name is about to mean, before it means it.
                    match files::resolve(&name, &self.content_root) {
                        Ok(path) => {
                            let label = files::label(&path, &self.content_root);
                            let exists = path.exists();
                            let note = if exists && kind == PromptKind::SaveAs {
                                theme::warn(format!("{label}  -- overwrites the map already there"))
                            } else {
                                theme::caption(label)
                            };
                            ui.label(note);
                        }
                        Err(e) => {
                            ui.label(theme::err(e));
                        }
                    }
                    if let Some(error) = &error {
                        ui.label(theme::err(error));
                    }
                    entered
                },
                |ui| {
                    if widgets::primary_button(
                        ui,
                        RichText::new(kind.verb()).color(colors::ON_ACCENT),
                    )
                    .clicked()
                    {
                        confirm = true
                    }
                    if ui.button("cancel").clicked() {
                        cancel = true
                    }
                },
            );

            // Enter in the field is the same as the button.
            confirm |= modal.inner.0;
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
            let modal = widgets::dialog(
                ctx,
                "chisel-unsaved",
                "Unsaved changes",
                360.0,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::icon(icons::WARNING).size(20.0).color(colors::WARN));
                        ui.label(format!(
                            "{} has changes that have not been saved.",
                            self.document.title()
                        ));
                    });
                },
                |ui| {
                    if ui.button("cancel").clicked() {
                        decided = Some(Decision::Cancel)
                    }
                    if ui.button("discard them").clicked() {
                        decided = Some(Decision::Discard)
                    }
                    if widgets::primary_button(
                        ui,
                        RichText::new("save first").color(colors::ON_ACCENT),
                    )
                    .clicked()
                    {
                        decided = Some(Decision::Save)
                    }
                },
            );
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
