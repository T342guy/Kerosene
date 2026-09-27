// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! An entity's wiring: what it fires, and what fires at it.
//!
//! Laid out the way Hammer lays it out, because that is the layout people
//! know how to read: a list of connections, one line each -- *output*,
//! *target*, *input*, *delay* -- with a light beside each saying whether it
//! will do anything, and the one picked out edited in a form underneath.
//! The **Inputs** tab is the same list turned round: every connection on any
//! entity that fires at this one, which is the question "what opens this
//! door?" that a list of outputs cannot answer.
//!
//! The editor this replaces grouped connections into "when X, do Y, then Z"
//! sentences. It read nicely for a two-step sequence and badly for
//! everything else, and it never said when a wire went nowhere.

use super::widgets;
use super::*;
use crate::wiring::{self, Status};
use kerosene_entity::IoSpec;
use kerosene_toolui::theme::{self, colors, icons};

/// What the outputs editor needs to know about the map, worked out before
/// the connection buffer is borrowed.
pub(super) struct IoData {
    /// The outputs the source's class fires.
    pub outputs: Vec<IoSpec>,
    /// Every name in the map, and the special targets.
    pub targets: Vec<String>,
    /// For each connection, the inputs its target takes.
    pub inputs_for: Vec<Vec<IoSpec>>,
    /// For each connection, whether it will do anything.
    pub statuses: Vec<Status>,
}

/// The colour of a status light.
pub(super) fn status_colour(status: &Status) -> egui::Color32 {
    match status {
        Status::Ok => colors::OK,
        Status::Runtime(_) => colors::WARN,
        Status::Broken(_) => colors::ERR,
    }
}

/// A status light: a filled dot, with the reason on hover.
fn status_dot(ui: &mut egui::Ui, status: &Status) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 3.5, status_colour(status));
    response.on_hover_text(status.explain())
}

/// One connection as a line of text: `OnPressed -> door . Open (x) +0.5s`.
fn connection_line(
    c: &Connection,
    lead: &str,
    lead_colour: egui::Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(11.5);
    let mut push = |text: &str, colour: egui::Color32| {
        job.append(text, 0.0, egui::TextFormat::simple(font.clone(), colour));
    };
    let or_dash = |s: &str| {
        if s.trim().is_empty() {
            "?".to_string()
        } else {
            s.to_string()
        }
    };
    push(&or_dash(lead), lead_colour);
    push("  ->  ", colors::TEXT_MUTED);
    push(&or_dash(&c.target), colors::ACCENT);
    push(".", colors::TEXT_MUTED);
    push(&or_dash(&c.input), colors::TEXT);
    if !c.parameter.trim().is_empty() {
        push(&format!("({})", c.parameter), colors::TEXT_MUTED);
    }
    if c.delay > 0.0 {
        push(
            &format!("  +{}s", kerosene_math::format_float(c.delay)),
            colors::INFO,
        );
    }
    if !c.is_unlimited() {
        push(
            &if c.times_to_fire == 1 {
                "  once".to_string()
            } else {
                format!("  x{}", c.times_to_fire)
            },
            colors::WARN,
        );
    }
    job
}

impl ChiselApp {
    /// Work out what the outputs editor shows for `source` with the
    /// connections as they stand in the buffer.
    pub(super) fn io_data(&self, source_id: u32, connections: &[Connection]) -> IoData {
        let entities = &self.document.map.entities;
        let mut source = self
            .document
            .find_entity(source_id)
            .cloned()
            .unwrap_or_else(|| kerosene_map::Entity::new(source_id, ""));
        source.connections = connections.to_vec();
        let outputs = self
            .schema
            .get(source.classname())
            .map(|s| s.outputs.clone())
            .unwrap_or_default();
        let mut targets = inspector::target_names(&self.document);
        targets.extend(wiring::SPECIAL_TARGETS.iter().map(|(n, _)| n.to_string()));
        let inputs_for = connections
            .iter()
            .map(|c| wiring::input_choices(&source, &c.target, entities, &self.schema))
            .collect();
        // Checked against the map with the edited entity in it, so renaming
        // a target in this very panel is reflected at once.
        let mut world: Vec<kerosene_map::Entity> = entities.clone();
        if let Some(own) = world.iter_mut().find(|e| e.id == source_id) {
            *own = source.clone();
        }
        let statuses = connections
            .iter()
            .map(|c| wiring::validate(&source, c, &world, &self.schema))
            .collect();
        IoData {
            outputs,
            targets,
            inputs_for,
            statuses,
        }
    }

    /// The Inputs tab: every connection that fires at `id`. Clicking one
    /// selects the entity it is on.
    pub(super) fn inputs_list(&mut self, ui: &mut egui::Ui, id: u32) {
        let Some(entity) = self.document.find_entity(id).cloned() else {
            return;
        };
        let entities = &self.document.map.entities;
        let inputs = wiring::inputs_to(entities, &entity);
        if inputs.is_empty() {
            ui.label(theme::caption(match entity.targetname() {
                Some(name) if !name.trim().is_empty() => {
                    format!("nothing fires at {name} yet. Wire another entity's output to it.")
                }
                _ => "nothing can fire at it: it has no name. Give it one on the Properties tab."
                    .to_string(),
            }));
            return;
        }
        ui.label(theme::caption(format!(
            "{} connection{} fire{} at this entity. Click one to go to it.",
            inputs.len(),
            if inputs.len() == 1 { "" } else { "s" },
            if inputs.len() == 1 { "s" } else { "" },
        )));
        ui.add_space(2.0);
        let mut go_to = None;
        for (source_id, index) in inputs {
            let Some(source) = entities.iter().find(|e| e.id == source_id) else {
                continue;
            };
            let connection = &source.connections[index];
            let status = wiring::validate(source, connection, entities, &self.schema);
            let who = match source.targetname().filter(|n| !n.trim().is_empty()) {
                Some(name) => format!("{name}.{}", connection.output),
                None => format!("{}.{}", source.classname(), connection.output),
            };
            ui.horizontal(|ui| {
                status_dot(ui, &status);
                let job = connection_line(connection, &who, colors::INFO);
                let row = ui
                    .add(egui::Button::selectable(false, job).frame_when_inactive(false))
                    .on_hover_text(format!(
                        "on {} {}\n{}",
                        source.classname(),
                        source.targetname().unwrap_or("(unnamed)"),
                        status.explain()
                    ));
                if row.clicked() {
                    go_to = Some(source_id);
                }
            });
        }
        if let Some(source) = go_to {
            self.commit_properties();
            self.document.selection.clear();
            self.document.selection.entities.insert(source);
            self.entity_tab = EntityTab::Outputs;
            self.status = "selected the entity that fires it".into();
        }
    }
}

/// The outputs editor: the list, the buttons, and the form for the row
/// picked out. Shared by the docked panel and the Object Properties popup;
/// `scope` keeps their widget ids and picked rows apart.
///
/// Returns whether the edit is finished with and should be written back.
pub(super) fn outputs_editor(
    ui: &mut egui::Ui,
    scope: &'static str,
    connections: &mut Vec<Connection>,
    io: &IoData,
    dirty: &mut bool,
) -> bool {
    let picked_id = egui::Id::new((scope, "io-picked"));
    let clipboard_id = egui::Id::new("chisel-io-clipboard");
    let mut picked: Option<usize> = ui
        .data(|d| d.get_temp::<usize>(picked_id))
        .filter(|&i| i < connections.len());
    let mut commit = false;
    let mut add: Option<Connection> = None;
    let mut remove: Option<usize> = None;

    // The other half of a choice, when only one half is wired.
    let events = wiring::events(connections);
    for event in &events {
        if let Some(other) = wiring::opposite_of(&event.name)
            && io.outputs.iter().any(|o| o.name == other)
            && !events.iter().any(|e| e.name == other)
        {
            ui.horizontal(|ui| {
                ui.label(theme::warn(format!("nothing happens on {other}")).size(11.0));
                if ui.small_button(format!("+ {other}")).clicked() {
                    add = Some(Connection::new(other, "", ""));
                }
            });
        }
    }

    if connections.is_empty() {
        ui.label(theme::caption(
            "nothing wired up yet. + Add wires an output to another entity.",
        ));
    }
    // In firing order: grouped by output, then by delay.
    for event in &events {
        for &index in &event.steps {
            let Some(connection) = connections.get(index) else {
                continue;
            };
            let status = io.statuses.get(index).cloned().unwrap_or(Status::Ok);
            ui.horizontal(|ui| {
                status_dot(ui, &status);
                let job = connection_line(connection, &connection.output, colors::TEXT);
                let row = ui
                    .add(
                        egui::Button::selectable(picked == Some(index), job)
                            .frame_when_inactive(false),
                    )
                    .on_hover_text(status.explain());
                if row.clicked() {
                    picked = Some(index);
                }
            });
        }
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .button(format!("{} Add", icons::PLUS))
            .on_hover_text("A new connection -- after the picked one, on the same output and target, when one is picked")
            .clicked()
        {
            add = Some(match picked.and_then(|i| connections.get(i)) {
                Some(c) => {
                    let mut next = c.clone();
                    next.delay = c.delay + wiring::THEN_STEP;
                    next.input.clear();
                    next.parameter.clear();
                    next
                }
                None => Connection::new(
                    io.outputs.first().map_or("", |o| o.name.as_str()),
                    "",
                    "",
                ),
            });
        }
        let has_pick = picked.is_some();
        if ui
            .add_enabled(has_pick, egui::Button::new(format!("{} Copy", icons::COPY)))
            .on_hover_text("Copy the picked connection, to paste onto this or another entity")
            .clicked()
            && let Some(c) = picked.and_then(|i| connections.get(i))
        {
            ui.data_mut(|d| d.insert_temp(clipboard_id, vec![c.clone()]));
        }
        let pasteable: Option<Vec<Connection>> = ui.data(|d| d.get_temp(clipboard_id));
        if ui
            .add_enabled(pasteable.is_some(), egui::Button::new(format!("{} Paste", icons::CLIPBOARD)))
            .on_hover_text("Add the copied connection here")
            .clicked()
            && let Some(copied) = pasteable
        {
            connections.extend(copied);
            picked = Some(connections.len() - 1);
            *dirty = true;
            commit = true;
        }
        if ui
            .add_enabled(has_pick, egui::Button::new(format!("{} Delete", icons::TRASH)))
            .clicked()
        {
            remove = picked;
        }
    });

    // The form for the picked connection.
    if let Some(index) = picked
        && let Some(connection) = connections.get_mut(index)
    {
        ui.add_space(4.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            let status = io.statuses.get(index).cloned().unwrap_or(Status::Ok);
            let inputs = io.inputs_for.get(index).map(Vec::as_slice).unwrap_or(&[]);
            let output_names: Vec<String> = io.outputs.iter().map(|o| o.name.clone()).collect();
            let input_names: Vec<String> = inputs.iter().map(|i| i.name.clone()).collect();

            egui::Grid::new((scope, "io-form"))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    let width = (ui.available_width() - 90.0).max(140.0);

                    ui.label(theme::caption("my output"));
                    let r = widgets::combo_or_text(
                        ui,
                        (scope, "out", index),
                        &mut connection.output,
                        &output_names,
                        width,
                    );
                    *dirty |= r.changed;
                    commit |= r.finished;
                    ui.end_row();
                    if let Some(help) = io
                        .outputs
                        .iter()
                        .find(|o| o.name.eq_ignore_ascii_case(&connection.output))
                        .map(|o| o.help.as_str())
                        .filter(|h| !h.is_empty())
                    {
                        ui.label("");
                        ui.label(theme::caption(help));
                        ui.end_row();
                    }

                    ui.label(theme::caption("target"));
                    let r = widgets::combo_or_text(
                        ui,
                        (scope, "tgt", index),
                        &mut connection.target,
                        &io.targets,
                        width,
                    );
                    *dirty |= r.changed;
                    commit |= r.finished;
                    ui.end_row();
                    if let Some((_, what)) = wiring::SPECIAL_TARGETS
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(connection.target.trim()))
                    {
                        ui.label("");
                        ui.label(theme::caption(*what));
                        ui.end_row();
                    }

                    ui.label(theme::caption("input"));
                    let r = widgets::combo_or_text(
                        ui,
                        (scope, "in", index),
                        &mut connection.input,
                        &input_names,
                        width,
                    );
                    *dirty |= r.changed;
                    commit |= r.finished;
                    ui.end_row();
                    let spec = inputs
                        .iter()
                        .find(|i| i.name.eq_ignore_ascii_case(&connection.input));
                    if let Some(help) = spec.map(|i| i.help.as_str()).filter(|h| !h.is_empty()) {
                        ui.label("");
                        ui.label(theme::caption(help));
                        ui.end_row();
                    }

                    ui.label(theme::caption("parameter"));
                    let hint = spec
                        .and_then(|i| i.parameter.clone())
                        .unwrap_or_else(|| "none".to_string());
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut connection.parameter)
                            .desired_width(width)
                            .hint_text(hint),
                    );
                    *dirty |= r.changed();
                    commit |= r.lost_focus();
                    ui.end_row();

                    ui.label(theme::caption("delay"));
                    let r = ui.add(
                        egui::DragValue::new(&mut connection.delay)
                            .speed(0.05)
                            .range(0.0..=600.0)
                            .suffix(" s"),
                    );
                    *dirty |= r.changed();
                    commit |= r.drag_stopped() || r.lost_focus();
                    ui.end_row();

                    ui.label(theme::caption("only once"));
                    let mut once = !connection.is_unlimited();
                    if ui
                        .checkbox(&mut once, "")
                        .on_hover_text("Fire the first time only, then never again.")
                        .changed()
                    {
                        connection.times_to_fire = if once { 1 } else { -1 };
                        *dirty = true;
                        commit = true;
                    }
                    ui.end_row();
                });
            ui.horizontal(|ui| {
                status_dot(ui, &status);
                ui.label(
                    RichText::new(status.explain())
                        .size(11.0)
                        .color(status_colour(&status)),
                );
            });
        });
    }

    if let Some(index) = remove {
        connections.remove(index);
        picked = None;
        *dirty = true;
        commit = true;
    }
    if let Some(connection) = add {
        connections.push(connection);
        picked = Some(connections.len() - 1);
        *dirty = true;
        commit = true;
    }
    ui.data_mut(|d| match picked {
        Some(i) => d.insert_temp(picked_id, i),
        None => d.remove::<usize>(picked_id),
    });
    commit
}
