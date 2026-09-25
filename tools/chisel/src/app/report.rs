// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The entity report and the undo history: two windows that list things.
//!
//! The report is every entity in the map as a table you can filter, sort
//! and click, which is how a `logic_relay` named three weeks ago is found
//! again -- and it marks the wiring that points at nothing, which is
//! otherwise found by playing the map and wondering why the door did not
//! open. The history is the undo stack with names on it.

use super::*;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

/// Which entities the report lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReportKind {
    #[default]
    All,
    Point,
    Brush,
}

/// The report window's state.
#[derive(Clone, Debug, Default)]
pub struct EntityReport {
    pub open: bool,
    pub filter: String,
    pub kind: ReportKind,
    /// Only entities with a problem: an output aimed at nothing.
    pub problems_only: bool,
}

/// One row of the report, computed from the map.
#[derive(Clone, Debug, PartialEq)]
pub struct ReportRow {
    pub id: u32,
    pub classname: String,
    pub targetname: String,
    pub origin: Vec3,
    pub brushes: usize,
    /// Outputs whose target names no entity in the map.
    pub dangling: Vec<String>,
}

impl ChiselApp {
    /// The rows the report would show, given its filters.
    pub fn report_rows(&self) -> Vec<ReportRow> {
        let names = inspector::target_names(&self.document);
        let filter = self.report.filter.trim().to_ascii_lowercase();
        let mut rows: Vec<ReportRow> = self
            .document
            .map
            .entities
            .iter()
            .filter(|e| match self.report.kind {
                ReportKind::All => true,
                ReportKind::Point => e.solids.is_empty(),
                ReportKind::Brush => !e.solids.is_empty(),
            })
            .map(|e| {
                let dangling = e
                    .connections
                    .iter()
                    .filter(|c| {
                        let t = c.target.trim();
                        // `!activator` and friends are not names in the map.
                        !t.is_empty() && !t.starts_with('!') && !names.iter().any(|n| n == t)
                    })
                    .map(|c| format!("{} -> {}", c.output, c.target))
                    .collect();
                ReportRow {
                    id: e.id,
                    classname: e.classname().to_string(),
                    targetname: e.targetname().unwrap_or("").to_string(),
                    origin: e.origin(),
                    brushes: e.solids.len(),
                    dangling,
                }
            })
            .filter(|r| {
                filter.is_empty()
                    || r.classname.to_ascii_lowercase().contains(&filter)
                    || r.targetname.to_ascii_lowercase().contains(&filter)
            })
            .filter(|r| !self.report.problems_only || !r.dangling.is_empty())
            .collect();
        rows.sort_by(|a, b| {
            a.classname
                .cmp(&b.classname)
                .then_with(|| a.targetname.cmp(&b.targetname))
                .then_with(|| a.id.cmp(&b.id))
        });
        rows
    }

    /// Select one entity and bring every pane to it.
    pub(super) fn go_to_entity(&mut self, id: u32) {
        self.document.selection.clear();
        self.document.selection.entities.insert(id);
        if let Some(bounds) = self.document.selection_bounds() {
            let bounds = bounds.expanded(64.0);
            for viewport in &mut self.viewports {
                viewport.focus_on(bounds);
            }
        }
    }

    pub(super) fn report_window(&mut self, ctx: &Context) {
        if !self.report.open {
            return;
        }
        let mut open = true;
        let mut go: Option<(u32, bool)> = None;
        egui::Window::new("Entity report")
            .open(&mut open)
            .resizable(true)
            .default_width(560.0)
            .default_height(380.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(icons::MAGNIFYING_GLASS).color(colors::TEXT_MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.report.filter)
                            .desired_width(180.0)
                            .hint_text("class or name"),
                    );
                    for (kind, label) in [
                        (ReportKind::All, "all"),
                        (ReportKind::Point, "point"),
                        (ReportKind::Brush, "brush"),
                    ] {
                        if ui
                            .selectable_label(self.report.kind == kind, label)
                            .clicked()
                        {
                            self.report.kind = kind;
                        }
                    }
                    widgets::chip(ui, "problems only", &mut self.report.problems_only);
                });
                ui.separator();

                let rows = self.report_rows();
                let problems = rows.iter().filter(|r| !r.dangling.is_empty()).count();
                ui.horizontal(|ui| {
                    ui.label(theme::caption(format!("{} entities", rows.len())));
                    if problems > 0 {
                        ui.label(
                            theme::warn(format!("{problems} with an output aimed at nothing"))
                                .size(11.0),
                        );
                    }
                });

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("entity-report")
                            .striped(true)
                            .num_columns(4)
                            .min_col_width(60.0)
                            .show(ui, |ui| {
                                ui.label(theme::section_title("class"));
                                ui.label(theme::section_title("name"));
                                ui.label(theme::section_title("where"));
                                ui.label(theme::section_title("wiring"));
                                ui.end_row();
                                for row in &rows {
                                    let selected =
                                        self.document.selection.entities.contains(&row.id);
                                    let r =
                                        ui.selectable_label(selected, theme::mono(&row.classname));
                                    if r.double_clicked() {
                                        go = Some((row.id, true));
                                    } else if r.clicked() {
                                        go = Some((row.id, false));
                                    }
                                    ui.label(theme::mono(&row.targetname).color(colors::TEXT));
                                    if row.brushes > 0 {
                                        ui.label(theme::caption(format!(
                                            "{} brushes",
                                            row.brushes
                                        )));
                                    } else {
                                        ui.label(theme::caption(format!(
                                            "{} {} {}",
                                            kerosene_math::format_float(row.origin.x),
                                            kerosene_math::format_float(row.origin.y),
                                            kerosene_math::format_float(row.origin.z),
                                        )));
                                    }
                                    if row.dangling.is_empty() {
                                        ui.label("");
                                    } else {
                                        ui.label(theme::warn(row.dangling.join(", ")).size(11.0))
                                            .on_hover_text(
                                                "These outputs name no entity in the map.",
                                            );
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
        if let Some((id, properties)) = go {
            self.go_to_entity(id);
            if properties {
                self.open_property_window();
            }
        }
        self.report.open = open;
    }

    /// The undo stack, newest first, and the redo stack above it.
    pub(super) fn history_window(&mut self, ctx: &Context) {
        if !self.show_history {
            return;
        }
        let mut open = true;
        let mut target: Option<usize> = None;
        egui::Window::new("History")
            .open(&mut open)
            .resizable(true)
            .default_width(260.0)
            .default_height(320.0)
            .show(ctx, |ui| {
                let undo = self.document.undo_labels();
                let redo = self.document.redo_labels();
                ui.label(theme::caption(
                    "Click a step to go back to it; the steps above come back with redo.",
                ));
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Redo entries: the top of the redo stack is the next
                        // to be redone, so it sits nearest the present.
                        for (i, label) in redo.iter().enumerate() {
                            let depth = undo.len() + redo.len() - i;
                            if ui
                                .selectable_label(
                                    false,
                                    theme::mono(*label).color(colors::TEXT_MUTED),
                                )
                                .clicked()
                            {
                                target = Some(depth);
                            }
                        }
                        let now =
                            ui.selectable_label(true, theme::mono("now").color(colors::ACCENT));
                        if now.clicked() {
                            target = Some(undo.len());
                        }
                        for (i, label) in undo.iter().enumerate().rev() {
                            if ui.selectable_label(false, theme::mono(*label)).clicked() {
                                target = Some(i);
                            }
                        }
                        if undo.is_empty() && redo.is_empty() {
                            ui.label(theme::caption("nothing has happened yet"));
                        }
                    });
            });
        if let Some(depth) = target {
            self.commit_properties();
            self.commit_property_window();
            let moved = self.document.undo_to(depth);
            if moved != 0 {
                self.status = format!(
                    "history: {} {}",
                    moved.abs(),
                    if moved < 0 { "undone" } else { "redone" }
                );
            }
        }
        self.show_history = open;
    }
}
