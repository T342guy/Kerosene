// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The Build tab: a form over `kerosene-tools kiln`.
//!
//! Kiln is the whole content build -- textures, sounds, models, maps, the
//! archive -- and this page is where it is pointed and started. Each stage
//! is a card that says which tool it runs and what over, the options are
//! the ones that change what a build means rather than how it prints, and
//! the job's state stays on the page while its log goes to the output
//! panel with every other job's.

use std::path::PathBuf;

use egui::{Align, Layout, Ui};
use kerosene_toolui::theme::{self, colors, icons};
use kerosene_toolui::widgets::{self, Kind as ButtonKind};

use super::home::job_line;
use crate::job::Job;
use crate::toolset::{Action, JobKind};

/// One stage of a build, as Kiln's `--only` names it.
struct Stage {
    name: &'static str,
    title: &'static str,
    glyph: &'static str,
    tools: &'static str,
    help: &'static str,
}

const STAGES: [Stage; 5] = [
    Stage {
        name: "textures",
        title: "Textures",
        glyph: icons::IMAGE,
        tools: "Alchemy",
        help: "every image under art/ into materials/",
    },
    Stage {
        name: "sounds",
        title: "Sounds",
        glyph: icons::SPEAKER_HIGH,
        tools: "Timbre",
        help: "every sound under sound/ into .kaud",
    },
    Stage {
        name: "models",
        title: "Models",
        glyph: icons::PACKAGE,
        tools: "Forge",
        help: "every mesh under art/ into models/ as .kmdl",
    },
    Stage {
        name: "maps",
        title: "Maps",
        glyph: icons::MAP_TRIFOLD,
        tools: "Cleave · Umbra · Resonance · Radiance",
        help: "every map: geometry, visibility, acoustics, lighting",
    },
    Stage {
        name: "pack",
        title: "Pack",
        glyph: icons::ARCHIVE,
        tools: "Vault",
        help: "everything compiled into one archive",
    },
];

/// What a build is asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// One per stage, in [`STAGES`] order.
    pub stages: [bool; 5],
    pub fast: bool,
    pub force: bool,
    pub ignore_leaks: bool,
    pub dry_run: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            stages: [true; 5],
            fast: false,
            force: false,
            ignore_leaks: false,
            dry_run: false,
        }
    }
}

impl Settings {
    /// Kiln's arguments for these settings over `content`.
    pub fn args(&self, content: &std::path::Path) -> Vec<String> {
        let mut args = vec!["--content".to_string(), content.display().to_string()];
        for (flag, on) in [
            ("--fast", self.fast),
            ("--force", self.force),
            ("--ignore-leaks", self.ignore_leaks),
            ("--dry-run", self.dry_run),
        ] {
            if on {
                args.push(flag.to_string());
            }
        }
        // Every stage is Kiln's default; `--only` is for when some are off.
        if self.stages.iter().any(|on| !on) {
            for (stage, on) in STAGES.iter().zip(self.stages) {
                if on {
                    args.push("--only".to_string());
                    args.push(stage.name.to_string());
                }
            }
        }
        args
    }
}

/// The tab.
pub struct BuildPage {
    content: PathBuf,
    pub settings: Settings,
    pub job: Option<Job>,
}

impl BuildPage {
    pub fn new(content: PathBuf) -> BuildPage {
        BuildPage {
            content,
            settings: Settings::default(),
            job: None,
        }
    }

    pub fn running(&self) -> bool {
        self.job.as_ref().is_some_and(Job::running)
    }

    /// Start a build with the page's settings; `fast` overrides the page's
    /// own for one run. The Home tab and the palette call this too, so
    /// "build" means the same thing from everywhere.
    pub fn start(&mut self, fast: bool) {
        if self.running() {
            return;
        }
        let mut settings = self.settings.clone();
        settings.fast |= fast;
        let label = if settings.fast {
            "Build (fast)"
        } else {
            "Build"
        };
        self.job = Some(Job::start(label, "kiln", &settings.args(&self.content)));
    }

    /// Delete what the build wrote.
    pub fn clean(&mut self) {
        if self.running() {
            return;
        }
        let args = vec![
            "--clean".to_string(),
            "--content".to_string(),
            self.content.display().to_string(),
        ];
        self.job = Some(Job::start("Clean", "kiln", &args));
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> Option<Action> {
        let mut action = None;
        let busy = self.running();
        widgets::page(ctx, 860.0, |ui| {
            widgets::page_header(
                ui,
                icons::HAMMER,
                "Build",
                "Compile the project's content with Kiln",
                |ui| {
                    if ui
                        .add_enabled_ui(!busy, |ui| {
                            widgets::button(ui, ButtonKind::Danger, icons::BROOM, "Clean")
                        })
                        .inner
                        .on_hover_text(
                            "Delete everything the build writes -- compiled textures, sounds, \
                             maps and the archive. Sources are left alone.",
                        )
                        .clicked()
                    {
                        action = Some(Action::Clean);
                    }
                },
            );

            self.run_card(ui, busy, &mut action);
            ui.add_space(theme::SPACE_LG);

            ui.label(theme::section_title("stages"));
            ui.add_space(theme::SPACE_SM);
            ui.spacing_mut().item_spacing.y = theme::SPACE_SM;
            for (stage, on) in STAGES.iter().zip(self.settings.stages.iter_mut()) {
                stage_card(ui, stage, on);
            }

            ui.add_space(theme::SPACE_LG);
            ui.label(theme::section_title("options"));
            ui.add_space(theme::SPACE_SM);
            widgets::card(ui, |ui| {
                let s = &mut self.settings;
                option(
                    ui,
                    &mut s.fast,
                    "Fast",
                    "Skip full visibility and lighting. For iterating, not shipping.",
                );
                option(
                    ui,
                    &mut s.force,
                    "Rebuild everything",
                    "Compile even what is already newer than its source.",
                );
                option(
                    ui,
                    &mut s.ignore_leaks,
                    "Ignore leaks",
                    "Compile a map even if it is open to the void.",
                );
                option(
                    ui,
                    &mut s.dry_run,
                    "Dry run",
                    "Say what would be built, and build nothing.",
                );
            });
        });
        action
    }

    fn run_card(&mut self, ui: &mut Ui, busy: bool, action: &mut Option<Action>) {
        let none = !self.settings.stages.iter().any(|on| *on);
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                if busy {
                    if widgets::button(ui, ButtonKind::Danger, icons::STOP, "Cancel").clicked() {
                        *action = Some(Action::Cancel(JobKind::Build));
                    }
                } else {
                    let build = ui
                        .add_enabled_ui(!none, |ui| {
                            widgets::button(ui, ButtonKind::Primary, icons::PLAY, "Build")
                        })
                        .inner;
                    if build.clicked() {
                        *action = Some(Action::Build { fast: false });
                    }
                }
                ui.add_space(theme::SPACE_SM);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(theme::mono(format!("kiln {}", self.summary())).color(colors::TEXT));
                    ui.label(theme::caption(if none {
                        "Pick at least one stage.".to_string()
                    } else {
                        format!("over {}", super::home::short(&self.content))
                    }));
                });
            });
            if let Some(job) = &self.job {
                ui.add_space(theme::SPACE_SM);
                ui.separator();
                ui.add_space(theme::SPACE_XS);
                job_line(ui, job);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if ui
                        .link(theme::caption("show the log").color(colors::INFO))
                        .clicked()
                    {
                        *action = Some(Action::ShowOutput);
                    }
                });
            }
        });
    }

    /// What the build will run, the way it would be typed.
    fn summary(&self) -> String {
        self.settings
            .args(&self.content)
            .into_iter()
            .skip(2)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn stage_card(ui: &mut Ui, stage: &Stage, on: &mut bool) {
    widgets::card_frame()
        .inner_margin(egui::Margin::symmetric(16, 12))
        .fill(if *on {
            colors::BG_ELEVATED
        } else {
            colors::BG_PANEL
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::icon(stage.glyph).size(20.0).color(if *on {
                    colors::ACCENT
                } else {
                    colors::TEXT_FAINT
                }));
                ui.add_space(theme::SPACE_XS);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(stage.title).size(14.0).strong().color(
                            if *on {
                                colors::TEXT
                            } else {
                                colors::TEXT_MUTED
                            },
                        ));
                        ui.label(theme::caption(stage.tools).color(colors::TEXT_FAINT));
                    });
                    ui.label(theme::caption(stage.help));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::toggle(ui, on);
                });
            });
        });
}

fn option(ui: &mut Ui, on: &mut bool, title: &str, help: &str) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(title).color(colors::TEXT));
            ui.label(theme::caption(help));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::toggle(ui, on);
        });
    });
    ui.add_space(theme::SPACE_XS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn every_stage_is_kilns_default_and_needs_no_only() {
        let args = Settings::default().args(Path::new("/c"));
        assert_eq!(args, ["--content", "/c"]);
    }

    #[test]
    fn options_and_a_subset_of_stages_become_flags() {
        let settings = Settings {
            stages: [false, false, false, true, true],
            fast: true,
            ignore_leaks: true,
            ..Default::default()
        };
        let args = settings.args(Path::new("/c"));
        assert_eq!(
            args,
            [
                "--content",
                "/c",
                "--fast",
                "--ignore-leaks",
                "--only",
                "maps",
                "--only",
                "pack"
            ]
        );
    }

    #[test]
    fn the_page_draws_and_offers_a_build() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut page = BuildPage::new(PathBuf::from("/nowhere"));
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            assert_eq!(page.ui(ctx), None);
        });
        assert!(!output.shapes.is_empty());
        assert!(!page.running());
    }
}
