// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Cut, copy and paste, and the autosave that guards the work between saves.
//!
//! Two things that have nothing to do with each other except that both are
//! about not losing what you made: one moves objects around inside a
//! session, the other gets a session back after a crash.

use super::*;
use crate::autosave;
use crate::document::Clipboard;
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

/// What the autosave last wrote, and when.
#[derive(Debug, Default)]
pub struct AutosaveState {
    /// The `egui` time of the last check; `None` until the first frame, so
    /// a host whose clock is the time of day does not autosave at once.
    last: Option<f64>,
    /// The map and revision last written, so an idle map is not rewritten.
    written: Option<(Option<PathBuf>, u64)>,
}

impl ChiselApp {
    /// Copy the selection to the editor's clipboard.
    pub(super) fn copy(&mut self) {
        self.commit_properties();
        let clip = self.document.copy_selection();
        if clip.is_empty() {
            return;
        }
        self.status = format!("copied {}", clip.len());
        self.clipboard = clip;
        // The first paste lands one step over, so it is visibly a copy.
        self.pastes = 1;
    }

    /// Copy the selection, then delete it.
    pub(super) fn cut(&mut self) {
        self.commit_properties();
        let clip = self.document.cut_selection();
        if clip.is_empty() {
            return;
        }
        self.status = format!("cut {}", clip.len());
        self.clipboard = clip;
        // A cut is a move: the first paste goes back where it came from.
        self.pastes = 0;
    }

    /// Paste the clipboard, a grid step further over each time.
    pub(super) fn paste(&mut self) {
        self.commit_properties();
        let step = self.document.grid.size * self.pastes as f32;
        let n = self
            .document
            .paste(&self.clipboard, Vec3::new(step, step, 0.0), "paste");
        if n > 0 {
            self.pastes += 1;
            self.status = format!("pasted {n}");
        }
    }

    // ---- autosave --------------------------------------------------------

    /// Called every frame: write the autosave when a minute has passed and
    /// the map has changed since the last one.
    pub(super) fn tick_autosave(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        // Idle frames are not guaranteed, so ask for one when the next is due.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(autosave::INTERVAL));
        let last = *self.autosave.last.get_or_insert(now);
        if now - last < autosave::INTERVAL {
            return;
        }
        self.autosave.last = Some(now);
        self.write_autosave();
    }

    /// Write the autosave now, if the map has unsaved changes not yet in it.
    pub fn write_autosave(&mut self) -> bool {
        if !self.document.is_modified() {
            return false;
        }
        let key = (self.document.path.clone(), self.document.revision());
        if self.autosave.written.as_ref() == Some(&key) {
            return false;
        }
        let path = autosave::path_for(self.document.path.as_deref(), &self.content_root);
        match autosave::write(&self.document.map, &path) {
            Ok(()) => {
                self.autosave.written = Some(key);
                true
            }
            Err(e) => {
                self.status = format!("autosave failed: {e}");
                false
            }
        }
    }

    /// Offer the autosave of the map just opened, if it is newer than the
    /// map. With no map open, offers the untitled one.
    pub fn offer_recovery(&mut self) {
        let path = autosave::path_for(self.document.path.as_deref(), &self.content_root);
        if autosave::is_newer(&path, self.document.path.as_deref()) {
            self.recovery = Some(path);
        }
    }

    /// Replace the map with the autosave's.
    pub(super) fn recover(&mut self, path: &Path) {
        let recovered = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| kerosene_map::Map::parse(&text).map_err(|e| e.to_string()));
        match recovered {
            Ok(map) => {
                self.document.apply("recover autosave", |doc| {
                    doc.map = map;
                    doc.selection.clear();
                });
                self.frame_all();
                self.status = "recovered the autosave; save to keep it".into();
            }
            Err(e) => self.status = format!("could not read the autosave: {e}"),
        }
    }

    /// The question asked when an autosave is newer than its map.
    pub(super) fn recovery_window(&mut self, ctx: &Context) {
        let Some(path) = self.recovery.clone() else {
            return;
        };
        enum Choice {
            Recover,
            Discard,
            Later,
        }
        let mut choice = None;
        let age = autosave::age_label(&path);
        let modal = widgets::dialog(
            ctx,
            "chisel-recover",
            "Recover unsaved work?",
            380.0,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(icons::WARNING).size(20.0).color(colors::WARN));
                    ui.label(format!(
                        "{} was autosaved {age}, after the last save. The editor may have closed before it could be saved.",
                        self.document.title()
                    ));
                });
            },
            |ui| {
                if ui.button("ask me later").clicked() {
                    choice = Some(Choice::Later)
                }
                if ui.button("discard it").clicked() {
                    choice = Some(Choice::Discard)
                }
                if widgets::primary_button(ui, RichText::new("recover").color(colors::ON_ACCENT))
                    .clicked()
                {
                    choice = Some(Choice::Recover)
                }
            },
        );
        // Escape keeps the file: the answer that loses nothing.
        if modal.should_close() {
            choice = Some(Choice::Later)
        }
        match choice {
            Some(Choice::Recover) => {
                self.recovery = None;
                self.recover(&path);
            }
            Some(Choice::Discard) => {
                self.recovery = None;
                autosave::remove(&path);
            }
            Some(Choice::Later) => self.recovery = None,
            None => {}
        }
    }
}

impl Clipboard {
    /// A short description for the edit menu: "3 objects".
    pub fn describe(&self) -> String {
        match self.len() {
            1 => "1 object".into(),
            n => format!("{n} objects"),
        }
    }
}
