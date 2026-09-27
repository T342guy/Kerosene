// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The bar across the top: where you are, a box that searches everything,
//! and what is running.
//!
//! A build can take minutes and the person who started it has usually gone
//! back to the editor, so its state lives here, on every tab: a spinner,
//! what it is, how long it has run, and a way to stop it. For a little while
//! after it finishes, it says how it went.

use std::time::Duration;

use egui::{Align, Align2, FontId, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets::{self, Kind as ButtonKind};

use super::{Action, JobKind, Tab, Toolset};
use crate::job::Job;

/// How tall the bar is.
const HEIGHT: f32 = 44.0;

/// How long a finished job's verdict stays in the bar.
const VERDICT: Duration = Duration::from_secs(20);

impl Toolset {
    pub(super) fn topbar(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        egui::TopBottomPanel::top("kerosene-topbar")
            .exact_height(HEIGHT)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.painter().hline(
                    ui.clip_rect().x_range(),
                    rect.bottom() - 0.5,
                    egui::Stroke::new(1.0_f32, colors::BORDER),
                );

                let search_width = (rect.width() * 0.36).clamp(240.0, 480.0);
                let search = Rect::from_center_size(
                    egui::pos2(rect.center().x, rect.center().y),
                    Vec2::new(search_width, 30.0),
                );
                let left =
                    Rect::from_min_max(rect.min, egui::pos2(search.left() - 12.0, rect.max.y));
                let right =
                    Rect::from_min_max(egui::pos2(search.right() + 12.0, rect.min.y), rect.max);

                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(left)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| self.breadcrumb(ui, actions),
                );
                if search_box(ui, search).clicked() {
                    actions.push(Action::OpenPalette);
                }
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(right)
                        .layout(Layout::right_to_left(Align::Center)),
                    |ui| self.status(ui, actions),
                );
            });
    }

    fn breadcrumb(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !self.found {
            ui.label(egui::RichText::new("Kerosene").strong().color(colors::TEXT));
            return;
        }
        let project = ui.add(
            egui::Button::new(
                egui::RichText::new(&self.info.name)
                    .strong()
                    .color(colors::TEXT),
            )
            .frame(false),
        );
        if project.on_hover_text("Home").clicked() {
            actions.push(Action::Goto(Tab::Home));
        }
        ui.label(
            theme::icon(icons::CARET_RIGHT)
                .size(11.0)
                .color(colors::TEXT_FAINT),
        );
        let here = if self.showing_start {
            "Projects"
        } else {
            self.tab.name()
        };
        ui.label(egui::RichText::new(here).color(colors::TEXT_MUTED));
        if !self.showing_start && self.tab == Tab::Editor && self.editor.document.is_modified() {
            widgets::badge(ui, "unsaved", widgets::Tone::Warn);
        }
    }

    /// Play, and the running jobs, from the right edge inwards.
    fn status(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ui.spacing_mut().item_spacing.x = 8.0;
        if self.found {
            let playing = self.play.as_ref().is_some_and(Job::running);
            if playing {
                if widgets::button(ui, ButtonKind::Danger, icons::STOP, "Stop")
                    .on_hover_text("End the play session")
                    .clicked()
                {
                    actions.push(Action::Cancel(JobKind::Play));
                }
            } else if widgets::button(ui, ButtonKind::Primary, icons::PLAY, "Play")
                .on_hover_text("Build what changed, then run the game")
                .clicked()
            {
                actions.push(Action::Play);
            }
        }
        for (job, kind) in [
            (self.archive.job.as_ref(), JobKind::Archive),
            (self.build.job.as_ref(), JobKind::Build),
        ] {
            let Some(job) = job else { continue };
            let recent = job.finished_ago().is_none_or(|ago| ago < VERDICT);
            if recent && let Some(action) = job_pill(ui, job, kind) {
                actions.push(action);
            }
        }
    }
}

/// The box in the middle that opens the palette.
fn search_box(ui: &mut Ui, rect: Rect) -> egui::Response {
    let response = ui.interact(rect, ui.id().with("topbar-search"), Sense::click());
    let painter = ui.painter();
    let radius = egui::CornerRadius::same(theme::RADIUS + 1);
    painter.rect_filled(
        rect,
        radius,
        if response.hovered() {
            colors::BG_HEADER
        } else {
            colors::BG_FIELD
        },
    );
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(
            1.0_f32,
            if response.hovered() {
                colors::BORDER_STRONG
            } else {
                colors::BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        Align2::CENTER_CENTER,
        icons::MAGNIFYING_GLASS,
        FontId::proportional(14.0),
        colors::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(rect.left() + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Search maps, assets and commands",
        FontId::proportional(12.5),
        colors::TEXT_FAINT,
    );
    let keys = Rect::from_min_max(
        egui::pos2(rect.right() - 90.0, rect.top()),
        rect.max - Vec2::new(8.0, 0.0),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(keys)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| widgets::kbd(ui, "ctrl-P"),
    );
    response.on_hover_text("Search everything  (ctrl-P)")
}

/// A running or just-finished job, as a rounded pill.
fn job_pill(ui: &mut Ui, job: &Job, kind: JobKind) -> Option<Action> {
    let mut action = None;
    let (fill, stroke) = match job.outcome() {
        None => (colors::ACCENT_SOFT, colors::ACCENT.gamma_multiply(0.5)),
        Some(true) => (
            colors::ERR.gamma_multiply(0.14),
            colors::ERR.gamma_multiply(0.45),
        ),
        Some(false) => (
            colors::OK.gamma_multiply(0.14),
            colors::OK.gamma_multiply(0.45),
        ),
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0_f32, stroke))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if job.running()
                    && widgets::icon_button(ui, icons::X, &format!("Cancel {}", job.label()))
                        .clicked()
                {
                    action = Some(Action::Cancel(kind));
                }
                let text = match job.outcome() {
                    None => format!(
                        "{} · {}",
                        job.label(),
                        widgets::human_elapsed(job.elapsed())
                    ),
                    Some(_) if job.cancelled() => format!("{} cancelled", job.label()),
                    Some(true) => format!("{} failed", job.label()),
                    Some(false) => format!("{} done", job.label()),
                };
                let clicked = ui
                    .add(
                        egui::Label::new(egui::RichText::new(text).size(12.0).color(colors::TEXT))
                            .sense(Sense::click()),
                    )
                    .on_hover_text("Show the log")
                    .clicked();
                if clicked {
                    action = Some(Action::ShowOutput);
                }
                match job.outcome() {
                    None => {
                        ui.add(egui::Spinner::new().size(12.0).color(colors::ACCENT));
                    }
                    Some(true) => {
                        ui.label(theme::icon(icons::X_CIRCLE).color(colors::ERR));
                    }
                    Some(false) => {
                        ui.label(theme::icon(icons::CHECK_CIRCLE).color(colors::OK));
                    }
                }
            });
        });
    action
}
