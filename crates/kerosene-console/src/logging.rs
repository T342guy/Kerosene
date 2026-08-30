// SPDX-License-Identifier: LGPL-3.0-or-later
//! Where the game's log lines go.
//!
//! Two things used to be true at once: the console kept a scrollback of
//! everything printed *through* it, and the rest of the engine logged through
//! the `log` crate straight to stderr. So a door reporting that it could not
//! find its target went to a terminal nobody was looking at, while the console
//! -- the one place a player or designer would think to look -- showed
//! nothing. Half the engine was invisible from inside the game.
//!
//! A [`LogRelay`] is installed as the global logger and fans every record
//! three ways: to stderr, to a file if one is open, and into a queue the
//! console drains once a frame. That last hop is a queue rather than a direct
//! write because logging happens on whatever thread happens to be running and
//! the console is not shared; draining it on the main thread keeps the console
//! single-owner, which is what lets commands take `&mut Console`.
//!
//! Records the console itself emitted are written to the file and to stderr
//! but not queued back: the console already has them, and a line that appears
//! twice in a scrollback is a line nobody trusts.

use crate::{LogLevel, LogLine};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// The target `log` records carry when the console forwarded them itself.
const CONSOLE_TARGET: &str = "kerosene_console";

/// How many records may pile up between frames before the oldest are dropped.
///
/// A bound rather than none: a loop logging every iteration must not be able
/// to exhaust memory before the next frame gets a chance to drain it.
const MAX_PENDING: usize = 8192;

#[derive(Default)]
struct Shared {
    pending: Vec<LogLine>,
    dropped: usize,
    file: Option<std::fs::File>,
}

/// The global logger, and the handle the engine drains it through.
pub struct LogRelay {
    shared: Mutex<Shared>,
    /// How loudly our own crates may log.
    level: log::LevelFilter,
    /// How loudly everything else may. See [`LogRelay::level_for`].
    foreign: log::LevelFilter,
}

impl std::fmt::Debug for LogRelay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deliberately does not take the lock: a Debug print is often the
        // thing you reach for while chasing a deadlock.
        f.debug_struct("LogRelay")
            .field("level", &self.level)
            .field("foreign", &self.foreign)
            .finish_non_exhaustive()
    }
}

impl LogRelay {
    fn new(level: log::LevelFilter, foreign: log::LevelFilter) -> LogRelay {
        LogRelay { shared: Mutex::new(Shared::default()), level, foreign }
    }

    /// A relay that is not the global logger.
    ///
    /// Only one logger can be installed per process, which makes anything
    /// that depends on the global one untestable and unembeddable. A detached
    /// relay is fed by hand -- through [`log::Log`] -- and drained the same
    /// way as the real one.
    pub fn detached(level: log::LevelFilter) -> LogRelay {
        LogRelay::new(level, foreign_level(level))
    }

    /// A detached relay that holds everything to one level, ours or not.
    pub fn detached_uniform(level: log::LevelFilter) -> LogRelay {
        LogRelay::new(level, level)
    }

    /// Start writing every record to a file as well.
    ///
    /// Returns the error rather than logging it, because the thing that would
    /// report the failure is the thing that just failed.
    pub fn open_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.file = Some(file);
        Ok(())
    }

    pub fn close_file(&self) {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.file = None;
    }

    pub fn has_file(&self) -> bool {
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).file.is_some()
    }

    /// Take everything logged since the last call.
    ///
    /// The second element is how many records were dropped because the queue
    /// was full, so a flood is reported rather than silently truncated.
    pub fn take(&self) -> (Vec<LogLine>, usize) {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let dropped = std::mem::take(&mut shared.dropped);
        (std::mem::take(&mut shared.pending), dropped)
    }

    pub fn pending_len(&self) -> usize {
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).pending.len()
    }
}

impl LogRelay {
    /// The level a target is held to.
    ///
    /// Everything that is not ours is held to warnings. A console opened to
    /// read one line and found full of Vulkan loader chatter is a console
    /// nobody opens twice -- and the graphics backend narrating its startup
    /// at info level is not information anyone asked for. `RUST_LOG` lifts
    /// the restriction, because someone who sets it is debugging the thing
    /// they set it for.
    fn level_for(&self, target: &str) -> log::LevelFilter {
        if is_ours(target) { self.level } else { self.foreign }
    }
}

impl log::Log for LogRelay {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level_for(metadata.target())
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) { return }

        let level = match record.level() {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warning,
            log::Level::Info => LogLevel::Info,
            log::Level::Debug | log::Level::Trace => LogLevel::Developer,
        };
        let text = record.args().to_string();
        let from_console = record.target().starts_with(CONSOLE_TARGET);

        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(file) = shared.file.as_mut() {
            let _ = writeln!(file, "[{:<5}] {}", record.level(), text);
        }
        // stderr stays useful: a crash before the first frame has no console
        // to print into, and a terminal is where that has to be readable.
        let _ = writeln!(std::io::stderr(), "[{:<5}] {}", record.level(), text);

        if from_console { return }
        if shared.pending.len() >= MAX_PENDING {
            shared.dropped += 1;
            return;
        }
        shared.pending.push(LogLine { level, text });
    }

    fn flush(&self) {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(file) = shared.file.as_mut() { let _ = file.flush(); }
    }
}


/// Crates whose chatter belongs in the game's console.
///
/// Log targets are crate paths with underscores, so `kerosene` catches
/// `kerosene_engine`, `kerosene_render` and the rest, and each tool catches itself.
const OURS: &[&str] = &[
    "kerosene", "chisel", "cleave", "umbra", "radiance", "alchemy", "forge", "vault", "kiln",
];

/// Whether a log target is one of ours.
pub fn is_ours(target: &str) -> bool {
    OURS.iter().any(|name| {
        target == *name
            || target.strip_prefix(name).is_some_and(|rest| rest.starts_with(['_', ':']))
    })
}

/// Install the relay as the process-wide logger.
///
/// Returns the handle even if another logger got there first -- a test binary
/// or a host application may already have installed one -- so the caller can
/// still drain it, and only the stderr and file halves are lost. Failing hard
/// here would mean a game that refuses to start because something else set up
/// logging, which is not a trade anyone would take.
pub fn install(level: log::LevelFilter) -> Arc<LogRelay> {
    let foreign = foreign_level(level);
    let relay = Arc::new(LogRelay::new(level, foreign));
    // The global logger must outlive the process. Leaking one Arc clone is
    // the honest way to say that: the alternative is a static whose drop
    // order relative to the last log call nobody can reason about.
    let global: &'static Arc<LogRelay> = Box::leak(Box::new(Arc::clone(&relay)));
    if log::set_logger(global.as_ref()).is_ok() {
        // The loudest of the two, or records would be filtered out by `log`
        // itself before the relay ever got the chance to sort them.
        log::set_max_level(level.max(foreign));
    }
    relay
}

/// How loudly crates that are not ours may log.
///
/// Warnings and worse, which is the level at which a foreign crate is telling
/// you something you need to act on. Unless `RUST_LOG` is set: someone who
/// sets it is debugging the thing they set it for, and second-guessing them
/// would make the variable useless.
pub fn foreign_level(ours: log::LevelFilter) -> log::LevelFilter {
    if std::env::var_os("RUST_LOG").is_some() {
        return ours;
    }
    ours.min(log::LevelFilter::Warn)
}

/// Read a level filter the way `RUST_LOG` spells one, falling back to a
/// default. Only a bare level is understood -- per-module filtering is what
/// the `developer` convar and log targets are for.
pub fn level_from_env(default: log::LevelFilter) -> log::LevelFilter {
    match std::env::var("RUST_LOG").ok().as_deref().map(str::trim) {
        Some("error") => log::LevelFilter::Error,
        Some("warn") => log::LevelFilter::Warn,
        Some("info") => log::LevelFilter::Info,
        Some("debug") => log::LevelFilter::Debug,
        Some("trace") => log::LevelFilter::Trace,
        Some("off") => log::LevelFilter::Off,
        _ => default,
    }
}

#[cfg(test)]
mod tests;
