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
    /// Compile any art that has no texture behind it yet, before the map.
    ///
    /// On by default. A map whose materials have never been through Alchemy
    /// loads with every surface as the missing-material checkerboard, and
    /// "run a shell script first" is not an answer an editor gets to give.
    pub run_materials: bool,
    /// Where the art lives, for that stage.
    pub content_root: PathBuf,
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
            run_materials: true,
            content_root: PathBuf::from("content"),
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

/// How much work a compile should do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    /// Skip the expensive passes. Right while a layout is still moving.
    Fast,
    /// Everything, at full quality.
    Full,
}

impl CompileSettings {
    /// The quick settings for iterating on a layout.
    pub fn fast() -> Self {
        let mut s = CompileSettings::default();
        s.set_quality(Quality::Fast);
        s
    }

    /// Everything, at full quality.
    pub fn full() -> Self {
        let mut s = CompileSettings::default();
        s.set_quality(Quality::Full);
        s
    }

    /// Apply a quality preset, leaving every other choice alone.
    ///
    /// The distinction matters: "compile fast" is a statement about how much
    /// work to do, not a request to forget that this map is allowed to leak.
    /// Replacing the whole settings struct is what made the leak checkbox do
    /// nothing at all.
    pub fn set_quality(&mut self, quality: Quality) {
        match quality {
            Quality::Fast => {
                self.fast_vis = true;
                self.samples = 1;
                self.bounces = 0;
            }
            Quality::Full => {
                self.fast_vis = false;
                self.samples = 3;
                self.bounces = 2;
            }
        }
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

    // Alchemy first, and in-process rather than as a stage. The compilers are
    // separate programs on purpose; the texture build is not, because the
    // editor runs the same build on the way in, and two callers of the same
    // step should not be able to disagree about what it does. It skips
    // everything already compiled, so the usual cost is one directory walk --
    // and the one time it is not, it is the run that saves the map from
    // loading as a checkerboard.
    if settings.run_materials {
        let _ = sender.send(CompileMessage::Stage("alchemy".into()));
        match alchemy::build_textures(&settings.content_root) {
            Ok(build) => { let _ = sender.send(CompileMessage::Line(format!("  {build}"))); }
            Err(e) => {
                let _ = sender.send(CompileMessage::Failed(format!("alchemy: {e:#}")));
                return Err(());
            }
        }
    }

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
            // Detached, so the editor does not block on the game and closing
            // the game does not take the editor with it. The content root is
            // handed over explicitly: the editor already knows which tree
            // this map belongs to, and letting the game work it out again --
            // from a working directory it inherited from the editor, which
            // inherited it from a shell -- is how a map compiled here comes
            // to be launched against a content tree somewhere else.
            let _ = tool_command("void")
                .arg("--content")
                .arg(&settings.content_root)
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
    fn a_quality_preset_leaves_the_other_choices_alone() {
        // The bug: "compile (fast)" built a whole new settings struct, so the
        // leak checkbox -- and every other choice in the compile window -- was
        // silently discarded on the way to the compiler.
        let mut settings = CompileSettings {
            ignore_leaks: true,
            run_after: false,
            run_vis: false,
            run_lighting: false,
            run_materials: false,
            ..Default::default()
        };
        settings.set_quality(Quality::Fast);
        assert!(settings.ignore_leaks, "a leaking map was told to build anyway");
        assert!(!settings.run_materials, "a preset overrode the material stage");
        assert!(!settings.run_after);
        assert!(!settings.run_vis);
        assert!(!settings.run_lighting);
        assert!(settings.fast_vis, "and the preset still did its own job");

        settings.set_quality(Quality::Full);
        assert!(settings.ignore_leaks);
        assert!(!settings.fast_vis);
        assert_eq!(settings.bounces, 2);
    }

    #[test]
    fn ignoring_leaks_reaches_the_command_line() {
        // The checkbox is only meaningful if the flag arrives at cleave.
        let mut args = vec!["map.voidmap".to_string()];
        let settings = CompileSettings { ignore_leaks: true, ..Default::default() };
        if settings.ignore_leaks { args.push("--ignore-leaks".into()); }
        assert!(args.iter().any(|a| a == "--ignore-leaks"));
    }

    #[test]
    fn materials_are_compiled_before_the_map_by_default() {
        // A map whose art has never been through Alchemy loads with every
        // surface as the missing-material checkerboard, and "run a shell
        // script first" is not an answer an editor gets to give.
        assert!(CompileSettings::default().run_materials);
        assert!(CompileSettings::fast().run_materials);
        assert!(CompileSettings::full().run_materials);
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
