// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! The engine's part in the lifecycle: the modules it registers, and the
//! one place work is sent to a background thread.
//!
//! Startup runs the engine's own modules, then the ones the game brought
//! ([`Game::modules`](crate::game::Game::modules)), then the game's own
//! `setup`; shutdown is the game's `shutdown` hook first, then the modules
//! in reverse. The engine never calls `std::thread::spawn`: every thread it
//! runs is one of these, named, counted, and joined on the way out.

use super::Engine;
use kerosene_lifecycle::{Ctx, Manager, Module, Phase, Pool};
use std::sync::{Arc, Mutex};

/// Where the streaming module leaves its pool for the engine to pick up,
/// since the manager owns the module and not the other way round.
type Slot = Arc<Mutex<Option<Pool>>>;

/// The most workers streaming starts, however many cores there are.
const MAX_STREAMING_WORKERS: usize = 4;

/// The workers that build level sections off the main thread.
struct Streaming(Slot);

impl Module for Streaming {
    fn name(&self) -> &'static str {
        "streaming"
    }

    fn start(&mut self, ctx: &mut Ctx) -> kerror::Result<()> {
        // Building a section is bursty, and a machine with sixty cores has
        // no use for sixty idle threads waiting on it.
        let workers = kerosene_lifecycle::default_workers().min(MAX_STREAMING_WORKERS);
        let pool = ctx.pool("build", workers)?;
        log::info!("streaming: {workers} worker thread(s)");
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(pool);
        Ok(())
    }
}

/// Start the engine's modules and the game's. Returns the manager, to keep
/// until [`Engine::shutdown`], and the streaming pool if it came up.
pub(super) fn start(modules: Vec<Box<dyn Module>>) -> (Manager, Option<Pool>) {
    let slot = Slot::default();
    let mut manager = Manager::new();
    manager.register(Phase::Engine, Streaming(Arc::clone(&slot)));
    for module in modules {
        manager.register(Phase::Modules, module);
    }
    if let Err(e) = manager.start() {
        // Whatever did start has been stopped. The engine runs without
        // background workers: slower on a level change, not broken.
        log::error!("could not start the engine's modules: {e}");
    }
    let pool = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    (manager, pool)
}

impl Engine {
    /// Run `job` on one of the engine's background workers, never more at
    /// once than the pool has threads. With no workers -- the modules
    /// failed to start, or the engine has shut down -- it runs here, on the
    /// calling thread, so the work is never lost.
    pub fn spawn_job(&self, job: impl FnOnce() + Send + 'static) {
        let Some(pool) = &self.workers else {
            job();
            return;
        };
        // A closed pool hands the job back as `false` having dropped it;
        // that only happens during shutdown, when nobody wants the result.
        pool.execute(job);
    }

    /// The names of the engine's managed threads that are running.
    pub fn thread_names(&self) -> Vec<String> {
        self.lifecycle.thread_names()
    }
}
