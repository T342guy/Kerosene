// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! Managed threads: named, stoppable, joined with a deadline.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::mem;

#[derive(Default)]
struct Inner {
    stopped: AtomicBool,
    lock: Mutex<()>,
    wake: Condvar,
}

/// How a managed thread is told to stop. Cheap to clone; every clone sees
/// the same state.
#[derive(Clone, Default)]
pub struct StopToken {
    inner: Arc<Inner>,
}

impl StopToken {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the thread has been asked to stop. Check it between units
    /// of work.
    pub fn is_stopped(&self) -> bool {
        self.inner.stopped.load(Ordering::SeqCst)
    }

    /// Sleep for `time`, or until a stop is asked for, whichever is first.
    /// Returns `true` when it is time to stop.
    pub fn sleep(&self, time: Duration) -> bool {
        let guard = self.inner.lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_stopped() {
            return true;
        }
        let _ = self
            .inner
            .wake
            .wait_timeout_while(guard, time, |_| !self.is_stopped());
        self.is_stopped()
    }

    pub(crate) fn stop(&self) {
        // Taking the lock first means a sleeper is either not yet waiting
        // (and will see the flag) or waiting (and will be woken).
        let _guard = self.inner.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.inner.stopped.store(true, Ordering::SeqCst);
        self.inner.wake.notify_all();
    }
}

/// A thread the manager owns.
pub(crate) struct ManagedThread {
    pub(crate) name: String,
    handle: Option<std::thread::JoinHandle<()>>,
    done: Arc<AtomicBool>,
}

impl ManagedThread {
    /// Start `body` on a thread named `kerosene-<module>-<name>`, counting
    /// its allocations under `module`.
    pub(crate) fn spawn(
        module: &'static str,
        name: &str,
        token: StopToken,
        body: impl FnOnce(StopToken) + Send + 'static,
    ) -> kerror::Result<Self> {
        let full = format!("kerosene-{module}-{name}");
        let done = Arc::new(AtomicBool::new(false));
        let finished = Arc::clone(&done);
        let label = full.clone();
        let tag = mem::tag(module);
        let handle = std::thread::Builder::new()
            .name(full.clone())
            .spawn(move || {
                mem::set_thread_tag(tag);
                if catch_unwind(AssertUnwindSafe(|| body(token))).is_err() {
                    kerror::abort(&kerror::EngineError::Thread(format!("`{label}` panicked")));
                }
                finished.store(true, Ordering::SeqCst);
            })
            .map_err(|e| kerror::EngineError::Thread(format!("could not start `{full}`: {e}")))?;
        Ok(Self {
            name: full,
            handle: Some(handle),
            done,
        })
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    /// Wait until `deadline` for the thread to end. A thread still running
    /// then is left behind: logged, and never waited on again.
    pub(crate) fn join_by(&mut self, deadline: Instant) -> bool {
        while !self.is_finished() {
            if Instant::now() >= deadline {
                log::error!("thread `{}` did not stop in time; leaving it", self.name);
                self.handle = None;
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        true
    }
}
