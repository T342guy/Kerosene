// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! Graceful abort and shutdown.
//!
//! Nothing here exits the process. A request sets a flag; the host loop sees
//! it, stops, and runs the cleanup hooks on its way out, so config is written
//! and files are closed whether the cause was a menu, Ctrl+C or a fatal error.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::{KError, Severity, health};

/// Why the engine is stopping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownReason {
    /// The player or a script asked to quit.
    UserQuit,
    /// SIGINT or SIGTERM.
    Signal,
    /// An error the engine cannot continue past.
    Fatal(String),
}

impl ShutdownReason {
    /// The process exit code for this reason.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UserQuit => 0,
            Self::Signal => 130,
            Self::Fatal(_) => 1,
        }
    }
}

type Hook = Box<dyn FnOnce() + Send>;

/// A shutdown request and the hooks to run for it.
#[derive(Default)]
pub struct Shutdown {
    requested: AtomicBool,
    reason: Mutex<Option<ShutdownReason>>,
    hooks: Mutex<Vec<(String, Hook)>>,
}

impl Shutdown {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask for a shutdown. The first reason wins: a Ctrl+C that arrives
    /// while a fatal error is already unwinding does not change why.
    pub fn request(&self, reason: ShutdownReason) {
        let mut slot = self.reason.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(reason);
        }
        self.requested.store(true, Ordering::SeqCst);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    pub fn reason(&self) -> Option<ShutdownReason> {
        self.reason.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Register cleanup to run at shutdown, named for the log.
    pub fn on_shutdown(&self, name: &str, hook: impl FnOnce() + Send + 'static) {
        self.hooks.lock().unwrap_or_else(|e| e.into_inner()).push((name.to_string(), Box::new(hook)));
    }

    /// Run the hooks, last registered first, each at most once. A hook that
    /// panics is logged and the rest still run. Returns how many ran.
    pub fn run_hooks(&self) -> usize {
        let hooks = std::mem::take(&mut *self.hooks.lock().unwrap_or_else(|e| e.into_inner()));
        let mut ran = 0;
        for (name, hook) in hooks.into_iter().rev() {
            log::debug!("shutdown: {name}");
            if catch_unwind(AssertUnwindSafe(hook)).is_err() {
                log::error!("shutdown hook `{name}` panicked");
            }
            ran += 1;
        }
        ran
    }
}

/// The process-wide shutdown.
pub fn shutdown() -> &'static Shutdown {
    static SHUTDOWN: OnceLock<Shutdown> = OnceLock::new();
    SHUTDOWN.get_or_init(Shutdown::new)
}

pub fn request_shutdown(reason: ShutdownReason) {
    shutdown().request(reason);
}

pub fn shutdown_requested() -> bool {
    shutdown().is_requested()
}

pub fn shutdown_reason() -> Option<ShutdownReason> {
    shutdown().reason()
}

pub fn on_shutdown(name: &str, hook: impl FnOnce() + Send + 'static) {
    shutdown().on_shutdown(name, hook);
}

pub fn run_shutdown_hooks() -> usize {
    shutdown().run_hooks()
}

/// Report `error`: log it at a level to match its severity, count it, and for
/// a fatal one ask the engine to shut down.
pub fn abort(error: &impl KError) {
    health::health().record(error);
    match error.severity() {
        Severity::Warning => log::warn!("{}: {error}", error.id()),
        Severity::Recoverable => log::error!("{}: {error}", error.id()),
        Severity::Fatal => {
            log::error!("{}: {error} -- shutting down", error.id());
            request_shutdown(ShutdownReason::Fatal(format!("{}: {error}", error.id())));
        }
    }
}

/// Turn SIGINT and SIGTERM into a [`ShutdownReason::Signal`] request. A
/// second one, from a player whose first went unheeded because the engine is
/// stuck, exits at once. A no-op off Unix.
pub fn install_signal_handlers() {
    #[cfg(unix)]
    unix::install();
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::sync::atomic::AtomicU32;

    static SIGNALS: AtomicU32 = AtomicU32::new(0);

    // Only async-signal-safe work: atomics, and `_exit`. The reason is
    // recorded by `poll_signal`, which the host loop calls.
    extern "C" fn handler(_: libc::c_int) {
        if SIGNALS.fetch_add(1, Ordering::SeqCst) >= 1 {
            unsafe { libc::_exit(130) };
        }
        shutdown().requested.store(true, Ordering::SeqCst);
    }

    pub fn install() {
        for sig in [libc::SIGINT, libc::SIGTERM] {
            // SAFETY: `handler` is async-signal-safe.
            unsafe { libc::signal(sig, handler as extern "C" fn(libc::c_int) as libc::sighandler_t) };
        }
    }

    pub fn received() -> bool {
        SIGNALS.load(Ordering::SeqCst) > 0
    }
}

/// Whether shutdown has been requested, by a signal or otherwise. Call this
/// from the loop rather than [`shutdown_requested`] to have a signal's reason
/// recorded.
pub fn poll_shutdown() -> bool {
    #[cfg(unix)]
    if unix::received() {
        request_shutdown(ShutdownReason::Signal);
    }
    shutdown_requested()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn first_reason_wins() {
        let s = Shutdown::new();
        assert!(!s.is_requested());
        s.request(ShutdownReason::Fatal("a".into()));
        s.request(ShutdownReason::UserQuit);
        assert!(s.is_requested());
        assert_eq!(s.reason(), Some(ShutdownReason::Fatal("a".into())));
    }

    #[test]
    fn hooks_run_in_reverse_once_and_survive_a_panic() {
        let s = Shutdown::new();
        let order = Arc::new(Mutex::new(Vec::new()));
        for n in 1..=3 {
            let order = order.clone();
            s.on_shutdown("h", move || {
                if n == 2 {
                    panic!("boom");
                }
                order.lock().unwrap().push(n);
            });
        }
        assert_eq!(s.run_hooks(), 3);
        assert_eq!(*order.lock().unwrap(), vec![3, 1]);
        assert_eq!(s.run_hooks(), 0);
    }

    #[test]
    fn exit_codes() {
        assert_eq!(ShutdownReason::UserQuit.exit_code(), 0);
        assert_eq!(ShutdownReason::Signal.exit_code(), 130);
        assert_eq!(ShutdownReason::Fatal("x".into()).exit_code(), 1);
    }
}
