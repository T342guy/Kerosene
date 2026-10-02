// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! Modules, phases, and the manager that starts and stops them.

use std::time::{Duration, Instant};

use crate::mem;
use crate::pool::Pool;
use crate::thread::{ManagedThread, StopToken};

/// How long [`Manager::stop`] waits for one module's threads before it
/// leaves them behind.
const STOP_DEADLINE: Duration = Duration::from_secs(5);

/// When a module starts, relative to the others. Stopping runs the phases
/// backwards: the game first, the engine last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// The engine's own parts.
    Engine,
    /// Module crates between the engine and the game.
    Modules,
    /// The game's own.
    Game,
}

/// One unit that starts and stops: the engine's streaming workers, a
/// module crate's background service, a game's own threads.
///
/// Every method but [`name`](Self::name) has a do-nothing default.
pub trait Module: Send {
    /// A short lowercase name. It names the module's threads
    /// (`kerosene-<name>-<thread>`), its memory tag and its log lines.
    fn name(&self) -> &'static str;

    /// Load what the module needs. Runs for every module, in order, before
    /// any module's [`start`](Self::start), so nothing is running yet.
    fn preload(&mut self, _ctx: &mut Ctx) -> kerror::Result<()> {
        Ok(())
    }

    /// Bring the module up, spawning its threads through `ctx`.
    fn start(&mut self, _ctx: &mut Ctx) -> kerror::Result<()> {
        Ok(())
    }

    /// Take the module down. By the time this runs the module's threads
    /// have been signalled and joined. Not called for a module whose
    /// `start` failed.
    fn stop(&mut self) {}
}

/// A boxed module is a module, so a game can hand over a list of them.
impl Module for Box<dyn Module> {
    fn name(&self) -> &'static str {
        (**self).name()
    }

    fn preload(&mut self, ctx: &mut Ctx) -> kerror::Result<()> {
        (**self).preload(ctx)
    }

    fn start(&mut self, ctx: &mut Ctx) -> kerror::Result<()> {
        (**self).start(ctx)
    }

    fn stop(&mut self) {
        (**self).stop()
    }
}

/// What a module is handed while it preloads and starts: the way to get
/// threads.
pub struct Ctx<'a> {
    module: &'static str,
    token: &'a StopToken,
    threads: &'a mut Vec<ManagedThread>,
    pools: &'a mut Vec<Pool>,
}

impl Ctx<'_> {
    /// The name of the module this belongs to.
    pub fn module(&self) -> &'static str {
        self.module
    }

    /// Start one thread, named `kerosene-<module>-<name>`. `body` gets a
    /// [`StopToken`] and should return soon after it says to stop.
    pub fn spawn(
        &mut self,
        name: &str,
        body: impl FnOnce(StopToken) + Send + 'static,
    ) -> kerror::Result<()> {
        let thread = ManagedThread::spawn(self.module, name, self.token.clone(), body)?;
        self.threads.push(thread);
        Ok(())
    }

    /// Start a pool of `size` workers (at least one), named
    /// `kerosene-<module>-<name>-<n>`.
    pub fn pool(&mut self, name: &str, size: usize) -> kerror::Result<Pool> {
        let pool = Pool::new(size.max(1));
        for n in 0..pool.size() {
            let worker = pool.clone();
            self.spawn(&format!("{name}-{n}"), move |_| worker.work())?;
        }
        self.pools.push(pool.clone());
        Ok(pool)
    }
}

/// How many background workers to use: the cores the machine has, less one
/// for the thread that runs the game, and at least one. The environment
/// variable `KEROSENE_THREADS` overrides it.
pub fn default_workers() -> usize {
    if let Some(n) = std::env::var("KEROSENE_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        return n.max(1);
    }
    std::thread::available_parallelism()
        .map_or(1, |n| n.get().saturating_sub(1))
        .max(1)
}

struct Entry {
    phase: Phase,
    module: Box<dyn Module>,
    token: StopToken,
    threads: Vec<ManagedThread>,
    pools: Vec<Pool>,
    started: bool,
}

impl Entry {
    /// Signal and join this module's threads, then run its `stop`.
    fn bring_down(&mut self, run_stop: bool, wait: Duration) {
        let name = self.module.name();
        log::debug!("lifecycle: stopping {name}");
        self.token.stop();
        for pool in &self.pools {
            pool.close();
        }
        let deadline = Instant::now() + wait;
        for thread in &mut self.threads {
            thread.join_by(deadline);
        }
        self.threads.clear();
        self.pools.clear();
        if run_stop {
            let _scope = mem::scope(mem::tag(name));
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.module.stop();
            }));
            if outcome.is_err() {
                log::error!("module `{name}` panicked while stopping");
            }
        }
        self.started = false;
    }
}

/// Starts modules in order and stops them in reverse.
///
/// Dropping a started manager stops it.
pub struct Manager {
    entries: Vec<Entry>,
    running: bool,
    deadline: Duration,
}

impl Default for Manager {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            running: false,
            deadline: STOP_DEADLINE,
        }
    }
}

impl Manager {
    /// A manager with nothing registered.
    pub fn new() -> Self {
        Self::default()
    }

    /// How long a stop waits for one module's threads before leaving them
    /// behind. Five seconds unless changed.
    pub fn with_stop_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// Add `module` to `phase`. Within a phase modules start in the order
    /// they were registered. Modules registered after [`start`](Self::start)
    /// are started by the next call to it.
    pub fn register(&mut self, phase: Phase, module: impl Module + 'static) {
        self.entries.push(Entry {
            phase,
            module: Box::new(module),
            token: StopToken::new(),
            threads: Vec::new(),
            pools: Vec::new(),
            started: false,
        });
    }

    /// Whether anything is started.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Preload every module not yet started, then start each. A module that
    /// fails either step stops everything this call started, in reverse,
    /// and its error is returned.
    pub fn start(&mut self) -> kerror::Result<()> {
        // Stable, so registration order holds within a phase.
        self.entries.sort_by_key(|e| e.phase);

        let pending: Vec<usize> = (0..self.entries.len())
            .filter(|&i| !self.entries[i].started)
            .collect();

        for &i in &pending {
            let entry = &mut self.entries[i];
            let name = entry.module.name();
            log::debug!("lifecycle: preloading {name}");
            let _scope = mem::scope(mem::tag(name));
            let mut ctx = Ctx {
                module: name,
                token: &entry.token,
                threads: &mut entry.threads,
                pools: &mut entry.pools,
            };
            if let Err(e) = entry.module.preload(&mut ctx) {
                self.unwind(&pending, 0..0, Some(i));
                return Err(e);
            }
        }
        for (done, &i) in pending.iter().enumerate() {
            let entry = &mut self.entries[i];
            let name = entry.module.name();
            log::debug!("lifecycle: starting {name}");
            let _scope = mem::scope(mem::tag(name));
            let mut ctx = Ctx {
                module: name,
                token: &entry.token,
                threads: &mut entry.threads,
                pools: &mut entry.pools,
            };
            let result = entry.module.start(&mut ctx);
            match result {
                Ok(()) => entry.started = true,
                Err(e) => {
                    self.unwind(&pending, 0..done, Some(i));
                    return Err(e);
                }
            }
        }
        self.running = true;
        Ok(())
    }

    /// Undo a start that failed: the modules `pending[started]` that did
    /// start, in reverse, and the failed one's threads without its `stop`.
    fn unwind(
        &mut self,
        pending: &[usize],
        started: std::ops::Range<usize>,
        failed: Option<usize>,
    ) {
        if let Some(i) = failed {
            self.entries[i].bring_down(false, self.deadline);
        }
        for &i in pending[started].iter().rev() {
            self.entries[i].bring_down(true, self.deadline);
        }
        // Modules that only preloaded own no threads of their own to stop,
        // but may have spawned some while preloading.
        for &i in pending.iter().rev() {
            if !self.entries[i].threads.is_empty() {
                self.entries[i].bring_down(false, self.deadline);
            }
        }
    }

    /// Stop every started module, last started first. Does nothing if
    /// nothing is started, so it can be called more than once.
    pub fn stop(&mut self) {
        let wait = self.deadline;
        for entry in self.entries.iter_mut().rev() {
            if entry.started || !entry.threads.is_empty() {
                entry.bring_down(entry.started, wait);
            }
        }
        self.running = false;
    }

    /// The names of every managed thread still running, in start order.
    pub fn thread_names(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|e| &e.threads)
            .filter(|t| !t.is_finished())
            .map(|t| t.name.clone())
            .collect()
    }

    /// The names of the registered modules in the order they start.
    pub fn module_names(&self) -> Vec<&'static str> {
        let mut order: Vec<_> = self
            .entries
            .iter()
            .map(|e| (e.phase, e.module.name()))
            .collect();
        order.sort_by_key(|(phase, _)| *phase);
        order.into_iter().map(|(_, name)| name).collect()
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.stop();
    }
}
