// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Getting around a big map: go to an object by id or name, and camera
//! bookmarks.
//!
//! Cleave's warnings say `brush 12`, and a level with four hundred brushes
//! and a `logic_relay` named three weeks ago is not navigated by scrolling.

use super::*;
use crate::viewport::Viewport;
use kerosene_toolui::theme::{self, colors};
use kerosene_toolui::widgets;

/// How many bookmarks there are: one per number key.
pub const BOOKMARKS: usize = 9;

/// The go-to box.
#[derive(Clone, Debug, Default)]
pub struct GoTo {
    pub open: bool,
    pub query: String,
    /// Set the frame it opens, to give the field the keyboard once.
    fresh: bool,
    error: Option<String>,
}

impl ChiselApp {
    /// Open the go-to box.
    pub(super) fn begin_go_to(&mut self) {
        self.go_to = GoTo {
            open: true,
            fresh: true,
            ..GoTo::default()
        };
    }

    /// Select what the query names, and fly to it.
    pub fn go_to(&mut self, query: &str) -> bool {
        self.commit_properties();
        let n = self.document.go_to(query);
        if n == 0 {
            self.status = format!("nothing matches {:?}", query.trim());
            return false;
        }
        self.frame_all();
        self.status = format!("went to {n} matching {:?}", query.trim());
        true
    }

    /// Remember where the active pane is looking.
    pub(super) fn set_bookmark(&mut self, slot: usize) {
        self.bookmarks[slot] = Some(self.viewports[self.active].clone());
        self.status = format!("bookmark {} set", slot + 1);
    }

    /// Put the active pane back where a bookmark was taken.
    pub(super) fn go_to_bookmark(&mut self, slot: usize) {
        let Some(mark) = self.bookmarks[slot].clone() else {
            self.status = format!(
                "bookmark {} is empty; ctrl-alt-{} sets it",
                slot + 1,
                slot + 1
            );
            return;
        };
        let pane = &mut self.viewports[self.active];
        let size = pane.size;
        *pane = Viewport { size, ..mark };
        self.status = format!("bookmark {}", slot + 1);
    }

    pub(super) fn go_to_window(&mut self, ctx: &Context) {
        if !self.go_to.open {
            return;
        }
        let mut query = std::mem::take(&mut self.go_to.query);
        let fresh = std::mem::take(&mut self.go_to.fresh);
        let error = self.go_to.error.clone();
        let (mut cancel, mut confirm) = (false, false);
        let modal = widgets::dialog(
            ctx,
            "chisel-goto",
            "Go to",
            360.0,
            |ui| {
                ui.label(theme::caption("an id, or part of a name or class"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .desired_width(f32::INFINITY)
                        .hint_text("12  or  gate"),
                );
                if fresh {
                    field.request_focus();
                }
                if let Some(error) = &error {
                    ui.label(theme::err(error));
                }
                field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
            },
            |ui| {
                if widgets::primary_button(ui, RichText::new("go").color(colors::ON_ACCENT))
                    .clicked()
                {
                    confirm = true
                }
                if ui.button("cancel").clicked() {
                    cancel = true
                }
            },
        );
        confirm |= modal.inner.0;
        cancel |= modal.should_close();
        self.go_to.query = query.clone();
        if cancel {
            self.go_to.open = false;
        } else if confirm {
            if self.go_to(&query) {
                self.go_to.open = false;
            } else {
                self.go_to.error = Some(format!("nothing matches {:?}", query.trim()));
            }
        }
    }
}
