// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The modal questions: a name for the map, and whether to throw work away.

use super::*;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

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

    /// Every shortcut, on one sheet, grouped by what it is for.
    pub(super) fn shortcuts_window(&mut self, ctx: &Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = true;
        egui::Window::new("keyboard shortcuts")
            .open(&mut open)
            .default_width(620.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.columns(2, |columns| {
                    for (column, groups) in columns.iter_mut().zip(SHORTCUTS.chunks(3)) {
                        for (title, keys) in groups {
                            column.label(theme::section_title(*title));
                            egui::Grid::new(("shortcuts", *title))
                                .num_columns(2)
                                .spacing([14.0, 2.0])
                                .show(column, |ui| {
                                    for (key, what) in *keys {
                                        ui.label(theme::mono(*key).color(colors::ACCENT));
                                        ui.label(RichText::new(*what).size(12.0));
                                        ui.end_row();
                                    }
                                });
                            column.add_space(8.0);
                        }
                    }
                });
                ui.label(theme::caption(
                    "Keys reach the pane under the pointer. F1 shows this sheet.",
                ));
            });
        if !open {
            self.show_shortcuts = false;
        }
    }
}

/// The sheet the shortcuts window shows. Kept beside it, and checked by a
/// test against the tools' own shortcuts, so the two cannot drift.
pub(super) const SHORTCUTS: [(&str, &[(&str, &str)]); 6] = [
    (
        "tools",
        &[
            ("1", "select"),
            ("2", "block"),
            ("3", "entity"),
            ("4", "texture"),
            ("5", "shape"),
            ("6", "clip (again: which side is kept)"),
        ],
    ),
    (
        "selection",
        &[
            ("click", "select; shift adds or takes away"),
            ("double-click", "object properties"),
            ("ctrl-A", "select all"),
            ("ctrl-J", "go to an object by id or name"),
            ("ctrl-1 .. 9", "go to a camera bookmark (ctrl-alt sets)"),
            ("escape", "clear the selection, or the clip line"),
            ("delete", "delete"),
            ("ctrl-X / ctrl-C / ctrl-V", "cut / copy / paste"),
            ("ctrl-D", "duplicate one grid step over"),
            ("ctrl-G / ctrl-U", "group / ungroup"),
            ("H / ctrl-H / U", "hide / hide the rest / unhide all"),
        ],
    ),
    (
        "brushes",
        &[
            ("[ / ]", "finer / coarser grid"),
            ("ctrl-B", "align to the grid"),
            ("R", "rotate 90 degrees"),
            ("ctrl-L / ctrl-I", "flip horizontally / vertically"),
            ("ctrl-M", "transform..."),
            ("ctrl-shift-C", "carve"),
            ("ctrl-shift-H", "hollow..."),
            ("enter", "cut along the clip line"),
        ],
    ),
    (
        "views",
        &[
            ("W A S D", "fly the 3D view; Q / E down and up"),
            ("right-drag", "look around in 3D"),
            ("middle-drag", "pan"),
            ("wheel", "zoom a flat view; fly forward in 3D"),
            ("ctrl-wheel", "3D camera speed"),
            ("F", "frame the selection, or everything"),
            ("shift-space", "maximise the pane under the pointer"),
        ],
    ),
    (
        "materials and entities",
        &[
            ("M", "asset browser"),
            ("T", "texture tool: cycle what a click does"),
            ("ctrl-click", "texture tool: pick up a face's material"),
            ("alt-enter", "object properties"),
            ("ctrl-shift-E", "entity report"),
        ],
    ),
    (
        "file",
        &[
            ("ctrl-N", "new map"),
            ("ctrl-S", "save"),
            ("ctrl-shift-S", "save as"),
            ("ctrl-Z / ctrl-shift-Z", "undo / redo"),
            ("F9", "compile (fast) and run"),
            ("F1", "this sheet"),
        ],
    ),
];
