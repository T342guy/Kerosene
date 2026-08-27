// SPDX-License-Identifier: LGPL-3.0-or-later
//! Drawing the developer console over the game.
//!
//! The console's behaviour -- history, completion, scrollback -- is a state
//! machine in `void-console` with no window in it, tested there. What is left
//! here is the drawing and the keystrokes, which is the part that genuinely
//! needs a display.
//!
//! Two rules shape everything below. The console takes the keyboard
//! completely while it is open, because a console you cannot type an `n` into
//! without walking forward is not a console. And it releases the mouse, so
//! the view stops following the pointer the moment it opens.

use egui::{Color32, RichText};
use void_console::{Console, ConsoleUi, LogLevel};

/// Fraction of the window height the console covers.
const HEIGHT: f32 = 0.5;

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
    if !ui_state.open { return }

    let height = ctx.screen_rect().height() * HEIGHT;
    egui::TopBottomPanel::top("console")
        .exact_height(height)
        .frame(egui::Frame::new().fill(Color32::from_rgba_unmultiplied(8, 10, 14, 235)).inner_margin(8.0))
        .show(ctx, |ui| {
            // Keys the text field must not see. Consumed before it is built,
            // because egui gives a focused TextEdit first refusal otherwise
            // and tab would move focus instead of completing.
            let (submit, up, down, tab, page_up, page_down) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown),
                )
            });

            if up { ui_state.history_previous(console); }
            if down { ui_state.history_next(console); }
            if tab { ui_state.complete(console); }
            if page_up { ui_state.scroll_up(console.log_len()); }
            if page_down { ui_state.scroll_down(); }

            let row = ui.text_style_height(&egui::TextStyle::Monospace).max(1.0);
            let rows = ((height - 60.0) / row).max(1.0) as usize;
            let range = ui_state.visible_range(console.log_len(), rows);

            egui::ScrollArea::vertical()
                .max_height(height - 48.0)
                .auto_shrink([false, false])
                .stick_to_bottom(ui_state.scroll == 0)
                .show(ui, |ui| {
                    for line in console.log().skip(range.start).take(range.len()) {
                        ui.label(
                            RichText::new(&line.text).monospace().size(12.0).color(colour(line.level)),
                        );
                    }
                });

            ui.separator();

            // Candidates, so cycling with tab shows what is being cycled.
            if !ui_state.completions().is_empty() {
                let list = ui_state.completions().join("   ");
                ui.label(RichText::new(list).monospace().size(11.0).weak());
            }

            ui.horizontal(|ui| {
                ui.label(RichText::new(">").monospace().strong());
                let mut text = ui_state.input.clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .hint_text("type a command; tab completes, up walks back"),
                );
                if text != ui_state.input { ui_state.set_input(text); }
                // The caret belongs in the input from the moment it opens.
                if !response.has_focus() { response.request_focus(); }
            });

            if submit {
                ui_state.submit(console);
            }
        });
}
