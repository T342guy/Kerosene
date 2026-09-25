// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The menu bar.
//!
//! Every shortcut is written beside its item, right-aligned, so the menu is
//! the reference card: open it once and the keys are there to be read.

use super::*;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets::{menu_item, menu_item_enabled};

impl ChiselApp {
    pub(super) fn menu_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu")
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_HEADER)
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| self.file_menu(ui));
                    ui.menu_button("Edit", |ui| self.edit_menu(ui));
                    ui.menu_button("Map", |ui| self.map_menu(ui));
                    ui.menu_button("Tools", |ui| self.tools_menu(ui));
                    ui.menu_button("View", |ui| self.view_menu(ui));

                    // The map's name, at the far end, with a mark when it has
                    // unsaved changes.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let modified = self.document.is_modified();
                        let title = theme::mono(self.document.title()).color(if modified {
                            colors::WARN
                        } else {
                            colors::TEXT_MUTED
                        });
                        ui.label(title)
                            .on_hover_text(match self.document.path.as_deref() {
                                Some(path) => path.display().to_string(),
                                None => "not saved anywhere yet".to_string(),
                            });
                    });
                });
            });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        if menu_item(ui, "New", Some("ctrl-N")).clicked() {
            self.discard_or_ask(Discarding::New);
            ui.close();
        }

        // The maps in this project, by name. A file browser would
        // be the general answer; this is the one that is right
        // almost every time, and it is one click.
        let maps = files::maps_in(&self.content_root);
        ui.menu_button("Open", |ui| {
            if maps.is_empty() {
                ui.label(theme::caption("no maps in this project yet"));
            }
            for map in &maps {
                let name = files::label(map, &self.content_root);
                let name = name.strip_prefix("maps/").unwrap_or(&name);
                if ui.button(theme::mono(name)).clicked() {
                    self.discard_or_ask(Discarding::Open(map.clone()));
                    ui.close();
                }
            }
        });

        ui.separator();
        if menu_item(ui, "Save", Some("ctrl-S")).clicked() {
            self.save(None);
            ui.close();
        }
        if menu_item(ui, "Save as...", Some("ctrl-shift-S")).clicked() {
            self.begin_prompt(PromptKind::SaveAs);
            ui.close();
        }
        if menu_item(ui, "Rename...", None)
            .on_hover_text("Moves the map and everything compiled from it.")
            .clicked()
        {
            self.begin_prompt(PromptKind::Rename);
            ui.close();
        }

        ui.separator();
        let where_it_is = match self.document.path.as_deref() {
            Some(path) => files::label(path, &self.content_root),
            None => "not saved anywhere yet".to_string(),
        };
        ui.label(theme::caption(where_it_is));

        ui.separator();
        if menu_item(ui, "Quit", None).clicked() {
            self.discard_or_ask(Discarding::Quit);
            ui.close();
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        let undo = self.document.undo_label().map(str::to_string);
        let label = undo.map_or("Undo".to_string(), |l| format!("Undo {l}"));
        if menu_item_enabled(ui, self.document.undo_depth() > 0, &label, Some("ctrl-Z")).clicked() {
            self.undo();
            ui.close();
        }
        if menu_item_enabled(
            ui,
            self.document.redo_depth() > 0,
            "Redo",
            Some("ctrl-shift-Z"),
        )
        .clicked()
        {
            self.redo();
            ui.close();
        }
        ui.separator();
        if menu_item(ui, "Select all", Some("ctrl-A")).clicked() {
            let n = self.document.select_all();
            self.status = format!("selected {n}");
            ui.close();
        }
        let has_selection = !self.document.selection.is_empty();
        if menu_item_enabled(ui, has_selection, "Duplicate", Some("ctrl-D")).clicked() {
            let step = self.document.grid.size;
            let n = self
                .document
                .duplicate_selection(Vec3::new(step, step, 0.0));
            self.status = format!("duplicated {n}; drag to place");
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Delete", Some("del")).clicked() {
            let n = self.document.delete_selection();
            if n > 0 {
                self.status = format!("deleted {n}")
            }
            ui.close();
        }
        ui.separator();
        if menu_item(ui, "Object properties...", Some("alt-enter")).clicked() {
            self.open_property_window();
            ui.close();
        }
        if menu_item(ui, "Entity report...", Some("ctrl-shift-E"))
            .on_hover_text("Every entity, filterable, with the wiring that points at nothing.")
            .clicked()
        {
            self.report.open = true;
            ui.close();
        }
        if menu_item(ui, "History...", None)
            .on_hover_text("The undo stack, with names; click a step to go back to it.")
            .clicked()
        {
            self.show_history = true;
            ui.close();
        }
    }

    fn map_menu(&mut self, ui: &mut egui::Ui) {
        if menu_item(ui, "Compile (fast) and run", Some("F9")).clicked() {
            self.compile_now(Quality::Fast);
            ui.close();
        }
        if menu_item(ui, "Compile (full) and run", None).clicked() {
            self.compile_now(Quality::Full);
            ui.close();
        }
        if menu_item(ui, "Compile settings...", None).clicked() {
            self.show_compile = true;
            ui.close();
        }
        ui.separator();
        if !self.leak.is_empty() && menu_item(ui, "Clear the leak trace", None).clicked() {
            self.leak = crate::leak::LeakTrace::default();
            self.status = "leak trace cleared".into();
            ui.close();
        }
        if menu_item(ui, "Check for problems", None).clicked() {
            let problems = self.document.problems();
            self.status = if problems.is_empty() {
                "no problems found".into()
            } else {
                format!("{} problems: {}", problems.len(), problems[0])
            };
            ui.close();
        }
        if menu_item(ui, "Check tools are installed", None).clicked() {
            self.show_tools_check = true;
            ui.close();
        }
    }

    fn tools_menu(&mut self, ui: &mut egui::Ui) {
        let has_selection = !self.document.selection.is_empty();
        if menu_item_enabled(ui, has_selection, "Group", Some("ctrl-G")).clicked() {
            self.group_selection();
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Ungroup", Some("ctrl-U")).clicked() {
            self.ungroup_selection();
            ui.close();
        }
        let mut select_groups = !self.document.ignore_groups;
        if ui
            .checkbox(&mut select_groups, "Select whole groups")
            .changed()
        {
            self.document.ignore_groups = !select_groups;
        }
        ui.separator();
        if menu_item_enabled(ui, has_selection, "Hide selection", Some("H")).clicked() {
            self.hide_selection();
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Hide everything else", Some("ctrl-H")).clicked() {
            self.hide_unselected();
            ui.close();
        }
        if menu_item(ui, "Unhide all", Some("U")).clicked() {
            self.unhide_all();
            ui.close();
        }
        if menu_item_enabled(
            ui,
            has_selection,
            "New visgroup from selection",
            Some("ctrl-shift-G"),
        )
        .clicked()
        {
            self.new_visgroup_from_selection();
            ui.close();
        }
        ui.separator();
        if menu_item_enabled(ui, has_selection, "Clip tool", Some("6"))
            .on_hover_text("Drag a line in a 2D pane, then Enter. 6 again cycles what is kept.")
            .clicked()
        {
            self.tool.set_kind(ToolKind::Clip);
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Carve", Some("ctrl-shift-C"))
            .on_hover_text("Take the selected brushes out of every world brush they overlap.")
            .clicked()
        {
            self.carve();
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Hollow...", Some("ctrl-shift-H")).clicked() {
            self.show_hollow = true;
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Convert to mesh", None)
            .on_hover_text(
                "Turn the selected world brushes into polygon meshes: detail geometry that is \
                 drawn and collided with, but no longer seals the map or blocks visibility.",
            )
            .clicked()
        {
            self.convert_to_mesh();
            ui.close();
        }
        ui.separator();
        if menu_item_enabled(ui, has_selection, "Transform...", Some("ctrl-M")).clicked() {
            self.show_transform = true;
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Rotate 90 degrees", Some("R"))
            .on_hover_text("About the axis the active pane looks along.")
            .clicked()
        {
            self.rotate_90();
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Flip horizontally", Some("ctrl-L")).clicked() {
            self.flip(true);
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Flip vertically", Some("ctrl-I")).clicked() {
            self.flip(false);
            ui.close();
        }
        if menu_item_enabled(ui, has_selection, "Align to grid", Some("ctrl-B")).clicked() {
            self.align_to_grid();
            ui.close();
        }
        ui.separator();
        let cordon = if self.document.cordon_active() {
            "Cordon off"
        } else {
            "Cordon on"
        };
        if menu_item(ui, cordon, None)
            .on_hover_text("Only what is inside the box is shown and compiled.")
            .clicked()
        {
            self.toggle_cordon();
            ui.close();
        }
        if self.document.map.cordon.is_some() {
            ui.checkbox(&mut self.document.editing_cordon, "Edit cordon bounds");
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.document.grid.visible, "Show grid");
        ui.checkbox(&mut self.document.grid.snap, "Snap to grid");

        ui.separator();
        ui.label(theme::caption("3D panes"));
        for mode in Shading::all() {
            if ui
                .radio_value(&mut self.shading, mode, mode.label())
                .clicked()
            {
                self.status = format!("3D panes: {}", mode.label());
            }
        }
        ui.separator();
        if menu_item(ui, "Browse materials...", Some("M")).clicked() {
            self.browsing = Some(Browsing::Material);
            ui.close();
        }
        if menu_item(ui, "Browse models...", None).clicked() {
            self.browsing = Some(Browsing::Model {
                row: None,
                current: String::new(),
            });
            ui.close();
        }
        if menu_item(ui, "Reload textures", None).clicked() {
            self.reload_textures();
            ui.close();
        }

        ui.separator();
        if menu_item(ui, "Frame everything", None).clicked() {
            self.frame_all();
            ui.close();
        }
        let label = if self.maximised.is_some() {
            "Show four panes"
        } else {
            "Maximise the active pane"
        };
        if menu_item(ui, label, Some("shift-space")).clicked() {
            self.toggle_maximised();
            ui.close();
        }
        ui.separator();
        ui.label(theme::caption(format!(
            "{}  keys reach the pane under the pointer",
            icons::INFO
        )));
    }
}
