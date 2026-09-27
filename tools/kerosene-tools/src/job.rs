// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! A background job: this same executable, re-run with a subcommand, its
//! output piped back a line at a time.
//!
//! The build and archive pages are not applications; they are jobs to run
//! and logs to read. Each re-invokes the toolset with the stage's name, the
//! same way the editor already runs the compilers, and for the same reason:
//! a crash in a build cannot take the toolset's unsaved editor work with it.
//! The log is shown by the toolset's output panel, beside the editor's
//! compile log, rather than by each page separately.
//!
//! A job can be stopped. The child is kept where both the waiting thread and
//! the page can reach it, so Cancel kills the process rather than leaving a
//! twenty-minute lighting bake running with nobody watching it. A stage runs
//! compilers of its own, so the job is started as the head of its own
//! process group and Cancel ends the whole group, not just its head.

use kerosene_toolui::output::Line as OutputLine;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One line of a running job's output.
#[derive(Clone, Debug, PartialEq)]
enum Event {
    Line(String),
    /// The process finished; `None` when it could not be waited on or was
    /// killed.
    Exit(Option<i32>),
}

/// A job that re-invokes this same executable with a subcommand.
pub struct Job {
    /// What the job is, in a word: "Build", "Pack", "Verify".
    label: String,
    receiver: Receiver<Event>,
    child: Arc<Mutex<Option<Child>>>,
    /// Shared with the waiting thread, which stops waiting for the output to
    /// end once a job is cancelled: a killed stage's own children may hold
    /// the pipes open a little longer.
    stop: Arc<AtomicBool>,
    /// Every line received so far, in order.
    pub log: Vec<String>,
    finished: bool,
    failed: bool,
    cancelled: bool,
    started: Instant,
    ended: Option<Instant>,
}

impl Job {
    /// Start `kerosene-tools <subcommand> <args>` and capture its output.
    pub fn start(label: &str, subcommand: &str, args: &[String]) -> Job {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("kerosene-tools"));
        let mut command = Command::new(exe);
        command.arg(subcommand).args(args);
        Job::spawn(label, command)
    }

    /// Start any command and capture its output. What [`Job::start`] runs,
    /// and what the tests run in its place.
    pub fn spawn(label: &str, mut command: Command) -> Job {
        let (sender, receiver) = channel();
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut job = Job {
            label: label.to_string(),
            receiver,
            child: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
            log: Vec::new(),
            finished: false,
            failed: false,
            cancelled: false,
            started: Instant::now(),
            ended: None,
        };

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                job.log.push(format!("error: could not run {label}: {e}"));
                job.finished = true;
                job.failed = true;
                job.ended = Some(Instant::now());
                return job;
            }
        };

        // Both streams matter: the stages print progress on stdout and
        // warnings on stderr, and a log missing half of it is worse than none.
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(pipe(stdout, sender.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(pipe(stderr, sender.clone()));
        }
        *job.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);

        let shared = Arc::clone(&job.child);
        let stop = Arc::clone(&job.stop);
        std::thread::spawn(move || {
            let code = wait(&shared);
            // The readers drain to EOF before the exit is announced, so the
            // last lines -- the summary, or the error -- land in the log
            // before the panel stops repainting for them. Unless the job was
            // cancelled, when nobody is waiting for a summary.
            if !stop.load(Ordering::SeqCst) {
                for reader in readers {
                    let _ = reader.join();
                }
            }
            let _ = sender.send(Event::Exit(code));
        });
        job
    }

    /// Collect whatever the job has produced since the last call.
    pub fn poll(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Line(text) => self.log.push(text),
                Event::Exit(code) => {
                    self.finished = true;
                    self.failed = code != Some(0);
                    self.ended = Some(Instant::now());
                    if self.cancelled {
                        self.log.push("warning: cancelled".to_string());
                    }
                }
            }
        }
    }

    /// Stop the job. Its log so far is kept, and it finishes as failed.
    pub fn cancel(&mut self) {
        if self.finished {
            return;
        }
        self.cancelled = true;
        self.stop.store(true, Ordering::SeqCst);
        if let Some(child) = self
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            kill_tree(child);
        }
    }

    pub fn running(&self) -> bool {
        !self.finished
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// How long it ran, or has run so far.
    pub fn elapsed(&self) -> Duration {
        self.ended.unwrap_or_else(Instant::now) - self.started
    }

    /// How long ago it finished.
    pub fn finished_ago(&self) -> Option<Duration> {
        self.ended.map(|e| e.elapsed())
    }

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

    /// How many lines of the log are warnings and errors.
    pub fn problems(&self) -> (usize, usize) {
        use kerosene_toolui::output::Level;
        self.log
            .iter()
            .fold((0, 0), |(e, w), line| match Level::of(line) {
                Level::Error => (e + 1, w),
                Level::Warn => (e, w + 1),
                _ => (e, w),
            })
    }
}

fn pipe(
    stream: impl std::io::Read + Send + 'static,
    sender: std::sync::mpsc::Sender<Event>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if sender.send(Event::Line(line)).is_err() {
                break;
            }
        }
    })
}

/// End a job and everything it started.
fn kill_tree(child: &mut Child) {
    let pid = child.id().to_string();
    let quiet = |mut command: Command| {
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let whole_tree = if cfg!(windows) {
        let mut taskkill = Command::new("taskkill");
        taskkill.args(["/PID", &pid, "/T", "/F"]);
        quiet(taskkill)
    } else {
        // The job leads its own process group, whose id is its pid; a
        // negative pid is the whole group.
        let mut kill = Command::new("kill");
        kill.args(["-KILL", "--", &format!("-{pid}")]);
        quiet(kill)
    };
    if !whole_tree {
        let _ = child.kill();
    }
}

/// Wait for the child without holding its lock, so [`Job::cancel`] can take
/// it to kill the process.
fn wait(child: &Mutex<Option<Child>>) -> Option<i32> {
    loop {
        {
            let mut guard = child.lock().unwrap_or_else(|e| e.into_inner());
            let process = guard.as_mut()?;
            match process.try_wait() {
                Ok(Some(status)) => return status.code(),
                Ok(None) => {}
                Err(_) => return None,
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish(job: &mut Job) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while job.running() && Instant::now() < deadline {
            job.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }

    #[cfg(unix)]
    #[test]
    fn a_job_collects_both_streams_and_its_outcome() {
        let mut job = Job::spawn("Test", shell("echo one; echo 'warning: two' >&2; exit 3"));
        finish(&mut job);
        assert_eq!(job.outcome(), Some(true));
        assert!(job.log.contains(&"one".to_string()));
        assert!(job.log.contains(&"warning: two".to_string()));
        assert_eq!(job.problems(), (0, 1));
        assert!(job.finished_ago().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn a_job_can_be_cancelled() {
        let mut job = Job::spawn("Test", shell("echo started; sleep 30"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while job.log.is_empty() && Instant::now() < deadline {
            job.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        job.cancel();
        finish(&mut job);
        assert!(job.cancelled());
        assert_eq!(job.outcome(), Some(true));
        assert!(job.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_job_that_cannot_start_says_so() {
        let mut job = Job::spawn("Test", Command::new("/no/such/program/anywhere"));
        job.poll();
        assert_eq!(job.outcome(), Some(true));
        assert!(job.log[0].starts_with("error"));
    }
}
