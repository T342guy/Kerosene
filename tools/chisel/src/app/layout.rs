// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! How the window is arranged, and remembering it between sessions.
//!
//! An editor that forgets you closed the asset browser, or that you work
//! with two views rather than four, asks you the same question every time
//! it opens. The answers are a few words in a file in the user's data
//! directory -- not in the project, because they are the person's, not the
//! map's.

use super::*;

/// The settings file's name, in the user's data directory.
pub const LAYOUT_FILE: &str = "chisel.layout";

impl ChiselApp {
    /// The pane layout in force.
    pub fn pane_layout(&self) -> PaneLayout {
        if self.maximised.is_some() {
            PaneLayout::One
        } else if self.two_panes {
            PaneLayout::Two
        } else {
            PaneLayout::Four
        }
    }

    pub fn set_pane_layout(&mut self, layout: PaneLayout) {
        match layout {
            PaneLayout::Four => {
                self.maximised = None;
                self.two_panes = false;
            }
            PaneLayout::Two => {
                self.maximised = None;
                self.two_panes = true;
            }
            PaneLayout::One => {
                self.maximised = Some(self.active);
            }
        }
        self.status = layout.label().to_string();
        self.save_layout();
    }

    /// The layout as text: one `key value` line per setting.
    pub fn layout_text(&self) -> String {
        let layout = match self.pane_layout() {
            PaneLayout::Four => "four",
            PaneLayout::Two => "two",
            PaneLayout::One => "one",
        };
        let helpers = match self.helper_mode {
            crate::helpers::HelperMode::Selected => "selected",
            crate::helpers::HelperMode::All => "all",
            crate::helpers::HelperMode::None => "none",
        };
        let shading = match self.shading {
            Shading::Textured => "textured",
            Shading::Flat => "flat",
            Shading::Shaded => "shaded",
            Shading::Walkmap => "walkmap",
        };
        format!(
            "layout {layout}\nassets {}\nhelpers {helpers}\nshading {shading}\nsplit {} {}\nfly {}\n",
            self.show_assets as u8, self.split.x, self.split.y, self.fly_speed
        )
    }

    /// Read a layout written by [`Self::layout_text`]. Anything missing or
    /// unreadable keeps what is there: a settings file from a newer or
    /// older editor should never stop this one opening.
    pub fn apply_layout_text(&mut self, text: &str) {
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some(key), Some(value)) = (words.next(), words.next()) else {
                continue;
            };
            match key {
                "layout" => {
                    self.two_panes = value == "two";
                    self.maximised = (value == "one").then_some(self.active);
                }
                "assets" => self.show_assets = value == "1",
                "helpers" => {
                    self.helper_mode = match value {
                        "all" => crate::helpers::HelperMode::All,
                        "none" => crate::helpers::HelperMode::None,
                        _ => crate::helpers::HelperMode::Selected,
                    }
                }
                "shading" => {
                    self.shading = match value {
                        "flat" => Shading::Flat,
                        "shaded" => Shading::Shaded,
                        "walkmap" => Shading::Walkmap,
                        _ => Shading::Textured,
                    }
                }
                "split" => {
                    if let (Ok(x), Some(Ok(y))) = (value.parse(), words.next().map(str::parse)) {
                        self.split = egui::vec2(x, y);
                    }
                }
                "fly" => {
                    if let Ok(speed) = value.parse::<f32>() {
                        self.fly_speed = speed.clamp(16.0, 8192.0);
                    }
                }
                _ => {}
            }
        }
    }

    /// Where the layout lives, once the host has said: the toolset window
    /// sets it, a test or a headless shot never does, so neither can
    /// overwrite a person's settings.
    pub fn set_layout_file(&mut self, path: std::path::PathBuf) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            self.apply_layout_text(&text);
        }
        self.saved_layout = self.layout_text();
        self.layout_file = Some(path);
    }

    /// Write the layout if it changed since it was last written.
    pub(super) fn save_layout(&mut self) {
        let Some(path) = &self.layout_file else {
            return;
        };
        let text = self.layout_text();
        if text == self.saved_layout {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(path, &text) {
            Ok(()) => self.saved_layout = text,
            Err(e) => log::warn!(
                "could not save the editor layout to {}: {e}",
                path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> ChiselApp {
        ChiselApp::new(std::path::PathBuf::from("/nonexistent"))
    }

    #[test]
    fn a_layout_survives_being_written_and_read_back() {
        let mut a = app();
        a.set_pane_layout(PaneLayout::Two);
        a.show_assets = true;
        a.helper_mode = crate::helpers::HelperMode::All;
        a.shading = Shading::Walkmap;
        a.split = egui::vec2(0.3, 0.6);
        a.fly_speed = 700.0;
        let text = a.layout_text();

        let mut b = app();
        b.apply_layout_text(&text);
        assert_eq!(b.layout_text(), text);
        assert_eq!(b.pane_layout(), PaneLayout::Two);
    }

    #[test]
    fn a_settings_file_it_does_not_understand_changes_nothing() {
        let mut a = app();
        let before = a.layout_text();
        a.apply_layout_text("layout\nsplit banana\nfly -\nfrom the future 3\n\n");
        assert_eq!(a.layout_text(), before);
    }

    #[test]
    fn nothing_is_written_until_the_host_names_a_file() {
        let dir = std::env::temp_dir().join(format!("chisel-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = app();
        a.set_pane_layout(PaneLayout::One);
        assert!(!dir.exists());

        let file = dir.join(LAYOUT_FILE);
        a.set_layout_file(file.clone());
        // Reading an absent file keeps the current layout, and changing it
        // writes it.
        a.set_pane_layout(PaneLayout::Two);
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("layout two"), "{written}");

        let mut b = app();
        b.set_layout_file(file);
        assert_eq!(b.pane_layout(), PaneLayout::Two);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
