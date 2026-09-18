// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The GUI panels for the build and archive stages.
//!
//! The editor and the sound editor are whole applications embedded as tabs.
//! The build and archive stages are not applications; they are jobs to run and
//! logs to read. Each is a small form that re-invokes the toolset executable
//! with the stage's name as a subcommand and pipes its output back -- the
//! same way the editor already runs the compilers, and for the same reason: a
//! crash in a build cannot take the toolset's unsaved editor work with it.
//! The log itself is shown by the toolset's output panel, alongside the
//! editor's compile log, rather than by each form separately.

use kerosene_ui::output::Line as OutputLine;
use kerosene_ui::theme::{self, colors, icons};
use kerosene_ui::widgets;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

/// One line of a running job's output.
#[derive(Clone, Debug, PartialEq)]
enum Line {
    Out(String),
    Err(String),
    /// The process finished; `None` when it could not be waited on.
    Exit(Option<i32>),
}

/// A background job that re-invokes this same executable with a subcommand.
pub struct Job {
    receiver: Receiver<Line>,
    /// Every line received so far, in order.
    pub log: Vec<String>,
    pub finished: bool,
    pub failed: bool,
}

impl Job {
    /// Start `kerosene-tools <subcommand> <args>` and capture its output.
    pub fn start(subcommand: &str, args: &[String]) -> Job {
        let (sender, receiver) = channel();
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("kerosene-tools"));

        let mut command = Command::new(exe);
        command
            .arg(subcommand)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                let _ = sender.send(Line::Err(format!("could not run {subcommand}: {e}")));
                let _ = sender.send(Line::Exit(None));
                return Job {
                    receiver,
                    log: Vec::new(),
                    finished: false,
                    failed: true,
                };
            }
        };

        // Both streams matter: the stages print progress on stdout and
        // warnings on stderr, and a log missing half of it is worse than none.
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            let sender = sender.clone();
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if sender.send(Line::Out(line)).is_err() {
                        break;
                    }
                }
            }));
        }
        if let Some(stderr) = child.stderr.take() {
            let sender = sender.clone();
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if sender.send(Line::Err(line)).is_err() {
                        break;
                    }
                }
            }));
        }
        let sender = sender.clone();
        std::thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code());
            // The readers drain to EOF before the exit is announced, so the
            // last lines -- the summary, or the error -- land in the log
            // before the panel stops repainting for them.
            for reader in readers {
                let _ = reader.join();
            }
            let _ = sender.send(Line::Exit(code));
        });

        Job {
            receiver,
            log: Vec::new(),
            finished: false,
            failed: false,
        }
    }

    /// Collect whatever the job has produced since the last call.
    pub fn poll(&mut self) {
        while let Ok(line) = self.receiver.try_recv() {
            match line {
                Line::Out(text) => self.log.push(text),
                Line::Err(text) => self.log.push(text),
                Line::Exit(code) => {
                    self.finished = true;
                    self.failed = code != Some(0);
                }
            }
        }
    }

    pub fn running(&self) -> bool {
        !self.finished
    }
}

impl Job {
    /// The log as the output panel wants it, each line coloured by what it
    /// looks like.
    pub fn lines(&self) -> Vec<OutputLine<'_>> {
        self.log.iter().map(|l| OutputLine::classified(l)).collect()
    }

    /// `Some(true)` when it finished badly, `Some(false)` when it finished
    /// well, `None` while it runs.
    pub fn outcome(&self) -> Option<bool> {
        self.finished.then_some(self.failed)
    }
}

/// The one-line verdict under a job's controls.
fn outcome_line(ui: &mut egui::Ui, job: Option<&Job>, idle: &str) {
    match job {
        None => {
            ui.label(theme::caption(idle));
        }
        Some(job) if job.running() => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0).color(colors::ACCENT));
                ui.label(theme::caption(format!(
                    "running -- {} lines so far, in the output panel",
                    job.log.len()
                )));
            });
        }
        Some(job) if job.failed => {
            ui.horizontal(|ui| {
                ui.label(theme::icon(icons::X_CIRCLE).color(colors::ERR));
                ui.label(theme::err("failed -- the output panel has the reason"));
            });
        }
        Some(_) => {
            ui.horizontal(|ui| {
                ui.label(theme::icon(icons::CHECK_CIRCLE).color(colors::OK));
                ui.label(theme::ok("finished"));
            });
        }
    }
}

/// The frame every job panel sits in: a canvas margin and a readable width.
fn page(ctx: &egui::Context, title: &str, glyph: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(colors::BG_APP)
                .inner_margin(egui::Margin::same(24)),
        )
        .show(ctx, |ui| {
            ui.set_max_width(720.0);
            ui.horizontal(|ui| {
                ui.label(theme::icon(glyph).size(22.0).color(colors::ACCENT));
                ui.label(egui::RichText::new(title).size(20.0).strong());
            });
            ui.add_space(12.0);
            add(ui);
        });
}

/// The build panel: a GUI over `kerosene-tools kiln`.
pub struct BuildPanel {
    content_root: PathBuf,
    /// textures, sounds, models, maps, pack.
    stages: [bool; 5],
    fast: bool,
    pub job: Option<Job>,
}

const STAGE_NAMES: [&str; 5] = ["textures", "sounds", "models", "maps", "pack"];

const STAGE_HELP: [&str; 5] = [
    "Alchemy: every image under art/ into materials/",
    "Timbre: every sound under sound/ into .keroaud",
    "Forge: every mesh under models/ into .keromdl",
    "Cleave, Umbra, Resonance, Radiance: every map",
    "Vault: everything compiled into one archive",
];

impl BuildPanel {
    pub fn new(content_root: PathBuf) -> BuildPanel {
        BuildPanel {
            content_root,
            stages: [true; 5],
            fast: false,
            job: None,
        }
    }

    pub fn running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.running())
    }

    /// Start a build with the panel's settings. The Project tab calls this
    /// too, so "build" means the same thing from both places.
    pub fn start(&mut self) {
        let mut args = vec![
            "--content".to_string(),
            self.content_root.display().to_string(),
        ];
        if self.fast {
            args.push("--fast".to_string());
        }
        let enabled: Vec<&str> = STAGE_NAMES
            .iter()
            .zip(self.stages.iter())
            .filter(|(_, on)| **on)
            .map(|(n, _)| *n)
            .collect();
        if enabled.len() != STAGE_NAMES.len() {
            for name in enabled {
                args.push("--only".to_string());
                args.push(name.to_string());
            }
        }
        self.job = Some(Job::start("kiln", &args));
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        let busy = self.running();
        let mut start = false;
        page(ctx, "Build", icons::HAMMER, |ui| {
            widgets::fact(ui, "content", self.content_root.display().to_string());
            ui.add_space(8.0);

            widgets::section(ui, "stages", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for ((name, on), help) in STAGE_NAMES
                        .iter()
                        .zip(self.stages.iter_mut())
                        .zip(STAGE_HELP)
                    {
                        widgets::chip(ui, name, on).on_hover_text(help);
                    }
                });
                ui.add_space(4.0);
                ui.checkbox(&mut self.fast, "fast")
                    .on_hover_text("Skip full visibility and lighting. For iterating, not shipping.");
            });

            ui.add_space(10.0);
            let label = if busy {
                format!("{}  Building...", icons::CIRCLE_NOTCH)
            } else {
                format!("{}  Build", icons::PLAY)
            };
            let none = !self.stages.iter().any(|on| *on);
            let button = ui.add_enabled(
                !busy && !none,
                egui::Button::new(
                    egui::RichText::new(label)
                        .size(13.0)
                        .color(colors::ON_ACCENT),
                )
                .fill(colors::ACCENT)
                .stroke(egui::Stroke::NONE),
            );
            if button.clicked() {
                start = true;
            }
            if none {
                ui.label(theme::caption("pick at least one stage"));
            }
            ui.add_space(6.0);
            if let Some(job) = &mut self.job {
                job.poll();
            }
            outcome_line(ui, self.job.as_ref(), "run a build; its log goes to the output panel");
        });
        if start {
            self.start();
        }
    }
}

/// The archive panel: a GUI over `kerosene-tools vault`.
pub struct ArchivePanel {
    content_root: PathBuf,
    /// The archive Kiln builds for this project, so pack, verify and list
    /// all mean the same file the build tab wrote.
    archive: PathBuf,
    pub job: Option<Job>,
}

impl ArchivePanel {
    pub fn new(content_root: PathBuf, project: Option<&kerosene_vfs::Project>) -> ArchivePanel {
        ArchivePanel {
            archive: kiln::archive_path(&content_root, project),
            content_root,
            job: None,
        }
    }

    pub fn running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.running())
    }

    /// Write the archive. The Project tab calls this too.
    pub fn pack(&mut self) {
        let mut args = vec![
            "pack".to_string(),
            self.content_root.display().to_string(),
            "-o".to_string(),
            self.archive.display().to_string(),
        ];
        for ext in kiln::PACKED {
            args.push("--ext".to_string());
            args.push((*ext).to_string());
        }
        self.job = Some(Job::start("vault", &args));
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        let archive = self.archive.clone();
        let busy = self.running();
        let exists = archive.exists();
        let size = std::fs::metadata(&archive).map(|m| m.len()).ok();
        let mut run: Option<&str> = None;

        page(ctx, "Archive", icons::ARCHIVE, |ui| {
            widgets::fact(ui, "content", self.content_root.display().to_string());
            ui.horizontal(|ui| {
                ui.label(theme::caption("archive"));
                ui.label(theme::mono(archive.display().to_string()));
                match size {
                    Some(bytes) => {
                        ui.label(theme::caption(format!("{:.1} MB", bytes as f64 / 1_048_576.0)));
                    }
                    None => {
                        ui.label(theme::caption("not written yet"));
                    }
                }
            });
            ui.add_space(8.0);

            widgets::section(ui, "actions", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let pack = ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(
                                egui::RichText::new(format!("{}  Pack", icons::PACKAGE))
                                    .size(13.0)
                                    .color(colors::ON_ACCENT),
                            )
                            .fill(colors::ACCENT)
                            .stroke(egui::Stroke::NONE),
                        )
                        .on_hover_text("Everything compiled, into one .vault the game reads.");
                    let verify = ui
                        .add_enabled(
                            !busy && exists,
                            egui::Button::new(format!("{}  Verify", icons::CHECK)),
                        )
                        .on_hover_text("Read every entry back and check its hash.");
                    let list = ui
                        .add_enabled(
                            !busy && exists,
                            egui::Button::new(format!("{}  List", icons::LIST_BULLETS)),
                        )
                        .on_hover_text("Every entry, with its size.");
                    if pack.clicked() {
                        run = Some("pack");
                    } else if verify.clicked() {
                        run = Some("verify");
                    } else if list.clicked() {
                        run = Some("list");
                    }
                });
            });

            ui.add_space(6.0);
            if let Some(job) = &mut self.job {
                job.poll();
            }
            outcome_line(
                ui,
                self.job.as_ref(),
                "pack, verify or list the archive; the log goes to the output panel",
            );
        });

        match run {
            Some("pack") => self.pack(),
            Some("verify") => {
                self.job = Some(Job::start(
                    "vault",
                    &["verify".to_string(), archive.display().to_string()],
                ));
            }
            Some("list") => {
                self.job = Some(Job::start(
                    "vault",
                    &["list".to_string(), archive.display().to_string()],
                ));
            }
            _ => {}
        }
    }
}
