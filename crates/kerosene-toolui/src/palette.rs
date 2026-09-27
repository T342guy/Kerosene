// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! A command palette: type a few letters of anything, press Enter.
//!
//! A toolset with seven tabs, a hundred materials and a dozen jobs has more
//! things to do than it has room for buttons, and the person who knows the
//! name of the map they want should not have to know which tab lists it.
//! The palette is one list of everything the host can do -- open a map, run
//! a build, switch a tab -- filtered as you type by a forgiving match:
//! `bfa` finds "Build (fast)", `arena` finds `maps/dm/arena.kmap`.
//!
//! The palette knows nothing about what a command does. The host hands it a
//! list each frame and gets back the id of the one that was chosen, the same
//! way the output panel is handed its logs.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Align2, Color32, FontId, Key, Layout, RichText, Sense, Vec2};

use crate::theme::{self, colors, icons};
use crate::widgets;

/// One thing the palette can run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// What the host gets back when this is chosen.
    pub id: String,
    /// What the list shows, and what typing matches first.
    pub title: String,
    /// A quieter second line: a path, what the command does. Matched too,
    /// less eagerly than the title.
    pub detail: String,
    pub glyph: &'static str,
    pub shortcut: Option<&'static str>,
    /// A tag on the right of the row: "Map", "Build", "Go to".
    pub group: &'static str,
}

impl Command {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        glyph: &'static str,
        group: &'static str,
    ) -> Command {
        Command {
            id: id.into(),
            title: title.into(),
            detail: String::new(),
            glyph,
            shortcut: None,
            group,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Command {
        self.detail = detail.into();
        self
    }

    pub fn shortcut(mut self, shortcut: &'static str) -> Command {
        self.shortcut = Some(shortcut);
        self
    }
}

/// How well a query matched a text, and which characters it matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub score: i32,
    /// Indices, in characters, of the text's matched characters.
    pub positions: Vec<usize>,
}

/// Match `query` against `text` as a subsequence, ignoring case and spaces
/// in the query. `None` when some letter of the query is not there.
///
/// Letters that start a word, and letters that follow the previous match
/// directly, score higher than letters found in the middle of a word after
/// a gap, so `bf` ranks "Build (fast)" above "buffer".
pub fn fuzzy_match(query: &str, text: &str) -> Option<Match> {
    let needle: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if needle.is_empty() {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }
    let hay: Vec<char> = text.chars().collect();
    let lower: Vec<char> = hay
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let starts: Vec<bool> = (0..hay.len())
        .map(|i| {
            i == 0 || {
                let prev = hay[i - 1];
                !prev.is_alphanumeric() || (prev.is_lowercase() && hay[i].is_uppercase())
            }
        })
        .collect();

    // Two greedy passes -- one that reaches for the next word start when
    // there is one, one that takes the nearest letter -- and the better
    // score of the two. Cheaper than a full alignment and right in every
    // case that comes up with names like these.
    let passes = [true, false];
    passes
        .iter()
        .filter_map(|&prefer_starts| walk(&needle, &lower, &starts, prefer_starts))
        .map(|positions| {
            let score = score(&positions, &starts, hay.len());
            Match { score, positions }
        })
        .max_by_key(|m| m.score)
}

fn walk(needle: &[char], hay: &[char], starts: &[bool], prefer_starts: bool) -> Option<Vec<usize>> {
    let mut positions = Vec::with_capacity(needle.len());
    let mut from = 0;
    for (n, &c) in needle.iter().enumerate() {
        let nearest = (from..hay.len()).find(|&i| hay[i] == c)?;
        let continues = positions.last().is_some_and(|&p: &usize| p + 1 == nearest);
        let chosen = if prefer_starts && !continues && !starts[nearest] {
            // A later word start with this letter, if the rest of the query
            // still fits after it.
            (nearest..hay.len())
                .find(|&i| hay[i] == c && starts[i])
                .filter(|&i| {
                    walk(&needle[n + 1..], &hay[i + 1..], &starts[i + 1..], false).is_some()
                })
                .unwrap_or(nearest)
        } else {
            nearest
        };
        positions.push(chosen);
        from = chosen + 1;
    }
    Some(positions)
}

fn score(positions: &[usize], starts: &[bool], len: usize) -> i32 {
    let mut score = 0;
    let mut previous: Option<usize> = None;
    for &p in positions {
        score += 10;
        if starts[p] {
            score += 12;
        }
        match previous {
            Some(q) if q + 1 == p => score += 14,
            Some(q) => score -= ((p - q - 1) as i32).min(8),
            None => score -= (p as i32).min(10),
        }
        previous = Some(p);
    }
    // Between two equally good matches, the shorter text is the better one:
    // "Build" before "Build and pack everything".
    score - (len as i32 / 8)
}

/// One row of the palette's list: which command, and how it matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    pub index: usize,
    pub score: i32,
    /// Matched characters of the title; empty when it matched on its detail.
    pub positions: Vec<usize>,
}

/// The commands that match `query`, best first. With no query, all of them
/// in the order given.
pub fn rank(query: &str, commands: &[Command]) -> Vec<Ranked> {
    let mut ranked: Vec<Ranked> = commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| {
            if let Some(m) = fuzzy_match(query, &command.title) {
                return Some(Ranked {
                    index,
                    score: m.score,
                    positions: m.positions,
                });
            }
            fuzzy_match(query, &command.detail).map(|m| Ranked {
                index,
                score: m.score - 25,
                positions: Vec::new(),
            })
        })
        .collect();
    // Stable, so equal scores keep the host's order.
    ranked.sort_by_key(|r| std::cmp::Reverse(r.score));
    ranked
}

/// The palette's state between frames.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    open: bool,
    query: String,
    selected: usize,
    /// The selection moved by key this frame, so the list should scroll to
    /// it; a mouse hover moving it should not.
    scroll_to_selected: bool,
}

impl Palette {
    /// Show the palette, empty, with the first command selected.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.scroll_to_selected = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Type into the palette. For hosts that open it pre-filled, and tests.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.selected = 0;
    }

    /// Draw the palette over everything, when it is open. Returns the id of
    /// the command chosen this frame, and closes itself when one is.
    pub fn ui(&mut self, ctx: &egui::Context, commands: &[Command]) -> Option<String> {
        if !self.open {
            return None;
        }
        let ranked = rank(&self.query, commands);
        self.selected = self.selected.min(ranked.len().saturating_sub(1));

        // The keys are taken before the text field sees them: up and down
        // move the selection rather than the cursor, Enter runs rather than
        // defocusing.
        let mut chosen = None;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                self.open = false;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
                && self.selected + 1 < ranked.len()
            {
                self.selected += 1;
                self.scroll_to_selected = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) && self.selected > 0 {
                self.selected -= 1;
                self.scroll_to_selected = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Enter) {
                chosen = ranked.get(self.selected).map(|r| r.index);
            }
        });
        if !self.open {
            return None;
        }

        let id = egui::Id::new("kerosene-palette");
        let modal = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 90.0)))
            .backdrop_color(Color32::from_black_alpha(110))
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_ELEVATED)
                    .stroke(egui::Stroke::new(1.0_f32, colors::BORDER_STRONG))
                    .corner_radius(egui::CornerRadius::same(10))
                    .shadow(ctx.style().visuals.window_shadow),
            )
            .show(ctx, |ui| {
                ui.set_width(620.0);
                self.search_row(ui);
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().top(),
                    egui::Stroke::new(1.0_f32, colors::BORDER),
                );
                if let Some(index) = self.results(ui, commands, &ranked) {
                    chosen = Some(index);
                }
                footer(ui, ranked.len());
            });
        if modal.should_close() {
            self.open = false;
        }

        let chosen = chosen.map(|index| commands[index].id.clone());
        if chosen.is_some() {
            self.open = false;
        }
        chosen
    }

    fn search_row(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        theme::icon(icons::MAGNIFYING_GLASS)
                            .size(18.0)
                            .color(colors::ACCENT),
                    );
                    let before = self.query.clone();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text(
                                RichText::new("Search maps, assets and commands")
                                    .color(colors::TEXT_FAINT),
                            )
                            .font(FontId::proportional(16.0))
                            .frame(false)
                            .desired_width(f32::INFINITY),
                    );
                    response.request_focus();
                    if self.query != before {
                        self.selected = 0;
                        self.scroll_to_selected = true;
                    }
                });
            });
    }

    fn results(
        &mut self,
        ui: &mut egui::Ui,
        commands: &[Command],
        ranked: &[Ranked],
    ) -> Option<usize> {
        let mut chosen = None;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                if ranked.is_empty() {
                    widgets::empty_state(
                        ui,
                        icons::MAGNIFYING_GLASS,
                        "Nothing matches",
                        "Try fewer letters, or a different word.",
                    );
                    return;
                }
                let row_height = 40.0;
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .auto_shrink([false, true])
                    .show_rows(ui, row_height, ranked.len(), |ui, rows| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for position in rows {
                            let entry = &ranked[position];
                            let command = &commands[entry.index];
                            let selected = position == self.selected;
                            let response = row(ui, command, &entry.positions, selected, row_height);
                            if selected && self.scroll_to_selected {
                                response.scroll_to_me(None);
                            }
                            if response.hovered() && ui.input(|i| i.pointer.delta() != Vec2::ZERO) {
                                self.selected = position;
                            }
                            if response.clicked() {
                                chosen = Some(entry.index);
                            }
                        }
                    });
                self.scroll_to_selected = false;
            });
        chosen
    }
}

/// One command in the list.
fn row(
    ui: &mut egui::Ui,
    command: &Command,
    matched: &[usize],
    selected: bool,
    height: f32,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter().clone();
    if selected {
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(theme::RADIUS + 1),
            colors::ACCENT_SOFT,
        );
    }
    painter.text(
        egui::pos2(rect.left() + 20.0, rect.center().y),
        Align2::CENTER_CENTER,
        command.glyph,
        FontId::proportional(16.0),
        if selected {
            colors::ACCENT
        } else {
            colors::TEXT_MUTED
        },
    );

    // The title, with the letters the query matched in the accent.
    let mut job = LayoutJob::default();
    for (index, c) in command.title.chars().enumerate() {
        let hit = matched.contains(&index);
        job.append(
            c.encode_utf8(&mut [0; 4]),
            0.0,
            TextFormat {
                font_id: FontId::proportional(13.5),
                color: if hit { colors::ACCENT } else { colors::TEXT },
                ..Default::default()
            },
        );
    }
    let title = ui.fonts(|f| f.layout_job(job));
    let text_left = rect.left() + 42.0;
    let title_y = if command.detail.is_empty() {
        rect.center().y - title.size().y / 2.0
    } else {
        rect.top() + 5.0
    };
    painter.galley(egui::pos2(text_left, title_y), title, colors::TEXT);
    if !command.detail.is_empty() {
        let detail = ui.fonts(|f| {
            f.layout(
                command.detail.clone(),
                FontId::proportional(11.0),
                colors::TEXT_MUTED,
                rect.width() - 200.0,
            )
        });
        painter.galley(
            egui::pos2(text_left, rect.bottom() - 17.0),
            detail,
            colors::TEXT_MUTED,
        );
    }

    // The group tag and the shortcut, on the right.
    let right = egui::Rect::from_min_max(
        egui::pos2(rect.right() - 190.0, rect.top()),
        rect.max - Vec2::new(10.0, 0.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(right)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            if let Some(shortcut) = command.shortcut {
                widgets::kbd(ui, shortcut);
            }
            ui.label(theme::caption(command.group).color(colors::TEXT_FAINT));
        },
    );
    response
}

fn footer(ui: &mut egui::Ui, count: usize) {
    egui::Frame::new()
        .fill(colors::BG_PANEL)
        .corner_radius(egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: 10,
            se: 10,
        })
        .inner_margin(egui::Margin::symmetric(14, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                widgets::kbd(ui, "↑");
                widgets::kbd(ui, "↓");
                ui.label(theme::caption("move"));
                ui.add_space(8.0);
                widgets::kbd(ui, "enter");
                ui.label(theme::caption("run"));
                ui.add_space(8.0);
                widgets::kbd(ui, "esc");
                ui.label(theme::caption("close"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::caption(format!("{count} results")));
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands() -> Vec<Command> {
        vec![
            Command::new("goto:build", "Go to Build", icons::HAMMER, "Go to").shortcut("ctrl-6"),
            Command::new("build", "Build", icons::HAMMER, "Build"),
            Command::new("build:fast", "Build (fast)", icons::LIGHTNING, "Build"),
            Command::new("map:arena", "arena", icons::MAP_TRIFOLD, "Map")
                .detail("maps/dm/arena.kmap"),
            Command::new("buffer", "buffer overrun test", icons::BUG, "Debug"),
        ]
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn a_subsequence_matches_and_a_missing_letter_does_not() {
        assert!(fuzzy_match("bld", "Build").is_some());
        assert!(fuzzy_match("B L D", "build").is_some());
        assert!(fuzzy_match("bx", "Build").is_none());
        assert_eq!(fuzzy_match("", "anything").unwrap().score, 0);
        assert_eq!(fuzzy_match("ar", "arena").unwrap().positions, vec![0, 1]);
    }

    #[test]
    fn word_starts_beat_letters_in_the_middle() {
        let fast = fuzzy_match("bf", "Build (fast)").unwrap();
        let buffer = fuzzy_match("bf", "buffer overrun test").unwrap();
        assert!(fast.score > buffer.score, "{fast:?} vs {buffer:?}");
        assert_eq!(fast.positions, vec![0, 7], "the f of fast, not of nothing");
    }

    #[test]
    fn camel_case_humps_count_as_word_starts() {
        let m = fuzzy_match("ts", "ToolSet").unwrap();
        assert_eq!(m.positions, vec![0, 4]);
    }

    #[test]
    fn ranking_puts_the_best_match_first_and_falls_back_to_the_detail() {
        let list = commands();
        let ranked = rank("build", &list);
        assert_eq!(
            list[ranked[0].index].id, "build",
            "shortest exact title first"
        );
        let by_path = rank("dm/arena", &list);
        assert_eq!(list[by_path[0].index].id, "map:arena");
        assert!(by_path[0].positions.is_empty(), "matched on its detail");
        assert_eq!(rank("", &list).len(), list.len());
        assert!(rank("zzz", &list).is_empty());
    }

    #[test]
    fn arrows_and_enter_choose_a_command() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let list = commands();
        let mut palette = Palette::default();
        palette.open();
        palette.set_query("build");
        let input = egui::RawInput {
            events: vec![key(Key::ArrowDown), key(Key::Enter)],
            ..Default::default()
        };
        let mut chosen = None;
        let _ = ctx.run(input, |ctx| chosen = palette.ui(ctx, &list));
        let ranked = rank("build", &list);
        assert_eq!(chosen.as_deref(), Some(list[ranked[1].index].id.as_str()));
        assert!(!palette.is_open(), "running a command closes the palette");
    }

    #[test]
    fn escape_closes_without_choosing() {
        let ctx = egui::Context::default();
        let list = commands();
        let mut palette = Palette::default();
        palette.open();
        let input = egui::RawInput {
            events: vec![key(Key::Escape)],
            ..Default::default()
        };
        let mut chosen = Some(String::new());
        let _ = ctx.run(input, |ctx| chosen = palette.ui(ctx, &list));
        assert_eq!(chosen, None);
        assert!(!palette.is_open());
    }

    #[test]
    fn an_open_palette_draws() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let list = commands();
        let mut palette = Palette::default();
        palette.toggle();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            assert_eq!(palette.ui(ctx, &list), None);
        });
        assert!(!output.shapes.is_empty());
        assert!(palette.is_open());
        palette.set_query("nothing like it");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            palette.ui(ctx, &list);
        });
    }
}
