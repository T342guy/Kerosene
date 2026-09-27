// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The widgets every tool draws.
//!
//! An icon button with a tooltip that names the shortcut, a tab bar, a
//! section heading, a dialog with its buttons where a dialog's buttons go,
//! and the pieces a page is made of: a card, a stat tile, a badge, a list
//! row, a search field. None of them is clever; the point is that there is
//! one of each, so the editor's tool strip and the toolset's sidebar are the
//! same widget rather than two that nearly match.

use egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2,
};

use crate::theme::{self, colors, icons};

// ---- buttons ---------------------------------------------------------------

/// A square icon button for a tool strip.
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
            painter.rect_filled(rect, radius, colors::ACCENT_SOFT);
            painter.rect_stroke(
                rect,
                radius,
                Stroke::new(1.0_f32, colors::ACCENT.gamma_multiply(0.7)),
                StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(rect, radius, colors::HOVER);
        }
        let colour = if selected {
            colors::ACCENT
        } else if response.hovered() {
            Color32::WHITE
        } else {
            colors::TEXT_MUTED
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(19.0, FontFamily::Proportional),
            colour,
        );
    }
    response.on_hover_ui(|ui| shortcut_tooltip(ui, name, shortcut))
}

/// The tooltip every shortcut-bearing control shows: the name, then the key.
fn shortcut_tooltip(ui: &mut Ui, name: &str, shortcut: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).strong());
        if let Some(shortcut) = shortcut {
            kbd(ui, shortcut);
        }
    });
}

/// A small frameless icon button for a toolbar row or a pane header.
pub fn icon_button(ui: &mut Ui, glyph: &str, tooltip: &str) -> Response {
    let size = Vec2::splat(24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, CornerRadius::same(theme::RADIUS), colors::HOVER);
        }
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(15.0, FontFamily::Proportional),
            if response.hovered() {
                Color32::WHITE
            } else {
                colors::TEXT_MUTED
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
    let size = Vec2::splat(24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS);
        if *on {
            painter.rect_filled(rect, radius, colors::ACCENT_SOFT);
        } else if response.hovered() {
            painter.rect_filled(rect, radius, colors::HOVER);
        }
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
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

/// What a button is for, which decides how loudly it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The one thing to press on a page: amber.
    Primary,
    /// Everything else a page offers.
    Secondary,
    /// Something that deletes or stops.
    Danger,
    /// A button that should read as text until it is hovered.
    Ghost,
}

/// A button with an icon and a label, drawn as `kind` says.
///
/// Every page button goes through here, so a primary button on the build
/// page and one on the project page are the same size and the same amber.
pub fn button(ui: &mut Ui, kind: Kind, glyph: &str, label: &str) -> Response {
    let text = if glyph.is_empty() {
        label.to_string()
    } else if label.is_empty() {
        glyph.to_string()
    } else {
        format!("{glyph}  {label}")
    };
    let (fill, stroke, colour) = match kind {
        Kind::Primary => (colors::ACCENT, Stroke::NONE, colors::ON_ACCENT),
        Kind::Secondary => (
            colors::BG_HEADER,
            Stroke::new(1.0_f32, colors::BORDER),
            colors::TEXT,
        ),
        Kind::Danger => (
            colors::ERR.gamma_multiply(0.18),
            Stroke::new(1.0_f32, colors::ERR.gamma_multiply(0.6)),
            colors::ERR,
        ),
        Kind::Ghost => (Color32::TRANSPARENT, Stroke::NONE, colors::TEXT_MUTED),
    };
    let rich = RichText::new(text).size(13.0).color(colour);
    let rich = if kind == Kind::Primary {
        rich.strong()
    } else {
        rich
    };
    ui.add(
        egui::Button::new(rich)
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(theme::RADIUS))
            .min_size(Vec2::new(0.0, 30.0)),
    )
}

/// A button that is the thing to press: *build*, *compile*, *save*.
pub fn primary_button(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> Response {
    ui.add(
        egui::Button::new(text)
            .fill(colors::ACCENT)
            .stroke(Stroke::NONE),
    )
}

/// A switch: the build stages, an option on a form.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = Vec2::new(32.0, 18.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let painter = ui.painter();
        let radius = CornerRadius::same((rect.height() / 2.0) as u8);
        let track = lerp_colour(colors::BG_FIELD, colors::ACCENT, t);
        painter.rect_filled(rect, radius, track);
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(
                1.0_f32,
                lerp_colour(colors::BORDER_STRONG, colors::ACCENT, t),
            ),
            StrokeKind::Inside,
        );
        let knob_x = egui::lerp((rect.left() + 9.0)..=(rect.right() - 9.0), t);
        let knob = if *on {
            colors::ON_ACCENT
        } else {
            colors::TEXT_MUTED
        };
        painter.circle_filled(egui::pos2(knob_x, rect.center().y), 6.0, knob);
    }
    response
}

fn lerp_colour(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// A toggle drawn as a pill: a filter, an option in a row of them.
pub fn chip(ui: &mut Ui, label: &str, on: &mut bool) -> Response {
    let text = if *on {
        format!("{}  {label}", icons::CHECK)
    } else {
        label.to_string()
    };
    let button = egui::Button::new(RichText::new(text).size(12.0).color(if *on {
        colors::ACCENT
    } else {
        colors::TEXT_MUTED
    }))
    .corner_radius(CornerRadius::same(12))
    .fill(if *on {
        colors::ACCENT_SOFT
    } else {
        colors::BG_HEADER
    })
    .stroke(if *on {
        Stroke::new(1.0_f32, colors::ACCENT.gamma_multiply(0.7))
    } else {
        Stroke::new(1.0_f32, colors::BORDER)
    });
    let mut response = ui.add(button);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
}

// ---- structure -------------------------------------------------------------

/// A section of a panel: a small capitalised title, a rule, the body.
pub fn section<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(theme::SPACE_SM);
    ui.label(theme::section_title(title));
    let rule = ui.available_rect_before_wrap();
    let y = ui.cursor().min.y + 1.0;
    ui.painter().hline(
        rule.min.x..=rule.max.x,
        y,
        Stroke::new(1.0_f32, colors::BORDER),
    );
    ui.add_space(6.0);
    let inner = add_contents(ui);
    ui.add_space(2.0);
    inner
}

/// A page: the canvas behind it, a scroll area, and a readable column.
///
/// Every full-window page in the toolset -- the project, the build, the
/// archive, the start screen -- sits in one of these, so they share their
/// margins and their widest line.
pub fn page<R>(ctx: &egui::Context, max_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(colors::BG_APP))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width = ui.available_width();
                    let column = max_width.min(width - 2.0 * theme::SPACE_XL).max(200.0);
                    let side = ((width - column) / 2.0).max(theme::SPACE_LG);
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: side as i8,
                            right: 0,
                            top: 28,
                            bottom: 28,
                        })
                        .show(ui, |ui| {
                            ui.set_max_width(column);
                            add(ui)
                        })
                        .inner
                })
                .inner
        })
        .inner
}

/// The top of a page: an icon in a tile, a title, a line under it, and
/// whatever the page wants on the right.
pub fn page_header(
    ui: &mut Ui,
    glyph: &str,
    title: &str,
    subtitle: &str,
    trailing: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::RADIUS_LARGE),
            colors::ACCENT_SOFT,
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(22.0, FontFamily::Proportional),
            colors::ACCENT,
        );
        ui.add_space(theme::SPACE_XS);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(theme::title(title));
            if !subtitle.is_empty() {
                ui.label(theme::subtitle(subtitle));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    ui.add_space(theme::SPACE_LG);
}

/// A card: a raised panel with a border, for grouping one thing on a page.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        add(ui)
    })
}

/// The frame a [`card`] is drawn in, for callers that need to size it.
pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(colors::BG_ELEVATED)
        .stroke(Stroke::new(1.0_f32, colors::BORDER))
        .corner_radius(CornerRadius::same(theme::RADIUS_LARGE))
        .inner_margin(egui::Margin::same(16))
}

/// A card with a small capitalised title and an optional control on the
/// right of it.
pub fn titled_card<R>(
    ui: &mut Ui,
    title: &str,
    trailing: impl FnOnce(&mut Ui),
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::section_title(title));
            ui.with_layout(Layout::right_to_left(Align::Center), trailing);
        });
        ui.add_space(theme::SPACE_SM);
        add(ui)
    })
    .inner
}

/// A number with a name: the project page's counts. Clickable, since a
/// count is usually a way into the list it counts.
pub fn stat_tile(ui: &mut Ui, glyph: &str, label: &str, value: &str, note: &str) -> Response {
    let size = Vec2::new(150.0, 86.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS_LARGE);
        painter.rect_filled(
            rect,
            radius,
            if response.hovered() {
                colors::BG_HEADER
            } else {
                colors::BG_ELEVATED
            },
        );
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(
                1.0_f32,
                if response.hovered() {
                    colors::BORDER_STRONG
                } else {
                    colors::BORDER
                },
            ),
            StrokeKind::Inside,
        );
        let left = rect.left() + 14.0;
        painter.text(
            egui::pos2(left, rect.top() + 16.0),
            Align2::LEFT_CENTER,
            glyph,
            FontId::new(14.0, FontFamily::Proportional),
            colors::TEXT_MUTED,
        );
        painter.text(
            egui::pos2(left + 20.0, rect.top() + 16.0),
            Align2::LEFT_CENTER,
            label,
            FontId::new(11.5, FontFamily::Proportional),
            colors::TEXT_MUTED,
        );
        let dim = value == "0";
        painter.text(
            egui::pos2(left, rect.top() + 46.0),
            Align2::LEFT_CENTER,
            value,
            FontId::new(26.0, FontFamily::Proportional),
            if dim {
                colors::TEXT_FAINT
            } else {
                colors::TEXT
            },
        );
        painter.text(
            egui::pos2(left, rect.bottom() - 13.0),
            Align2::LEFT_CENTER,
            note,
            FontId::new(10.5, FontFamily::Proportional),
            colors::TEXT_FAINT,
        );
    }
    response
}

/// What a badge or a status means, which decides its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Nothing to report.
    Neutral,
    /// Current, selected.
    Accent,
    /// Done and good.
    Ok,
    /// Worth a look.
    Warn,
    /// Broken.
    Err,
    /// For information.
    Info,
}

impl Tone {
    /// The colour of text and marks in this tone.
    pub fn colour(self) -> Color32 {
        match self {
            Tone::Neutral => colors::TEXT_MUTED,
            Tone::Accent => colors::ACCENT,
            Tone::Ok => colors::OK,
            Tone::Warn => colors::WARN,
            Tone::Err => colors::ERR,
            Tone::Info => colors::INFO,
        }
    }
}

/// A small rounded label: "compiled", "stale", "3 errors".
pub fn badge(ui: &mut Ui, text: &str, tone: Tone) -> Response {
    let colour = tone.colour();
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        FontId::new(10.5, FontFamily::Proportional),
        colour,
    );
    let size = Vec2::new(galley.size().x + 14.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let radius = CornerRadius::same(9);
        ui.painter()
            .rect_filled(rect, radius, colour.gamma_multiply(0.14));
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0_f32, colour.gamma_multiply(0.35)),
            StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, colour);
    }
    response
}

/// A key, or a chord of them, drawn as keycaps: `ctrl-P` is two caps.
///
/// Laid out as one allocation and painted left to right, so the chord reads
/// the same way in a right-to-left row as in a left-to-right one.
pub fn kbd(ui: &mut Ui, keys: &str) {
    let gap = 3.0;
    let caps: Vec<_> = keys
        .split(['-', '+'])
        .filter(|k| !k.is_empty())
        .map(|key| {
            let label = match key.to_ascii_lowercase().as_str() {
                "ctrl" => "Ctrl".to_string(),
                "shift" => "Shift".to_string(),
                "alt" => "Alt".to_string(),
                "enter" => "Enter".to_string(),
                "esc" => "Esc".to_string(),
                _ => key.to_uppercase(),
            };
            let galley = ui.painter().layout_no_wrap(
                label,
                FontId::new(10.5, FontFamily::Monospace),
                colors::TEXT_MUTED,
            );
            let width = (galley.size().x + 10.0).max(18.0);
            (galley, width)
        })
        .collect();
    let total =
        caps.iter().map(|(_, w)| w).sum::<f32>() + gap * caps.len().saturating_sub(1) as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 18.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let mut x = rect.left();
    for (galley, width) in caps {
        let cap = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(width, 18.0));
        let radius = CornerRadius::same(4);
        ui.painter().rect_filled(cap, radius, colors::BG_FIELD);
        ui.painter().rect_stroke(
            cap,
            radius,
            Stroke::new(1.0_f32, colors::BORDER_STRONG),
            StrokeKind::Inside,
        );
        ui.painter().galley(
            cap.center() - galley.size() / 2.0,
            galley,
            colors::TEXT_MUTED,
        );
        x += width + gap;
    }
}

/// A text field with a magnifier in it and a button to clear it. Returns
/// the field's response, so a caller can focus it.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let height = 30.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = CornerRadius::same(theme::RADIUS);
    let edit_rect = Rect::from_min_max(
        rect.min + Vec2::new(28.0, 0.0),
        rect.max - Vec2::new(if text.is_empty() { 8.0 } else { 28.0 }, 0.0),
    );
    // The frame is drawn under the text, but its colour depends on focus,
    // which is only known once the field exists: reserve its place first.
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.put(
        edit_rect,
        egui::TextEdit::singleline(text)
            .hint_text(RichText::new(hint).color(colors::TEXT_FAINT))
            .frame(false)
            .vertical_align(Align::Center)
            .desired_width(edit_rect.width()),
    );
    let focused = response.has_focus();
    let border = if focused {
        colors::ACCENT.gamma_multiply(0.8)
    } else if response.hovered() {
        colors::BORDER_STRONG
    } else {
        colors::BORDER
    };
    ui.painter().set(
        background,
        egui::epaint::RectShape::new(
            rect,
            radius,
            colors::BG_FIELD,
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        ),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Align2::CENTER_CENTER,
        icons::MAGNIFYING_GLASS,
        FontId::new(14.0, FontFamily::Proportional),
        if focused {
            colors::ACCENT
        } else {
            colors::TEXT_FAINT
        },
    );
    if !text.is_empty() {
        let clear = Rect::from_center_size(
            egui::pos2(rect.right() - 14.0, rect.center().y),
            Vec2::splat(20.0),
        );
        if ui
            .put(
                clear,
                egui::Button::new(RichText::new(icons::X).size(12.0).color(colors::TEXT_MUTED))
                    .frame(false),
            )
            .on_hover_text("clear")
            .clicked()
        {
            text.clear();
        }
    }
    response
}

/// One row of a list: an icon, a title, a quieter line under it, and
/// controls on the right. The whole row is the click target, except for
/// what `trailing` adds, which takes its own clicks.
pub fn list_row(
    ui: &mut Ui,
    glyph: &str,
    title: &str,
    subtitle: &str,
    selected: bool,
    trailing: impl FnOnce(&mut Ui),
) -> Response {
    let height = if subtitle.is_empty() { 32.0 } else { 44.0 };
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS);
        if selected {
            painter.rect_filled(rect, radius, colors::ACCENT_SOFT);
        } else if response.hovered() {
            painter.rect_filled(rect, radius, colors::HOVER);
        }
        painter.text(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(16.0, FontFamily::Proportional),
            if selected {
                colors::ACCENT
            } else {
                colors::TEXT_MUTED
            },
        );
        let text_left = rect.left() + 38.0;
        if subtitle.is_empty() {
            painter.text(
                egui::pos2(text_left, rect.center().y),
                Align2::LEFT_CENTER,
                title,
                FontId::new(13.0, FontFamily::Proportional),
                colors::TEXT,
            );
        } else {
            painter.text(
                egui::pos2(text_left, rect.top() + 15.0),
                Align2::LEFT_CENTER,
                title,
                FontId::new(13.0, FontFamily::Proportional),
                colors::TEXT,
            );
            painter.text(
                egui::pos2(text_left, rect.bottom() - 13.0),
                Align2::LEFT_CENTER,
                subtitle,
                FontId::new(11.0, FontFamily::Proportional),
                colors::TEXT_MUTED,
            );
        }
    }
    let right = Rect::from_min_max(
        egui::pos2(rect.center().x, rect.top()),
        rect.max - Vec2::new(8.0, 0.0),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(right)
            .layout(Layout::right_to_left(Align::Center)),
        trailing,
    );
    response
}

/// An item in the toolset's sidebar: an icon over a short label, a bar of
/// accent on the left edge of the current one, and a dot while it is busy.
pub fn sidebar_item(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    shortcut: Option<&str>,
    selected: bool,
    busy: bool,
) -> Response {
    let size = Vec2::new(ui.available_width(), 52.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let inner = rect.shrink2(Vec2::new(6.0, 2.0));
        let radius = CornerRadius::same(theme::RADIUS_LARGE);
        if selected {
            painter.rect_filled(inner, radius, colors::ACCENT_SOFT);
            let bar = Rect::from_min_size(
                egui::pos2(rect.left(), inner.top() + 10.0),
                Vec2::new(3.0, inner.height() - 20.0),
            );
            painter.rect_filled(bar, CornerRadius::same(2), colors::ACCENT);
        } else if response.hovered() {
            painter.rect_filled(inner, radius, colors::HOVER);
        }
        let colour = if selected {
            colors::ACCENT
        } else if response.hovered() {
            colors::TEXT
        } else {
            colors::TEXT_MUTED
        };
        painter.text(
            egui::pos2(inner.center().x, inner.top() + 18.0),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(19.0, FontFamily::Proportional),
            colour,
        );
        painter.text(
            egui::pos2(inner.center().x, inner.bottom() - 11.0),
            Align2::CENTER_CENTER,
            label,
            FontId::new(10.0, FontFamily::Proportional),
            colour,
        );
        if busy {
            let at = egui::pos2(inner.center().x + 12.0, inner.top() + 9.0);
            painter.circle_filled(at, 4.0, colors::BG_PANEL);
            painter.circle_filled(at, 3.0, colors::ACCENT);
        }
    }
    response.on_hover_ui(|ui| shortcut_tooltip(ui, label, shortcut))
}

/// What a page shows where its list would be when there is nothing in it:
/// a large quiet icon, what is missing, and what to do about it.
pub fn empty_state(ui: &mut Ui, glyph: &str, title: &str, detail: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(theme::SPACE_LG);
        ui.label(theme::icon(glyph).size(36.0).color(colors::TEXT_FAINT));
        ui.add_space(theme::SPACE_SM);
        ui.label(RichText::new(title).size(14.0).strong().color(colors::TEXT));
        if !detail.is_empty() {
            ui.add_space(2.0);
            ui.label(theme::caption(detail));
        }
        ui.add_space(theme::SPACE_LG);
    });
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
            let size = Vec2::new(galley.size().x + 20.0, 28.0);
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            if response.clicked() && !current {
                *selected = index;
                changed = true;
            }
            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                if response.hovered() && !current {
                    painter.rect_filled(
                        rect.shrink2(Vec2::new(0.0, 2.0)),
                        CornerRadius::same(theme::RADIUS),
                        colors::HOVER,
                    );
                }
                let colour = if current || response.hovered() {
                    colors::TEXT
                } else {
                    colors::TEXT_MUTED
                };
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::new(12.5, FontFamily::Proportional),
                    colour,
                );
                if current {
                    let y = rect.max.y - 1.0;
                    painter.hline(
                        rect.min.x + 6.0..=rect.max.x - 6.0,
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

/// A menu entry with its shortcut on the right, where a shortcut goes.
pub fn menu_item(ui: &mut Ui, label: &str, shortcut: Option<&str>) -> Response {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(theme::mono(shortcut).color(colors::TEXT_FAINT));
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
        button = button.shortcut_text(theme::mono(shortcut).color(colors::TEXT_FAINT));
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
        .backdrop_color(Color32::from_black_alpha(120))
        .frame(
            egui::Frame::window(&ctx.style())
                .inner_margin(egui::Margin::same(18))
                .fill(colors::BG_ELEVATED),
        )
        .show(ctx, |ui| {
            ui.set_min_width(min_width);
            ui.label(theme::heading(title));
            ui.add_space(theme::SPACE_MD);
            let b = body(ui);
            ui.add_space(theme::SPACE_LG);
            let f = ui
                .with_layout(Layout::right_to_left(Align::Center), footer)
                .inner;
            (b, f)
        })
}

/// A key/value line for a summary: the label muted, the value plain.
pub fn fact(ui: &mut Ui, label: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(78.0, 18.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            FontId::new(11.5, FontFamily::Proportional),
            colors::TEXT_MUTED,
        );
        ui.label(theme::mono(value).color(colors::TEXT));
    });
}

/// Bytes as a person reads them: `812 B`, `14.2 KB`, `3.1 MB`.
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.2} GB", b / (KB * KB * KB))
    }
}

/// How long ago, as a person says it: `just now`, `5 min ago`, `3 days ago`.
pub fn human_age(age: std::time::Duration) -> String {
    let s = age.as_secs();
    match s {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86_400 => format!("{} h ago", s / 3600),
        86_400..172_800 => "yesterday".to_string(),
        _ => format!("{} days ago", s / 86_400),
    }
}

/// A span of time on a clock: `0:07`, `12:41`, `1:02:09`.
pub fn human_elapsed(elapsed: std::time::Duration) -> String {
    let s = elapsed.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
        let mut query = String::from("arena");
        let output = frame(|ui| {
            tool_button(ui, icons::CUBE, "Editor", Some("1"), true);
            icon_button(ui, icons::GEAR, "settings");
            icon_toggle(ui, icons::MAGNET, "snap", &mut on);
            primary_button(ui, "build");
            for kind in [Kind::Primary, Kind::Secondary, Kind::Danger, Kind::Ghost] {
                button(ui, kind, icons::HAMMER, "Build");
            }
            toggle(ui, &mut on);
            section(ui, "grid", |ui| {
                ui.label("body");
            });
            tab_bar(ui, &[("", "Object"), (icons::CUBE, "Tool")], &mut tab);
            chip(ui, "textures", &mut on);
            menu_item(ui, "save", Some("ctrl-S"));
            fact(ui, "content", "/tmp");
            card(ui, |ui| ui.label("in a card"));
            titled_card(ui, "maps", |_| {}, |ui| ui.label("list"));
            stat_tile(ui, icons::MAP_TRIFOLD, "maps", "3", "source");
            for tone in [Tone::Neutral, Tone::Accent, Tone::Ok, Tone::Warn, Tone::Err] {
                badge(ui, "stale", tone);
            }
            kbd(ui, "ctrl-shift-P");
            search_field(ui, &mut query, "Search", 240.0);
            list_row(
                ui,
                icons::MAP_TRIFOLD,
                "arena",
                "maps/arena.kmap",
                false,
                |ui| {
                    badge(ui, "compiled", Tone::Ok);
                },
            );
            sidebar_item(ui, icons::HOUSE, "Home", Some("ctrl-1"), true, true);
            page_header(ui, icons::HOUSE, "Kerosene", "a project", |_| {});
            empty_state(ui, icons::MAP_TRIFOLD, "No maps", "make one");
        });
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn a_page_draws_its_column() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut drew = false;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            page(ctx, 800.0, |ui| {
                ui.label("hello");
                drew = true;
            });
        });
        assert!(drew);
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

    #[test]
    fn sizes_ages_and_clocks_read_like_a_person_wrote_them() {
        assert_eq!(human_size(812), "812 B");
        assert_eq!(human_size(14_540), "14.2 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(human_age(Duration::from_secs(5)), "just now");
        assert_eq!(human_age(Duration::from_secs(300)), "5 min ago");
        assert_eq!(human_age(Duration::from_secs(100_000)), "yesterday");
        assert_eq!(human_age(Duration::from_secs(3 * 86_400)), "3 days ago");
        assert_eq!(human_elapsed(Duration::from_secs(7)), "0:07");
        assert_eq!(human_elapsed(Duration::from_secs(761)), "12:41");
        assert_eq!(human_elapsed(Duration::from_secs(3729)), "1:02:09");
    }
}
