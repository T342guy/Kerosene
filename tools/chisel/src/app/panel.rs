// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The inspector panel: faces, brushes and entities.

use super::*;

impl ChiselApp {
    pub(super) fn inspector(&mut self, ctx: &Context) {
        let selected: Vec<u32> = self.document.selection.entities.iter().copied().collect();
        self.sync_properties(selected.first().copied());

        egui::SidePanel::right("inspector")
            .exact_width(320.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);

                // A face selection is what you are looking at when you have one,
                // so it comes first.
                if self.document.selected_face_count() > 0 {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.face_panel(ui);
                        });
                    return;
                }

                // Brushes and brush entities are the same panel. What a brush
                // *is* is a setting on it, not a separate ceremony called "tie to
                // entity" that you have to go through before its settings exist.
                let brush_entity = selected
                    .first()
                    .and_then(|id| self.document.find_entity(*id))
                    .is_some_and(|e| e.is_brush_entity());
                if brush_entity || !self.document.selection.solids.is_empty() {
                    self.brush_panel(ui);
                    return;
                }

                let Some(&id) = selected.first() else {
                    self.brush_panel(ui);
                    return;
                };

                let Some(entity) = self.document.find_entity(id) else {
                    return;
                };
                let classname = entity.classname().to_string();
                let is_brush_entity = entity.is_brush_entity();
                let spec = self.schema.get(&classname).cloned();

                // The thing a Source mapper looks for by name: one panel that
                // shows and edits every key the object carries or the game reads
                // for its class, with a widget suited to each key's type.
                ui.label(RichText::new("object properties").weak().size(11.0));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&classname).monospace().strong());
                    if spec.is_none() {
                        ui.label(
                            RichText::new("(no definition)")
                                .color(egui::Color32::from_rgb(220, 160, 90)),
                        )
                        .on_hover_text(
                            "No class definition describes this class, so only the keys it \
                             already carries can be shown.",
                        );
                    }
                });
                if let Some(help) = spec
                    .as_ref()
                    .map(|s| s.help.as_str())
                    .filter(|h| !h.is_empty())
                {
                    ui.label(RichText::new(help).size(11.0).weak());
                }
                if selected.len() > 1 {
                    ui.label(
                        RichText::new(format!(
                            "{} entities selected -- editing the first",
                            selected.len()
                        ))
                        .size(11.0)
                        .weak(),
                    );
                }
                // What it will do once the map is running, next to the keys that
                // decide it -- and drawn in the 2D panes in the same colour.
                if let Some(motion) = crate::motion::of_selection(&self.document) {
                    ui.label(
                        RichText::new(motion.label)
                            .size(11.0)
                            .color(draw::colors::MOTION),
                    )
                    .on_hover_text(
                        "Drawn in the 2D panes: the arrow is the travel, the outline is \
                     where it ends up.",
                    );
                }
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.property_rows(ui);
                        ui.add_space(6.0);
                        self.outputs_section(ui, id, spec.as_ref());

                        if is_brush_entity {
                            ui.add_space(6.0);
                            ui.separator();
                            if ui.button("move brushes back to world").clicked() {
                                let n = self.document.untie_to_world();
                                self.status = format!("moved {n} brushes to the world");
                            }
                        }
                    });
            });
    }

    /// The inspector for brushes, whatever they are.
    ///
    /// One panel, because a brush's type *is* one of its settings. It used to
    /// be two: a page headed "properties" that offered only a list of classes
    /// to "tie to", and then, once you had tied, a different page with the
    /// settings on it. That is a step and a mode change to reach something
    /// that was never anywhere else, and it left `func_detail` -- which has no
    /// settings at all, being a wall -- looking exactly as configurable as a
    /// door.
    pub(super) fn brush_panel(&mut self, ui: &mut egui::Ui) {
        use crate::brush::BrushInfo;

        ui.label(RichText::new("object properties").strong());
        let Some(info) = BrushInfo::of_selection(&self.document) else {
            ui.label(RichText::new("nothing selected").weak());
            return;
        };
        let current = self.document.selected_brush_class();
        let spec = current
            .as_ref()
            .and_then(|(_, c)| self.schema.get(c).cloned());

        // The entity being edited, so the shared property and output widgets
        // point at the right thing whether a brush or its entity was clicked.
        let entity_id = current.as_ref().map(|(id, _)| *id);
        self.sync_properties(entity_id);

        let mut change: Option<Option<String>> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let size = info.bounds.size();
                ui.label(format!(
                    "{} {}, {} faces  {} x {} x {}",
                    info.brushes,
                    if info.brushes == 1 {
                        "brush"
                    } else {
                        "brushes"
                    },
                    info.faces,
                    kerosene_math::units::length_short(size.x),
                    kerosene_math::units::length_short(size.y),
                    kerosene_math::units::length_short(size.z),
                ));

                // The type, first, because it decides everything below it.
                ui.add_space(4.0);
                ui.label(RichText::new("type").size(11.0).weak());
                let label = current
                    .as_ref()
                    .map_or("world geometry", |(_, c)| c.as_str());
                egui::ComboBox::from_id_salt("brush-type")
                    .selected_text(label)
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(current.is_none(), "world geometry")
                            .clicked()
                        {
                            change = Some(None);
                        }
                        for class in self.brush_classes() {
                            let selected = current.as_ref().is_some_and(|(_, c)| *c == class);
                            let help = self
                                .schema
                                .get(&class)
                                .map(|s| s.help.clone())
                                .unwrap_or_default();
                            let item = ui.selectable_label(selected, &class);
                            let item = if help.is_empty() {
                                item
                            } else {
                                item.on_hover_text(help)
                            };
                            if item.clicked() {
                                change = Some(Some(class.clone()))
                            }
                        }
                    });
                if let Some(help) = spec
                    .as_ref()
                    .map(|s| s.help.as_str())
                    .filter(|h| !h.is_empty())
                {
                    ui.label(RichText::new(help).size(11.0).weak());
                }

                ui.label(
                    RichText::new(&info.compiles_as)
                        .size(11.0)
                        .color(draw::colors::BRUSH_ENTITY),
                );
                if let Some(motion) = crate::motion::of_selection(&self.document) {
                    ui.label(
                        RichText::new(motion.label)
                            .size(11.0)
                            .color(draw::colors::MOTION),
                    )
                    .on_hover_text("Drawn in the 2D panes: the arrow is the travel.");
                }

                // Its settings, right here, with nothing to press first.
                ui.add_space(6.0);
                ui.separator();
                match (&current, &spec) {
                    (None, _) => {
                        ui.label(
                            RichText::new(
                                "World geometry has no settings: it is a wall, and the compiler \
                             builds it into the level itself. Give it a type above to make \
                             it a door, a trigger or a platform.",
                            )
                            .size(11.0)
                            .weak(),
                        );
                    }
                    (Some(_), None) => {
                        ui.label(
                            RichText::new("no definition for this class")
                                .weak()
                                .size(11.0),
                        );
                    }
                    (Some(_), Some(spec)) if spec.keys.is_empty() && spec.outputs.is_empty() => {
                        ui.label(
                            RichText::new(
                                "Nothing to configure. This class is a way of marking brushes \
                             rather than something with settings.",
                            )
                            .size(11.0)
                            .weak(),
                        );
                    }
                    (Some((id, _)), Some(spec)) => {
                        self.property_rows(ui);
                        self.outputs_section(ui, *id, Some(spec));
                    }
                }

                // Materials last: they matter, but they are not what a brush is.
                ui.add_space(6.0);
                ui.separator();
                ui.label(RichText::new("materials").size(11.0).weak());
                for (material, meaning) in info.material_meanings() {
                    ui.label(RichText::new(material).monospace().size(11.0))
                        .on_hover_text(meaning);
                    ui.label(RichText::new(meaning).size(10.0).weak());
                }
                if info.mixed_materials {
                    ui.label(
                        RichText::new(
                            "faces do not all wear the same material; the most specific one \
                         decides what the brush is",
                        )
                        .size(10.0)
                        .color(egui::Color32::from_rgb(240, 200, 90)),
                    );
                }
                for unknown in info.unknown_tools() {
                    ui.label(
                        RichText::new(format!(
                            "{unknown} is not a tool the compiler knows -- it will be an \
                         ordinary wall"
                        ))
                        .size(11.0)
                        .color(draw::colors::LEAK),
                    );
                }
            });

        if let Some(class) = change {
            let said = class.clone().unwrap_or_else(|| "world geometry".into());
            if self.document.set_brush_class(class.as_deref()) {
                self.status = format!("now {said}");
                // The buffer is pointed at whatever the change produced.
                let now = self.document.selected_brush_class().map(|(id, _)| id);
                self.properties = None;
                self.sync_properties(now);
            }
        }
    }

    /// One widget per key the class defines, typed by the schema.
    pub(super) fn property_rows(&mut self, ui: &mut egui::Ui) {
        let Some(edit) = self.properties.as_mut() else {
            return;
        };
        if edit.rows.is_empty() {
            ui.label(RichText::new("this class has no settings").weak());
            return;
        }

        let materials = &self.materials;
        let models = &self.models;
        let mut commit = false;

        let mut browse: Option<(usize, String)> = None;
        for (index, row) in edit.rows.iter_mut().enumerate() {
            let response = property_widget(ui, index, row, materials, models);
            if response.browse {
                browse = Some((index, row.text().to_string()));
            }
            if response.changed {
                edit.dirty = true;
            }
            // Discrete widgets are done the moment they change; text and
            // number fields are done when they are left.
            if response.finished {
                commit = true;
            }
        }

        ui.add_space(4.0);
        if ui
            .small_button("+ add a key the game does not define")
            .clicked()
        {
            edit.rows.push(PropertyRow {
                key: format!("key{}", edit.rows.len()),
                label: format!("key{}", edit.rows.len()),
                kind: KeyKind::String,
                help: String::new(),
                choices: Vec::new(),
                default: String::new(),
                value: Some(String::new()),
                described: false,
            });
            edit.dirty = true;
            commit = true;
        }

        if let Some((row, current)) = browse {
            self.browsing = Some(Browsing::Model {
                row: Some(row),
                current,
            });
        }
        if commit {
            self.commit_properties();
        }
    }

    /// The wiring: what this entity does, and when.
    ///
    /// Grouped by event rather than shown as a flat list of connections,
    /// because the flat list is the storage format and not the thing anyone is
    /// building. What a designer means is "when this happens, do these things,
    /// in this order" -- and a column of rows with delays in them makes the
    /// order something you work out in your head.
    pub(super) fn outputs_section(
        &mut self,
        ui: &mut egui::Ui,
        _id: u32,
        spec: Option<&kerosene_entity::ClassSpec>,
    ) {
        ui.separator();
        ui.label(RichText::new("when this happens").strong());

        let (outputs, help_for) = class_outputs(spec);
        let targets = inspector::target_names(&self.document);

        // Worked out before the buffer is borrowed: the answer depends on the
        // whole map rather than on this entity.
        let inputs_for: Vec<Vec<String>> = self
            .properties
            .as_ref()
            .map(|edit| {
                edit.connections
                    .iter()
                    .map(|c| inspector::inputs_for_target(&self.schema, &self.document, &c.target))
                    .collect()
            })
            .unwrap_or_default();

        let Some(edit) = self.properties.as_mut() else {
            return;
        };
        let mut dirty = edit.dirty;
        let commit = outputs_editor(
            ui,
            "dock",
            &mut edit.connections,
            &outputs,
            &help_for,
            &targets,
            &inputs_for,
            &mut dirty,
        );
        edit.dirty = dirty;

        if commit {
            self.commit_properties();
        }
    }

    /// The face editor: how the texture sits on the selected faces.
    ///
    /// The arithmetic is in [`crate::faces`] and tested there; this is the
    /// dial in front of it. Every control acts on the whole selection at once,
    /// as one undo step.
    pub(super) fn face_panel(&mut self, ui: &mut egui::Ui) {
        use crate::faces::{self, Justify};

        let specs = self.document.selected_face_specs();
        let Some(first) = specs.first().cloned() else {
            return;
        };
        let count = specs.len();

        ui.horizontal(|ui| {
            ui.label(RichText::new("face").strong());
            ui.label(
                RichText::new(if count == 1 {
                    "1 face".to_string()
                } else {
                    format!("{count} faces")
                })
                .size(11.0)
                .weak(),
            );
        });

        // Which material, and the texture behind it.
        let materials: std::collections::BTreeSet<&str> =
            specs.iter().map(|f| f.side.material.as_str()).collect();
        let shown = match materials.len() {
            1 => first.side.material.clone(),
            n => format!("{n} different materials"),
        };
        ui.label(RichText::new(&shown).monospace().size(11.0));

        let size = self
            .textures
            .get(&self.vfs, &first.side.material)
            .map(|t| (t.width(), t.height()))
            // The content may not be built. 256 is what the dev set is, and a
            // fit against the wrong size is better than no fit at all.
            .unwrap_or((256, 256));
        ui.label(
            RichText::new(format!("{} x {} texels", size.0, size.1))
                .size(10.0)
                .weak(),
        );

        ui.horizontal(|ui| {
            let current = self.document.current_material.clone();
            if ui
                .button("apply current")
                .on_hover_text(format!("Put {current} on {count} face(s)"))
                .clicked()
            {
                let applied = self.document.apply_material();
                self.status = format!("{current} on {applied} faces");
            }
            if ui
                .button("pick up")
                .on_hover_text("Take this face's material")
                .clicked()
            {
                self.document.current_material = first.side.material.clone();
                self.status = format!("picked up {}", first.side.material);
            }
        });

        ui.separator();

        // How the selected faces take part in the NPC walkmap. Set per face
        // here; the compiler folds these rules into the `.kerowalk` it writes
        // on every compile.
        ui.label(RichText::new("walkmap").size(11.0).weak());
        let rules: std::collections::BTreeSet<WalkmapRule> =
            specs.iter().map(|f| f.side.walkmap).collect();
        let current_rule = (rules.len() == 1).then(|| *rules.iter().next().unwrap());
        ui.horizontal(|ui| {
            for rule in WalkmapRule::all() {
                if ui
                    .selectable_label(current_rule == Some(rule), rule.as_str())
                    .on_hover_text(rule.describe())
                    .clicked()
                {
                    let changed = self.document.apply_walkmap(rule);
                    self.status = format!("walkmap {rule} on {changed} faces");
                }
            }
        });
        if rules.len() > 1 {
            ui.label(RichText::new("selected faces differ").size(10.0).weak());
        }

        ui.separator();

        // A value shared by every selected face, or None when they differ.
        // Showing one face's number for six is how you overwrite five of them
        // by accident.
        let shared = |get: fn(&crate::document::FaceSpec) -> f32| -> Option<f32> {
            let first = get(&specs[0]);
            specs
                .iter()
                .all(|f| (get(f) - first).abs() < 1e-4)
                .then_some(first)
        };

        let mut edit: Option<(&'static str, FaceEdit)> = None;

        ui.label(RichText::new("scale (units per texel)").size(11.0).weak());
        ui.horizontal(|ui| {
            if let Some(value) = number(ui, "u", shared(|f| f.side.uaxis.scale), 0.01) {
                edit = Some(("scale", FaceEdit::ScaleU(value)));
            }
            if let Some(value) = number(ui, "v", shared(|f| f.side.vaxis.scale), 0.01) {
                edit = Some(("scale", FaceEdit::ScaleV(value)));
            }
        });

        ui.label(RichText::new("shift (texels)").size(11.0).weak());
        ui.horizontal(|ui| {
            if let Some(value) = number(ui, "u", shared(|f| f.side.uaxis.offset), 1.0) {
                edit = Some(("shift", FaceEdit::ShiftU(value)));
            }
            if let Some(value) = number(ui, "v", shared(|f| f.side.vaxis.offset), 1.0) {
                edit = Some(("shift", FaceEdit::ShiftV(value)));
            }
        });

        ui.horizontal(|ui| {
            ui.label(RichText::new("rotate").size(11.0).weak());
            for degrees in [-90.0f32, -15.0, 15.0, 90.0] {
                if ui.small_button(format!("{degrees:+.0}")).clicked() {
                    edit = Some(("rotate", FaceEdit::Rotate(degrees)));
                }
            }
        });

        ui.separator();
        ui.label(RichText::new("alignment").size(11.0).weak());
        ui.horizontal(|ui| {
            if ui
                .button("world")
                .on_hover_text(
                    "The default projection. Adjacent faces of a wall share a \
                     continuous texture.",
                )
                .clicked()
            {
                edit = Some(("align to world", FaceEdit::AlignWorld));
            }
            if ui
                .button("face")
                .on_hover_text(
                    "Axes in the face's own plane. A texture on a slope stops \
                     being foreshortened, at the cost of no longer lining up \
                     with its neighbours.",
                )
                .clicked()
            {
                edit = Some(("align to face", FaceEdit::AlignFace));
            }
            if ui
                .button("fit")
                .on_hover_text("Scale so the texture spans the face exactly once")
                .clicked()
            {
                edit = Some(("fit", FaceEdit::Justify(Justify::Fit)));
            }
        });

        ui.label(RichText::new("justify").size(11.0).weak());
        ui.horizontal(|ui| {
            for how in [
                Justify::Left,
                Justify::Right,
                Justify::Top,
                Justify::Bottom,
                Justify::Centre,
            ] {
                if ui.small_button(how.label()).clicked() {
                    edit = Some(("justify", FaceEdit::Justify(how)));
                }
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("lightmap").size(11.0).weak());
            if let Some(value) = number(ui, "ku/luxel", shared(|f| f.side.lightmap_scale), 0.5) {
                edit = Some(("lightmap scale", FaceEdit::Lightmap(value)));
            }
        });

        ui.separator();
        if ui.button("clear face selection").clicked() {
            self.document.selection.faces.clear();
        }

        if let Some((label, what)) = edit {
            let changed = self
                .document
                .edit_faces(label, move |side, plane, winding| match what {
                    FaceEdit::ScaleU(v) => faces::set_scale(side, v, side.vaxis.scale),
                    FaceEdit::ScaleV(v) => faces::set_scale(side, side.uaxis.scale, v),
                    FaceEdit::ShiftU(v) => faces::set_shift(side, v, side.vaxis.offset),
                    FaceEdit::ShiftV(v) => faces::set_shift(side, side.uaxis.offset, v),
                    FaceEdit::Rotate(d) => faces::rotate_by(side, plane, winding, d),
                    FaceEdit::AlignWorld => faces::align_to_world(side, plane),
                    FaceEdit::AlignFace => faces::align_to_face(side, plane),
                    FaceEdit::Justify(how) => faces::justify(side, winding, how, size),
                    FaceEdit::Lightmap(v) => side.lightmap_scale = v.clamp(1.0, 128.0),
                });
            self.status = format!("{label} on {changed} faces");
        }
    }
}

/// One thing the face panel can ask for.
///
/// A value rather than a closure so the panel can decide *what* to do while
/// the UI is being drawn and apply it afterwards, without holding a borrow of
/// the document across the whole panel.
#[derive(Clone, Copy, Debug)]
pub(super) enum FaceEdit {
    ScaleU(f32),
    ScaleV(f32),
    ShiftU(f32),
    ShiftV(f32),
    Rotate(f32),
    AlignWorld,
    AlignFace,
    Justify(crate::faces::Justify),
    /// World units per luxel. Finer means a sharper shadow and a bigger
    /// lightmap, so it is a per-face choice rather than a map-wide one.
    Lightmap(f32),
}
