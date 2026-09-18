// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The menu bar.

use super::*;

impl ChiselApp {
    pub(super) fn menu_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("file", |ui| {
                    if ui.button("new             ctrl-N").clicked() {
                        self.discard_or_ask(Discarding::New);
                        ui.close();
                    }

                    // The maps in this project, by name. A file browser would
                    // be the general answer; this is the one that is right
                    // almost every time, and it is one click.
                    let maps = files::maps_in(&self.content_root);
                    ui.menu_button("open", |ui| {
                        if maps.is_empty() {
                            ui.label(
                                RichText::new("no maps in this project yet")
                                    .size(11.0)
                                    .weak(),
                            );
                        }
                        for map in &maps {
                            let name = files::label(map, &self.content_root);
                            let name = name.strip_prefix("maps/").unwrap_or(&name);
                            if ui.button(name).clicked() {
                                self.discard_or_ask(Discarding::Open(map.clone()));
                                ui.close();
                            }
                        }
                    });

                    ui.separator();
                    if ui.button("save            ctrl-S").clicked() {
                        self.save(None);
                        ui.close();
                    }
                    if ui.button("save as...      ctrl-shift-S").clicked() {
                        self.begin_prompt(PromptKind::SaveAs);
                        ui.close();
                    }
                    if ui.button("rename...").clicked() {
                        self.begin_prompt(PromptKind::Rename);
                        ui.close();
                    }

                    ui.separator();
                    let where_it_is = match self.document.path.as_deref() {
                        Some(path) => files::label(path, &self.content_root),
                        None => "not saved anywhere yet".to_string(),
                    };
                    ui.label(RichText::new(where_it_is).size(11.0).weak());

                    ui.separator();
                    if ui.button("quit").clicked() {
                        self.discard_or_ask(Discarding::Quit);
                        ui.close();
                    }
                });

                ui.menu_button("edit", |ui| {
                    let undo = self.document.undo_label().map(str::to_string);
                    let label = undo.map_or("undo".to_string(), |l| format!("undo {l}"));
                    if ui
                        .add_enabled(self.document.undo_depth() > 0, egui::Button::new(label))
                        .clicked()
                    {
                        // The same steps the keyboard shortcut takes, so a
                        // half-typed property becomes its own undo step here
                        // too rather than the *next* one.
                        self.commit_properties();
                        self.commit_property_window();
                        if let Some(label) = self.document.undo() {
                            self.status = format!("undid {label}");
                        }
                        ui.close();
                    }
                    if ui
                        .add_enabled(self.document.redo_depth() > 0, egui::Button::new("redo"))
                        .clicked()
                    {
                        self.commit_properties();
                        self.commit_property_window();
                        if let Some(label) = self.document.redo() {
                            self.status = format!("redid {label}");
                        }
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("select all      ctrl-A").clicked() {
                        let n = self.document.select_all();
                        self.status = format!("selected {n}");
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.document.selection.is_empty(),
                            egui::Button::new("duplicate       ctrl-D"),
                        )
                        .clicked()
                    {
                        let step = self.document.grid.size;
                        let n = self
                            .document
                            .duplicate_selection(Vec3::new(step, step, 0.0));
                        self.status = format!("duplicated {n}; drag to place");
                        ui.close();
                    }
                    if ui.button("delete").clicked() {
                        let n = self.document.delete_selection();
                        if n > 0 {
                            self.status = format!("deleted {n}")
                        }
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("object properties   alt-enter").clicked() {
                        self.open_property_window();
                        ui.close();
                    }
                });

                ui.menu_button("map", |ui| {
                    if ui.button("compile (fast)  F9").clicked() {
                        self.compile_now(Quality::Fast);
                        ui.close();
                    }
                    if ui.button("compile (full)").clicked() {
                        self.compile_now(Quality::Full);
                        ui.close();
                    }
                    ui.separator();
                    if !self.leak.is_empty() && ui.button("clear the leak trace").clicked() {
                        self.leak = crate::leak::LeakTrace::default();
                        self.status = "leak trace cleared".into();
                        ui.close();
                    }
                    if ui.button("check for problems").clicked() {
                        let problems = self.document.problems();
                        self.status = if problems.is_empty() {
                            "no problems found".into()
                        } else {
                            format!("{} problems: {}", problems.len(), problems[0])
                        };
                        ui.close();
                    }
                    if ui.button("check tools are installed").clicked() {
                        self.show_tools_check = true;
                        ui.close();
                    }
                });

                ui.menu_button("view", |ui| {
                    ui.checkbox(&mut self.document.grid.visible, "show grid");
                    ui.checkbox(&mut self.document.grid.snap, "snap to grid");

                    ui.separator();
                    ui.label(RichText::new("3D panes").size(11.0).weak());
                    for mode in Shading::all() {
                        if ui
                            .radio_value(&mut self.shading, mode, mode.label())
                            .clicked()
                        {
                            self.status = format!("3D panes: {}", mode.label());
                        }
                    }
                    ui.separator();
                    if ui.button("browse materials...  M").clicked() {
                        self.browsing = Some(Browsing::Material);
                        ui.close();
                    }
                    if ui.button("browse models...").clicked() {
                        self.browsing = Some(Browsing::Model {
                            row: None,
                            current: String::new(),
                        });
                        ui.close();
                    }
                    if ui.button("reload textures").clicked() {
                        self.textures.clear();
                        self.thumbnails.clear();
                        self.materials = scan_materials(&self.content_root);
                        self.status = "textures reloaded".into();
                        ui.close();
                    }

                    ui.separator();
                    if ui.button("frame everything").clicked() {
                        self.frame_all();
                        ui.close();
                    }
                    if self.maximised.is_some() && ui.button("show four panes").clicked() {
                        self.maximised = None;
                        ui.close();
                    }
                });

                ui.separator();
                ui.label(RichText::new(self.document.title()).monospace());
            });
        });
    }
}
