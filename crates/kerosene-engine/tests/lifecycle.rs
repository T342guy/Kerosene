// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The engine's threads, from start to stop: the ones it runs are named and
//! managed, a game's modules come up around it, and shutdown leaves none.

use kerosene_engine::engine::{Engine, EngineConfig};
use kerosene_engine::game::Game;
use kerosene_lifecycle::{Ctx, Module};
use std::sync::{Arc, Mutex};

type Log = Arc<Mutex<Vec<&'static str>>>;

struct Service(Log);

impl Module for Service {
    fn name(&self) -> &'static str {
        "service"
    }

    fn start(&mut self, ctx: &mut Ctx) -> kerror::Result<()> {
        self.0.lock().unwrap().push("module start");
        ctx.spawn("tick", |token| {
            while !token.sleep(std::time::Duration::from_millis(5)) {}
        })
    }

    fn stop(&mut self) {
        self.0.lock().unwrap().push("module stop");
    }
}

struct Hosted(Log);

impl Game for Hosted {
    fn modules(&self) -> Vec<Box<dyn Module>> {
        vec![Box::new(Service(Arc::clone(&self.0)))]
    }

    fn setup(&mut self, _: &mut Engine) {
        self.0.lock().unwrap().push("game setup");
    }

    fn shutdown(&mut self, _: &mut Engine) {
        self.0.lock().unwrap().push("game shutdown");
    }
}

#[test]
fn the_engine_runs_named_workers_and_a_stopped_engine_runs_none() {
    let mut engine = Engine::new(&EngineConfig::default());
    let names = engine.thread_names();
    assert!(!names.is_empty(), "streaming workers are running");
    assert!(
        names
            .iter()
            .all(|n| n.starts_with("kerosene-streaming-build-")),
        "{names:?}"
    );
    engine.shutdown();
    assert!(engine.thread_names().is_empty());
}

#[test]
fn a_games_modules_start_before_its_setup_and_stop_after_its_shutdown() {
    let log = Log::default();
    let mut engine =
        Engine::with_game(&EngineConfig::default(), Box::new(Hosted(Arc::clone(&log))));
    assert!(
        engine
            .thread_names()
            .contains(&"kerosene-service-tick".to_string())
    );
    engine.shutdown();
    assert_eq!(
        *log.lock().unwrap(),
        ["module start", "game setup", "game shutdown", "module stop"]
    );
    assert!(engine.thread_names().is_empty());
}

#[test]
fn work_sent_to_an_engine_with_no_workers_runs_on_the_calling_thread() {
    let mut engine = Engine::new(&EngineConfig::default());
    engine.shutdown();
    let ran = Arc::new(Mutex::new(false));
    let flag = Arc::clone(&ran);
    engine.spawn_job(move || *flag.lock().unwrap() = true);
    assert!(*ran.lock().unwrap(), "done by the time the call returns");
}

#[test]
fn work_sent_to_the_workers_gets_done() {
    let engine = Engine::new(&EngineConfig::default());
    let (tx, rx) = std::sync::mpsc::channel();
    engine.spawn_job(move || {
        tx.send(std::thread::current().name().map(String::from))
            .unwrap()
    });
    let name = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert!(name.unwrap().starts_with("kerosene-streaming-"));
}
