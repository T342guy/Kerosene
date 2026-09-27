// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! How the toolset looks.
//!
//! One palette, one spacing scale, one type scale and one icon font,
//! installed into the egui context once by [`crate::run`]. Every tool draws
//! with these rather than its own `Color32::from_rgb` literals, so the
//! editor, the sound tab and a build page read as rooms of one house rather
//! than three programs that happen to share a window.
//!
//! The palette is a layered slate: the canvas darkest, panels a step up,
//! cards and popups a step above that, so depth reads without needing a
//! border on everything. It is low in chroma on purpose, because the things
//! that need colour -- a selection, a leak, a warning -- should be the only
//! colourful things on screen. The selection amber is the same one the
//! editor's panes use for a selected brush, so "selected" looks like one
//! thing whether it is a brush or a tab.

use egui::{
    Color32, CornerRadius, FontDefinitions, FontFamily, FontId, RichText, Stroke, TextStyle, Vec2,
};

pub use egui_phosphor::regular as icons;

/// The palette.
pub mod colors {
    use egui::Color32;

    /// Behind the viewports and anything else that is a canvas.
    pub const BG_APP: Color32 = Color32::from_rgb(16, 18, 22);
    /// Panels: toolbars, the inspector, the status bar, the sidebar.
    pub const BG_PANEL: Color32 = Color32::from_rgb(25, 28, 33);
    /// A card on a page, a popup, anything that sits above a panel.
    pub const BG_ELEVATED: Color32 = Color32::from_rgb(30, 34, 40);
    /// A heading strip inside a panel, the header of a pane, a resting
    /// button.
    pub const BG_HEADER: Color32 = Color32::from_rgb(36, 40, 47);
    /// Text fields and other things you type into.
    pub const BG_FIELD: Color32 = Color32::from_rgb(13, 15, 18);
    /// Where two panels meet.
    pub const BORDER: Color32 = Color32::from_rgb(44, 49, 58);
    /// A border that should be seen: a focused field, a hovered card.
    pub const BORDER_STRONG: Color32 = Color32::from_rgb(66, 73, 86);
    /// Anything you can hover.
    pub const HOVER: Color32 = Color32::from_rgb(46, 52, 62);
    /// Ordinary text.
    pub const TEXT: Color32 = Color32::from_rgb(224, 228, 235);
    /// Captions, hints, and anything that is there for when you need it.
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(140, 149, 164);
    /// Text that is there to be skipped: a placeholder, a disabled label.
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(92, 100, 114);
    /// Selected, active, current. The editor's selection amber.
    pub const ACCENT: Color32 = Color32::from_rgb(255, 190, 70);
    /// The accent laid thinly over a panel: a selected row, an active tab.
    pub const ACCENT_SOFT: Color32 = Color32::from_rgb(64, 55, 36);
    /// Text drawn on top of the accent.
    pub const ON_ACCENT: Color32 = Color32::from_rgb(28, 22, 8);
    /// Finished, sealed, compiled.
    pub const OK: Color32 = Color32::from_rgb(98, 206, 140);
    /// Something to look at before shipping.
    pub const WARN: Color32 = Color32::from_rgb(240, 196, 84);
    /// Something broken. The leak-trace red.
    pub const ERR: Color32 = Color32::from_rgb(246, 96, 96);
    /// Links, a stage heading in a log, the odd thing worth pointing at.
    pub const INFO: Color32 = Color32::from_rgb(112, 168, 255);
}

/// Widget rounding, in points.
pub const RADIUS: u8 = 5;
/// Rounding for cards and dialogs: a step softer than a button.
pub const RADIUS_LARGE: u8 = 8;

/// The spacing scale. Every gap in the toolset is one of these, which is
/// most of what makes a page look arranged rather than accumulated.
pub const SPACE_XS: f32 = 4.0;
/// See [`SPACE_XS`].
pub const SPACE_SM: f32 = 8.0;
/// See [`SPACE_XS`].
pub const SPACE_MD: f32 = 12.0;
/// See [`SPACE_XS`].
pub const SPACE_LG: f32 = 20.0;
/// See [`SPACE_XS`].
pub const SPACE_XL: f32 = 32.0;

/// Put the theme into a context. Safe to call more than once.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    ctx.style_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, FontId::proportional(18.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();

        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 4.0);
        style.spacing.window_margin = egui::Margin::same(12);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.menu_width = 230.0;
        style.spacing.interact_size = Vec2::new(40.0, 24.0);
        style.spacing.indent = 14.0;
        style.spacing.slider_width = 130.0;
        style.spacing.combo_width = 130.0;
        style.spacing.tooltip_width = 360.0;
        style.spacing.scroll = egui::style::ScrollStyle::thin();

        style.visuals = visuals();
    });
}

fn visuals() -> egui::Visuals {
    use colors::*;
    let mut v = egui::Visuals::dark();
    let radius = CornerRadius::same(RADIUS);

    v.override_text_color = None;
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_ELEVATED;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_corner_radius = CornerRadius::same(RADIUS_LARGE);
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    v.menu_corner_radius = CornerRadius::same(RADIUS + 1);
    v.extreme_bg_color = BG_FIELD;
    v.faint_bg_color = Color32::from_rgb(29, 32, 38);
    v.code_bg_color = BG_FIELD;
    v.hyperlink_color = INFO;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERR;
    v.striped = true;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.32);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.text_cursor.stroke = Stroke::new(2.0_f32, ACCENT);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_MUTED);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = BG_HEADER;
    w.inactive.weak_bg_fill = BG_HEADER;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = ACCENT.gamma_multiply(0.42);
    w.active.weak_bg_fill = ACCENT.gamma_multiply(0.42);
    w.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.bg_fill = HOVER;
    w.open.weak_bg_fill = HOVER;
    w.open.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
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
    RichText::new(text).size(11.5).color(colors::TEXT_MUTED)
}

/// A name, a number, a path: something read exactly.
pub fn mono(text: impl Into<String>) -> RichText {
    RichText::new(text).monospace().size(12.0)
}

/// A panel or dialog heading.
pub fn heading(text: impl Into<String>) -> RichText {
    RichText::new(text).size(16.0).strong().color(colors::TEXT)
}

/// The title of a page: the largest text in the toolset.
pub fn title(text: impl Into<String>) -> RichText {
    RichText::new(text).size(24.0).strong().color(colors::TEXT)
}

/// The line under a page's title.
pub fn subtitle(text: impl Into<String>) -> RichText {
    RichText::new(text).size(13.0).color(colors::TEXT_MUTED)
}

/// The title of a section inside a panel.
pub fn section_title(text: impl Into<String>) -> RichText {
    RichText::new(text.into().to_uppercase())
        .size(10.5)
        .strong()
        .extra_letter_spacing(0.6)
        .color(colors::TEXT_MUTED)
}

/// Something to look at before shipping.
pub fn warn(text: impl Into<String>) -> RichText {
    RichText::new(text).size(12.0).color(colors::WARN)
}

/// Something broken.
pub fn err(text: impl Into<String>) -> RichText {
    RichText::new(text).size(12.0).color(colors::ERR)
}

/// Something that went right.
pub fn ok(text: impl Into<String>) -> RichText {
    RichText::new(text).size(12.0).color(colors::OK)
}

/// An icon glyph on its own, sized to sit beside text.
pub fn icon(glyph: &str) -> RichText {
    RichText::new(glyph)
        .family(FontFamily::Proportional)
        .size(15.0)
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
                ui.label(title("Kerosene"));
                ui.label(subtitle("a project"));
                ui.label(heading("Build"));
                ui.label(caption("a caption"));
                ui.label(icon(icons::HAMMER));
                ui.label(mono("maps/arena.kmap"));
            });
        });
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn the_accent_is_the_editors_selection_colour() {
        // draw::colors::SELECTED in Chisel; the two must not drift apart.
        assert_eq!(colors::ACCENT, Color32::from_rgb(255, 190, 70));
    }

    #[test]
    fn the_layers_get_lighter_as_they_come_forward() {
        let luma = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        assert!(luma(colors::BG_FIELD) < luma(colors::BG_APP));
        assert!(luma(colors::BG_APP) < luma(colors::BG_PANEL));
        assert!(luma(colors::BG_PANEL) < luma(colors::BG_ELEVATED));
        assert!(luma(colors::BG_ELEVATED) < luma(colors::BG_HEADER));
        assert!(luma(colors::TEXT_FAINT) < luma(colors::TEXT_MUTED));
        assert!(luma(colors::TEXT_MUTED) < luma(colors::TEXT));
    }
}
