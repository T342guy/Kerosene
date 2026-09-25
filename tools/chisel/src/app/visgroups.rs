// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The VisGroups tab.
//!
//! Two lists. The user's own visgroups, a tree with a checkbox on each row,
//! a colour, a member count and a mark for the ones that are streamed
//! sections; and the automatic ones -- every entity, every tool brush, one
//! per class -- that exist because "hide all the lights" should be one
//! click and not a selection exercise.

use super::*;
use crate::document::AutoGroup;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

/// What a row asked for, acted on after the tree is drawn so the map is not
/// borrowed while it is being changed.
enum Request {
    Show(u32, bool),
    Stream(u32, bool),
    Rename(u32, String),
    Colour(u32, [u8; 3]),
    NewChild(u32),
    Delete(u32),
    Select(u32),
    AddSelection(u32),
    RemoveSelection(u32),
}

impl ChiselApp {
    pub(super) fn visgroups_tab(&mut self, ui: &mut egui::Ui) {
        let mut requests: Vec<Request> = Vec::new();

        ui.horizontal(|ui| {
            ui.label(theme::section_title("visgroups"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_selection = !self.document.selection.is_empty();
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new(theme::icon(icons::PLUS)).small(),
                    )
                    .on_hover_text("New visgroup from the selection  (ctrl-shift-G)")
                    .clicked()
                {
                    self.new_visgroup_from_selection();
                }
                if ui
                    .add(egui::Button::new(theme::icon(icons::FOLDER_PLUS)).small())
                    .on_hover_text("New empty visgroup")
                    .clicked()
                {
                    let id = self.document.add_visgroup("new visgroup", None);
                    self.renaming_visgroup = Some((id, "new visgroup".into(), true));
                }
            });
        });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.document.map.visgroups.is_empty() {
                    ui.label(theme::caption(
                        "None yet. Select some brushes and press ctrl-shift-G to put \
                         them in one; untick it to hide them all.",
                    ));
                }
                let rows: Vec<(u32, usize)> = self
                    .document
                    .map
                    .walk_visgroups()
                    .iter()
                    .map(|(g, depth)| (g.id, *depth))
                    .collect();
                for (id, depth) in rows {
                    self.visgroup_row(ui, id, depth, &mut requests);
                }

                ui.add_space(10.0);
                ui.label(theme::section_title("auto"));
                ui.label(theme::caption(
                    "Made from what the map holds. Untick one to hide everything in it.",
                ));
                let mut autos: Vec<AutoGroup> = AutoGroup::fixed().to_vec();
                autos.extend(
                    self.document
                        .point_classes_present()
                        .into_iter()
                        .map(AutoGroup::Class),
                );
                for group in autos {
                    let mut on = self.document.is_auto_visible(&group);
                    let label = match &group {
                        AutoGroup::Class(c) => format!("    {c}"),
                        other => other.label(),
                    };
                    if ui
                        .checkbox(&mut on, theme::mono(label).size(11.0))
                        .changed()
                    {
                        self.document.set_auto_visible(group, on);
                    }
                }

                ui.add_space(10.0);
                let hidden = self.document.hidden_count();
                if hidden > 0 {
                    ui.horizontal(|ui| {
                        ui.label(theme::warn(format!("{hidden} hidden")).size(11.0));
                        if ui.small_button("unhide all (U)").clicked() {
                            let n = self.document.unhide_all();
                            self.status = format!("unhid {n}");
                        }
                    });
                }
            });

        for request in requests {
            match request {
                Request::Show(id, on) => self.document.set_visgroup_visible(id, on),
                Request::Stream(id, on) => self.document.set_visgroup_stream(id, on),
                Request::Rename(id, name) => self.document.rename_visgroup(id, &name),
                Request::Colour(id, c) => self.document.set_visgroup_color(id, c),
                Request::NewChild(parent) => {
                    let id = self.document.add_visgroup("new visgroup", Some(parent));
                    self.renaming_visgroup = Some((id, "new visgroup".into(), true));
                }
                Request::Delete(id) => {
                    self.document.remove_visgroup(id);
                }
                Request::Select(id) => {
                    let n = self.document.select_visgroup(id);
                    self.status = format!("selected {n}");
                }
                Request::AddSelection(id) => {
                    let n = self.document.add_selection_to_visgroup(id);
                    self.status = format!("added {n}");
                }
                Request::RemoveSelection(id) => {
                    let n = self.document.remove_selection_from_visgroup(id);
                    self.status = format!("removed {n}");
                }
            }
        }
    }

    fn visgroup_row(&mut self, ui: &mut egui::Ui, id: u32, depth: usize, out: &mut Vec<Request>) {
        let Some(group) = self.document.map.visgroup(id) else {
            return;
        };
        let (name, mut visible, mut stream, color) =
            (group.name.clone(), group.visible, group.stream, group.color);
        let members = self.document.map.visgroup_members(id).len();
        let shown = self.document.map.is_visgroup_visible(id);
        let renaming = self
            .renaming_visgroup
            .as_ref()
            .is_some_and(|(r, _, _)| *r == id);

        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0);
            if ui
                .checkbox(&mut visible, "")
                .on_hover_text("Shown. A hidden parent hides its children too.")
                .changed()
            {
                out.push(Request::Show(id, visible));
            }
            // The colour lands in the map when the picker is let go of,
            // not on every frame of a drag: each change is an undo step.
            let mut rgb = self
                .pending_visgroup_color
                .filter(|(g, _)| *g == id)
                .map_or(color, |(_, c)| c);
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                self.pending_visgroup_color = Some((id, rgb));
            }
            if let Some((g, c)) = self.pending_visgroup_color
                && g == id
                && ui.input(|i| i.pointer.any_released())
            {
                out.push(Request::Colour(id, c));
                self.pending_visgroup_color = None;
            }

            if renaming {
                let (_, text, fresh) = self.renaming_visgroup.as_mut().expect("checked");
                let r = ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(140.0)
                        .font(egui::TextStyle::Monospace),
                );
                if *fresh {
                    r.request_focus();
                    *fresh = false;
                }
                let done = r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                if done {
                    let name = text.trim().to_string();
                    if !name.is_empty() {
                        out.push(Request::Rename(id, name));
                    }
                    self.renaming_visgroup = None;
                }
            } else {
                let label = theme::mono(&name).color(if shown {
                    colors::TEXT
                } else {
                    colors::TEXT_MUTED
                });
                let r = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
                if r.double_clicked() {
                    self.renaming_visgroup = Some((id, name.clone(), true));
                } else if r.clicked() {
                    out.push(Request::Select(id));
                }
                r.context_menu(|ui| {
                    if widgets::menu_item(ui, "Select members", None).clicked() {
                        out.push(Request::Select(id));
                        ui.close();
                    }
                    if widgets::menu_item(ui, "Add selection", None).clicked() {
                        out.push(Request::AddSelection(id));
                        ui.close();
                    }
                    if widgets::menu_item(ui, "Remove selection", None).clicked() {
                        out.push(Request::RemoveSelection(id));
                        ui.close();
                    }
                    ui.separator();
                    if widgets::menu_item(ui, "New child", None).clicked() {
                        out.push(Request::NewChild(id));
                        ui.close();
                    }
                    if widgets::menu_item(ui, "Rename", None).clicked() {
                        self.renaming_visgroup = Some((id, name.clone(), true));
                        ui.close();
                    }
                    if widgets::menu_item(ui, "Delete", None)
                        .on_hover_text("The members stay; they just leave the group.")
                        .clicked()
                    {
                        out.push(Request::Delete(id));
                        ui.close();
                    }
                });
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_toggle(
                    ui,
                    icons::STACK,
                    "Streamed section: the engine loads and unloads this visgroup's \
                     geometry around the player.",
                    &mut stream,
                )
                .clicked()
                {
                    out.push(Request::Stream(id, stream));
                }
                ui.label(theme::caption(members.to_string()));
            });
        });
    }

    /// ctrl-shift-G: a visgroup of the selection, named after what is in
    /// it and offered for renaming straight away.
    pub(super) fn new_visgroup_from_selection(&mut self) {
        if self.document.selection.is_empty() {
            self.status = "select something to make a visgroup of".into();
            return;
        }
        let n = self.document.map.visgroups.len() + 1;
        let name = format!("visgroup {n}");
        let id = self.document.new_visgroup_from_selection(&name);
        self.renaming_visgroup = Some((id, name, true));
        self.inspector_tab = InspectorTab::VisGroups;
        self.status = format!("new visgroup of {}", self.document.selection.len());
    }
}

impl ChiselApp {
    pub(super) fn group_selection(&mut self) {
        match self.document.group_selection() {
            Some(_) => self.status = format!("grouped {}", self.document.selection.len()),
            None => self.status = "select two or more things to group".into(),
        }
    }

    pub(super) fn ungroup_selection(&mut self) {
        let n = self.document.ungroup_selection();
        self.status = if n == 0 {
            "nothing selected is in a group".into()
        } else {
            format!("ungrouped {n}")
        };
    }

    pub(super) fn hide_selection(&mut self) {
        let n = self.document.hide_selection();
        self.status = if n == 0 {
            "select something to hide".into()
        } else {
            format!("hid {n}; U shows everything again")
        };
    }

    pub(super) fn hide_unselected(&mut self) {
        if self.document.selection.is_empty() {
            self.status = "select what to keep first".into();
            return;
        }
        let n = self.document.hide_unselected();
        self.status = format!("hid {n} others; U shows everything again");
    }

    pub(super) fn unhide_all(&mut self) {
        let n = self.document.unhide_all();
        self.status = if n == 0 {
            "nothing is hidden".into()
        } else {
            format!("unhid {n}")
        };
    }

    /// Cordon on or off, from the toolbar and the menu.
    pub(super) fn toggle_cordon(&mut self) {
        let on = !self.document.cordon_active();
        self.document.set_cordon_active(on);
        if !on {
            self.document.editing_cordon = false;
        }
        self.status = if on {
            "cordon on: only what is inside is shown and compiled".into()
        } else {
            "cordon off".into()
        };
    }
}
