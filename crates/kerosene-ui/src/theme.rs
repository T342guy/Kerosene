// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! How the toolset looks.
//!
//! One palette, one set of spacings and one icon font, installed into the
//! egui context once by [`crate::run`]. Every tool draws with these rather
//! than its own `Color32::from_rgb` literals, so the editor, the sound tab
//! and a build panel read as rooms of one house rather than three programs
//! that happen to share a window.
//!
//! The palette is dark and low in chroma, because the things that need
//! colour -- a selection, a leak, a warning -- should be the only colourful
//! things on screen. The selection amber is the same one the editor's panes
//! use for a selected brush, so "selected" looks like one thing whether it is
//! a brush or a tab.

use egui::{Color32, CornerRadius, FontDefinitions, FontFamily, RichText, Stroke, Vec2};

pub use egui_phosphor::regular as icons;

/// The palette.
pub mod colors {
    use egui::Color32;

    /// Behind the viewports and anything else that is a canvas.
    pub const BG_APP: Color32 = Color32::from_rgb(22, 24, 28);
    /// Panels: toolbars, the inspector, the status bar.
    pub const BG_PANEL: Color32 = Color32::from_rgb(31, 34, 39);
    /// A heading strip inside a panel, or the header of a pane.
    pub const BG_HEADER: Color32 = Color32::from_rgb(38, 42, 48);
    /// Text fields and other things you type into.
    pub const BG_FIELD: Color32 = Color32::from_rgb(18, 20, 23);
    /// Where two panels meet.
    pub const BORDER: Color32 = Color32::from_rgb(50, 54, 62);
    /// Anything you can hover.
    pub const HOVER: Color32 = Color32::from_rgb(52, 57, 66);
    /// Ordinary text.
    pub const TEXT: Color32 = Color32::from_rgb(210, 214, 222);
    /// Captions, hints, and anything that is there for when you need it.
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(140, 148, 162);
    /// Selected, active, current. The editor's selection amber.
    pub const ACCENT: Color32 = Color32::from_rgb(255, 190, 70);
    /// Text drawn on top of the accent.
    pub const ON_ACCENT: Color32 = Color32::from_rgb(28, 22, 8);
    /// Finished, sealed, compiled.
    pub const OK: Color32 = Color32::from_rgb(110, 205, 130);
    /// Something to look at before shipping.
    pub const WARN: Color32 = Color32::from_rgb(240, 200, 90);
    /// Something broken. The leak-trace red.
    pub const ERR: Color32 = Color32::from_rgb(255, 90, 90);
    /// Links, a stage heading in a log, the odd thing worth pointing at.
    pub const INFO: Color32 = Color32::from_rgb(120, 170, 255);
}

/// Widget rounding, in points. Small: this is a tool, not a toy.
pub const RADIUS: u8 = 3;

/// Put the theme into a context. Safe to call more than once.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    ctx.style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(6.0, 4.0);
        style.spacing.button_padding = Vec2::new(8.0, 3.0);
        style.spacing.window_margin = egui::Margin::same(10);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.menu_width = 220.0;
        style.spacing.interact_size = Vec2::new(40.0, 20.0);
        style.spacing.indent = 14.0;
        style.spacing.slider_width = 120.0;
        style.spacing.combo_width = 120.0;
        style.spacing.tooltip_width = 360.0;

        style.visuals = visuals();
    });
}

fn visuals() -> egui::Visuals {
    use colors::*;
    let mut v = egui::Visuals::dark();
    let radius = CornerRadius::same(RADIUS);

    v.override_text_color = None;
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_corner_radius = CornerRadius::same(RADIUS + 2);
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(100),
    };
    v.menu_corner_radius = radius;
    v.extreme_bg_color = BG_FIELD;
    v.faint_bg_color = Color32::from_rgb(35, 38, 44);
    v.code_bg_color = BG_FIELD;
    v.hyperlink_color = INFO;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERR;
    v.striped = true;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_MUTED);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = BG_HEADER;
    w.inactive.weak_bg_fill = BG_HEADER;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(70, 76, 88));
    w.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = ACCENT.gamma_multiply(0.45);
    w.active.weak_bg_fill = ACCENT.gamma_multiply(0.45);
    w.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.bg_fill = HOVER;
    w.open.weak_bg_fill = HOVER;
    w.open.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.open.corner_radius = radius;

    v
}

// ---- text ------------------------------------------------------------------
//
// The chains below used to be repeated at every call site. A caption that is
// 11 points in one panel and 10 in the next is the kind of thing nobody
// decides and everybody notices.

/// Small, quiet text: a hint, a section caption, a count.
pub fn caption(text: impl Into<String>) -> RichText {
    RichText::new(text).size(11.0).color(colors::TEXT_MUTED)
}

/// A name, a number, a path: something read exactly.
pub fn mono(text: impl Into<String>) -> RichText {
    RichText::new(text).monospace().size(11.5)
}

/// A panel or dialog heading.
pub fn heading(text: impl Into<String>) -> RichText {
    RichText::new(text).size(15.0).strong()
}

/// The title of a section inside a panel.
pub fn section_title(text: impl Into<String>) -> RichText {
    RichText::new(text.into().to_uppercase())
        .size(10.0)
        .strong()
        .color(colors::TEXT_MUTED)
}

/// Something to look at before shipping.
pub fn warn(text: impl Into<String>) -> RichText {
    RichText::new(text).size(11.5).color(colors::WARN)
}

/// Something broken.
pub fn err(text: impl Into<String>) -> RichText {
    RichText::new(text).size(11.5).color(colors::ERR)
}

/// Something that went right.
pub fn ok(text: impl Into<String>) -> RichText {
    RichText::new(text).size(11.5).color(colors::OK)
}

/// An icon glyph on its own, sized to sit beside text.
pub fn icon(glyph: &str) -> RichText {
    RichText::new(glyph)
        .family(FontFamily::Proportional)
        .size(14.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_installs_and_a_frame_draws() {
        let ctx = egui::Context::default();
        install(&ctx);
        install(&ctx); // and again, because the toolset may be re-entered.
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(heading("Kerosene"));
                ui.label(caption("a caption"));
                ui.label(icon(icons::HAMMER));
                ui.label(mono("maps/arena.keromap"));
            });
        });
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn the_accent_is_the_editors_selection_colour() {
        // draw::colors::SELECTED in Chisel; the two must not drift apart.
        assert_eq!(colors::ACCENT, Color32::from_rgb(255, 190, 70));
    }
}
