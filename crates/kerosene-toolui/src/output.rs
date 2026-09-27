// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The output panel: one place at the bottom of the window where every job
//! writes its log.
//!
//! A compile, a build and an archive pack are all the same thing to a person
//! waiting on them -- a stream of lines ending in "done" or a reason -- and
//! they used to be read in three different places, one of them a floating
//! window that covered the map it was compiling. Now each is a *source* the
//! panel can show, the way an IDE's output pane has a tab per speaker.
//!
//! The panel owns no log. A job's lines live with the job, and the panel is
//! handed a view of them every frame, so a tool never has to know the panel
//! exists in order to be shown in it.
//!
//! A full content build prints thousands of lines, and the one that matters
//! is usually a warning somewhere in the middle. So the panel only lays out
//! the rows that are on screen, counts the errors and warnings of each
//! source in its tab, and can narrow a log to its problems or to the lines
//! matching a search.

use std::borrow::Cow;

use egui::{Align, Layout, Ui};

use crate::theme::{self, colors, icons};
use crate::widgets::{self, Tone};

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

    fn is_problem(self) -> bool {
        matches!(self, Level::Warn | Level::Error)
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

impl Source<'_> {
    /// How many errors and warnings the log holds.
    pub fn problems(&self) -> (usize, usize) {
        self.lines
            .iter()
            .fold((0, 0), |(e, w), line| match line.level {
                Level::Error => (e + 1, w),
                Level::Warn => (e, w + 1),
                _ => (e, w),
            })
    }
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
    /// Only lines containing this, ignoring case. Empty shows everything.
    pub filter: String,
    /// Only errors and warnings.
    pub problems_only: bool,
}

impl Default for OutputPanel {
    fn default() -> OutputPanel {
        OutputPanel {
            open: false,
            selected: 0,
            height: 200.0,
            filter: String::new(),
            problems_only: false,
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

    /// Which of `source`'s lines pass the panel's filters, by index.
    pub fn visible(&self, source: &Source<'_>) -> Vec<usize> {
        let needle = self.filter.trim().to_lowercase();
        source
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !self.problems_only || line.level.is_problem())
            .filter(|(_, line)| needle.is_empty() || line.text.to_lowercase().contains(&needle))
            .map(|(index, _)| index)
            .collect()
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
            .min_height(90.0)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_APP)
                    .stroke(egui::Stroke::new(1.0_f32, colors::BORDER)),
            )
            .show(ctx, |ui| {
                self.header(ui, sources, &mut action);
                let source = &sources[self.selected];
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| self.lines(ui, source));
            });
        self.height = response.response.rect.height();
        action
    }

    fn header(&mut self, ui: &mut Ui, sources: &[Source<'_>], action: &mut OutputAction) {
        egui::Frame::new()
            .fill(colors::BG_PANEL)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::section_title("output"));
                    ui.add_space(theme::SPACE_SM);
                    for (index, source) in sources.iter().enumerate() {
                        self.source_tab(ui, index, source);
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::icon_button(ui, icons::X, "hide the output panel  (ctrl-`)")
                            .clicked()
                        {
                            self.open = false;
                        }
                        if widgets::icon_button(ui, icons::TRASH, "clear this log").clicked() {
                            action.clear = Some(self.selected);
                        }
                        let source = &sources[self.selected];
                        if widgets::icon_button(ui, icons::COPY, "copy this log").clicked() {
                            let text: Vec<&str> =
                                source.lines.iter().map(|l| l.text.as_ref()).collect();
                            ui.ctx().copy_text(text.join("\n"));
                        }
                        widgets::icon_toggle(
                            ui,
                            icons::WARNING,
                            "only errors and warnings",
                            &mut self.problems_only,
                        );
                        ui.add_space(theme::SPACE_XS);
                        filter_field(ui, &mut self.filter);
                    });
                });
            });
    }

    fn source_tab(&mut self, ui: &mut Ui, index: usize, source: &Source<'_>) {
        let current = index == self.selected;
        let (mark, colour) = if source.running {
            (icons::CIRCLE_NOTCH, colors::ACCENT)
        } else {
            match source.failed {
                Some(true) => (icons::X_CIRCLE, colors::ERR),
                Some(false) => (icons::CHECK_CIRCLE, colors::OK),
                None => (icons::CIRCLE, colors::TEXT_FAINT),
            }
        };
        let text = egui::RichText::new(format!("{mark}  {}", source.name))
            .size(12.0)
            .color(if current { colors::TEXT } else { colour });
        let response = ui.selectable_label(current, text);
        if response.clicked() {
            self.selected = index;
        }
        let (errors, warnings) = source.problems();
        if errors > 0 {
            widgets::badge(ui, &errors.to_string(), Tone::Err)
                .on_hover_text(format!("{errors} error(s)"));
        }
        if warnings > 0 {
            widgets::badge(ui, &warnings.to_string(), Tone::Warn)
                .on_hover_text(format!("{warnings} warning(s)"));
        }
        ui.add_space(theme::SPACE_XS);
    }

    fn lines(&self, ui: &mut Ui, source: &Source<'_>) {
        let visible = self.visible(source);
        if visible.is_empty() {
            let why = if source.lines.is_empty() {
                if source.running {
                    "waiting for output..."
                } else {
                    "nothing yet"
                }
            } else {
                "no line matches the filter"
            };
            ui.label(theme::caption(why));
            return;
        }
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let row_height = ui.fonts(|f| f.row_height(&font)) + 1.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show_rows(ui, row_height, visible.len(), |ui, rows| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for &index in &visible[rows] {
                    let line = &source.lines[index];
                    let text = egui::RichText::new(line.text.as_ref())
                        .monospace()
                        .color(line.level.colour());
                    let text = if line.level == Level::Stage {
                        text.strong()
                    } else {
                        text
                    };
                    ui.add(egui::Label::new(text).truncate());
                }
            });
    }
}

/// The small search box in the panel's header.
fn filter_field(ui: &mut Ui, filter: &mut String) {
    ui.add(
        egui::TextEdit::singleline(filter)
            .hint_text(
                egui::RichText::new(format!("{}  filter", icons::FUNNEL)).color(colors::TEXT_FAINT),
            )
            .desired_width(170.0),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_log() -> Source<'static> {
        Source {
            name: "Build",
            lines: vec![
                Line::classified("--- textures ---"),
                Line::classified("warning: grid.png has no mips"),
                Line::classified("compiled dev/grid"),
                Line::classified("error: arena leaks"),
                Line::classified("done"),
            ],
            running: false,
            failed: Some(true),
        }
    }

    #[test]
    fn lines_are_classified_by_how_they_start() {
        assert_eq!(Level::of("error: map leaks"), Level::Error);
        assert_eq!(Level::of("  Error: leaks"), Level::Error);
        assert_eq!(Level::of("FAILED: no such file"), Level::Error);
        assert_eq!(Level::of("warning: 3 materials unbuilt"), Level::Warn);
        assert_eq!(Level::of("--- cleave ---"), Level::Stage);
        assert_eq!(Level::of("done: maps/arena.kbsp"), Level::Ok);
        assert_eq!(Level::of("compiling 12 brushes"), Level::Info);
    }

    #[test]
    fn problems_are_counted_and_can_be_shown_alone() {
        let source = build_log();
        assert_eq!(source.problems(), (1, 1));
        let mut panel = OutputPanel::default();
        assert_eq!(panel.visible(&source).len(), 5);
        panel.problems_only = true;
        assert_eq!(panel.visible(&source), vec![1, 3]);
    }

    #[test]
    fn the_filter_ignores_case() {
        let source = build_log();
        let panel = OutputPanel {
            filter: "ARENA".into(),
            ..Default::default()
        };
        assert_eq!(panel.visible(&source), vec![3]);
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
            build_log(),
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
