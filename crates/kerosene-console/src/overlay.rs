// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The developer console's state, without any drawing.
//!
//! A console is mostly not a text box. It is history you can walk back
//! through, completion that gets you to a convar you half remember, and
//! scrollback you can page through while the game keeps running. All of that
//! is a state machine over keystrokes, and none of it needs a window -- so it
//! lives here and is tested here, and the host only draws what it says.
//!
//! The behaviours are the ones every Quake-lineage console has had, because
//! they are the ones anyone opening a console already knows:
//!
//! * Up and down walk history, and the half-typed line you walked away from
//!   comes back when you walk past the end of it again.
//! * Tab completes as far as the candidates agree; pressing it again cycles
//!   through them.
//! * Page up and down scroll the log without disturbing what you are typing.

use crate::Console;

/// How many lines a page-up moves.
const PAGE: usize = 10;

/// The console overlay's state.
#[derive(Debug, Default)]
pub struct ConsoleUi {
    pub open: bool,
    /// The line being typed.
    pub input: String,
    /// How far back through history we have walked; `None` means "at the
    /// line I am typing".
    history_index: Option<usize>,
    /// The half-typed line set aside while walking history.
    draft: String,
    /// Completion candidates for the current prefix, and which one is showing.
    completions: Vec<String>,
    completion_index: usize,
    /// What the input was when the completions were worked out. Any other
    /// edit invalidates them.
    completion_source: String,
    /// Whether the console has introduced itself yet.
    greeted: bool,
    /// Lines scrolled back from the bottom. Zero means pinned to the newest.
    pub scroll: usize,
}

impl ConsoleUi {
    pub fn new() -> ConsoleUi { ConsoleUi::default() }

    /// Say what the console is, the first time it is opened.
    ///
    /// An empty console with a blinking cursor tells you nothing about what
    /// it accepts, and "there are two hundred of them, here is how to search"
    /// is not something anybody guesses. Once per session: after that it is
    /// noise between you and the output you opened the console to read.
    pub fn greet(&mut self, console: &mut Console) {
        if self.greeted { return }
        self.greeted = true;
        console.print(format!(
            "Kerosene console -- {} commands and convars. \
             `find <text>` searches them, `help <name>` explains one, \
             `cvarlist` lists the lot. Tab completes, up walks back, \
             ` or escape closes.",
            console.name_count(),
        ));
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open { self.scroll = 0; }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Candidates for the current input, if tab has been pressed.
    pub fn completions(&self) -> &[String] { &self.completions }

    /// Type into the line. Any edit drops a stale completion cycle.
    pub fn set_input(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text != self.completion_source { self.completions.clear(); }
        self.input = text;
    }

    /// Walk back through history. The line being typed is kept and comes
    /// back on the way down.
    pub fn history_previous(&mut self, console: &Console) {
        let history = console.history();
        if history.is_empty() { return }
        let next = match self.history_index {
            None => {
                self.draft = std::mem::take(&mut self.input);
                history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_index = Some(next);
        self.input = history[next].clone();
        self.completions.clear();
    }

    /// Walk forward again, ending at the half-typed line.
    pub fn history_next(&mut self, console: &Console) {
        let history = console.history();
        let Some(index) = self.history_index else { return };
        if index + 1 >= history.len() {
            self.history_index = None;
            self.input = std::mem::take(&mut self.draft);
        } else {
            self.history_index = Some(index + 1);
            self.input = history[index + 1].clone();
        }
        self.completions.clear();
    }

    /// Complete the current word.
    ///
    /// The first press fills in as far as every candidate agrees, which is
    /// the press that does the work. Pressing again cycles, because the times
    /// you cannot remember a name you also cannot remember which of four it
    /// was.
    pub fn complete(&mut self, console: &Console) {
        if !self.completions.is_empty() && self.input == self.completion_source {
            self.completion_index = (self.completion_index + 1) % self.completions.len();
            self.input = self.completions[self.completion_index].clone();
            self.completion_source = self.input.clone();
            return;
        }

        // Only the command word completes; arguments are values, and guessing
        // at those would fight the person typing.
        let prefix = self.input.trim_start();
        if prefix.contains(char::is_whitespace) || prefix.is_empty() { return }

        let candidates = console.complete(prefix);
        match candidates.len() {
            0 => {}
            1 => {
                self.input = format!("{} ", candidates[0]);
                self.completions.clear();
            }
            _ => {
                let shared = common_prefix(&candidates);
                self.completions = candidates;
                self.completion_index = 0;
                self.input = if shared.len() > prefix.len() {
                    shared
                } else {
                    self.completions[0].clone()
                };
                self.completion_source = self.input.clone();
            }
        }
    }

    /// Submit the line. Returns what was run, if anything.
    pub fn submit(&mut self, console: &mut Console) -> Option<String> {
        let line = self.input.trim().to_string();
        self.input.clear();
        self.draft.clear();
        self.history_index = None;
        self.completions.clear();
        self.scroll = 0;
        if line.is_empty() { return None }
        console.execute_user(&line);
        Some(line)
    }

    pub fn scroll_up(&mut self, log_len: usize) {
        self.scroll = (self.scroll + PAGE).min(log_len.saturating_sub(1));
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_sub(PAGE);
    }

    /// The window of the log to show, given how many lines fit.
    ///
    /// Returned as a range so the caller does not have to think about what
    /// happens when the log is shorter than the window, which is the case
    /// every console has got wrong at least once.
    pub fn visible_range(&self, log_len: usize, rows: usize) -> std::ops::Range<usize> {
        let end = log_len.saturating_sub(self.scroll).max(rows.min(log_len));
        let start = end.saturating_sub(rows);
        start..end
    }
}

/// The longest prefix every candidate shares.
fn common_prefix(candidates: &[String]) -> String {
    let Some(first) = candidates.first() else { return String::new() };
    let mut length = first.len();
    for other in &candidates[1..] {
        length = length.min(
            first
                .bytes()
                .zip(other.bytes())
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .count(),
        );
    }
    first[..length].to_string()
}

#[cfg(test)]
mod tests;
