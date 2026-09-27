// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The Archive tab: the `.vault` a game ships, over `kerosene-tools vault`.
//!
//! Packing and verifying are jobs, like a build, and log to the output
//! panel. What is *in* the archive is not a job: its index is read here,
//! in-process, and shown as a table that can be searched -- the question
//! "did my new map make it in" deserves a better answer than scrolling a
//! log for it.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{Align, Layout, Ui};
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets::{self, Kind as ButtonKind, Tone};

use super::assets::Index;
use super::home::{job_line, short};
use crate::job::Job;
use crate::toolset::{Action, JobKind};

/// What the archive's index says, read when the file changes.
struct Listing {
    /// The file's modification time when it was read, to know when to read
    /// it again.
    read_at: Option<SystemTime>,
    entries: Result<Vec<(String, u64)>, String>,
}

/// The tab.
pub struct ArchivePage {
    content: PathBuf,
    /// The archive Kiln builds for this project, so pack, verify and the
    /// listing all mean the file the build tab wrote.
    pub archive: PathBuf,
    pub job: Option<Job>,
    listing: Option<Listing>,
    filter: String,
}

impl ArchivePage {
    pub fn new(content: PathBuf, archive: PathBuf) -> ArchivePage {
        ArchivePage {
            content,
            archive,
            job: None,
            listing: None,
            filter: String::new(),
        }
    }

    pub fn running(&self) -> bool {
        self.job.as_ref().is_some_and(Job::running)
    }

    /// Write the archive. The Home tab and the palette call this too.
    pub fn pack(&mut self) {
        if self.running() {
            return;
        }
        let mut args = vec![
            "pack".to_string(),
            self.content.display().to_string(),
            "-o".to_string(),
            self.archive.display().to_string(),
        ];
        for ext in kiln::PACKED {
            args.push("--ext".to_string());
            args.push((*ext).to_string());
        }
        self.job = Some(Job::start("Pack", "vault", &args));
    }

    /// Read every entry back and check its hash.
    pub fn verify(&mut self) {
        if self.running() {
            return;
        }
        let args = ["verify".to_string(), self.archive.display().to_string()];
        self.job = Some(Job::start("Verify", "vault", &args));
    }

    /// Read the archive's index again, if the file changed since.
    fn refresh_listing(&mut self) {
        let modified = std::fs::metadata(&self.archive)
            .and_then(|m| m.modified())
            .ok();
        if self.listing.as_ref().is_some_and(|l| l.read_at == modified) {
            return;
        }
        let entries = if modified.is_none() {
            Ok(Vec::new())
        } else {
            kerosene_vfs::Archive::open(&self.archive)
                .map(|a| {
                    a.entries()
                        .iter()
                        .map(|e| (e.path.clone(), e.size))
                        .collect()
                })
                .map_err(|e| format!("{e:#}"))
        };
        self.listing = Some(Listing {
            read_at: modified,
            entries,
        });
    }

    pub fn ui(&mut self, ctx: &egui::Context, index: &Index) -> Option<Action> {
        let mut action = None;
        let busy = self.running();
        if !busy {
            self.refresh_listing();
        }
        let meta = std::fs::metadata(&self.archive).ok();
        let written = meta.as_ref().and_then(|m| m.modified().ok());
        let behind = written.map(|w| newer_than(index, w)).unwrap_or(0);

        widgets::page(ctx, 900.0, |ui| {
            widgets::page_header(
                ui,
                icons::ARCHIVE,
                "Archive",
                "The one file the game reads its content from",
                |ui| {
                    if widgets::button(ui, ButtonKind::Ghost, icons::FOLDER_OPEN, "")
                        .on_hover_text("Show the archive's folder")
                        .clicked()
                    {
                        action = Some(Action::Reveal(self.archive.clone()));
                    }
                },
            );

            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(icons::PACKAGE).size(28.0).color(colors::ACCENT));
                    ui.add_space(theme::SPACE_XS);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let name = self
                            .archive
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        ui.label(egui::RichText::new(name).size(15.0).strong());
                        ui.label(theme::caption(short(&self.archive)));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &meta {
                        None => {
                            widgets::badge(ui, "not written yet", Tone::Neutral);
                        }
                        Some(meta) => {
                            if behind > 0 {
                                widgets::badge(ui, &format!("{behind} newer files"), Tone::Warn)
                                    .on_hover_text(
                                        "Compiled files changed since the archive was packed.",
                                    );
                            } else {
                                widgets::badge(ui, "up to date", Tone::Ok);
                            }
                            let age = written
                                .and_then(|w| w.elapsed().ok())
                                .map(widgets::human_age)
                                .unwrap_or_default();
                            ui.label(theme::caption(format!(
                                "{} · packed {age}",
                                widgets::human_size(meta.len())
                            )));
                        }
                    });
                });
                ui.add_space(theme::SPACE_MD);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        if widgets::button(ui, ButtonKind::Primary, icons::PACKAGE, "Pack")
                            .on_hover_text("Everything compiled, into the archive the game reads.")
                            .clicked()
                        {
                            action = Some(Action::Pack);
                        }
                    });
                    ui.add_enabled_ui(!busy && meta.is_some(), |ui| {
                        if widgets::button(ui, ButtonKind::Secondary, icons::SEAL_CHECK, "Verify")
                            .on_hover_text("Read every entry back and check its hash.")
                            .clicked()
                        {
                            action = Some(Action::Verify);
                        }
                    });
                    if busy
                        && widgets::button(ui, ButtonKind::Danger, icons::STOP, "Cancel").clicked()
                    {
                        action = Some(Action::Cancel(JobKind::Archive));
                    }
                });
                if let Some(job) = &self.job {
                    ui.add_space(theme::SPACE_SM);
                    ui.separator();
                    job_line(ui, job);
                }
            });

            ui.add_space(theme::SPACE_LG);
            self.contents(ui);
        });
        action
    }

    fn contents(&mut self, ui: &mut Ui) {
        let Some(listing) = &self.listing else {
            return;
        };
        let filter = &mut self.filter;
        match &listing.entries {
            Err(e) => {
                widgets::card(ui, |ui| {
                    ui.label(theme::err(format!("The archive could not be read: {e}")));
                });
            }
            Ok(entries) if entries.is_empty() => {
                widgets::empty_state(
                    ui,
                    icons::ARCHIVE,
                    "Nothing packed yet",
                    "Pack writes every compiled file into the archive.",
                );
            }
            Ok(entries) => {
                let total: u64 = entries.iter().map(|(_, s)| s).sum();
                let title = format!(
                    "contents · {} files · {}",
                    entries.len(),
                    widgets::human_size(total)
                );
                widgets::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::section_title(&title));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            widgets::search_field(ui, filter, "Filter entries", 220.0);
                        });
                    });
                    ui.add_space(theme::SPACE_SM);
                    {
                        let needle = filter.to_lowercase();
                        let shown: Vec<&(String, u64)> = entries
                            .iter()
                            .filter(|(p, _)| {
                                needle.is_empty() || p.to_lowercase().contains(&needle)
                            })
                            .collect();
                        let row = 22.0;
                        egui::ScrollArea::vertical()
                            .max_height(420.0)
                            .auto_shrink([false, true])
                            .show_rows(ui, row, shown.len(), |ui, range| {
                                for (path, size) in &shown[range] {
                                    ui.horizontal(|ui| {
                                        ui.set_height(row - 4.0);
                                        ui.label(theme::mono(path.as_str()).color(colors::TEXT));
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(theme::caption(widgets::human_size(
                                                    *size,
                                                )));
                                            },
                                        );
                                    });
                                }
                            });
                    }
                });
            }
        }
    }
}

/// How many packable files changed after `written`.
fn newer_than(index: &Index, written: SystemTime) -> usize {
    index
        .entries
        .iter()
        .filter(|e| packed(&e.path))
        .filter(|e| e.modified.is_some_and(|m| m > written))
        .count()
}

fn packed(path: &Path) -> bool {
    kiln::PACKED
        .iter()
        .any(|ext| kerosene_vfs::ext::is(path, ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_draws_with_no_archive_and_with_a_real_one() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/kerosene-engine/base/base.vault");
        for archive in [PathBuf::from("/nowhere/game.vault"), base] {
            let mut page = ArchivePage::new(PathBuf::from("/nowhere"), archive);
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                assert_eq!(page.ui(ctx, &Index::default()), None);
            });
            assert!(!output.shapes.is_empty());
        }
    }

    #[test]
    fn the_listing_is_read_from_the_archive_itself() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/kerosene-engine/base/base.vault");
        let mut page = ArchivePage::new(PathBuf::from("/nowhere"), base);
        page.refresh_listing();
        let entries = page.listing.as_ref().unwrap().entries.as_ref().unwrap();
        assert!(entries.iter().any(|(p, _)| p == "ui/hud.kui"));
    }
}
