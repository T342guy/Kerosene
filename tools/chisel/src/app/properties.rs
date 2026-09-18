// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Property editing: the buffers behind the inspector and the
//! Hammer-style Object Properties popup, and the right-click menu.

use super::*;

impl ChiselApp {
    pub(super) fn commit_properties(&mut self) {
        let Some(edit) = self.properties.as_mut() else {
            return;
        };
        if !edit.dirty {
            return;
        }
        edit.dirty = false;
        let (id, rows, connections) = (edit.entity, edit.rows.clone(), edit.connections.clone());
        self.document.apply("edit properties", |doc| {
            if let Some(entity) = doc.find_entity_mut(id) {
                inspector::apply(entity, &rows);
                entity.connections = connections;
            }
        });
        let revision = self.document.revision();
        if let Some(edit) = self.properties.as_mut() {
            edit.revision = revision;
        }
    }

    // ---- the object properties popup ------------------------------------

    /// Which entity the Object Properties popup should edit, given the
    /// selection: the selected entity, the brush entity the selected brushes
    /// belong to, or `worldspawn` for plain world brushes.
    pub(super) fn properties_target(&self) -> Option<u32> {
        if let Some(&id) = self.document.selection.entities.iter().next() {
            return Some(id);
        }
        if self.document.selection.solids.is_empty() {
            return None;
        }
        if let Some((id, _)) = self.document.selected_brush_class() {
            return Some(id);
        }
        Some(self.document.map.world.id)
    }

    /// Open the popup on the current selection.
    pub(super) fn open_property_window(&mut self) {
        let Some(entity) = self.properties_target() else {
            self.status = "select something to see its object properties".into();
            return;
        };
        let (rows, connections) = {
            let e = self
                .document
                .find_entity(entity)
                .expect("the target entity exists");
            let spec = self.schema.get(e.classname());
            (inspector::rows(spec, e), e.connections.clone())
        };
        self.property_window = Some(PropertyWindow {
            entity,
            rows,
            dirty: false,
            revision: self.document.revision(),
            connections,
            new_key: String::new(),
            new_value: String::new(),
            #[cfg(test)]
            narrowest_value: f32::INFINITY,
        });
    }

    /// Keep the popup's buffer in step with the document: refresh after an
    /// undo or an edit made elsewhere, and close when its entity disappears.
    pub(super) fn sync_property_window(&mut self) {
        let Some(window) = self.property_window.as_ref() else {
            return;
        };
        let id = window.entity;
        let revision = self.document.revision();
        if window.revision == revision || window.dirty {
            return;
        }
        if self.document.find_entity(id).is_none() {
            self.property_window = None;
            return;
        }
        let (rows, connections) = {
            let e = self.document.find_entity(id).expect("checked above");
            let spec = self.schema.get(e.classname());
            (inspector::rows(spec, e), e.connections.clone())
        };
        if let Some(window) = self.property_window.as_mut() {
            window.rows = rows;
            window.connections = connections;
            window.revision = revision;
            window.dirty = false;
        }
    }

    /// Write the popup's buffer back into the map.
    pub(super) fn commit_property_window(&mut self) {
        let Some(window) = self.property_window.as_ref() else {
            return;
        };
        if !window.dirty {
            return;
        }
        let id = window.entity;
        let rows = window.rows.clone();
        let connections = window.connections.clone();
        self.document.apply("edit object properties", |doc| {
            if let Some(entity) = doc.find_entity_mut(id) {
                inspector::apply(entity, &rows);
                entity.connections = connections;
            }
        });
        let revision = self.document.revision();
        if let Some(window) = self.property_window.as_mut() {
            window.dirty = false;
            window.revision = revision;
        }
    }

    /// The popup itself: a grid of key/value rows, Hammer's "Object
    /// Properties" dialog.
    pub(super) fn property_window_ui(&mut self, ctx: &Context) {
        self.sync_property_window();
        if self.property_window.is_none() {
            return;
        }

        let mut open = true;
        let mut commit = false;
        egui::Window::new("Object Properties")
            .open(&mut open)
            .resizable(true)
            .default_width(520.0)
            .default_height(420.0)
            .show(ctx, |ui| {
                let (classname, help) = {
                    let window = self.property_window.as_ref().expect("checked above");
                    let classname = self
                        .document
                        .find_entity(window.entity)
                        .map(|e| e.classname().to_string())
                        .unwrap_or_default();
                    let help = self
                        .schema
                        .get(&classname)
                        .map(|s| s.help.clone())
                        .unwrap_or_default();
                    (classname, help)
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&classname).monospace().strong());
                });
                if !help.is_empty() {
                    ui.label(RichText::new(&help).size(11.0).weak());
                }
                ui.separator();

                // Panels rather than a plain column, because the split has to
                // be decided before the rows are laid out.
                //
                // egui grows a window to fit its contents, and a scroll area
                // told to fill the space it is offered reports back everything
                // it was given. Put the footer after such a scroll area in a
                // plain column and the window's contents measure taller than
                // the window every frame, so it walks off the screen a row at a
                // time. Giving the footer a panel takes its height out of the
                // reckoning first and leaves the body a definite height to
                // fill, which is also what makes the resize handle work: the
                // body follows the window instead of the window following the
                // body.
                egui::TopBottomPanel::bottom("object-properties-footer").show_inside(ui, |ui| {
                    commit |= self.property_window_footer(ui);
                });
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    commit |= self.property_window_body(ui, &classname);
                });
            });

        // Whatever is in the fields goes into the map before the window
        // goes: a value typed and then dismissed with the close button was
        // otherwise lost, with no undo step to bring it back.
        if commit || !open {
            self.commit_property_window();
        }
        if !open {
            self.property_window = None;
        }
    }

    /// The scrolling half of the popup: every key, then the wiring.
    pub(super) fn property_window_body(&mut self, ui: &mut egui::Ui, classname: &str) -> bool {
        let mut commit = false;

        // Worked out before the buffer is borrowed: what a target accepts is a
        // question about the whole map, not about this entity.
        let (outputs, help_for) = class_outputs(self.schema.get(classname));
        let targets = inspector::target_names(&self.document);
        let inputs_for: Vec<Vec<String>> = self
            .property_window
            .as_ref()
            .map(|window| {
                window
                    .connections
                    .iter()
                    .map(|c| inspector::inputs_for_target(&self.schema, &self.document, &c.target))
                    .collect()
            })
            .unwrap_or_default();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let window = self.property_window.as_mut().expect("checked above");
                if property_grid(ui, window, &self.materials, &self.models) {
                    commit = true;
                }

                // The other half of Hammer's dialog. Keyvalues say what an
                // entity is; outputs say what it does, and an editor that only
                // shows the first half means reaching for the docked panel to
                // finish every job the popup started.
                let mut dirty = window.dirty;
                if outputs_editor(
                    ui,
                    "popup",
                    &mut window.connections,
                    &outputs,
                    &help_for,
                    &targets,
                    &inputs_for,
                    &mut dirty,
                ) {
                    commit = true;
                }
                window.dirty = dirty;
            });

        commit
    }

    /// The add-a-key row: any keyvalue, on any brush or entity.
    pub(super) fn property_window_footer(&mut self, ui: &mut egui::Ui) -> bool {
        let mut commit = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("add").size(11.0).weak());
            let window = self.property_window.as_mut().expect("checked above");
            let key = ui.add(
                egui::TextEdit::singleline(&mut window.new_key)
                    .desired_width(150.0)
                    .hint_text("key, e.g. playercollision"),
            );
            let value = ui.add(
                egui::TextEdit::singleline(&mut window.new_value)
                    .desired_width(120.0)
                    .hint_text("value"),
            );
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.small_button("add").clicked() || (enter && (key.has_focus() || value.has_focus()))
            {
                let name = window.new_key.trim().to_string();
                if !name.is_empty() {
                    if let Some(row) = window
                        .rows
                        .iter_mut()
                        .find(|r| r.key.eq_ignore_ascii_case(&name))
                    {
                        row.value = Some(window.new_value.clone());
                    } else {
                        window.rows.push(PropertyRow {
                            key: name.clone(),
                            label: name.clone(),
                            kind: KeyKind::String,
                            help: String::new(),
                            choices: Vec::new(),
                            default: String::new(),
                            value: Some(window.new_value.clone()),
                            described: false,
                        });
                    }
                    window.new_key.clear();
                    window.new_value.clear();
                    window.dirty = true;
                    commit = true;
                }
            }
        });
        commit
    }

    /// Pick whatever is at a pane point and select it, used by right-click.
    pub(super) fn select_at(&mut self, index: usize, x: f32, y: f32) {
        let kind = self.viewports[index].kind;
        if kind.is_2d() {
            let viewport = self.viewports[index].clone();
            let axis = kind.axes().2;
            let depth = self
                .document
                .selection_bounds()
                .map(|b| b.min[axis])
                .unwrap_or(0.0);
            let point = self
                .document
                .grid
                .snap_point(viewport.screen_to_world(x, y, depth));
            draw::apply_action(
                &mut self.document,
                &viewport,
                crate::tools::ToolAction::PickAt(point, false),
            );
        } else {
            let (origin, direction) = self.viewports[index].pick_ray(x, y);
            self.document.selection.clear();
            if let Some(id) = crate::tools::pick_solid_3d(&self.document, origin, direction) {
                let owner = self
                    .document
                    .map
                    .all_solids()
                    .find(|(_, s)| s.id == id)
                    .map(|(e, _)| (e.id, e.is_brush_entity() && e.classname() != "worldspawn"));
                match owner {
                    Some((entity, true)) => {
                        self.document.selection.entities.insert(entity);
                    }
                    _ => {
                        self.document.selection.solids.insert(id);
                    }
                }
            }
        }
    }

    /// The right-click menu in a viewport.
    pub(super) fn context_menu(&mut self, ui: &mut egui::Ui) {
        let has_target = self.properties_target().is_some();
        if ui
            .add_enabled(has_target, egui::Button::new("Object Properties…"))
            .on_hover_text("Every key this object reads, and room to add your own")
            .clicked()
        {
            self.open_property_window();
            ui.close();
        }

        // Brushes are what a designer has under the cursor most of the time,
        // so the same menu offers the types a brush can be.
        let is_brush = !self.document.selection.solids.is_empty()
            || self.document.selected_brush_class().is_some();
        if is_brush {
            ui.separator();
            let classes = self.brush_classes();
            ui.menu_button("tie to entity", |ui| {
                if ui.button("world geometry").clicked() {
                    self.document.set_brush_class(None);
                    ui.close();
                }
                for class in &classes {
                    if ui.button(class).clicked() {
                        self.document.set_brush_class(Some(class));
                        ui.close();
                    }
                }
            });
            if self.document.selected_brush_class().is_some()
                && ui.button("move brushes back to world").clicked()
            {
                let n = self.document.untie_to_world();
                self.status = format!("moved {n} brushes to the world");
                ui.close();
            }
        }

        if !self.document.selection.is_empty() {
            ui.separator();
            if ui.button("delete").clicked() {
                self.document.delete_selection();
                ui.close();
            }
        }
    }

    /// Point the edit buffer at whatever is selected now.
    pub(super) fn sync_properties(&mut self, id: Option<u32>) {
        let revision = self.document.revision();
        match self.properties.as_ref() {
            // Same entity, and nothing has changed underneath a buffer that is
            // not mid-edit: leave it alone. Rebuilding every frame would throw
            // away what is being typed.
            Some(edit) if Some(edit.entity) == id && (edit.dirty || edit.revision == revision) => {
                return;
            }
            None if id.is_none() => return,
            _ => {}
        }
        // Moving on commits what was in flight; an edit is not lost by
        // clicking somewhere else, which is what a person expects.
        self.commit_properties();
        let revision = self.document.revision();
        self.properties = id.and_then(|id| {
            let entity = self.document.find_entity(id)?;
            let spec = self.schema.get(entity.classname());
            Some(PropertyEdit {
                entity: id,
                rows: inspector::rows(spec, entity),
                connections: entity.connections.clone(),
                dirty: false,
                revision,
            })
        });
    }
}
