// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The sidebar down the left edge: the mark, one item per tab, and at the
//! bottom the search, the output panel's switch and the project menu.

use egui::{Align2, FontId, RectAlign, Sense, Vec2};
use kerosene_toolui::App as _;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

use super::{Action, Tab, Toolset};

/// How wide the sidebar is: an icon with a short label under it.
const WIDTH: f32 = 72.0;

impl Toolset {
    pub(super) fn sidebar(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        egui::SidePanel::left("kerosene-sidebar")
            .exact_width(WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(0, 10)),
            )
            .show(ctx, |ui| {
                // A line on the right edge, where the sidebar meets the page.
                let edge = ui.max_rect().right() + 0.5;
                ui.painter().vline(
                    edge,
                    ui.clip_rect().y_range(),
                    egui::Stroke::new(1.0_f32, colors::BORDER),
                );
                ui.spacing_mut().item_spacing.y = 2.0;

                if mark(ui).clicked() {
                    actions.push(Action::Goto(Tab::Home));
                }
                ui.add_space(theme::SPACE_MD);

                let running = self.running();
                for tab in Tab::ALL {
                    let busy = match tab {
                        Tab::Editor => running[super::SOURCE_COMPILE],
                        Tab::Build => running[super::SOURCE_BUILD],
                        Tab::Archive => running[super::SOURCE_ARCHIVE],
                        Tab::Sound => self
                            .sound
                            .as_ref()
                            .is_some_and(|s| s.wants_continuous_redraw()),
                        _ => false,
                    };
                    let selected = !self.showing_start && self.tab == tab;
                    let enabled = self.found || matches!(tab, Tab::Editor);
                    let response = ui
                        .add_enabled_ui(enabled, |ui| {
                            widgets::sidebar_item(
                                ui,
                                tab.glyph(),
                                tab.name(),
                                Some(tab.shortcut()),
                                selected,
                                busy,
                            )
                        })
                        .inner;
                    if response.clicked() {
                        actions.push(Action::Goto(tab));
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    self.project_menu(ui, actions);
                    ui.add_space(theme::SPACE_XS);
                    let mut open = self.output.open;
                    if widgets::icon_toggle(
                        ui,
                        icons::TERMINAL_WINDOW,
                        "Output panel  (ctrl-`)",
                        &mut open,
                    )
                    .clicked()
                    {
                        actions.push(Action::ToggleOutput);
                    }
                    if widgets::icon_button(
                        ui,
                        icons::MAGNIFYING_GLASS,
                        "Search everything  (ctrl-P)",
                    )
                    .clicked()
                    {
                        actions.push(Action::OpenPalette);
                    }
                });
            });
    }

    /// The project's initials in a circle, and a menu of projects behind it.
    fn project_menu(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let name = if self.found {
            self.info.name.clone()
        } else {
            "No project".to_string()
        };
        let initials: String = name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .take(2)
            .filter_map(|w| w.chars().next())
            .collect::<String>()
            .to_uppercase();
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
        let painter = ui.painter();
        painter.circle_filled(
            rect.center(),
            16.0,
            if response.hovered() {
                colors::HOVER
            } else {
                colors::BG_HEADER
            },
        );
        painter.circle_stroke(
            rect.center(),
            16.0,
            egui::Stroke::new(1.0_f32, colors::BORDER_STRONG),
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            if initials.is_empty() { "?" } else { &initials },
            FontId::proportional(12.0),
            colors::TEXT,
        );
        let response = response.on_hover_text(format!(
            "{name}\n{}",
            super::super::pages::home::short(&self.info.content)
        ));

        egui::Popup::menu(&response)
            .align(RectAlign::RIGHT_END)
            .gap(8.0)
            .width(260.0)
            .show(|ui| {
                ui.label(egui::RichText::new(&name).strong());
                if self.found {
                    ui.label(theme::caption(super::super::pages::home::short(
                        &self.info.content,
                    )));
                }
                ui.separator();
                let others: Vec<_> = self
                    .recent
                    .entries
                    .iter()
                    .filter(|e| e.content != self.info.content && e.exists())
                    .take(6)
                    .collect();
                if !others.is_empty() {
                    ui.label(theme::section_title("recent"));
                    for entry in others {
                        if ui
                            .button(format!("{}  {}", icons::FOLDER_SIMPLE, entry.name))
                            .on_hover_text(entry.content.display().to_string())
                            .clicked()
                        {
                            actions.push(Action::SwitchProject(entry.content.clone()));
                        }
                    }
                    ui.separator();
                }
                if ui
                    .button(format!("{}  Switch project...", icons::SWAP))
                    .clicked()
                {
                    actions.push(Action::ShowStart);
                }
                if self.found
                    && ui
                        .button(format!("{}  Show content folder", icons::FOLDER_OPEN))
                        .clicked()
                {
                    actions.push(Action::Reveal(self.info.content.clone()));
                }
            });
    }
}

/// The mark at the top, so the window is recognisable in a taskbar of grey
/// rectangles.
fn mark(ui: &mut egui::Ui) -> egui::Response {
    let size = Vec2::new(ui.available_width(), 40.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let tile = egui::Rect::from_center_size(rect.center(), Vec2::splat(36.0));
    ui.painter().rect_filled(
        tile,
        egui::CornerRadius::same(theme::RADIUS_LARGE + 1),
        colors::ACCENT,
    );
    ui.painter().text(
        tile.center(),
        Align2::CENTER_CENTER,
        icons::FIRE,
        FontId::proportional(21.0),
        colors::ON_ACCENT,
    );
    response.on_hover_text("Kerosene")
}
