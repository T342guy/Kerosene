// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The store against a real, compiled map, with no store: the entity I/O
//! classes, the map script's `platform` object and its hook, all answered by
//! the null backend exactly as Steam would answer them.

mod common;

use cleave::{CompileOptions, compile};
use kerosene_engine::engine::{Engine, EngineConfig, report_unhandled, take_console_requests};
use kerosene_engine::input::InputState;
use kerosene_entity::Target;
use kerosene_map::{Connection, Entity, Map, Solid};
use kerosene_math::{Aabb, Vec3};
use kerosene_platform::{PlatformConfig, StatKind};
use kerosene_ui::Value;
use std::path::PathBuf;

const TICK: f32 = 1.0 / 64.0;

fn point(map: &mut Map, class: &str, name: &str, keys: &[(&str, &str)]) -> usize {
    let id = map.next_id();
    let mut e = Entity::new(id, class);
    e.set("targetname", name);
    e.set_origin(Vec3::new(64.0, 64.0, 32.0));
    for (k, v) in keys {
        e.set(k, *v);
    }
    map.entities.push(e);
    map.entities.len() - 1
}

fn wire(map: &mut Map, entity: usize, output: &str, target: &str, input: &str, param: &str) {
    let mut c = Connection::new(output, target, input);
    c.parameter = param.to_string();
    map.entities[entity].connect(c);
}

fn room() -> Map {
    let mut map = Map::new();
    let t = 16.0;
    let (len, wide, tall) = (256.0f32, 128.0f32, 128.0f32);
    for slab in [
        Aabb::new(Vec3::new(-t, -t, -t), Vec3::new(len + t, wide + t, 0.0)),
        Aabb::new(
            Vec3::new(-t, -t, tall),
            Vec3::new(len + t, wide + t, tall + t),
        ),
        Aabb::new(Vec3::new(-t, -t, 0.0), Vec3::new(0.0, wide + t, tall)),
        Aabb::new(Vec3::new(len, -t, 0.0), Vec3::new(len + t, wide + t, tall)),
        Aabb::new(Vec3::new(0.0, -t, 0.0), Vec3::new(len, 0.0, tall)),
        Aabb::new(Vec3::new(0.0, wide, 0.0), Vec3::new(len, wide + t, tall)),
    ] {
        map.add_world_solid(Solid::cube(slab, "dev/grid"));
    }

    point(&mut map, "logic_ui", "ui", &[]);

    let ach = point(
        &mut map,
        "logic_achievement",
        "ach",
        &[("achievement", "ACH_DOOR")],
    );
    wire(&mut map, ach, "OnUnlocked", "ui", "SetValue", "got.door 1");
    point(
        &mut map,
        "logic_achievement",
        "bad",
        &[("achievement", "NOPE")],
    );

    let stat = point(
        &mut map,
        "logic_stat",
        "doors",
        &[
            ("stat", "doors"),
            ("threshold", "3"),
            ("achievement", "ACH_TEN"),
        ],
    );
    wire(
        &mut map,
        stat,
        "OnThreshold",
        "ui",
        "SetValue",
        "got.threshold 1",
    );
    let ten = point(
        &mut map,
        "logic_achievement",
        "ten",
        &[("achievement", "ACH_TEN")],
    );
    wire(&mut map, ten, "OnUnlocked", "ui", "SetValue", "got.ten 1");

    let board = point(
        &mut map,
        "logic_leaderboard",
        "board",
        &[("leaderboard", "time"), ("sort", "asc")],
    );
    wire(&mut map, board, "OnRankImproved", "improved", "Trigger", "");
    wire(&mut map, board, "OnSubmitted", "submitted", "Trigger", "");
    let relay = point(&mut map, "logic_relay", "improved", &[]);
    wire(&mut map, relay, "OnTrigger", "ui", "SetValue", "got.best 1");
    let relay = point(&mut map, "logic_relay", "submitted", &[]);
    wire(&mut map, relay, "OnTrigger", "counter", "Add", "1");
    point(&mut map, "math_counter", "counter", &[]);

    let store = point(&mut map, "logic_platform", "store", &[("dlc", "111")]);
    wire(
        &mut map,
        store,
        "OnUnavailable",
        "ui",
        "SetValue",
        "got.offline 1",
    );
    wire(&mut map, store, "OnDlcNotOwned", "nodlc", "Trigger", "");
    let relay = point(&mut map, "logic_relay", "nodlc", &[]);
    wire(
        &mut map,
        relay,
        "OnTrigger",
        "ui",
        "SetValue",
        "got.nodlc 1",
    );

    let id = map.next_id();
    let mut spawn = Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(32.0, 64.0, 8.0));
    map.entities.push(spawn);
    map
}

const SCRIPT: &str = r#"
fn award() {
    if !platform.is_unlocked("ACH_SCRIPT") {
        steam.unlock("ACH_SCRIPT");
    }
}
fn on_platform_event(name, data) {
    if name == "achievement_unlocked" {
        ui_set("script.heard." + data, true);
    }
}
"#;

fn setup(name: &str) -> (Engine, PathBuf) {
    let dir = std::env::temp_dir().join(format!("kerosene-platform-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["maps", "scripts"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    let out = compile(&room(), &CompileOptions::default()).expect("the test map compiles");
    assert!(out.leak.is_none());
    std::fs::write(dir.join("maps/store.kerobsp"), out.bsp.to_bytes()).unwrap();
    std::fs::write(dir.join("scripts/store.keroscript"), SCRIPT).unwrap();
    let mut engine = common::stock(
        &EngineConfig::default()
            .with_content(dir.clone())
            .with_platform(PlatformConfig {
                achievements: ["ACH_DOOR", "ACH_TEN", "ACH_SCRIPT"]
                    .iter()
                    .map(|a| (a.to_string(), a.to_string()))
                    .collect(),
                stats: vec![("doors".into(), StatKind::Int)],
                dlc: vec![(111, "Soundtrack".into())],
                ..Default::default()
            }),
    );
    engine.load_map("store").unwrap();
    (engine, dir)
}

fn run(engine: &mut Engine, ticks: usize) {
    for _ in 0..ticks {
        engine.tick(TICK, &InputState::default());
        engine.platform_frame(TICK);
        engine.console.run_buffered();
        let unclaimed = take_console_requests(engine);
        report_unhandled(engine, unclaimed);
    }
}

fn fire(engine: &mut Engine, target: &str, input: &str, parameter: &str) {
    engine.entities.queue_input(
        Target::Named(target.to_string()),
        input,
        parameter,
        0.0,
        None,
        None,
    );
}

fn got(engine: &Engine, key: &str) -> bool {
    engine.ui.store.get(key).is_some_and(Value::truthy)
}

#[test]
fn a_map_with_no_store_says_so_at_spawn() {
    let (mut engine, dir) = setup("spawn");
    run(&mut engine, 4);
    assert!(got(&engine, "got.offline"));
    assert!(!got(&engine, "platform.available"));
    assert_eq!(
        engine.ui.store.get("platform.name"),
        Some(&Value::Str("none".into()))
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn logic_achievement_unlocks_and_fires_on_unlocked() {
    let (mut engine, dir) = setup("unlock");
    fire(&mut engine, "ach", "Unlock", "");
    run(&mut engine, 4);
    assert!(engine.platform().is_unlocked("ACH_DOOR"));
    assert!(got(&engine, "got.door"));
    assert!(got(&engine, "platform.achievements.ACH_DOOR"));
    // The map script heard it too.
    assert!(got(&engine, "script.heard.ACH_DOOR"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_undeclared_achievement_is_refused_out_loud() {
    let (mut engine, dir) = setup("undeclared");
    fire(&mut engine, "bad", "Unlock", "");
    run(&mut engine, 3);
    let warned = engine
        .console
        .log()
        .any(|l| l.text.contains("NOPE") && l.text.contains("not declared"));
    assert!(warned, "the console should say which id and why");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stat_reaching_its_threshold_fires_and_awards() {
    let (mut engine, dir) = setup("stat");
    for _ in 0..2 {
        fire(&mut engine, "doors", "Increment", "");
        run(&mut engine, 2);
    }
    assert_eq!(engine.platform().stat("doors"), Some(2.0));
    assert!(!got(&engine, "got.threshold"));
    assert!(!engine.platform().is_unlocked("ACH_TEN"));

    fire(&mut engine, "doors", "Add", "1");
    run(&mut engine, 4);
    assert!(got(&engine, "got.threshold"));
    assert!(engine.platform().is_unlocked("ACH_TEN"));
    assert!(
        got(&engine, "got.ten"),
        "OnUnlocked on the achievement's own entity"
    );
    assert_eq!(
        engine
            .ui
            .store
            .get("platform.stats.doors")
            .map(Value::as_f64),
        Some(3.0)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn leaderboard_results_come_back_as_outputs() {
    let (mut engine, dir) = setup("board");
    fire(&mut engine, "board", "Submit", "500");
    run(&mut engine, 4);
    assert!(got(&engine, "got.best"));

    engine.ui.store.set("got.best", false);
    fire(&mut engine, "board", "Submit", "900");
    run(&mut engine, 4);
    // Slower, on a board where lower is better: posted, not improved.
    assert!(!got(&engine, "got.best"));
    let counter = engine.entities.find_by_name("counter")[0];
    assert_eq!(
        engine
            .entities
            .get(counter)
            .unwrap()
            .fields
            .f32("value", -1.0),
        2.0,
        "OnSubmitted fired for both"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dlc_checks_answer_the_entity_that_asked() {
    let (mut engine, dir) = setup("dlc");
    fire(&mut engine, "store", "CheckDlc", "");
    run(&mut engine, 4);
    assert!(got(&engine, "got.nodlc"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn map_scripts_reach_the_store_through_the_platform_object() {
    let (mut engine, dir) = setup("script");
    // As a `logic_script` CallFunction would; the `script` command is a cheat.
    engine.call_script_hook("award", vec![]);
    run(&mut engine, 3);
    assert!(engine.platform().is_unlocked("ACH_SCRIPT"));
    assert!(got(&engine, "script.heard.ACH_SCRIPT"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn console_cheats_need_sv_cheats() {
    let (mut engine, dir) = setup("cheats");
    engine.console.enqueue("achievement_unlock ACH_DOOR");
    run(&mut engine, 2);
    assert!(!engine.platform().is_unlocked("ACH_DOOR"));
    engine.console.enqueue("sv_cheats 1");
    engine.console.enqueue("achievement_unlock ACH_DOOR");
    engine.console.enqueue("platform add_stat doors 5");
    run(&mut engine, 3);
    assert!(engine.platform().is_unlocked("ACH_DOOR"));
    assert_eq!(engine.platform().stat("doors"), Some(5.0));
    let _ = std::fs::remove_dir_all(dir);
}
