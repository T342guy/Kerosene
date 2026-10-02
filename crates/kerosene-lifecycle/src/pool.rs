// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! A bounded pool of managed worker threads.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

type Job = Box<dyn FnOnce() + Send>;

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    size: usize,
    running: AtomicUsize,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

/// Worker threads that run queued jobs, never more of them than the size it
/// was made with. Cloning gives another handle to the same pool.
///
/// A job that panics is logged and does not stop its worker.
///
/// Stopping the pool drops the jobs that have not started and waits for the
/// ones that have; submitting to a stopped pool does nothing.
#[derive(Clone)]
pub struct Pool {
    shared: Arc<Shared>,
}

impl Pool {
    pub(crate) fn new(size: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                wake: Condvar::new(),
                size,
                running: AtomicUsize::new(0),
            }),
        }
    }

    /// How many workers the pool has.
    pub fn size(&self) -> usize {
        self.shared.size
    }

    /// Queue `job`. Returns `false`, dropping the job, if the pool has
    /// been stopped.
    pub fn execute(&self, job: impl FnOnce() + Send + 'static) -> bool {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.closed {
            return false;
        }
        queue.jobs.push_back(Box::new(job));
        self.shared.wake.notify_one();
        true
    }

    /// Jobs queued and not yet picked up.
    pub fn queued(&self) -> usize {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .jobs
            .len()
    }

    /// Jobs running right now. Never more than [`size`](Self::size).
    pub fn running(&self) -> usize {
        self.shared.running.load(Ordering::SeqCst)
    }

    pub(crate) fn close(&self) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.closed = true;
        queue.jobs.clear();
        self.shared.wake.notify_all();
    }

    /// The body of one worker thread.
    pub(crate) fn work(&self) {
        let shared = &self.shared;
        loop {
            let job = {
                let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if let Some(job) = queue.jobs.pop_front() {
                        break job;
                    }
                    if queue.closed {
                        return;
                    }
                    queue = shared.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
                }
            };
            shared.running.fetch_add(1, Ordering::SeqCst);
            // A job that panics is that job's failure, not the pool's: it is
            // logged and the worker takes the next one.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            shared.running.fetch_sub(1, Ordering::SeqCst);
            if outcome.is_err() {
                log::error!("a job on a worker thread panicked");
            }
        }
    }
}
