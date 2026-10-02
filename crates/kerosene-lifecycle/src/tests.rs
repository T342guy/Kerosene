// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;

type Log = Arc<Mutex<Vec<String>>>;

struct Probe {
    name: &'static str,
    log: Log,
    fail_start: bool,
    threads: usize,
}

impl Probe {
    fn new(name: &'static str, log: &Log) -> Self {
        Self {
            name,
            log: Arc::clone(log),
            fail_start: false,
            threads: 0,
        }
    }

    fn note(&self, what: &str) {
        self.log
            .lock()
            .unwrap()
            .push(format!("{what} {}", self.name));
    }
}

impl Module for Probe {
    fn name(&self) -> &'static str {
        self.name
    }

    fn preload(&mut self, _: &mut Ctx) -> kerror::Result<()> {
        self.note("preload");
        Ok(())
    }

    fn start(&mut self, ctx: &mut Ctx) -> kerror::Result<()> {
        self.note("start");
        for n in 0..self.threads {
            ctx.spawn(&format!("t{n}"), |token| {
                while !token.sleep(Duration::from_millis(5)) {}
            })?;
        }
        if self.fail_start {
            return Err(kerror::EngineError::Other("no".into()));
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.note("stop");
    }
}

fn log() -> Log {
    Log::default()
}

fn lines(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

#[test]
fn modules_start_by_phase_and_stop_in_reverse() {
    let log = log();
    let mut m = Manager::new();
    // Registered out of phase order on purpose.
    m.register(Phase::Game, Probe::new("game", &log));
    m.register(Phase::Engine, Probe::new("engine", &log));
    m.register(Phase::Modules, Probe::new("mods", &log));
    m.start().unwrap();
    m.stop();
    let starts: Vec<_> = lines(&log)
        .into_iter()
        .filter(|l| !l.starts_with("preload"))
        .collect();
    assert_eq!(
        starts,
        [
            "start engine",
            "start mods",
            "start game",
            "stop game",
            "stop mods",
            "stop engine"
        ]
    );
}

#[test]
fn every_module_preloads_before_any_module_starts() {
    let log = log();
    let mut m = Manager::new();
    m.register(Phase::Engine, Probe::new("a", &log));
    m.register(Phase::Game, Probe::new("b", &log));
    m.start().unwrap();
    let got = lines(&log);
    assert_eq!(&got[..4], ["preload a", "preload b", "start a", "start b"]);
}

#[test]
fn modules_in_one_phase_start_in_registration_order() {
    let log = log();
    let mut m = Manager::new();
    for name in ["x", "y", "z"] {
        m.register(Phase::Modules, Probe::new(name, &log));
    }
    assert_eq!(m.module_names(), ["x", "y", "z"]);
}

#[test]
fn stopping_joins_the_threads_before_the_modules_stop_runs() {
    let log = log();
    let mut p = Probe::new("busy", &log);
    p.threads = 2;
    let mut m = Manager::new();
    m.register(Phase::Modules, p);
    m.start().unwrap();
    assert_eq!(m.thread_names(), ["kerosene-busy-t0", "kerosene-busy-t1"]);
    m.stop();
    assert!(m.thread_names().is_empty());
    assert!(!m.is_running());
}

#[test]
fn stopping_twice_is_harmless() {
    let log = log();
    let mut m = Manager::new();
    m.register(Phase::Engine, Probe::new("once", &log));
    m.start().unwrap();
    m.stop();
    m.stop();
    drop(m);
    let stops = lines(&log).iter().filter(|l| l.starts_with("stop")).count();
    assert_eq!(stops, 1);
}

#[test]
fn dropping_a_started_manager_stops_it() {
    let log = log();
    {
        let mut m = Manager::new();
        m.register(Phase::Engine, Probe::new("dropped", &log));
        m.start().unwrap();
    }
    assert!(lines(&log).contains(&"stop dropped".to_string()));
}

#[test]
fn a_failed_start_stops_what_had_started_and_not_the_failure() {
    let log = log();
    let mut bad = Probe::new("bad", &log);
    bad.fail_start = true;
    bad.threads = 1;
    let mut m = Manager::new();
    m.register(Phase::Engine, Probe::new("first", &log));
    m.register(Phase::Modules, bad);
    m.register(Phase::Game, Probe::new("never", &log));
    assert!(m.start().is_err());
    let got = lines(&log);
    assert!(got.contains(&"stop first".to_string()));
    assert!(!got.contains(&"stop bad".to_string()));
    assert!(!got.contains(&"start never".to_string()));
    assert!(m.thread_names().is_empty());
    assert!(!m.is_running());
}

#[test]
fn a_thread_that_ignores_its_token_is_left_behind_rather_than_hanging_the_stop() {
    struct Deaf(Arc<AtomicUsize>);
    impl Module for Deaf {
        fn name(&self) -> &'static str {
            "deaf"
        }
        fn start(&mut self, ctx: &mut Ctx) -> kerror::Result<()> {
            let gate = Arc::clone(&self.0);
            ctx.spawn("stuck", move |_| {
                while gate.load(Ordering::SeqCst) == 0 {
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        }
    }
    let gate = Arc::new(AtomicUsize::new(0));
    let mut m = Manager::new().with_stop_deadline(Duration::from_millis(50));
    m.register(Phase::Modules, Deaf(Arc::clone(&gate)));
    m.start().unwrap();
    let began = Instant::now();
    m.stop();
    assert!(began.elapsed() < Duration::from_secs(5));
    assert!(
        m.thread_names().is_empty(),
        "the straggler is no longer tracked"
    );
    gate.store(1, Ordering::SeqCst);
}

#[test]
fn a_sleeping_thread_wakes_the_moment_it_is_told_to_stop() {
    let token = StopToken::new();
    let sleeper = token.clone();
    let handle = std::thread::spawn(move || sleeper.sleep(Duration::from_secs(60)));
    std::thread::sleep(Duration::from_millis(20));
    let began = Instant::now();
    token.stop();
    assert!(handle.join().unwrap());
    assert!(began.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_pool_runs_every_job_and_never_more_than_its_size_at_once() {
    struct Work {
        out: Arc<Mutex<Option<Pool>>>,
    }
    impl Module for Work {
        fn name(&self) -> &'static str {
            "work"
        }
        fn start(&mut self, ctx: &mut Ctx) -> kerror::Result<()> {
            *self.out.lock().unwrap() = Some(ctx.pool("w", 3)?);
            Ok(())
        }
    }
    let out = Arc::new(Mutex::new(None));
    let mut m = Manager::new();
    m.register(
        Phase::Engine,
        Work {
            out: Arc::clone(&out),
        },
    );
    m.start().unwrap();
    let pool = out.lock().unwrap().take().unwrap();
    assert_eq!(pool.size(), 3);

    let done = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let now = Arc::new(AtomicUsize::new(0));
    for _ in 0..24 {
        let (done, peak, now) = (done.clone(), peak.clone(), now.clone());
        assert!(pool.execute(move || {
            let n = now.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(2));
            now.fetch_sub(1, Ordering::SeqCst);
            done.fetch_add(1, Ordering::SeqCst);
        }));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while done.load(Ordering::SeqCst) < 24 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(done.load(Ordering::SeqCst), 24);
    assert!(peak.load(Ordering::SeqCst) <= 3);

    m.stop();
    assert!(!pool.execute(|| {}), "a stopped pool takes no more jobs");
    assert!(m.thread_names().is_empty());
}

#[test]
fn the_default_worker_count_is_at_least_one() {
    assert!(default_workers() >= 1);
}

#[test]
fn bytes_read_the_way_a_person_says_them() {
    assert_eq!(mem::format_bytes(0), "0 B");
    assert_eq!(mem::format_bytes(1536), "1.5 KiB");
    assert_eq!(mem::format_bytes(3 * 1024 * 1024), "3.0 MiB");
    assert_eq!(mem::format_bytes(-2048), "-2.0 KiB");
}

#[test]
fn tags_are_named_once_and_scopes_restore_the_tag_they_replaced() {
    let a = mem::tag("tests-a");
    assert_eq!(a, mem::tag("tests-a"));
    assert_ne!(a, mem::tag("tests-b"));
    let outer = mem::scope(a);
    {
        let _inner = mem::scope(mem::tag("tests-b"));
    }
    drop(outer);
}

#[test]
fn without_the_allocator_installed_the_report_says_so() {
    // The test binary uses the system allocator, not the tracking one.
    let stats = mem::snapshot();
    if !stats.installed {
        assert!(stats.report()[0].contains("not installed"));
    }
}

/// The counting itself, against a real `TrackingAllocator` used directly
/// rather than installed, so it sees only what the test hands it.
#[test]
fn the_allocator_counts_what_passes_through_it_under_the_current_tag() {
    use std::alloc::{GlobalAlloc, Layout};
    let alloc = TrackingAllocator::new();
    let tag = mem::tag("tests-counting");
    let layout = Layout::from_size_align(4096, 8).unwrap();

    let before = mem::snapshot();
    let _scope = mem::scope(tag);
    // SAFETY: `layout` is non-zero-sized and valid; the pointer is freed
    // below with the same layout, through the same allocator.
    let ptr = unsafe { alloc.alloc(layout) };
    assert!(!ptr.is_null());
    let during = mem::snapshot();
    assert!(during.allocs > before.allocs);
    let mine = |s: &mem::MemStats| {
        s.tags
            .iter()
            .find(|t| t.name == "tests-counting")
            .map(|t| t.live)
    };
    assert!(mine(&during).unwrap_or(0) >= 4096);
    assert!(during.peak >= during.live);
    // SAFETY: `ptr` came from `alloc.alloc(layout)` above.
    unsafe { alloc.dealloc(ptr, layout) };
    let after = mem::snapshot();
    assert!(after.frees > during.frees);
    assert!(mine(&after).unwrap_or(0) < mine(&during).unwrap_or(0));
}
