// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The property widgets: one row of the inspector, the grid of rows the
//! popup draws, and the wiring editor both of them share.

use super::*;

/// A number that several faces may or may not agree on.
///
/// `None` means they differ, and the field shows nothing rather than one
/// face's value -- which is how you overwrite the other five by accident.
/// Returns the new value only when the edit is finished, so dragging does not
/// push an undo step per pixel.
pub(super) fn number(ui: &mut egui::Ui, label: &str, shared: Option<f32>, speed: f32) -> Option<f32> {
    ui.label(RichText::new(label).size(11.0).weak());
    let mut value = shared.unwrap_or(0.0) as f64;
    let mut widget = egui::DragValue::new(&mut value).speed(speed as f64);
    if shared.is_none() {
        widget = widget.custom_formatter(|_, _| "--".to_string());
    }
    let response = ui.add(widget);
    let finished = response.drag_stopped() || response.lost_focus();
    (finished && response.changed()).then_some(value as f32)
}

/// What a property widget did this frame.
pub(super) struct WidgetResult {
    /// The value in the buffer moved.
    pub(super) changed: bool,
    /// The edit is complete and worth an undo step -- a discrete choice was
    /// made, or a text field was left.
    pub(super) finished: bool,
    /// The row asked for the asset browser to be opened on it.
    pub(super) browse: bool,
}

/// Draw one property, with a widget suited to what the schema says it holds.
///
/// Typing a vector into a text box works, but it is not editing: the reason to
/// know a key is a colour or an angle is to hand a person the control they
/// would have reached for.
pub(super) fn property_widget(
    ui: &mut egui::Ui,
    index: usize,
    row: &mut PropertyRow,
    materials: &[String],
    models: &[String],
) -> WidgetResult {
    let mut out = WidgetResult {
        changed: false,
        finished: false,
        browse: false,
    };

    ui.horizontal(|ui| {
        let label = ui.add(
            egui::Label::new(
                RichText::new(&row.label)
                    .monospace()
                    .size(11.0)
                    // An unset key is drawn faintly: it is showing the game's
                    // default, not a value anyone chose.
                    .color(if row.is_set() {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    }),
            )
            .truncate(),
        );
        let mut hover = String::new();
        if !row.described {
            hover.push_str("Not defined by this class.\n");
        }
        if !row.help.is_empty() {
            hover.push_str(&row.help);
            hover.push('\n');
        }
        hover.push_str(&format!("key: {}  ({})", row.key, row.kind.name()));
        if !row.default.is_empty() {
            hover.push_str(&format!("\ndefault: {}", row.default));
        }
        label.on_hover_text(hover);
    });

    ui.horizontal(|ui| {
        let r = property_value_widget(ui, "inspector", index, row, materials, models);
        out.changed |= r.changed;
        out.finished |= r.finished;
        out.browse |= r.browse;

        // Clearing a key is how you go back to the game's default, so it needs
        // to be reachable. Only offered when there is something to clear.
        if row.is_set()
            && ui
                .small_button("clear")
                .on_hover_text("Remove this key")
                .clicked()
        {
            row.value = None;
            out.changed = true;
            out.finished = true;
        }
    });

    out
}

/// The value half of a property: the widget itself, without the key label or
/// the clear button. Split out so the docked inspector and the object
/// properties popup can lay the same control out differently.
pub(super) fn property_value_widget(
    ui: &mut egui::Ui,
    salt: &'static str,
    index: usize,
    row: &mut PropertyRow,
    materials: &[String],
    models: &[String],
) -> WidgetResult {
    let mut out = WidgetResult {
        changed: false,
        finished: false,
        browse: false,
    };
    let id = (salt, index, row.key.as_str());
    match row.kind {
        KeyKind::Boolean => {
            let mut on = matches!(row.text().trim(), "1" | "true" | "yes");
            if ui.checkbox(&mut on, "").changed() {
                row.value = Some(if on { "1".into() } else { "0".into() });
                out.changed = true;
                out.finished = true;
            }
        }
        KeyKind::Integer => {
            let mut v: i64 = row.text().trim().parse().unwrap_or(0);
            let r = ui.add_sized(
                [VALUE_FIELD, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut v).speed(1.0),
            );
            if r.changed() {
                row.value = Some(v.to_string());
                out.changed = true;
            }
            out.finished |= r.drag_stopped() || r.lost_focus();
        }
        KeyKind::Float => {
            let mut v: f64 = row.text().trim().parse().unwrap_or(0.0);
            let r = ui.add_sized(
                [VALUE_FIELD, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut v).speed(0.5),
            );
            if r.changed() {
                row.value = Some(kerosene_kv::format_float(v as f32));
                out.changed = true;
            }
            out.finished |= r.drag_stopped() || r.lost_focus();
        }
        KeyKind::Vector | KeyKind::Angles => {
            let mut v = inspector::parse_vec3(row.text());
            let names = if row.kind == KeyKind::Angles {
                ["pitch", "yaw", "roll"]
            } else {
                ["x", "y", "z"]
            };
            let mut any = false;
            for (i, name) in names.iter().enumerate() {
                let r = ui.add(
                    egui::DragValue::new(&mut v[i])
                        .speed(1.0)
                        .prefix(format!("{name} ")),
                );
                any |= r.changed();
                out.finished |= r.drag_stopped() || r.lost_focus();
            }
            if any {
                row.value = Some(inspector::format_vec3(v));
                out.changed = true;
            }
        }
        KeyKind::Color => {
            let (mut rgb, mut brightness) = inspector::parse_color(row.text());
            let mut any = ui.color_edit_button_srgb(&mut rgb).changed();
            let r = ui.add_sized(
                [VALUE_FIELD, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut brightness)
                    .speed(5.0)
                    .range(0.0..=100000.0),
            );
            any |= r.changed();
            out.finished |= r.drag_stopped() || r.lost_focus();
            if any {
                row.value = Some(inspector::format_color(rgb, brightness));
                out.changed = true;
                out.finished = true;
            }
        }
        KeyKind::Choices => {
            let mut current = row.text().to_string();
            let label = row
                .choices
                .iter()
                .find(|(v, _)| *v == current)
                .map(|(_, l)| l.clone())
                .unwrap_or_else(|| current.clone());
            let mut picked = None;
            egui::ComboBox::from_id_salt(id)
                .selected_text(label)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for (value, label) in &row.choices {
                        if ui.selectable_label(*value == current, label).clicked() {
                            picked = Some(value.clone());
                        }
                    }
                });
            if let Some(value) = picked {
                current = value;
                row.value = Some(current);
                out.changed = true;
                out.finished = true;
            }
        }
        KeyKind::Flags => {
            // A bit field is a row of checkboxes, because that is what it
            // is. Bits the schema does not name are preserved untouched.
            let mut bits: u32 = row.text().trim().parse().unwrap_or(0);
            let mut any = false;
            ui.vertical(|ui| {
                for (value, label) in &row.choices {
                    let Ok(bit) = value.parse::<u32>() else {
                        continue;
                    };
                    let mut on = bits & bit != 0;
                    if ui
                        .checkbox(&mut on, RichText::new(label).size(11.0))
                        .changed()
                    {
                        if on {
                            bits |= bit
                        } else {
                            bits &= !bit
                        }
                        any = true;
                    }
                }
            });
            if any {
                row.value = Some(bits.to_string());
                out.changed = true;
                out.finished = true;
            }
        }
        KeyKind::Material | KeyKind::Model => {
            let options: &[String] = if row.kind == KeyKind::Material {
                materials
            } else {
                models
            };
            let mut text = row.text().to_string();
            let r = combo_or_text(ui, id, &mut text, options, 150.0);
            if r.changed {
                row.value = Some(text);
                out.changed = true;
            }
            out.finished |= r.finished;
            // A name is not a shape. Whether `crate_wood` is the one you
            // want is a question a picture answers and a dropdown does
            // not.
            if ui
                .small_button("...")
                .on_hover_text("Browse, with pictures")
                .clicked()
            {
                out.browse = true;
            }
        }
        KeyKind::String | KeyKind::TargetSource | KeyKind::TargetDestination => {
            let mut text = row.text().to_string();
            let r = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(190.0)
                    .hint_text(row.default.as_str()),
            );
            if r.changed() {
                row.value = Some(text);
                out.changed = true;
            }
            out.finished |= r.lost_focus();
        }
    }

    out
}

/// A class's outputs and the one-line help for each, in the shape both
/// wiring editors want them.
pub(super) fn class_outputs(
    spec: Option<&kerosene_entity::ClassSpec>,
) -> (Vec<String>, std::collections::HashMap<String, String>) {
    let outputs = spec
        .map(|s| s.outputs.iter().map(|o| o.name.clone()).collect())
        .unwrap_or_default();
    let help_for = spec
        .map(|s| {
            s.outputs
                .iter()
                .map(|o| (o.name.clone(), o.help.clone()))
                .collect()
        })
        .unwrap_or_default();
    (outputs, help_for)
}

/// The wiring editor, shared by the docked panel and the Object Properties
/// popup.
///
/// It is handed the connection buffer rather than the document because both
/// callers keep one for the same reason: a target typed a character at a time
/// must not be a character's worth of undo history. `scope` keeps the two
/// copies' widget ids apart -- without it, opening the popup while the docked
/// panel shows the same entity makes the two fight over focus, and typing in
/// one moves the caret in the other.
///
/// Returns whether the edit is finished with and should be written back.
#[allow(clippy::too_many_arguments)]
pub(super) fn outputs_editor(
    ui: &mut egui::Ui,
    scope: &'static str,
    connections: &mut Vec<Connection>,
    outputs: &[String],
    help_for: &std::collections::HashMap<String, String>,
    targets: &[String],
    inputs_for: &[Vec<String>],
    dirty: &mut bool,
) -> bool {
    use crate::wiring;

    let events = wiring::events(connections);
    if events.is_empty() {
        ui.label(RichText::new("nothing wired up yet").weak().size(11.0));
    }

    let mut remove: Option<usize> = None;
    let mut add: Option<Connection> = None;
    let mut commit = false;

    for event in &events {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&event.name).monospace().strong());
                if let Some(help) = help_for.get(&event.name).filter(|h| !h.is_empty()) {
                    ui.label(RichText::new("?").weak().size(11.0))
                        .on_hover_text(help);
                }
            });

            // The other half of a choice, when this is one. An `OnTrue`
            // with no `OnFalse` beside it does nothing half the time.
            if let Some(other) = wiring::opposite_of(&event.name)
                && outputs.iter().any(|o| o == other)
                && !events.iter().any(|e| e.name == other)
            {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("nothing happens on {other}"))
                            .size(10.0)
                            .color(egui::Color32::from_rgb(240, 200, 90)),
                    );
                    if ui.small_button(format!("+ {other}")).clicked() {
                        add = Some(Connection::new(other, "", ""));
                    }
                });
            }

            for (step, &index) in event.steps.iter().enumerate() {
                let empty = Vec::new();
                let inputs = inputs_for.get(index).unwrap_or(&empty);
                let Some(connection) = connections.get_mut(index) else {
                    continue;
                };

                ui.horizontal(|ui| {
                    // "then" rather than a number, because that is the
                    // word for what the second step of a sequence is.
                    ui.label(
                        RichText::new(if step == 0 { "do" } else { "then" })
                            .size(11.0)
                            .weak(),
                    );
                    let r = combo_or_text(
                        ui,
                        (scope, "in", index),
                        &mut connection.input,
                        inputs,
                        120.0,
                    );
                    *dirty |= r.changed;
                    commit |= r.finished;

                    ui.label(RichText::new("on").size(11.0).weak());
                    let r = combo_or_text(
                        ui,
                        (scope, "tgt", index),
                        &mut connection.target,
                        targets,
                        110.0,
                    );
                    *dirty |= r.changed;
                    commit |= r.finished;

                    if ui
                        .small_button("x")
                        .on_hover_text("remove this step")
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });

                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new("after").size(10.0).weak());
                    let r = ui.add(
                        egui::DragValue::new(&mut connection.delay)
                            .speed(0.05)
                            .range(0.0..=600.0)
                            .suffix(" s"),
                    );
                    *dirty |= r.changed();
                    commit |= r.drag_stopped() || r.lost_focus();

                    ui.label(RichText::new("with").size(10.0).weak());
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut connection.parameter)
                            .desired_width(70.0)
                            .hint_text("no value"),
                    );
                    *dirty |= r.changed();
                    commit |= r.lost_focus();

                    let mut once = !connection.is_unlimited();
                    if ui
                        .checkbox(&mut once, RichText::new("once").size(10.0))
                        .changed()
                    {
                        connection.times_to_fire = if once { 1 } else { -1 };
                        *dirty = true;
                        commit = true;
                    }
                });
            }

            if ui
                .small_button("+ then")
                .on_hover_text("another action on this same event, after the ones above")
                .clicked()
            {
                add = Some(wiring::then(connections, event));
            }
        });
    }

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt((scope, "add-event"))
            .selected_text("+ when...")
            .width(150.0)
            .show_ui(ui, |ui| {
                for output in outputs {
                    let already = events.iter().any(|e| e.name == *output);
                    let label = if already {
                        format!("{output} (another)")
                    } else {
                        output.clone()
                    };
                    let item = ui.selectable_label(false, label);
                    let item = match help_for.get(output).filter(|h| !h.is_empty()) {
                        Some(help) => item.on_hover_text(help),
                        None => item,
                    };
                    if item.clicked() {
                        add = Some(Connection::new(output, "", ""));
                    }
                }
                if outputs.is_empty() {
                    ui.label(RichText::new("this class fires nothing").weak().size(11.0));
                }
            });
    });

    if let Some(index) = remove {
        connections.remove(index);
        *dirty = true;
        commit = true;
    }
    if let Some(connection) = add {
        connections.push(connection);
        *dirty = true;
        commit = true;
    }
    commit
}

/// How wide the key column is, so the value fields line up down the popup.
const KEY_COLUMN: f32 = 150.0;

/// How wide a single-value field is drawn.
///
/// egui sizes a `DragValue` to its digits, which leaves a number sitting in a
/// box a third the width of the text field on the row above it. In a dialog
/// that is a column of values, they want to be a column.
const VALUE_FIELD: f32 = 90.0;

/// The grid of key/value rows inside the object properties popup.
///
/// Four columns: the key (editable for keys the schema does not know), the
/// value widget, the type, and a clear button for returning to the game's
/// default. Returns whether anything asked to be written back.
pub(super) fn property_grid(
    ui: &mut egui::Ui,
    window: &mut PropertyWindow,
    materials: &[String],
    models: &[String],
) -> bool {
    if window.rows.is_empty() {
        ui.label(RichText::new("no properties to edit — add one below").weak());
        return false;
    }
    let mut commit = false;
    #[cfg(test)]
    let mut narrowest = f32::INFINITY;
    // Rows rather than an `egui::Grid`, which cannot lay this out.
    //
    // A grid caps each cell at the width its column measured last frame, and
    // every widget in these rows -- a truncating label, a `TextEdit` given a
    // desired width -- shrinks to the space it is offered. So a column that
    // starts narrow makes its contents narrow, which measures narrow, which
    // keeps the column narrow: the fields collapsed to 48pt and stayed there.
    // A plain row hands the widgets the real width, and a fixed key column
    // keeps the values lined up, which is all the grid was wanted for.
    for (index, row) in window.rows.iter_mut().enumerate() {
        let stripe = if index % 2 == 1 {
            ui.visuals().faint_bg_color
        } else {
            egui::Color32::TRANSPARENT
        };
        egui::Frame::new()
            .fill(stripe)
            .inner_margin(egui::Margin::symmetric(2, 2))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    // Column 1: the key. Schema keys are fixed labels; custom
                    // keys are renamed in place, which is how a brush gains a
                    // `playercollision`.
                    ui.allocate_ui_with_layout(
                        egui::vec2(KEY_COLUMN, ui.spacing().interact_size.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            if row.described {
                                let label = ui.add(
                                    egui::Label::new(
                                        RichText::new(&row.label).monospace().size(11.0).color(
                                            if row.is_set() {
                                                ui.visuals().text_color()
                                            } else {
                                                ui.visuals().weak_text_color()
                                            },
                                        ),
                                    )
                                    .truncate(),
                                );
                                let mut hover = String::new();
                                if !row.help.is_empty() {
                                    hover.push_str(&row.help);
                                    hover.push('\n');
                                }
                                hover.push_str(&format!("key: {}  ({})", row.key, row.kind.name()));
                                if !row.default.is_empty() {
                                    hover.push_str(&format!("\ndefault: {}", row.default));
                                }
                                label.on_hover_text(hover);
                            } else {
                                let mut key = row.key.clone();
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut key)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace)
                                        .hint_text("key"),
                                );
                                if r.changed() {
                                    row.key = key.clone();
                                    row.label = key;
                                    window.dirty = true;
                                }
                                commit |= r.lost_focus();
                            }
                        },
                    );

                    // Column 2: the value widget, typed by the schema.
                    let value_x0 = ui.cursor().min.x;
                    let r = property_value_widget(
                        ui,
                        "object-properties",
                        index,
                        row,
                        materials,
                        models,
                    );
                    // What the field actually got, as opposed to what it asked
                    // for. The two came apart badly once and nothing caught it.
                    #[cfg(test)]
                    {
                        narrowest = narrowest.min(ui.cursor().min.x - value_x0);
                    }
                    #[cfg(not(test))]
                    let _ = value_x0;
                    if r.changed {
                        window.dirty = true;
                    }
                    commit |= r.finished;

                    // What kind of value it is, then the way back to the
                    // game's default. Both sit at the right-hand end so the
                    // fields between them stay aligned.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if row.is_set()
                            && ui
                                .small_button("clear")
                                .on_hover_text("Remove this key")
                                .clicked()
                        {
                            row.value = None;
                            window.dirty = true;
                            commit = true;
                        }
                        ui.label(RichText::new(row.kind.name()).size(10.0).weak());
                    });
                });
            });
    }
    #[cfg(test)]
    {
        window.narrowest_value = narrowest;
    }
    commit
}

/// A combo box of known values that still accepts anything typed.
///
/// Both halves matter: the list is how a name is found, and the text box is
/// how a name that does not exist yet gets used -- wiring an output to an
/// entity you have not placed is a normal way to work.
pub(super) fn combo_or_text(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + Clone,
    value: &mut String,
    options: &[String],
    width: f32,
) -> WidgetResult {
    let mut out = WidgetResult {
        changed: false,
        finished: false,
        browse: false,
    };
    let text_width = (width - 30.0).max(60.0);

    let response = ui.add(egui::TextEdit::singleline(value).desired_width(text_width));
    out.changed |= response.changed();
    out.finished |= response.lost_focus();

    if !options.is_empty() {
        let mut picked = None;
        egui::ComboBox::from_id_salt(id)
            .selected_text("")
            .width(24.0)
            .show_ui(ui, |ui| {
                for option in options {
                    if ui.selectable_label(option == value, option).clicked() {
                        picked = Some(option.clone());
                    }
                }
            });
        if let Some(p) = picked {
            *value = p;
            out.changed = true;
            out.finished = true;
        }
    }
    out
}
