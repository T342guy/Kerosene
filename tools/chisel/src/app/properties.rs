// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Property editing: the buffers behind the inspector and the
//! Hammer-style Object Properties popup, and the right-click menu.

use super::*;
use kerosene_ui::theme;

impl ChiselApp {
    pub(super) fn commit_properties(&mut self) {
        if let Some(edit) = self.properties.as_mut() {
            edit.commit(&mut self.document, "edit properties");
        }
        if let Some(edit) = self.brush_properties.as_mut() {
            edit.commit(&mut self.document, "edit brush keys");
        }
    }

    // ---- what gets edited -----------------------------------------------

    /// What the key-value editors edit, given the selection.
    ///
    /// Faces when any are selected; otherwise entities -- the selected ones
    /// and the brush entities the selected brushes belong to; otherwise the
    /// selected brushes themselves; and with nothing selected, the world,
    /// which is where the skybox and the like live.
    pub(super) fn properties_targets(&self) -> Vec<TargetId> {
        let selection = &self.document.selection;
        if !selection.faces.is_empty() {
            let mut faces: Vec<(u32, u32)> = selection.faces.iter().copied().collect();
            faces.sort_unstable();
            return faces
                .into_iter()
                .map(|(solid, side)| TargetId::Face(solid, side))
                .collect();
        }
        let mut entities: Vec<u32> = selection.entities.iter().copied().collect();
        for &solid in &selection.solids {
            if let Some(owner) = self.document.map.owner_of_solid(solid)
                && !entities.contains(&owner.id)
            {
                entities.push(owner.id);
            }
        }
        if !entities.is_empty() {
            entities.sort_unstable();
            return entities.into_iter().map(TargetId::Entity).collect();
        }
        let mut solids: Vec<u32> = selection.solids.iter().copied().collect();
        if !solids.is_empty() {
            solids.sort_unstable();
            return solids.into_iter().map(TargetId::Solid).collect();
        }
        vec![TargetId::Entity(self.document.map.world.id)]
    }

    /// The selected brushes, for their own keys, when the main editor is
    /// on the entity they belong to.
    pub(super) fn brush_targets(&self) -> Vec<TargetId> {
        let mut solids: Vec<u32> = self.document.selection.solids.iter().copied().collect();
        solids.sort_unstable();
        solids.into_iter().map(TargetId::Solid).collect()
    }

    /// A short description of a target list for a heading: `func_door`,
    /// `3 entities`, `2 brushes`, `1 face`.
    pub(super) fn describe_targets(&self, targets: &[TargetId]) -> String {
        let plural = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        match targets {
            [] => "nothing".to_string(),
            [TargetId::Entity(id)] => self
                .document
                .find_entity(*id)
                .map(|e| e.classname().to_string())
                .unwrap_or_default(),
            [TargetId::Solid(_)] => "brush".to_string(),
            [TargetId::Face(..)] => "face".to_string(),
            many => match many[0] {
                TargetId::Entity(_) => plural(many.len(), "entity", "entities"),
                TargetId::Solid(_) => plural(many.len(), "brush", "brushes"),
                TargetId::Face(..) => plural(many.len(), "face", "faces"),
            },
        }
    }

    // ---- the object properties popup ------------------------------------

    /// Open the popup on the current selection.
    pub(super) fn open_property_window(&mut self) {
        let targets = self.properties_targets();
        self.property_window = Some(PropertyWindow {
            edit: PropertyEdit::build(&self.document, &self.schema, targets),
            new_key: String::new(),
            new_value: String::new(),
            #[cfg(test)]
            narrowest_value: f32::INFINITY,
        });
    }

    /// Keep the popup's buffer in step with the document: refresh after an
    /// undo or an edit made elsewhere, and close when its object disappears.
    pub(super) fn sync_property_window(&mut self) {
        let Some(window) = self.property_window.as_ref() else {
            return;
        };
        let revision = self.document.revision();
        if window.edit.revision == revision || window.edit.dirty || window.edit.editor_dirty {
            return;
        }
        if !window.edit.still_valid(&self.document, &self.schema) {
            self.property_window = None;
            return;
        }
        let targets = window.edit.targets.clone();
        let edit = PropertyEdit::build(&self.document, &self.schema, targets);
        if let Some(window) = self.property_window.as_mut() {
            window.edit = edit;
        }
    }

    /// Write the popup's buffer back into the map.
    pub(super) fn commit_property_window(&mut self) {
        if let Some(window) = self.property_window.as_mut() {
            window
                .edit
                .commit(&mut self.document, "edit object properties");
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
                let (title, classname, help) = {
                    let window = self.property_window.as_ref().expect("checked above");
                    let title = self.describe_targets(&window.edit.targets);
                    let classname = window
                        .edit
                        .single_entity()
                        .and_then(|id| self.document.find_entity(id))
                        .map(|e| e.classname().to_string())
                        .unwrap_or_default();
                    let help = self
                        .schema
                        .get(&classname)
                        .map(|s| s.help.clone())
                        .unwrap_or_default();
                    (title, classname, help)
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&title).monospace().strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        raw_toggle(ui, &mut self.raw_keys);
                    });
                });
                if !help.is_empty() {
                    ui.label(theme::caption(&help));
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
                    .edit
                    .connections
                    .iter()
                    .map(|c| inspector::inputs_for_target(&self.schema, &self.document, &c.target))
                    .collect()
            })
            .unwrap_or_default();
        let raw = self.raw_keys;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let window = self.property_window.as_mut().expect("checked above");
                let grid = property_grid(
                    ui,
                    "object-properties",
                    &mut window.edit,
                    raw,
                    &self.materials,
                    &self.models,
                );
                commit |= grid.commit;
                #[cfg(test)]
                {
                    window.narrowest_value = grid.narrowest;
                }

                if window.edit.has_editor_data() {
                    ui.add_space(6.0);
                    commit |= editor_data_rows(ui, "popup", &mut window.edit);
                }

                // The other half of Hammer's dialog. Keyvalues say what an
                // entity is; outputs say what it does, and an editor that only
                // shows the first half means reaching for the docked panel to
                // finish every job the popup started.
                if window.edit.single_entity().is_some() {
                    let mut dirty = window.edit.dirty;
                    if outputs_editor(
                        ui,
                        "popup",
                        &mut window.edit.connections,
                        &outputs,
                        &help_for,
                        &targets,
                        &inputs_for,
                        &mut dirty,
                    ) {
                        commit = true;
                    }
                    window.edit.dirty = dirty;
                }
            });

        commit
    }

    /// The add-a-key row: any keyvalue, on any brush, face or entity.
    pub(super) fn property_window_footer(&mut self, ui: &mut egui::Ui) -> bool {
        let mut commit = false;
        ui.horizontal(|ui| {
            ui.label(theme::caption("add"));
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
                    let value = window.new_value.clone();
                    add_key(&mut window.edit, &name, value);
                    window.new_key.clear();
                    window.new_value.clear();
                    commit = true;
                }
            }
        });
        commit
    }

    /// Point the main edit buffer at whatever is selected now.
    pub(super) fn sync_properties(&mut self, targets: Vec<TargetId>) {
        let revision = self.document.revision();
        if let Some(edit) = self.properties.as_ref() {
            // Same targets, and nothing has changed underneath a buffer that
            // is not mid-edit: leave it alone. Rebuilding every frame would
            // throw away what is being typed.
            if edit.targets == targets
                && (edit.dirty || edit.editor_dirty || edit.revision == revision)
            {
                return;
            }
        }
        // Moving on commits what was in flight; an edit is not lost by
        // clicking somewhere else, which is what a person expects.
        if let Some(edit) = self.properties.as_mut() {
            edit.commit(&mut self.document, "edit properties");
        }
        self.properties = (!targets.is_empty())
            .then(|| PropertyEdit::build(&self.document, &self.schema, targets));
    }

    /// The same, for the brushes of a brush entity.
    pub(super) fn sync_brush_properties(&mut self, targets: Vec<TargetId>) {
        let revision = self.document.revision();
        if let Some(edit) = self.brush_properties.as_ref()
            && edit.targets == targets
            && (edit.dirty || edit.editor_dirty || edit.revision == revision)
        {
            return;
        }
        if let Some(edit) = self.brush_properties.as_mut() {
            edit.commit(&mut self.document, "edit brush keys");
        }
        self.brush_properties = (!targets.is_empty())
            .then(|| PropertyEdit::build(&self.document, &self.schema, targets));
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
                self.document.expand_selection_groups();
            }
        }
    }

    /// The right-click menu in a viewport.
    pub(super) fn context_menu(&mut self, ui: &mut egui::Ui) {
        if ui
            .add(egui::Button::new("Object Properties…"))
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
}
