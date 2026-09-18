// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The handful of widgets every tool draws.
//!
//! An icon button with a tooltip that names the shortcut, a tab bar, a
//! section heading, a dialog with its buttons where a dialog's buttons go.
//! None of them is clever; the point is that there is one of each, so the
//! editor's tool strip and the toolset's activity bar are the same widget
//! rather than two that nearly match.

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Response, Sense, Stroke, Ui, Vec2,
};

use crate::theme::{self, colors};

/// A square icon button for a tool strip or an activity bar.
///
/// `name` and `shortcut` become the tooltip, "Block tool  2", which is where
/// the shortcuts live now: on the thing they select, rather than in a table
/// in the manual.
pub fn tool_button(
    ui: &mut Ui,
    glyph: &str,
    name: &str,
    shortcut: Option<&str>,
    selected: bool,
) -> Response {
    let size = Vec2::splat(36.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS + 1);
        if selected {
            painter.rect_filled(rect, radius, colors::ACCENT.gamma_multiply(0.28));
            painter.rect_stroke(
                rect,
                radius,
                Stroke::new(1.0_f32, colors::ACCENT),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(rect, radius, colors::HOVER);
        }
        let colour = if selected {
            colors::ACCENT
        } else if response.hovered() {
            Color32::WHITE
        } else {
            colors::TEXT
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            FontId::new(19.0, FontFamily::Proportional),
            colour,
        );
    }
    response.on_hover_ui(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(name).strong());
            if let Some(shortcut) = shortcut {
                ui.label(theme::mono(shortcut).color(colors::TEXT_MUTED));
            }
        });
    })
}

/// A small frameless icon button for a toolbar row or a pane header.
pub fn icon_button(ui: &mut Ui, glyph: &str, tooltip: &str) -> Response {
    let size = Vec2::splat(22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, CornerRadius::same(theme::RADIUS), colors::HOVER);
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            FontId::new(15.0, FontFamily::Proportional),
            if response.hovered() {
                Color32::WHITE
            } else {
                colors::TEXT
            },
        );
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// An icon that is on or off: snap to grid, show the output panel.
pub fn icon_toggle(ui: &mut Ui, glyph: &str, tooltip: &str, on: &mut bool) -> Response {
    let size = Vec2::splat(22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS);
        if *on {
            painter.rect_filled(rect, radius, colors::ACCENT.gamma_multiply(0.28));
        } else if response.hovered() {
            painter.rect_filled(rect, radius, colors::HOVER);
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            FontId::new(15.0, FontFamily::Proportional),
            if *on {
                colors::ACCENT
            } else if response.hovered() {
                Color32::WHITE
            } else {
                colors::TEXT_MUTED
            },
        );
    }
    response.on_hover_text(tooltip)
}

/// A button that is the thing to press: *build*, *compile*, *save*.
pub fn primary_button(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> Response {
    ui.add(
        egui::Button::new(text)
            .fill(colors::ACCENT)
            .stroke(Stroke::NONE),
    )
}

/// A section of a panel: a small capitalised title, a rule, the body.
pub fn section<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(6.0);
    ui.label(theme::section_title(title));
    let rule = ui.available_rect_before_wrap();
    let y = ui.cursor().min.y + 1.0;
    ui.painter().hline(
        rule.min.x..=rule.max.x,
        y,
        Stroke::new(1.0_f32, colors::BORDER),
    );
    ui.add_space(5.0);
    let inner = add_contents(ui);
    ui.add_space(2.0);
    inner
}

/// A row of tabs with the current one underlined. Returns whether it changed.
///
/// Each tab is an icon and a label; the icon may be empty.
pub fn tab_bar(ui: &mut Ui, tabs: &[(&str, &str)], selected: &mut usize) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (index, (glyph, label)) in tabs.iter().enumerate() {
            let current = index == *selected;
            let text = if glyph.is_empty() {
                (*label).to_string()
            } else {
                format!("{glyph}  {label}")
            };
            let galley = ui.painter().layout_no_wrap(
                text.clone(),
                FontId::new(12.5, FontFamily::Proportional),
                colors::TEXT,
            );
            let size = Vec2::new(galley.size().x + 18.0, 26.0);
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            if response.clicked() && !current {
                *selected = index;
                changed = true;
            }
            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                if response.hovered() && !current {
                    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS), colors::HOVER);
                }
                let colour = if current {
                    Color32::WHITE
                } else if response.hovered() {
                    colors::TEXT
                } else {
                    colors::TEXT_MUTED
                };
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    FontId::new(12.5, FontFamily::Proportional),
                    colour,
                );
                if current {
                    let y = rect.max.y - 1.0;
                    painter.hline(
                        rect.min.x + 4.0..=rect.max.x - 4.0,
                        y,
                        Stroke::new(2.0_f32, colors::ACCENT),
                    );
                }
            }
        }
    });
    let rule = ui.available_rect_before_wrap();
    let y = ui.cursor().min.y;
    ui.painter().hline(
        rule.min.x..=rule.max.x,
        y,
        Stroke::new(1.0_f32, colors::BORDER),
    );
    ui.add_space(4.0);
    changed
}

/// A toggle drawn as a pill: the build stages, a filter.
pub fn chip(ui: &mut Ui, label: &str, on: &mut bool) -> Response {
    let text = if *on {
        format!("{}  {label}", theme::icons::CHECK)
    } else {
        label.to_string()
    };
    let button = egui::Button::new(egui::RichText::new(text).size(12.0))
        .corner_radius(CornerRadius::same(12))
        .fill(if *on {
            colors::ACCENT.gamma_multiply(0.3)
        } else {
            colors::BG_HEADER
        })
        .stroke(if *on {
            Stroke::new(1.0_f32, colors::ACCENT)
        } else {
            Stroke::new(1.0_f32, colors::BORDER)
        });
    let response = ui.add(button);
    if response.clicked() {
        *on = !*on;
    }
    response
}

/// A menu entry with its shortcut on the right, where a shortcut goes.
pub fn menu_item(ui: &mut Ui, label: &str, shortcut: Option<&str>) -> Response {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(theme::mono(shortcut).color(colors::TEXT_MUTED));
    }
    ui.add(button)
}

/// A menu entry that may be greyed out, with its shortcut.
pub fn menu_item_enabled(
    ui: &mut Ui,
    enabled: bool,
    label: &str,
    shortcut: Option<&str>,
) -> Response {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(theme::mono(shortcut).color(colors::TEXT_MUTED));
    }
    ui.add_enabled(enabled, button)
}

/// A modal dialog: a heading, a body, and a footer of buttons on the right.
///
/// The body and the footer are separate closures so the buttons are always
/// where a dialog's buttons go, whatever the body did with its layout.
pub fn dialog<B, F>(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    min_width: f32,
    body: impl FnOnce(&mut Ui) -> B,
    footer: impl FnOnce(&mut Ui) -> F,
) -> egui::ModalResponse<(B, F)> {
    egui::Modal::new(egui::Id::new(id))
        .frame(
            egui::Frame::window(&ctx.style())
                .inner_margin(egui::Margin::same(14))
                .fill(colors::BG_PANEL),
        )
        .show(ctx, |ui| {
            ui.set_min_width(min_width);
            ui.label(theme::heading(title));
            ui.add_space(8.0);
            let b = body(ui);
            ui.add_space(12.0);
            let f = ui
                .with_layout(Layout::right_to_left(Align::Center), footer)
                .inner;
            (b, f)
        })
}

/// A key/value line for a summary: the label muted, the value plain.
pub fn fact(ui: &mut Ui, label: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(theme::caption(label));
        ui.label(theme::mono(value));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(mut add: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| add(ui));
        })
    }

    #[test]
    fn every_widget_draws() {
        let mut tab = 0;
        let mut on = false;
        let output = frame(|ui| {
            tool_button(ui, theme::icons::CUBE, "Editor", Some("1"), true);
            icon_button(ui, theme::icons::GEAR, "settings");
            icon_toggle(ui, theme::icons::MAGNET, "snap", &mut on);
            primary_button(ui, "build");
            section(ui, "grid", |ui| {
                ui.label("body");
            });
            tab_bar(
                ui,
                &[("", "Object"), (theme::icons::CUBE, "Tool")],
                &mut tab,
            );
            chip(ui, "textures", &mut on);
            menu_item(ui, "save", Some("ctrl-S"));
            fact(ui, "content", "/tmp");
        });
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn a_dialog_draws_its_body_and_footer() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut drew = (false, false);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            dialog(
                ctx,
                "test",
                "Save as",
                300.0,
                |ui| {
                    ui.label("name");
                    drew.0 = true;
                },
                |ui| {
                    let _ = ui.button("save");
                    drew.1 = true;
                },
            );
        });
        assert_eq!(drew, (true, true));
    }
}
