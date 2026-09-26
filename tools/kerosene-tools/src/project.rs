// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The Project tab: where the toolset opens, and what it found.
//!
//! An editor that opens straight onto an empty map answers none of the
//! questions a person has on arriving -- which project is this, where is
//! its content, how much of it is there, what was I working on. This tab
//! answers them and offers the three things people come to do: open a map,
//! build, pack.

use std::path::{Path, PathBuf};

use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets;

/// What the tab asks the toolset to do, acted on after it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectAction {
    OpenMap(PathBuf),
    NewMap,
    Build,
    Pack,
}

/// A count of one kind of content, with the file extension it was counted by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Count {
    pub label: &'static str,
    pub glyph: &'static str,
    pub count: usize,
    /// What the count means: "compiled" or "source".
    pub note: &'static str,
}

/// What the content tree holds, counted once and again on request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub counts: Vec<Count>,
    pub maps: Vec<PathBuf>,
}

impl Inventory {
    /// Walk the content tree. A directory that is not there counts zero,
    /// which is the honest answer rather than an error.
    pub fn of(content: &Path) -> Inventory {
        let maps = chisel::files::maps_in(content);
        let count = |dir: &str, ext: &str| count_by_extension(&content.join(dir), ext);
        Inventory {
            counts: vec![
                Count {
                    label: "maps",
                    glyph: icons::MAP_TRIFOLD,
                    count: maps.len(),
                    note: "source",
                },
                Count {
                    label: "compiled maps",
                    glyph: icons::CUBE,
                    count: count("maps", "kerobsp"),
                    note: "compiled",
                },
                Count {
                    label: "materials",
                    glyph: icons::PAINT_BUCKET,
                    count: count("materials", "keromat"),
                    note: "compiled",
                },
                Count {
                    label: "models",
                    glyph: icons::PACKAGE,
                    count: count("models", "keromdl"),
                    note: "compiled",
                },
                Count {
                    label: "sounds",
                    glyph: icons::SPEAKER_HIGH,
                    count: count("sound", "keroaud"),
                    note: "compiled",
                },
                Count {
                    label: "scripts",
                    glyph: icons::SCROLL,
                    count: count("scripts", "rhai"),
                    note: "source",
                },
            ],
            maps,
        }
    }
}

fn count_by_extension(dir: &Path, extension: &str) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                count_by_extension(&path, extension)
            } else {
                usize::from(path.extension().and_then(|e| e.to_str()) == Some(extension))
            }
        })
        .sum()
}

/// The tab.
pub struct ProjectPanel {
    content_root: PathBuf,
    /// The project file and its name, when a `.keroproj` named the tree.
    project: Option<(PathBuf, String, Option<String>)>,
    /// How the content was found, in the search's own words.
    found_note: String,
    inventory: Inventory,
}

impl ProjectPanel {
    pub fn new(
        content_root: PathBuf,
        project: Option<&kerosene_vfs::Project>,
        found_note: String,
    ) -> ProjectPanel {
        ProjectPanel {
            inventory: Inventory::of(&content_root),
            project: project.map(|p| (p.path.clone(), p.name.clone(), p.start_map.clone())),
            found_note,
            content_root,
        }
    }

    /// Count again: a build or a save changed what is there.
    pub fn refresh(&mut self) {
        self.inventory = Inventory::of(&self.content_root);
    }

    /// The project's name, for the activity bar.
    pub fn name(&self) -> String {
        match &self.project {
            Some((_, name, _)) => name.clone(),
            None => self
                .content_root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "no project".to_string()),
        }
    }

    pub fn content_root(&self) -> &Path {
        &self.content_root
    }

    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        building: bool,
        packing: bool,
    ) -> Option<ProjectAction> {
        let mut action = None;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_APP)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(820.0);
                        self.header(ui);
                        ui.add_space(18.0);
                        if let Some(a) = self.actions(ui, building, packing) {
                            action = Some(a);
                        }
                        ui.add_space(18.0);
                        self.counts(ui);
                        ui.add_space(18.0);
                        if let Some(a) = self.maps(ui) {
                            action = Some(a);
                        }
                    });
            });
        action
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(theme::icon(icons::HOUSE).size(26.0).color(colors::ACCENT));
            ui.label(egui::RichText::new(self.name()).size(24.0).strong());
        });
        ui.add_space(6.0);
        match &self.project {
            Some((path, _, start)) => {
                widgets::fact(ui, "project", path.display().to_string());
                widgets::fact(ui, "content", self.content_root.display().to_string());
                if let Some(start) = start {
                    widgets::fact(ui, "starts on", start.clone());
                }
            }
            None => {
                widgets::fact(ui, "content", self.content_root.display().to_string());
                ui.label(theme::caption(&self.found_note));
                ui.label(theme::caption(format!(
                    "{}  No .keroproj names this tree, so it was inferred. \
                     `kerosene-tools init` writes one.",
                    icons::INFO
                )));
            }
        }
    }

    fn actions(&self, ui: &mut egui::Ui, building: bool, packing: bool) -> Option<ProjectAction> {
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if widgets::primary_button(
                ui,
                egui::RichText::new(format!("{}  New map", icons::FILE_PLUS))
                    .size(13.0)
                    .color(colors::ON_ACCENT),
            )
            .clicked()
            {
                action = Some(ProjectAction::NewMap);
            }
            let build = egui::Button::new(
                egui::RichText::new(if building {
                    format!("{}  Building...", icons::CIRCLE_NOTCH)
                } else {
                    format!("{}  Build everything", icons::HAMMER)
                })
                .size(13.0),
            );
            if ui
                .add_enabled(!building, build)
                .on_hover_text(
                    "Every stage, over the whole tree: kiln. The Build tab picks stages.",
                )
                .clicked()
            {
                action = Some(ProjectAction::Build);
            }
            let pack = egui::Button::new(
                egui::RichText::new(if packing {
                    format!("{}  Packing...", icons::CIRCLE_NOTCH)
                } else {
                    format!("{}  Pack archive", icons::ARCHIVE)
                })
                .size(13.0),
            );
            if ui
                .add_enabled(!packing, pack)
                .on_hover_text("Write the .vault the game ships with: vault pack.")
                .clicked()
            {
                action = Some(ProjectAction::Pack);
            }
        });
        action
    }

    fn counts(&self, ui: &mut egui::Ui) {
        ui.label(theme::section_title("content"));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            for count in &self.inventory.counts {
                card(ui, count);
            }
        });
    }

    fn maps(&self, ui: &mut egui::Ui) -> Option<ProjectAction> {
        let mut action = None;
        ui.label(theme::section_title("maps"));
        ui.add_space(6.0);
        if self.inventory.maps.is_empty() {
            ui.label(theme::caption(
                "No maps yet. New map opens the editor on a starter room.",
            ));
            return None;
        }
        for map in &self.inventory.maps {
            let name = chisel::files::label(map, &self.content_root);
            let name = name.strip_prefix("maps/").unwrap_or(&name).to_string();
            let compiled = map.with_extension("kerobsp").exists();
            let row = ui.horizontal(|ui| {
                ui.label(theme::icon(icons::MAP_TRIFOLD).color(colors::TEXT_MUTED));
                let clicked = ui
                    .add(egui::Button::new(theme::mono(&name).color(colors::TEXT)).frame(false))
                    .clicked();
                if compiled {
                    ui.label(theme::caption("compiled"));
                } else {
                    ui.label(theme::warn("never compiled").size(11.0));
                }
                clicked
            });
            if row.inner {
                action = Some(ProjectAction::OpenMap(map.clone()));
            }
        }
        action
    }
}

/// One count, as a tile.
fn card(ui: &mut egui::Ui, count: &Count) {
    egui::Frame::new()
        .fill(colors::BG_PANEL)
        .stroke(egui::Stroke::new(1.0_f32, colors::BORDER))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS + 2))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(120.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(count.glyph).color(colors::TEXT_MUTED));
                    ui.label(theme::caption(count.label));
                });
                ui.label(
                    egui::RichText::new(count.count.to_string())
                        .size(22.0)
                        .strong()
                        .color(if count.count == 0 {
                            colors::TEXT_MUTED
                        } else {
                            colors::TEXT
                        }),
                );
                ui.label(theme::caption(count.note));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_content_is_counted() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let inventory = Inventory::of(&root);
        let maps = inventory.counts.iter().find(|c| c.label == "maps").unwrap();
        assert_eq!(maps.count, inventory.maps.len());
        assert!(maps.count > 0, "the repository ships at least one map");
    }

    #[test]
    fn a_missing_tree_counts_zero_rather_than_failing() {
        let inventory = Inventory::of(Path::new("/nowhere/at/all"));
        assert!(inventory.counts.iter().all(|c| c.count == 0));
        assert!(inventory.maps.is_empty());
    }

    #[test]
    fn the_tab_draws_with_and_without_a_project() {
        let ctx = egui::Context::default();
        kerosene_toolui::theme::install(&ctx);
        let mut panel = ProjectPanel::new(PathBuf::from("/nowhere"), None, "inferred".into());
        assert_eq!(panel.name(), "nowhere");
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            assert_eq!(panel.ui(ctx, false, false), None);
        });
        assert!(!output.shapes.is_empty());
    }
}
