// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Drawing the developer console over the game.
//!
//! The console's behaviour -- history, completion, scrollback -- is a state
//! machine in `kerosene-console` with no window in it, tested there. What is left
//! here is the drawing and the keystrokes, which is the part that genuinely
//! needs a display.
//!
//! Two rules shape everything below. The console takes the keyboard
//! completely while it is open, because a console you cannot type an `n` into
//! without walking forward is not a console. And it releases the mouse, so
//! the view stops following the pointer the moment it opens.

use egui::{Color32, RichText};
use kerosene_console::{Console, ConsoleUi, LogLevel};

/// Fraction of the window height the console covers.
const HEIGHT: f32 = 0.5;

/// Points of wheel travel per line of scrollback.
const WHEEL_LINE: f32 = 14.0;

/// Size of a scrollback line. The row height is measured from this exact font
/// rather than from the Monospace text style, so what is counted and what is
/// drawn cannot drift apart.
const LINE: f32 = 12.0;

/// Colour for each severity. Errors have to be findable in a wall of text.
fn colour(level: LogLevel) -> Color32 {
    match level {
        LogLevel::Error => Color32::from_rgb(255, 105, 97),
        LogLevel::Warning => Color32::from_rgb(240, 200, 90),
        LogLevel::Developer => Color32::from_rgb(130, 190, 230),
        LogLevel::Echo => Color32::from_rgb(210, 215, 225),
        LogLevel::Info => Color32::from_rgb(178, 186, 199),
    }
}

/// Draw the console, and run whatever was typed into it.
pub fn draw(ctx: &egui::Context, ui_state: &mut ConsoleUi, console: &mut Console) {
    if !ui_state.open {
        return;
    }

    let height = ctx.screen_rect().height() * HEIGHT;
    egui::TopBottomPanel::top("console")
        .exact_height(height)
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(8, 10, 14, 235))
                .inner_margin(8.0),
        )
        .show(ctx, |ui| {
            // Keys the text field must not see. Consumed before it is built,
            // because egui gives a focused TextEdit first refusal otherwise
            // and tab would move focus instead of completing.
            let (submit, up, down, tab, page_up, page_down, wheel) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown),
                    i.smooth_scroll_delta.y,
                )
            });

            if up {
                ui_state.history_previous(console);
            }
            if down {
                ui_state.history_next(console);
            }
            if tab {
                ui_state.complete(console);
            }
            if page_up {
                ui_state.scroll_up(console.log_len());
            }
            if page_down {
                ui_state.scroll_down();
            }
            // The wheel, which is what anyone actually reaches for. Positive
            // delta is a push away from you, which reads back through the log.
            if wheel.abs() > 0.5 {
                let lines = (wheel / WHEEL_LINE).round() as i32;
                if lines != 0 {
                    ui_state.scroll_by(lines, console.log_len());
                }
            }

            // The prompt is laid out first, upward from the bottom, so the
            // scrollback gets exactly the room that is left rather than a
            // guessed number of pixels. The guess was wrong whenever a
            // completion list appeared: the newest line -- the one you opened
            // the console to read -- went under the prompt.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(">").monospace().strong());
                    let mut text = ui_state.input.clone();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .hint_text("type a command; tab completes, up walks back"),
                    );
                    if text != ui_state.input {
                        ui_state.set_input(text);
                    }
                    // The caret belongs in the input from the moment it opens.
                    if !response.has_focus() {
                        response.request_focus();
                    }
                });

                // Candidates, so cycling with tab shows what is being cycled.
                if !ui_state.completions().is_empty() {
                    let list = ui_state.completions().join("   ");
                    ui.label(RichText::new(list).monospace().size(11.0).weak());
                }
                ui.separator();

                // Everything above the prompt is scrollback: the newest line
                // sits just above it and older ones stack upward, so it still
                // reads downward.
                //
                // Laid out upward rather than downward from the top of the gap,
                // because downward means working out how many lines fit and
                // being wrong is invisible until it bites. It did: the row
                // height used here left out `item_spacing`, so the estimate ran
                // a few lines over, and those lines were drawn under the prompt
                // where they could not be read. Filling upward from the prompt
                // and stopping when the room runs out cannot overflow, whatever
                // the font metrics turn out to be.
                let row = ui.fonts(|f| f.row_height(&egui::FontId::monospace(LINE)))
                    + ui.spacing().item_spacing.y;
                let rows = (ui.available_height() / row).max(1.0) as usize;
                let range = ui_state.visible_range(console.log_len(), rows);

                let window: Vec<_> = console.log().skip(range.start).take(range.len()).collect();
                for line in window.into_iter().rev() {
                    if ui.available_height() < row {
                        break;
                    }
                    ui.label(
                        RichText::new(&line.text)
                            .monospace()
                            .size(LINE)
                            .color(colour(line.level)),
                    );
                }
            });

            if submit {
                ui_state.submit(console);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, RawInput};

    fn press(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    /// An open console, and a frame drawn with the given events in it.
    struct Harness {
        ctx: egui::Context,
        ui: ConsoleUi,
        console: Console,
    }

    impl Harness {
        fn open() -> Harness {
            let mut harness = Harness {
                ctx: egui::Context::default(),
                ui: ConsoleUi::new(),
                console: Console::new(),
            };
            harness.ui.open = true;
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                events,
                ..Default::default()
            };
            let ui = &mut self.ui;
            let console = &mut self.console;
            let _ = self.ctx.run(input, |ctx| draw(ctx, ui, console));
        }

        fn said(&self) -> String {
            self.console
                .log()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    #[test]
    fn typing_a_command_and_pressing_enter_runs_it() {
        let mut harness = Harness::open();
        harness.frame(vec![Event::Text("echo hello".into()), press(Key::Enter)]);

        assert!(harness.said().contains("hello"), "{}", harness.said());
        assert_eq!(harness.ui.input, "", "the line is cleared once it has run");
    }

    #[test]
    fn tab_completes_the_command_word() {
        let mut harness = Harness::open();
        harness.ui.set_input("ech");
        harness.frame(vec![press(Key::Tab)]);

        assert_eq!(
            harness.ui.input, "echo ",
            "completed, with a space ready for an argument"
        );
    }

    #[test]
    fn tab_does_not_move_focus_out_of_the_input() {
        // egui gives a focused text field first refusal on tab, and it uses
        // it to move focus. The console has to take the key first or
        // completion silently does nothing.
        let mut harness = Harness::open();
        harness.ui.set_input("ech");
        harness.frame(vec![press(Key::Tab)]);
        harness.frame(vec![Event::Text("x".into())]);

        assert!(
            harness.ui.input.starts_with("echo"),
            "still typing into it: {:?}",
            harness.ui.input
        );
    }

    #[test]
    fn up_walks_back_through_history() {
        let mut harness = Harness::open();
        harness.frame(vec![Event::Text("echo one".into()), press(Key::Enter)]);
        harness.frame(vec![press(Key::ArrowUp)]);

        assert_eq!(harness.ui.input, "echo one");
    }

    #[test]
    fn the_console_keeps_the_keyboard_while_it_is_open() {
        // Which is the whole reason the keys that close it are handled by the
        // host before egui ever sees them; see `host::intercepted`.
        let harness = Harness::open();
        assert!(harness.ctx.wants_keyboard_input());
    }

    #[test]
    fn a_closed_console_draws_nothing_and_holds_no_keys() {
        let mut harness = Harness::open();
        harness.ui.close();
        harness.frame(Vec::new());
        assert!(!harness.ctx.wants_keyboard_input());
    }

    #[test]
    fn an_unknown_command_is_reported_rather_than_ignored() {
        let mut harness = Harness::open();
        harness.frame(vec![Event::Text("wibble".into()), press(Key::Enter)]);
        assert!(
            harness.said().to_lowercase().contains("wibble"),
            "{}",
            harness.said()
        );
    }
}
