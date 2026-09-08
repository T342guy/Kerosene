// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The GUI panels for the build and archive stages.
//!
//! The editor and the sound editor are whole applications embedded as tabs.
//! The build and archive stages are not applications; they are jobs to run and
//! logs to read. Each is a small panel that re-invokes the toolset executable
//! with the stage's name as a subcommand, pipes its output back, and shows it
//! in a scrollable log -- the same way the editor already runs the compilers,
//! and for the same reason: a crash in a build cannot take the toolset's
//! unsaved editor work with it.

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
        if let Some(stdout) = child.stdout.take() {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if sender.send(Line::Out(line)).is_err() {
                        break;
                    }
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if sender.send(Line::Err(line)).is_err() {
                        break;
                    }
                }
            });
        }
        let sender = sender.clone();
        std::thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code());
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

/// Draw a shared scrollable log, scrolled to the bottom as new lines arrive.
fn log_view(ui: &mut egui::Ui, job: &mut Job) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in &job.log {
                ui.label(egui::RichText::new(line).monospace());
            }
        });
}

/// The build panel: a GUI over `kerosene-tools kiln`.
pub struct BuildPanel {
    content_root: PathBuf,
    /// textures, sounds, models, maps, pack.
    stages: [bool; 5],
    fast: bool,
    job: Option<Job>,
}

const STAGE_NAMES: [&str; 5] = ["textures", "sounds", "models", "maps", "pack"];

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

    pub fn ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Build project");
            ui.label(format!("content: {}", self.content_root.display()));
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("stages:");
                for (name, on) in STAGE_NAMES.iter().zip(self.stages.iter_mut()) {
                    ui.checkbox(on, *name);
                }
            });
            ui.checkbox(&mut self.fast, "fast (skip full visibility and lighting)");

            let busy = self.running();
            let build = ui.add_enabled(
                !busy,
                egui::Button::new(if busy { "building..." } else { "build" }),
            );
            if build.clicked() {
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

            ui.separator();
            if let Some(job) = &mut self.job {
                job.poll();
                let status = if job.running() {
                    format!("running... ({} lines)", job.log.len())
                } else if job.failed {
                    "failed".to_string()
                } else {
                    "finished".to_string()
                };
                ui.label(status);
                log_view(ui, job);
            } else {
                ui.weak("run a build to see its output here");
            }
        });
    }
}

/// The archive panel: a GUI over `kerosene-tools vault`.
pub struct ArchivePanel {
    content_root: PathBuf,
    job: Option<Job>,
}

impl ArchivePanel {
    pub fn new(content_root: PathBuf) -> ArchivePanel {
        ArchivePanel {
            content_root,
            job: None,
        }
    }

    pub fn running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.running())
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Content archive");
            ui.label(format!("content: {}", self.content_root.display()));
            ui.separator();

            let archive = self.content_root.join("content.vault");
            let busy = self.running();

            ui.horizontal(|ui| {
                let pack = ui.add_enabled(!busy, egui::Button::new("pack"));
                let verify = ui.add_enabled(!busy, egui::Button::new("verify"));
                let list = ui.add_enabled(!busy, egui::Button::new("list"));

                if pack.clicked() {
                    let mut args = vec![
                        "pack".to_string(),
                        self.content_root.display().to_string(),
                        "-o".to_string(),
                        archive.display().to_string(),
                    ];
                    for ext in kiln::PACKED {
                        args.push("--ext".to_string());
                        args.push((*ext).to_string());
                    }
                    self.job = Some(Job::start("vault", &args));
                } else if verify.clicked() {
                    self.job = Some(Job::start(
                        "vault",
                        &["verify".to_string(), archive.display().to_string()],
                    ));
                } else if list.clicked() {
                    self.job = Some(Job::start(
                        "vault",
                        &["list".to_string(), archive.display().to_string()],
                    ));
                }
            });

            ui.separator();
            if let Some(job) = &mut self.job {
                job.poll();
                let status = if job.running() {
                    format!("running... ({} lines)", job.log.len())
                } else if job.failed {
                    "failed".to_string()
                } else {
                    "finished".to_string()
                };
                ui.label(status);
                log_view(ui, job);
            } else {
                ui.weak("pack, verify or list an archive to see its output here");
            }
        });
    }
}
