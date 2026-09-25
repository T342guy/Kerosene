// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Saved games and level changes, against real compiled maps.

use cleave::{CompileOptions, compile};
use kerosene_engine::Game;
use kerosene_engine::engine::{Engine, EngineConfig, report_unhandled, take_console_requests};
use kerosene_engine::input::InputState;
use kerosene_entity::{ClassRegistry, Target};
use kerosene_map::{Connection, Entity, Map, Solid};
use kerosene_math::{Aabb, Vec3};
use kerosene_ui::Value;
use std::path::PathBuf;

const TICK: f32 = 1.0 / 64.0;

/// The stock classes, and a number of the game's own that it saves.
struct Keeper {
    coins: i64,
}

impl Game for Keeper {
    fn classes(&self, registry: &mut ClassRegistry) {
        kerosene_game::register(registry);
    }
    fn map_loaded(&mut self, _: &mut Engine) {
        // What a fresh map does; a load must undo it.
        self.coins = 0;
    }
    fn save(&mut self, _: &mut Engine) -> serde_json::Value {
        serde_json::json!({ "coins": self.coins })
    }
    fn load(&mut self, engine: &mut Engine, data: &serde_json::Value) {
        self.coins = data["coins"].as_i64().unwrap_or(0);
        engine.ui_set("coins", self.coins);
    }
    fn console_request(&mut self, engine: &mut Engine, kind: &str, payload: &str) -> bool {
        if kind != "coins" {
            return false;
        }
        self.coins += payload.parse::<i64>().unwrap_or(1);
        engine.ui_set("coins", self.coins);
        true
    }
    fn setup(&mut self, engine: &mut Engine) {
        engine.console.register_command(
            "coins",
            kerosene_console::ConVarFlags::NONE,
            "add coins",
            |con, args| con.request("coins", args.rest.clone()),
        );
    }
}

fn point(map: &mut Map, class: &str, name: &str, at: Vec3, keys: &[(&str, &str)]) -> usize {
    let id = map.next_id();
    let mut e = Entity::new(id, class);
    if !name.is_empty() {
        e.set("targetname", name);
    }
    e.set_origin(at);
    for (k, v) in keys {
        e.set(k, *v);
    }
    map.entities.push(e);
    map.entities.len() - 1
}

fn wire(map: &mut Map, entity: usize, output: &str, target: &str, input: &str, delay: f32) {
    let mut c = Connection::new(output, target, input);
    c.parameter = "1".into();
    c.delay = delay;
    map.entities[entity].connect(c);
}

/// A closed box, `len` long, with a spawn point and a landmark.
fn room(len: f32, landmark: Vec3) -> Map {
    let mut map = Map::new();
    let t = 16.0;
    let (wide, tall) = (256.0f32, 128.0f32);
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
    point(
        &mut map,
        "info_player_start",
        "",
        Vec3::new(32.0, 64.0, 8.0),
        &[],
    );
    point(&mut map, "info_landmark", "seam", landmark, &[]);
    map
}

fn keep() -> Map {
    let mut map = room(512.0, Vec3::new(400.0, 128.0, 0.0));
    let at = Vec3::new(64.0, 64.0, 32.0);
    point(&mut map, "math_counter", "counter", at, &[("max", "1000")]);
    // Fires once, at the start. A load must not fire it again.
    let auto = point(&mut map, "logic_auto", "", at, &[]);
    wire(&mut map, auto, "OnMapSpawn", "counter", "Add", 0.0);
    // Something in flight across the save.
    let later = point(&mut map, "logic_relay", "later", at, &[]);
    wire(&mut map, later, "OnTrigger", "counter", "Add", 2.0);
    point(
        &mut map,
        "logic_autosave",
        "checkpoint",
        at,
        &[("savename", "cp")],
    );
    point(
        &mut map,
        "trigger_changelevel",
        "exit",
        at,
        &[("map", "next"), ("landmark", "seam")],
    );
    map
}

const SCRIPT: &str = r#"
let visits = 0;
let seen = [];
"#;

fn setup(name: &str) -> (Engine, PathBuf) {
    let dir = std::env::temp_dir().join(format!("kerosene-save-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["maps", "scripts"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    for (file, map) in [
        ("keep", keep()),
        ("next", room(768.0, Vec3::new(600.0, 64.0, 0.0))),
    ] {
        let out = compile(&map, &CompileOptions::default()).expect("the test map compiles");
        assert!(out.leak.is_none());
        std::fs::write(dir.join(format!("maps/{file}.kerobsp")), out.bsp.to_bytes()).unwrap();
    }
    std::fs::write(dir.join("scripts/keep.keroscript"), SCRIPT).unwrap();
    let mut engine = Engine::with_game(
        &EngineConfig {
            content_paths: vec![dir.clone()],
            ..Default::default()
        },
        Box::new(Keeper { coins: 0 }),
    );
    engine.load_map("keep").unwrap();
    (engine, dir)
}

fn run(engine: &mut Engine, ticks: usize) {
    for _ in 0..ticks {
        engine.tick(TICK, &InputState::default());
        console(engine);
    }
}

fn console(engine: &mut Engine) {
    engine.console.run_buffered();
    let unclaimed = take_console_requests(engine);
    report_unhandled(engine, unclaimed);
    engine.load_pending_map();
}

fn counter(engine: &Engine) -> f32 {
    let id = engine.entities.find_by_name("counter")[0];
    engine.entities.get(id).unwrap().fields.f32("value", -1.0)
}

fn fire(engine: &mut Engine, target: &str, input: &str) {
    engine
        .entities
        .queue_input(Target::Named(target.into()), input, "", 0.0, None, None);
}

fn errors(engine: &Engine) -> Vec<String> {
    engine
        .console
        .log()
        .filter(|l| l.text.contains("error") || l.text.contains("could not"))
        .map(|l| l.text.clone())
        .collect()
}

#[test]
fn a_saved_game_comes_back_as_it_was_and_plays_on_identically() {
    let (mut engine, dir) = setup("roundtrip");
    run(&mut engine, 4);
    assert_eq!(counter(&engine), 1.0, "logic_auto fired once");

    fire(&mut engine, "later", "Trigger");
    run(&mut engine, 32); // half a second of the two-second delay
    engine
        .run_script("visits = 5; seen.push(\"atrium\");")
        .unwrap();
    engine.console.execute_user("coins 7");
    engine.console.execute_user("ui_set quest.stage 3");
    console(&mut engine);
    engine.player.health = 42.0;
    engine.player.movement.velocity = Vec3::new(120.0, 0.0, 0.0);

    engine.console.execute_user("save slot1");
    console(&mut engine);
    assert!(dir.join("save/slot1.kerosave").is_file());
    let saved_origin = engine.player.movement.origin;

    // Play on from the save, and remember where that goes.
    run(&mut engine, 200);
    assert_eq!(counter(&engine), 2.0, "the delayed Add landed");
    let reference = engine.entities.snapshot();
    let reference_player = engine.player.movement.origin;

    // Muddle everything, then load.
    engine.player.health = 5.0;
    engine.run_script("visits = 99;").unwrap();
    engine.console.execute_user("coins 100");
    engine.console.execute_user("load slot1");
    console(&mut engine);
    assert!(errors(&engine).is_empty(), "{:?}", errors(&engine));

    assert_eq!(counter(&engine), 1.0, "logic_auto did not fire again");
    assert_eq!(engine.player.health, 42.0);
    assert_eq!(engine.player.movement.origin, saved_origin);
    assert_eq!(
        engine.run_script("`${visits} ${seen}`").unwrap().as_deref(),
        Some(r#"5 ["atrium"]"#)
    );
    assert_eq!(engine.ui.store.get("coins"), Some(&Value::Int(7)));
    assert_eq!(engine.ui.store.get("quest.stage"), Some(&Value::Int(3)));
    assert_eq!(
        engine.ui.store.get("save.last"),
        Some(&Value::Str("slot1".into()))
    );

    // The same 200 ticks from the save end in the same place.
    run(&mut engine, 200);
    assert_eq!(counter(&engine), 2.0, "the event in flight came back");
    assert_eq!(engine.entities.snapshot(), reference);
    assert_eq!(engine.player.movement.origin, reference_player);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_level_change_carries_the_player_across_the_landmark() {
    let (mut engine, dir) = setup("changelevel");
    engine.console.execute_user("coins 3");
    console(&mut engine);
    run(&mut engine, 8);
    engine.player.health = 64.0;
    // 40 units short of the landmark in `keep`, and 16 to its side.
    let offset = Vec3::new(-40.0, 16.0, 0.0);
    engine.player.movement.origin = Vec3::new(400.0, 128.0, 0.0) + offset;

    fire(&mut engine, "exit", "ChangeLevel");
    run(&mut engine, 1);
    assert_eq!(engine.level.as_ref().unwrap().name, "next");
    assert_eq!(engine.player.health, 64.0);
    assert_eq!(
        engine.player.movement.origin,
        Vec3::new(600.0, 64.0, 0.0) + offset,
        "the same place relative to the landmark"
    );
    assert_eq!(
        engine.ui.store.get("coins"),
        Some(&Value::Int(3)),
        "the game's state came along"
    );
    assert!(
        dir.join("save/auto.kerosave").is_file(),
        "sv_autosave saved on arrival"
    );

    // A map that does not exist leaves the player where they are.
    engine.console.execute_user("changelevel nowhere");
    console(&mut engine);
    assert_eq!(engine.level.as_ref().unwrap().name, "next");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_checkpoint_saves_and_quicksave_and_quickload_pair_up() {
    let (mut engine, dir) = setup("checkpoint");
    fire(&mut engine, "checkpoint", "Save");
    run(&mut engine, 2);
    assert!(dir.join("save/cp.kerosave").is_file());

    engine.console.execute_user("quicksave");
    console(&mut engine);
    run(&mut engine, 4);
    engine.player.health = 1.0;
    engine.console.execute_user("quickload");
    console(&mut engine);
    assert_eq!(engine.player.health, 100.0);

    let names: Vec<String> = engine.list_saves().into_iter().map(|s| s.name).collect();
    assert!(names.contains(&"cp".to_string()) && names.contains(&"quick".to_string()));
    assert!(engine.list_saves().iter().all(|s| s.map == "keep"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn bad_saves_are_refused_and_the_running_level_kept() {
    let (mut engine, dir) = setup("refusals");
    assert!(engine.save_game("../escape").is_err());
    assert!(engine.load_game("missing").is_err());

    std::fs::create_dir_all(dir.join("save")).unwrap();
    std::fs::write(dir.join("save/broken.kerosave"), b"{ not json").unwrap();
    let e = engine.load_game("broken").unwrap_err().to_string();
    assert!(e.contains("damaged"), "{e}");

    // Well-formed, but its entities do not fit together.
    engine.save_game("good").unwrap();
    let mut save = engine.read_save("good").unwrap();
    save.world.entities[0].id[1] += 3;
    std::fs::write(
        dir.join("save/twisted.kerosave"),
        serde_json::to_vec(&save).unwrap(),
    )
    .unwrap();
    let before = engine.entities.len();
    let e = engine.load_game("twisted").unwrap_err().to_string();
    assert!(e.contains("would not restore"), "{e}");
    assert_eq!(engine.entities.len(), before, "the level is untouched");
    assert_eq!(engine.level.as_ref().unwrap().name, "keep");

    // A save from a newer engine is not guessed at.
    save.format = 99;
    std::fs::write(
        dir.join("save/future.kerosave"),
        serde_json::to_vec(&save).unwrap(),
    )
    .unwrap();
    let e = engine.load_game("future").unwrap_err().to_string();
    assert!(e.contains("newer"), "{e}");

    engine.player.health = 0.0;
    assert!(engine.save_game("dead").is_err(), "no saving while dead");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saves_mirror_to_the_cloud_and_the_newer_copy_wins() {
    let dir = std::env::temp_dir().join(format!("kerosene-save-cloud-{}", std::process::id()));
    let cloud = dir.join("cloud");
    let (mut engine, content) = setup("cloud");
    engine.platform = kerosene_platform::Platform::new(kerosene_platform::PlatformConfig {
        local_cloud: Some(cloud.clone()),
        ..Default::default()
    });
    engine.save_game("slot").unwrap();
    assert!(cloud.join("slot.kerosave").is_file());

    // Another machine saved later: the local file is older than the cloud's.
    let mut other = engine.read_save("slot").unwrap();
    other.saved_at += 1000;
    other.player.health = 12.0;
    std::fs::write(
        cloud.join("slot.kerosave"),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    engine.load_game("slot").unwrap();
    assert_eq!(engine.player.health, 12.0);

    // And with no local copy at all, the cloud's still loads.
    std::fs::remove_file(content.join("save/slot.kerosave")).unwrap();
    engine.player.health = 99.0;
    engine.load_game("slot").unwrap();
    assert_eq!(engine.player.health, 12.0);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(content);
}

#[test]
fn a_falling_prop_is_still_falling_after_a_load() {
    let (mut engine, dir) = setup("prop");
    std::fs::create_dir_all(dir.join("models/props")).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/models/props/cube.keromdl"
        ),
        dir.join("models/props/cube.keromdl"),
    )
    .unwrap();
    let prop = engine.spawn_prop("props/cube", Vec3::new(200.0, 128.0, 100.0));
    run(&mut engine, 10);
    let motion = |e: &Engine| {
        e.physics
            .prop_motion()
            .into_iter()
            .find(|m| m.0 == prop)
            .expect("the prop has a body")
    };
    let falling = motion(&engine);
    assert!(falling.1.z < -10.0, "falling: {:?}", falling.1);
    engine.save_game("prop").unwrap();
    let saved_at = engine.entities.get(prop).unwrap().origin;

    run(&mut engine, 60);
    let settled = engine.entities.get(prop).unwrap().origin;

    engine.load_game("prop").unwrap();
    assert_eq!(engine.entities.get(prop).unwrap().origin, saved_at);
    let back = motion(&engine);
    assert_eq!(
        back.1, falling.1,
        "the same velocity, not dropped from rest"
    );
    run(&mut engine, 60);
    let again = engine.entities.get(prop).unwrap().origin;
    assert!(
        (again - settled).length() < 1.0,
        "lands where it landed before: {again:?} vs {settled:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
