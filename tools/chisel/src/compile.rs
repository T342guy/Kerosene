// SPDX-License-Identifier: LGPL-3.0-or-later
//! Running a compile from the editor.
//!
//! Chisel shells out to Cleave, Umbra and Radiance rather than linking them.
//! That is what Hammer does, and it is the right call for the same reasons:
//! the compilers stay genuinely separate programs that can be run from a
//! script or a build server, a crash in one cannot take the editor's unsaved
//! work with it, and anyone can substitute their own.
//!
//! The compile runs on a background thread and streams its output back, so the
//! editor stays responsive and the log appears as it happens rather than all
//! at once at the end. A compile of a real level takes minutes.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

/// What to run, and how thoroughly.
#[derive(Clone, Debug)]
pub struct CompileSettings {
    pub run_vis: bool,
    /// Skip the expensive visibility pass. Right while a layout is moving.
    pub fast_vis: bool,
    pub run_lighting: bool,
    /// Lightmap samples per luxel per axis.
    pub samples: u32,
    pub bounces: u32,
    /// Build even if the map leaks.
    pub ignore_leaks: bool,
    /// Launch the engine on the result.
    pub run_after: bool,
}

impl Default for CompileSettings {
    fn default() -> Self {
        CompileSettings {
            run_vis: true,
            fast_vis: false,
            run_lighting: true,
            samples: 2,
            bounces: 1,
            ignore_leaks: false,
            run_after: true,
        }
    }
}

impl CompileSettings {
    /// The quick settings for iterating on a layout.
    pub fn fast() -> Self {
        CompileSettings { fast_vis: true, samples: 1, bounces: 0, ..Default::default() }
    }

    /// Everything, at full quality.
    pub fn full() -> Self {
        CompileSettings { fast_vis: false, samples: 3, bounces: 2, ..Default::default() }
    }
}

/// A line of compile output.
#[derive(Clone, Debug, PartialEq)]
pub enum CompileMessage {
    /// A stage started.
    Stage(String),
    Line(String),
    Failed(String),
    /// Everything finished; the payload is the compiled map's path.
    Finished(PathBuf),
}

/// A compile in progress.
pub struct CompileJob {
    receiver: Receiver<CompileMessage>,
    pub log: Vec<CompileMessage>,
    pub finished: bool,
    pub failed: bool,
}

impl CompileJob {
    /// Start compiling a saved `.voidmap`.
    ///
    /// The map must already be on disk: the compilers read files, and writing
    /// the editor's buffer somewhere else first would mean compiling something
    /// other than what the designer saved.
    pub fn start(map: &Path, settings: CompileSettings) -> CompileJob {
        let (sender, receiver) = channel();
        let map = map.to_path_buf();

        std::thread::spawn(move || {
            let _ = run_compile(&map, &settings, &sender);
        });

        CompileJob { receiver, log: Vec::new(), finished: false, failed: false }
    }

    /// Collect whatever the compile has produced since the last call.
    pub fn poll(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            match &message {
                CompileMessage::Finished(_) => self.finished = true,
                CompileMessage::Failed(_) => {
                    self.failed = true;
                    self.finished = true;
                }
                _ => {}
            }
            self.log.push(message);
        }
    }

    /// The compiled map's path, once the compile has succeeded.
    pub fn output(&self) -> Option<&Path> {
        self.log.iter().rev().find_map(|m| match m {
            CompileMessage::Finished(path) => Some(path.as_path()),
            _ => None,
        })
    }

    /// The log as plain text, for copying out of the editor.
    pub fn text(&self) -> String {
        self.log
            .iter()
            .map(|m| match m {
                CompileMessage::Stage(s) => format!("--- {s} ---"),
                CompileMessage::Line(l) => l.clone(),
                CompileMessage::Failed(e) => format!("FAILED: {e}"),
                CompileMessage::Finished(p) => format!("done: {}", p.display()),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn run_compile(
    map: &Path,
    settings: &CompileSettings,
    sender: &Sender<CompileMessage>,
) -> Result<(), ()> {
    let compiled = map.with_extension("voidbsp");

    // Cleave.
    let mut args = vec![map.display().to_string()];
    if settings.ignore_leaks { args.push("--ignore-leaks".into()); }
    stage("cleave", &args, sender)?;

    if settings.run_vis {
        let mut args = vec![compiled.display().to_string()];
        if settings.fast_vis { args.push("--fast".into()); }
        stage("umbra", &args, sender)?;
    }

    if settings.run_lighting {
        let args = vec![
            compiled.display().to_string(),
            "--samples".into(),
            settings.samples.to_string(),
            "--bounces".into(),
            settings.bounces.to_string(),
        ];
        stage("radiance", &args, sender)?;
    }

    let _ = sender.send(CompileMessage::Finished(compiled.clone()));

    if settings.run_after {
        if let Some(name) = compiled.file_stem().and_then(|s| s.to_str()) {
            let _ = sender.send(CompileMessage::Stage(format!("launching {name}")));
            // Detached: the editor should not block on the game, and closing
            // the game should not take the editor with it.
            let _ = tool_command("void")
                .arg("+map")
                .arg(name)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }

    Ok(())
}

/// Run one tool, streaming its output.
fn stage(tool: &str, args: &[String], sender: &Sender<CompileMessage>) -> Result<(), ()> {
    let _ = sender.send(CompileMessage::Stage(tool.to_string()));

    let mut child = match tool_command(tool)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = sender.send(CompileMessage::Failed(format!(
                "could not run {tool}: {e}. Is it built and next to chisel, or on PATH?"
            )));
            return Err(());
        }
    };

    // Both streams matter: the compilers print progress on stdout and
    // warnings on stderr, and a log missing half of it is worse than useless.
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = sender.send(CompileMessage::Line(line));
        }
    }
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = sender.send(CompileMessage::Line(line));
        }
    }

    match child.wait() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            let _ = sender.send(CompileMessage::Failed(format!("{tool} exited with {status}")));
            Err(())
        }
        Err(e) => {
            let _ = sender.send(CompileMessage::Failed(format!("{tool}: {e}")));
            Err(())
        }
    }
}

/// Build a command for one of the sibling tools.
///
/// Looks beside Chisel's own executable first, so a built or installed tree
/// works without anything being on PATH -- which is how the tools are actually
/// laid out, all in one directory.
pub fn tool_command(name: &str) -> Command {
    if let Some(path) = tool_path(name) {
        return Command::new(path);
    }
    Command::new(name)
}

/// Where a sibling tool lives, if it is next to this executable.
pub fn tool_path(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
    candidate.is_file().then_some(candidate)
}

/// Which of the tools Chisel needs are actually present.
///
/// Reported in the compile dialog, because "nothing happened when I pressed
/// compile" is otherwise a mystery.
pub fn available_tools() -> Vec<(&'static str, bool)> {
    ["cleave", "umbra", "radiance", "void"]
        .iter()
        .map(|&name| {
            let found = tool_path(name).is_some()
                || Command::new(name).arg("--help").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok();
            (name, found)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_settings_skip_the_expensive_work() {
        let fast = CompileSettings::fast();
        assert!(fast.fast_vis);
        assert_eq!(fast.bounces, 0);
        assert_eq!(fast.samples, 1);
    }

    #[test]
    fn full_settings_ask_for_quality() {
        let full = CompileSettings::full();
        assert!(!full.fast_vis);
        assert!(full.bounces > 0);
        assert!(full.samples > 1);
    }

    #[test]
    fn a_missing_tool_reports_rather_than_hanging() {
        let (sender, receiver) = channel();
        let result = stage("definitely-not-a-real-tool", &[], &sender);
        assert!(result.is_err());

        let messages: Vec<CompileMessage> = receiver.try_iter().collect();
        assert!(matches!(messages[0], CompileMessage::Stage(_)));
        match &messages[1] {
            CompileMessage::Failed(text) => assert!(text.contains("could not run"), "{text}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn the_log_renders_as_text() {
        let mut job = CompileJob {
            receiver: channel().1,
            log: vec![
                CompileMessage::Stage("cleave".into()),
                CompileMessage::Line("68 faces".into()),
                CompileMessage::Finished(PathBuf::from("maps/x.voidbsp")),
            ],
            finished: true,
            failed: false,
        };
        job.poll();
        let text = job.text();
        assert!(text.contains("--- cleave ---"), "{text}");
        assert!(text.contains("68 faces"));
        assert_eq!(job.output(), Some(Path::new("maps/x.voidbsp")));
    }

    #[test]
    fn a_failure_ends_the_job() {
        let (sender, receiver) = channel();
        let mut job = CompileJob { receiver, log: Vec::new(), finished: false, failed: false };
        sender.send(CompileMessage::Failed("leak".into())).unwrap();
        job.poll();
        assert!(job.failed && job.finished);
        assert_eq!(job.output(), None);
    }
}
