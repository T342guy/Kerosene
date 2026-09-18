// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The output panel: one place at the bottom of the window where every job
//! writes its log.
//!
//! A compile, a build and an archive pack are all the same thing to a person
//! waiting on them -- a stream of lines ending in "done" or a reason -- and
//! they used to be read in three different places, one of them a floating
//! window that covered the map it was compiling. Now each is a *source* the
//! panel can show, the way an IDE's output pane has a dropdown of who is
//! talking.
//!
//! The panel owns no log. A job's lines live with the job, and the panel is
//! handed a view of them every frame, so a tool never has to know the panel
//! exists in order to be shown in it.

use std::borrow::Cow;

use egui::{Align, Layout, Ui};

use crate::theme::{self, colors, icons};
use crate::widgets;

/// What kind of line this is, which decides its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Stage,
    Warn,
    Error,
    Ok,
}

impl Level {
    /// Guess a line's level from its text. The stages all print through the
    /// same logger, so `warning:` and `error:` are dependable; anything
    /// else is information.
    pub fn of(text: &str) -> Level {
        let trimmed = text.trim_start();
        let lower: String = trimmed.chars().take(12).collect::<String>().to_lowercase();
        if lower.starts_with("error") || lower.starts_with("failed") || lower.starts_with("panic") {
            Level::Error
        } else if lower.starts_with("warn") {
            Level::Warn
        } else if trimmed.starts_with("--- ") || trimmed.starts_with("==> ") {
            Level::Stage
        } else if lower.starts_with("done") || lower.starts_with("finished") {
            Level::Ok
        } else {
            Level::Info
        }
    }

    fn colour(self) -> egui::Color32 {
        match self {
            Level::Info => colors::TEXT,
            Level::Stage => colors::INFO,
            Level::Warn => colors::WARN,
            Level::Error => colors::ERR,
            Level::Ok => colors::OK,
        }
    }
}

/// One line of a log, as the panel sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Line<'a> {
    pub level: Level,
    pub text: Cow<'a, str>,
}

impl<'a> Line<'a> {
    /// A line whose level is read off its text.
    pub fn classified(text: &'a str) -> Line<'a> {
        Line {
            level: Level::of(text),
            text: Cow::Borrowed(text),
        }
    }

    pub fn new(level: Level, text: impl Into<Cow<'a, str>>) -> Line<'a> {
        Line {
            level,
            text: text.into(),
        }
    }
}

/// A job's log, offered to the panel for one frame.
pub struct Source<'a> {
    /// Compile, Build, Archive.
    pub name: &'a str,
    pub lines: Vec<Line<'a>>,
    pub running: bool,
    /// `Some(true)` when it finished badly, `Some(false)` when it finished
    /// well, `None` while it is still going or never ran.
    pub failed: Option<bool>,
}

/// What the panel remembers between frames.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputPanel {
    pub open: bool,
    /// Which source is showing.
    pub selected: usize,
    /// Preferred height, in points, so collapsing and reopening it comes
    /// back the same size.
    pub height: f32,
}

impl Default for OutputPanel {
    fn default() -> OutputPanel {
        OutputPanel {
            open: false,
            selected: 0,
            height: 180.0,
        }
    }
}

/// What the person did with the panel this frame, for the owner to act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct OutputAction {
    /// Clear the log of this source.
    pub clear: Option<usize>,
}

impl OutputPanel {
    /// Bring the panel up on this source: a job started.
    pub fn show_source(&mut self, index: usize) {
        self.open = true;
        self.selected = index;
    }

    /// Draw the panel across the bottom of the window, when it is open.
    pub fn ui(&mut self, ctx: &egui::Context, sources: &[Source<'_>]) -> OutputAction {
        let mut action = OutputAction::default();
        if !self.open || sources.is_empty() {
            return action;
        }
        self.selected = self.selected.min(sources.len() - 1);

        let response = egui::TopBottomPanel::bottom("kerosene-output")
            .resizable(true)
            .default_height(self.height)
            .min_height(60.0)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_APP)
                    .stroke(egui::Stroke::new(1.0_f32, colors::BORDER)),
            )
            .show(ctx, |ui| {
                self.header(ui, sources, &mut action);
                let source = &sources[self.selected];
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        lines(ui, source);
                    });
            });
        self.height = response.response.rect.height();
        action
    }

    fn header(&mut self, ui: &mut Ui, sources: &[Source<'_>], action: &mut OutputAction) {
        egui::Frame::new()
            .fill(colors::BG_PANEL)
            .inner_margin(egui::Margin::symmetric(8, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::section_title("output"));
                    ui.add_space(8.0);
                    for (index, source) in sources.iter().enumerate() {
                        let current = index == self.selected;
                        let mark = if source.running {
                            icons::CIRCLE_NOTCH
                        } else {
                            match source.failed {
                                Some(true) => icons::X_CIRCLE,
                                Some(false) => icons::CHECK_CIRCLE,
                                None => icons::CIRCLE,
                            }
                        };
                        let colour = if source.running {
                            colors::ACCENT
                        } else {
                            match source.failed {
                                Some(true) => colors::ERR,
                                Some(false) => colors::OK,
                                None => colors::TEXT_MUTED,
                            }
                        };
                        let text = egui::RichText::new(format!("{mark} {}", source.name))
                            .size(12.0)
                            .color(if current { egui::Color32::WHITE } else { colour });
                        if ui.selectable_label(current, text).clicked() {
                            self.selected = index;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::icon_button(ui, icons::X, "hide the output panel").clicked() {
                            self.open = false;
                        }
                        if widgets::icon_button(ui, icons::TRASH, "clear this log").clicked() {
                            action.clear = Some(self.selected);
                        }
                        let source = &sources[self.selected];
                        if source.running {
                            ui.add(egui::Spinner::new().size(12.0).color(colors::ACCENT));
                            ui.label(theme::caption("running"));
                        } else {
                            ui.label(theme::caption(format!("{} lines", source.lines.len())));
                        }
                    });
                });
            });
    }
}

fn lines(ui: &mut Ui, source: &Source<'_>) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if source.lines.is_empty() {
                ui.label(theme::caption(if source.running {
                    "waiting for output..."
                } else {
                    "nothing yet"
                }));
            }
            for line in &source.lines {
                let text = egui::RichText::new(line.text.as_ref())
                    .monospace()
                    .size(11.5)
                    .color(line.level.colour());
                ui.label(if line.level == Level::Stage {
                    text.strong()
                } else {
                    text
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_classified_by_how_they_start() {
        assert_eq!(Level::of("error: map leaks"), Level::Error);
        assert_eq!(Level::of("  Error: leaks"), Level::Error);
        assert_eq!(Level::of("FAILED: no such file"), Level::Error);
        assert_eq!(Level::of("warning: 3 materials unbuilt"), Level::Warn);
        assert_eq!(Level::of("--- cleave ---"), Level::Stage);
        assert_eq!(Level::of("done: maps/arena.kerobsp"), Level::Ok);
        assert_eq!(Level::of("compiling 12 brushes"), Level::Info);
    }

    #[test]
    fn the_panel_draws_and_clears() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut panel = OutputPanel::default();
        panel.show_source(1);
        let sources = vec![
            Source {
                name: "Compile",
                lines: vec![],
                running: false,
                failed: None,
            },
            Source {
                name: "Build",
                lines: vec![Line::classified("--- textures ---"), Line::classified("ok")],
                running: true,
                failed: None,
            },
        ];
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let action = panel.ui(ctx, &sources);
            assert_eq!(action.clear, None);
            egui::CentralPanel::default().show(ctx, |_| {});
        });
        assert!(!output.shapes.is_empty());
        assert_eq!(panel.selected, 1);
        assert!(panel.open);
    }

    #[test]
    fn a_closed_panel_draws_nothing_and_asks_nothing() {
        let ctx = egui::Context::default();
        let mut panel = OutputPanel::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let action = panel.ui(ctx, &[]);
            assert_eq!(action, OutputAction::default());
        });
    }
}
