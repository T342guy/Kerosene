// SPDX-License-Identifier: LGPL-3.0-or-later
//! End-to-end tests: build a map, compile it, load it, and play it.
//!
//! These go through the whole stack -- `.voidmap` source, Cleave's compile, the
//! `.voidbsp` loader, entity spawning, movement and collision -- because that is
//! the only place the seams between them show. Every crate can pass its own
//! tests and still not add up to a level you can walk around.

use cleave::{CompileOptions, compile};
use void_bsp::Bsp;
use void_engine::collision::LevelCollision;
use void_engine::input::InputState;
use void_entity::{EntityWorld, InputEvent};
use void_map::{Connection, Entity, Map, Solid};
use void_math::{Aabb, Angles, Vec3};
use void_physics::{MoveInput, MoveParams, MoveState, player_move};

const TICK: f32 = 1.0 / 64.0;

/// A sealed corridor 512 long, with a gap in the middle wall.
///
/// `with_door` fills the gap with a `func_door`; `with_trigger` wires a
/// trigger in front of it to open it.
fn corridor_map(with_door: bool, with_trigger: bool) -> Map {
    let mut map = Map::new();
    let t = 16.0;
    let (len, wide, tall) = (512.0f32, 128.0f32, 128.0f32);

    // Shell.
    for slab in [
        Aabb::new(Vec3::new(-t, -t, -t), Vec3::new(len + t, wide + t, 0.0)),
        Aabb::new(Vec3::new(-t, -t, tall), Vec3::new(len + t, wide + t, tall + t)),
        Aabb::new(Vec3::new(-t, -t, 0.0), Vec3::new(0.0, wide + t, tall)),
        Aabb::new(Vec3::new(len, -t, 0.0), Vec3::new(len + t, wide + t, tall)),
        Aabb::new(Vec3::new(0.0, -t, 0.0), Vec3::new(len, 0.0, tall)),
        Aabb::new(Vec3::new(0.0, wide, 0.0), Vec3::new(len, wide + t, tall)),
    ] {
        map.add_world_solid(Solid::cube(slab, "dev/grid"));
    }

    // A wall across the middle with a 64-wide doorway in it.
    let (wx0, wx1) = (256.0, 272.0);
    let (dy0, dy1) = (32.0, 96.0);
    for slab in [
        Aabb::new(Vec3::new(wx0, 0.0, 0.0), Vec3::new(wx1, dy0, tall)),
        Aabb::new(Vec3::new(wx0, dy1, 0.0), Vec3::new(wx1, wide, tall)),
    ] {
        map.add_world_solid(Solid::cube(slab, "dev/grid"));
    }

    if with_door {
        let bounds = Aabb::new(Vec3::new(wx0, dy0, 0.0), Vec3::new(wx1, dy1, tall));
        let id = map.next_id();
        let solid_id = map.next_id();
        let side_ids: Vec<u32> = (0..6).map(|_| map.next_id()).collect();
        let mut solid = Solid::cube(bounds, "dev/grid");
        solid.id = solid_id;
        for (s, sid) in solid.sides.iter_mut().zip(side_ids) { s.id = sid; }

        let mut door = Entity::new(id, "func_door");
        door.set("targetname", "gate");
        door.set("movedir", "0 0 1");
        door.set("speed", "200");
        door.set("lip", "8");
        door.set("wait", "-1");
        door.solids.push(solid);
        map.entities.push(door);
    }

    if with_trigger {
        let bounds = Aabb::new(Vec3::new(96.0, 0.0, 0.0), Vec3::new(160.0, wide, tall));
        let id = map.next_id();
        let solid_id = map.next_id();
        let side_ids: Vec<u32> = (0..6).map(|_| map.next_id()).collect();
        let mut solid = Solid::cube(bounds, "tools/trigger");
        solid.id = solid_id;
        for (s, sid) in solid.sides.iter_mut().zip(side_ids) { s.id = sid; }

        let mut trigger = Entity::new(id, "trigger_multiple");
        trigger.set("targetname", "gate_trigger");
        trigger.connect(Connection::new("OnStartTouch", "gate", "Open"));
        trigger.solids.push(solid);
        map.entities.push(trigger);
    }

    let id = map.next_id();
    let mut spawn = Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(32.0, wide / 2.0, 8.0));
    map.entities.push(spawn);

    map
}

fn build(map: &Map) -> Bsp {
    let out = compile(map, &CompileOptions::default()).expect("the test map should compile");
    assert!(out.leak.is_none(), "the test map leaks");
    // Round-tripping through bytes is what the engine actually does.
    let bytes = out.bsp.to_bytes();
    Bsp::from_bytes(&bytes, "test.voidbsp").expect("the compiled map should reload")
}

fn spawned_world(bsp: &Bsp) -> EntityWorld {
    let mut world = EntityWorld::new(void_game::registry());
    world.load_from_bsp(bsp).expect("entities should load");
    world
}

/// Walk forward for `seconds` and report where the player ends up.
fn walk(bsp: &Bsp, entities: &mut EntityWorld, start: Vec3, seconds: f32) -> MoveState {
    let mut state = MoveState { origin: start, on_ground: true, ..Default::default() };
    let params = MoveParams::default();
    let input = MoveInput { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };

    let ticks = (seconds / TICK).ceil() as usize;
    for _ in 0..ticks {
        let world = LevelCollision::new(bsp, entities);
        player_move(&mut state, &input, &params, &world, TICK);
        entities.run(TICK);
    }
    state
}

#[test]
fn a_map_built_in_memory_compiles_and_loads() {
    let bsp = build(&corridor_map(false, false));
    assert!(!bsp.faces.is_empty());
    assert!(bsp.num_clusters() > 0);

    let world = spawned_world(&bsp);
    assert!(world.first_of_class("info_player_start").is_some());
}

#[test]
fn the_player_can_walk_down_an_open_corridor() {
    let bsp = build(&corridor_map(false, false));
    let mut entities = spawned_world(&bsp);
    let end = walk(&bsp, &mut entities, Vec3::new(32.0, 64.0, 1.0), 4.0);

    assert!(end.origin.x > 400.0, "should have crossed the corridor, got {:?}", end.origin);
    assert!(end.on_ground, "should still be on the floor");
}

#[test]
fn the_player_is_stopped_by_the_far_wall() {
    let bsp = build(&corridor_map(false, false));
    let mut entities = spawned_world(&bsp);
    let end = walk(&bsp, &mut entities, Vec3::new(32.0, 64.0, 1.0), 8.0);
    // The wall's inner face is at x = 512, and the player's hull is 16 wide.
    assert!(end.origin.x <= 497.0, "walked into the wall at {:?}", end.origin);
    assert!(end.origin.x > 480.0, "stopped short of the wall at {:?}", end.origin);
}

#[test]
fn a_closed_door_blocks_the_player() {
    // The gap this test was written to close: brush entities are separate
    // models, kept out of the world tree so they can move. Tracing only the
    // world model means walking straight through every door in the game.
    let bsp = build(&corridor_map(true, false));
    let mut entities = spawned_world(&bsp);
    assert!(entities.first_of_class("func_door").is_some(), "the door should have loaded");

    let end = walk(&bsp, &mut entities, Vec3::new(32.0, 64.0, 1.0), 4.0);
    assert!(
        end.origin.x < 256.0,
        "the closed door should have stopped the player, but they reached {:?}",
        end.origin
    );
}

#[test]
fn an_open_door_lets_the_player_through() {
    let bsp = build(&corridor_map(true, false));
    let mut entities = spawned_world(&bsp);

    // Open it and let it finish travelling.
    let gate = entities.find_by_name("gate")[0];
    entities.accept_input(gate, &InputEvent::new("Open"));
    for _ in 0..128 { entities.run(TICK); }
    assert!(
        entities.get(gate).unwrap().origin.z > 100.0,
        "the door should have risen, at {:?}",
        entities.get(gate).unwrap().origin
    );

    let end = walk(&bsp, &mut entities, Vec3::new(32.0, 64.0, 1.0), 4.0);
    assert!(end.origin.x > 300.0, "should have walked through, got {:?}", end.origin);
}

#[test]
fn a_door_blocks_again_after_closing() {
    let bsp = build(&corridor_map(true, false));
    let mut entities = spawned_world(&bsp);
    let gate = entities.find_by_name("gate")[0];

    entities.accept_input(gate, &InputEvent::new("Open"));
    for _ in 0..128 { entities.run(TICK); }
    entities.accept_input(gate, &InputEvent::new("Close"));
    for _ in 0..128 { entities.run(TICK); }
    assert_eq!(entities.get(gate).unwrap().origin.z, 0.0);

    let end = walk(&bsp, &mut entities, Vec3::new(32.0, 64.0, 1.0), 4.0);
    assert!(end.origin.x < 256.0, "a closed door should block again, got {:?}", end.origin);
}

#[test]
fn walking_into_a_trigger_opens_the_door_in_front_of_it() {
    // The full loop: geometry, a trigger volume, entity I/O, a mover, and
    // collision against the mover -- which is most of what a level is.
    let bsp = build(&corridor_map(true, true));
    let mut entities = spawned_world(&bsp);
    let gate = entities.find_by_name("gate")[0];
    let trigger = entities.find_by_name("gate_trigger")[0];
    assert_eq!(entities.get(gate).unwrap().origin.z, 0.0, "starts closed");

    // Drive the trigger the way the engine does, from the player's box.
    let mut state = MoveState { origin: Vec3::new(32.0, 64.0, 1.0), on_ground: true, ..Default::default() };
    let params = MoveParams::default();
    let input = MoveInput { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };

    let trigger_model = entities.get(trigger).unwrap().brush_model.unwrap();
    let trigger_bounds = bsp.models[trigger_model].bounds();

    for _ in 0..(4.0 / TICK) as usize {
        let hull = state.hull();
        let player_box = Aabb::new(state.origin + hull.mins, state.origin + hull.maxs);
        let inside = trigger_bounds.intersects(&player_box);
        void_game::triggers::update_touch(&mut entities, trigger, inside, None);

        let world = LevelCollision::new(&bsp, &entities);
        player_move(&mut state, &input, &params, &world, TICK);
        entities.run(TICK);
    }

    assert!(
        entities.get(gate).unwrap().origin.z > 100.0,
        "the trigger should have opened the door"
    );
    assert!(
        state.origin.x > 300.0,
        "and the player should have got through, but reached {:?}",
        state.origin
    );
}

#[test]
fn a_trigger_volume_does_not_block_the_player() {
    // Triggers are brush models too, and colliding with them would wall off
    // every trigger in the game.
    let bsp = build(&corridor_map(false, true));
    let mut entities = spawned_world(&bsp);
    let end = walk(&bsp, &mut entities, Vec3::new(32.0, 64.0, 1.0), 4.0);
    assert!(end.origin.x > 400.0, "a trigger stopped the player at {:?}", end.origin);
}

#[test]
fn the_engine_loads_and_ticks_a_compiled_map() {
    use void_engine::engine::{Engine, EngineConfig};

    // Write the compiled map somewhere the engine's VFS can find it.
    let dir = std::env::temp_dir().join(format!("voidengine-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    let bsp = build(&corridor_map(true, true));
    std::fs::write(dir.join("maps/testmap.voidbsp"), bsp.to_bytes()).unwrap();

    let mut engine = Engine::new(&EngineConfig {
        content_paths: vec![dir.clone()],
        ..Default::default()
    });
    engine.load_map("testmap").expect("the engine should load it");

    // The player should have spawned at the info_player_start.
    assert!(engine.player.entity.is_some());
    assert!(engine.player.movement.origin.x < 64.0, "{:?}", engine.player.movement.origin);

    let input = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(4.0 / TICK) as usize {
        engine.tick(TICK, &input);
    }

    assert!(engine.tick_count > 0);
    assert!(
        engine.player.movement.origin.x > 200.0,
        "the player should have walked forward, reaching {:?}",
        engine.player.movement.origin
    );
    assert!(engine.player.health > 0.0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_the_engine_logs_reaches_the_console() {
    // The gap this closes: everything logged through the `log` crate went to
    // stderr and nowhere else, so a door reporting a missing target was
    // invisible from inside the game -- and the console, the one place anyone
    // would look, showed nothing.
    use log::Log;
    use void_console::{LogLevel, LogRelay};
    use void_engine::engine::{Engine, EngineConfig};

    let relay = std::sync::Arc::new(LogRelay::detached(log::LevelFilter::Debug));
    let mut engine = Engine::new(&EngineConfig {
        log: Some(std::sync::Arc::clone(&relay)),
        content_paths: vec![],
        ..Default::default()
    });

    let before = engine.console.log_len();
    relay.log(
        &log::Record::builder()
            .level(log::Level::Warn)
            .target("void_game")
            .args(format_args!("func_door could not find `gate`"))
            .build(),
    );
    // Nothing arrives until a frame runs: logging happens on whatever thread
    // is running, and the console is single-owner on the main one.
    assert_eq!(engine.console.log_len(), before);

    engine.frame(0.016, &InputState::default());

    let found = engine
        .console
        .log()
        .any(|l| l.level == LogLevel::Warning && l.text.contains("could not find `gate`"));
    assert!(found, "the warning never reached the console");
}

#[test]
fn the_console_does_not_repeat_its_own_output() {
    // `Console::print` forwards to the `log` crate so the file and stderr get
    // it too. Without the target check that line would come straight back and
    // appear twice.
    use void_console::LogRelay;
    use void_engine::engine::{Engine, EngineConfig};

    let relay = std::sync::Arc::new(LogRelay::detached(log::LevelFilter::Debug));
    let mut engine = Engine::new(&EngineConfig {
        log: Some(std::sync::Arc::clone(&relay)),
        content_paths: vec![],
        ..Default::default()
    });

    engine.console.print("hello from the console");
    engine.frame(0.016, &InputState::default());

    let count = engine.console.log().filter(|l| l.text == "hello from the console").count();
    assert_eq!(count, 1, "the console echoed itself");
}

// ---- scripting ------------------------------------------------------------

/// An engine with a map and a script file on disk, since both are read
/// through the VFS.
fn engine_with_script(script: &str) -> (void_engine::engine::Engine, std::path::PathBuf) {
    use void_engine::engine::{Engine, EngineConfig};

    let dir = std::env::temp_dir().join(format!(
        "voidengine-script-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let bsp = build(&corridor_map(true, true));
    std::fs::write(dir.join("maps/testmap.voidbsp"), bsp.to_bytes()).unwrap();
    std::fs::write(dir.join("scripts/testmap.voidscript"), script).unwrap();

    let engine = Engine::new(&EngineConfig {
        content_paths: vec![dir.clone()],
        ..Default::default()
    });
    (engine, dir)
}

#[test]
fn a_maps_script_loads_with_it_and_its_start_hook_runs() {
    let (mut engine, dir) = engine_with_script(
        r#" fn on_map_start() { print("the script ran"); } "#,
    );
    engine.load_map("testmap").unwrap();
    assert!(
        engine.console.log().any(|l| l.text == "the script ran"),
        "on_map_start never ran"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_with_no_script_is_silent_rather_than_an_error() {
    use void_engine::engine::{Engine, EngineConfig};
    let dir = std::env::temp_dir().join(format!("voidengine-noscript-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    let bsp = build(&corridor_map(true, true));
    std::fs::write(dir.join("maps/testmap.voidbsp"), bsp.to_bytes()).unwrap();

    let mut engine = Engine::new(&EngineConfig {
        content_paths: vec![dir.clone()],
        ..Default::default()
    });
    engine.load_map("testmap").unwrap();
    assert!(!engine.console.log().any(|l| l.text.contains("script")), "it complained");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_script_can_open_a_door_through_the_same_path_a_wire_would() {
    // The point of the whole action queue: a script fires an input, and it
    // arrives through the event queue with the same ordering and delays an
    // output wired in the editor would have.
    let (mut engine, dir) = engine_with_script("");
    engine.load_map("testmap").unwrap();

    let door = engine.entities.find_by_name("gate").first().copied().expect("the map has a door");
    let before = engine.entities.get(door).unwrap().fields.f32("door_state", -1.0);

    engine.run_script(r#" ent_fire("gate", "Open"); "#).unwrap();
    for _ in 0..8 { engine.tick(TICK, &InputState::default()); }

    let after = engine.entities.get(door).unwrap().fields.f32("door_state", -1.0);
    assert_ne!(before, after, "the door never moved");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_script_reads_the_world_it_is_actually_in() {
    let (mut engine, dir) = engine_with_script("");
    engine.load_map("testmap").unwrap();

    assert_eq!(engine.run_script("map_name()").unwrap().as_deref(), Some("testmap"));
    assert_eq!(
        engine.run_script(r#" find_by_name("gate").classname "#).unwrap().as_deref(),
        Some("func_door")
    );
    // The player exists and is somewhere, and a script can measure from it.
    assert_eq!(engine.run_script("player() != ()").unwrap().as_deref(), Some("true"));
    assert_eq!(
        engine.run_script(r#" cvar_float("sv_gravity") > 0.0 "#).unwrap().as_deref(),
        Some("true")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_script_setting_a_keyvalue_changes_the_entity() {
    let (mut engine, dir) = engine_with_script("");
    engine.load_map("testmap").unwrap();
    engine.run_script(r#" find_by_name("gate").set("speed", 999.0); "#).unwrap();

    let door = engine.entities.find_by_name("gate")[0];
    assert_eq!(engine.entities.get(door).unwrap().fields.f32("speed", 0.0), 999.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_handle_a_script_kept_across_a_death_does_not_hit_the_next_entity() {
    // Handles carry the slot's generation for the same reason queued events
    // do. Without it a script holding a reference over a respawn would be
    // acting on whatever moved into the slot.
    use void_engine::scripting::{pack, unpack};
    use void_script::ScriptAction;

    let (mut engine, dir) = engine_with_script("");
    engine.load_map("testmap").unwrap();

    let victim = engine.entities.find_by_name("gate")[0];
    let stale = pack(victim);
    engine.entities.remove(victim);
    engine.entities.run(0.0);

    // The slot is free; something else takes it.
    let replacement = engine.entities.spawn("logic_relay");
    assert_eq!(replacement.index, unpack(stale).index, "the test needs the slot reused");

    engine.apply_script_actions(vec![ScriptAction::SetField {
        entity: stale,
        key: "speed".into(),
        value: "1".into(),
    }]);
    assert!(
        engine.entities.get(replacement).unwrap().fields.text("speed").is_none(),
        "a stale handle wrote to the entity that replaced it"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tick_hook_runs_every_tick_and_sees_time_moving() {
    let (mut engine, dir) = engine_with_script(
        r#"
        let ticks = 0;
        fn on_tick(dt) {
            ticks += 1;
            if ticks == 3 { print(`three ticks at ${time() > 0.0}`); }
        }
        "#,
    );
    engine.load_map("testmap").unwrap();
    for _ in 0..4 { engine.tick(TICK, &InputState::default()); }
    assert!(
        engine.console.log().any(|l| l.text == "three ticks at true"),
        "the tick hook did not run"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_script_reports_and_leaves_the_game_running() {
    let (mut engine, dir) = engine_with_script(" this is not valid ((( ");
    engine.load_map("testmap").unwrap();
    assert!(
        engine.console.log().any(|l| l.level == void_console::LogLevel::Error),
        "a broken script loaded silently"
    );
    // ...and the map is still playable.
    for _ in 0..4 { engine.tick(TICK, &InputState::default()); }
    assert!(engine.tick_count > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_logic_script_entity_calls_a_function_when_its_input_fires() {
    // The seam: an output fires an input, and a function in the map's script
    // runs. This is what makes scripting reachable from the editor.
    use void_entity::{InputEvent, Target};

    let (mut engine, dir) = engine_with_script(
        r#" fn on_used(who) { print(`used by ${who}`); } "#,
    );
    engine.load_map("testmap").unwrap();

    let id = engine.entities.spawn("logic_script");
    engine.entities.set_targetname(id, "brain");
    engine.entities.get_mut(id).unwrap().fields.set(
        "function",
        void_entity::Value::Text("on_used".into()),
    );

    engine.entities.accept_input(id, &InputEvent::new("CallScriptFunction"));
    engine.take_entity_requests();
    assert!(
        engine.console.log().any(|l| l.text == "used by brain"),
        "the script function never ran"
    );

    // ...and through the queue, the way an output would arrive.
    engine.entities.queue_input(Target::Named("brain".into()), "CallScriptFunction", "", 0.0, None, None);
    engine.tick(TICK, &InputState::default());
    let runs = engine.console.log().filter(|l| l.text == "used by brain").count();
    assert_eq!(runs, 2, "the queued input did not reach the script");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_logic_script_can_carry_its_code_inline() {
    use void_entity::InputEvent;

    let (mut engine, dir) = engine_with_script("");
    engine.load_map("testmap").unwrap();

    let id = engine.entities.spawn("logic_script");
    engine.entities.get_mut(id).unwrap().fields.set(
        "code",
        void_entity::Value::Text(r#" print("inline"); "#.into()),
    );
    engine.entities.accept_input(id, &InputEvent::new("RunScriptCode"));
    engine.take_entity_requests();

    assert!(engine.console.log().any(|l| l.text == "inline"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_script_console_command_runs_and_shows_its_value() {
    use void_engine::engine::take_console_requests;

    let (mut engine, dir) = engine_with_script("");
    engine.load_map("testmap").unwrap();
    engine.console.set("sv_cheats", "1");

    engine.console.execute("script 6 * 7");
    take_console_requests(&mut engine);
    assert!(engine.console.log().any(|l| l.text == "42"), "the value was not shown");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scripting_is_cheat_protected() {
    // A script can move entities and set convars. It is not something a
    // server should hand out for free.
    let (mut engine, dir) = engine_with_script("");
    engine.load_map("testmap").unwrap();
    engine.console.set("sv_cheats", "0");

    engine.console.execute(r#" script print("should not run") "#);
    void_engine::engine::take_console_requests(&mut engine);
    assert!(!engine.console.log().any(|l| l.text == "should not run"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- sound ----------------------------------------------------------------

#[test]
fn the_mixer_runs_whether_or_not_there_is_a_sound_card() {
    // If audio only existed when a device opened, then how many voices a
    // trigger starts would differ between a machine with sound and one
    // without -- and only one of those would ever be tested.
    use void_engine::audio::AudioSystem;
    let audio = AudioSystem::silent();
    assert!(!audio.is_audible());
    audio.with_mixer(|mixer| {
        let sound = std::sync::Arc::new(void_audio::Sound {
            channels: 1,
            sample_rate: 48_000,
            samples: vec![1.0; 4800],
        });
        mixer.play(sound, void_audio::SoundParams::default());
        assert_eq!(mixer.voice_count(), 1);
        let mut out = vec![0.0; 256 * 2];
        mixer.mix(&mut out);
        assert!(out.iter().any(|s| *s != 0.0), "silent mode still has to mix");
    });
}

/// An engine whose content tree has a sound in it.
fn engine_with_sound() -> (void_engine::engine::Engine, std::path::PathBuf) {
    use void_engine::engine::{Engine, EngineConfig};

    let dir = std::env::temp_dir().join(format!(
        "voidengine-audio-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::create_dir_all(dir.join("sound/test")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();

    let bsp = build(&corridor_map(true, true));
    std::fs::write(dir.join("maps/testmap.voidbsp"), bsp.to_bytes()).unwrap();
    std::fs::write(dir.join("sound/test/beep.wav"), &wav_bytes(4800)).unwrap();
    std::fs::write(
        dir.join("scripts/test.voidsnd"),
        r#" sound { "name" "test/beep" "file" "sound/test/beep.wav" "volume" "0.5" } "#,
    )
    .unwrap();

    let engine = Engine::new(&EngineConfig {
        content_paths: vec![dir.clone()],
        ..Default::default()
    });
    (engine, dir)
}

/// A minimal 16-bit mono WAV, since the engine reads real files.
fn wav_bytes(frames: usize) -> Vec<u8> {
    let data: Vec<u8> = (0..frames).flat_map(|_| 16384i16.to_le_bytes()).collect();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&22050u32.to_le_bytes());
    out.extend_from_slice(&44100u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[test]
fn sound_scripts_load_with_the_engine() {
    let (engine, dir) = engine_with_sound();
    assert!(engine.audio.bank.script().get("test/beep").is_some(), "the script did not load");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sound_is_decoded_the_first_time_it_is_asked_for() {
    // On demand rather than up front: a level references a handful of the
    // sounds a game ships.
    let (mut engine, dir) = engine_with_sound();
    assert!(!engine.audio.bank.is_loaded("test/beep"));

    let vfs = engine.vfs.clone();
    assert!(engine.audio.sound(&vfs, "test/beep").is_some());
    assert!(engine.audio.bank.is_loaded("test/beep"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sound_that_is_not_there_is_reported_once_and_then_not_again() {
    // A trigger firing every tick would otherwise fill the console until
    // nothing else in it is readable.
    let (mut engine, dir) = engine_with_sound();
    let vfs = engine.vfs.clone();
    assert!(engine.audio.sound(&vfs, "nope/missing").is_none());
    assert!(engine.audio.bank.already_missing("nope/missing"));
    assert!(engine.audio.sound(&vfs, "nope/missing").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_play_command_starts_a_voice() {
    use void_engine::engine::take_console_requests;
    let (mut engine, dir) = engine_with_sound();

    engine.console.execute("play test/beep");
    take_console_requests(&mut engine);
    assert_eq!(engine.audio.with_mixer(|m| m.voice_count()), 1);

    engine.console.execute("stopsound");
    take_console_requests(&mut engine);
    assert_eq!(engine.audio.with_mixer(|m| m.voice_count()), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_volume_convar_reaches_the_mixer() {
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();
    engine.console.set("volume", "0.25");
    engine.tick(TICK, &InputState::default());
    assert!((engine.audio.with_mixer(|m| m.volume) - 0.25).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_listener_follows_the_player() {
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();

    let input = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(2.0 / TICK) as usize { engine.tick(TICK, &input); }

    let ears = engine.audio.with_mixer(|m| m.listener.position);
    let eye = engine.player.movement.eye_position();
    assert!((ears - eye).length() < 1.0, "ears at {ears:?}, head at {eye:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ambient_generic_starts_with_the_map_and_can_be_stopped() {
    use void_entity::InputEvent;
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();

    let id = engine.entities.spawn("ambient_generic");
    engine.entities.get_mut(id).unwrap().fields.set(
        "message",
        void_entity::Value::Text("test/beep".into()),
    );
    // Spawn runs the class's own start, which is what a map load does.
    void_game::sound::register(&mut void_entity::ClassRegistry::new());
    engine.entities.accept_input(id, &InputEvent::new("PlaySound"));
    engine.take_entity_requests();
    assert_eq!(engine.audio.with_mixer(|m| m.voice_count()), 1, "it did not start");

    engine.entities.accept_input(id, &InputEvent::new("StopSound"));
    engine.take_entity_requests();
    assert_eq!(engine.audio.with_mixer(|m| m.voice_count()), 0, "it did not stop");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ambient_generic_is_heard_from_where_it_is() {
    use void_entity::InputEvent;
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();

    let id = engine.entities.spawn("ambient_generic");
    {
        let e = engine.entities.get_mut(id).unwrap();
        e.fields.set("message", void_entity::Value::Text("test/beep".into()));
        e.origin = Vec3::new(4000.0, 0.0, 0.0);
    }
    engine.entities.accept_input(id, &InputEvent::new("PlaySound"));
    engine.take_entity_requests();

    // Far away and off to one side: quiet, and not centred.
    engine.audio.set_listener(Vec3::ZERO, Angles::ZERO.vectors());
    let mut out = vec![0.0; 512 * 2];
    engine.audio.with_mixer(|m| m.mix(&mut out));
    let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(peak < 0.2, "a sound 4000 units away was loud: {peak}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_script_can_play_a_sound() {
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();
    engine.run_script(r#" play_sound("test/beep"); "#).unwrap();
    assert_eq!(engine.audio.with_mixer(|m| m.voice_count()), 1);

    engine.run_script(" stop_sounds(); ").unwrap();
    assert_eq!(engine.audio.with_mixer(|m| m.voice_count()), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn loading_a_map_silences_the_one_before_it() {
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();
    engine.run_script(r#" play_sound("test/beep"); "#).unwrap();
    assert_eq!(engine.audio.with_mixer(|m| m.voice_count()), 1);

    engine.load_map("testmap").unwrap();
    assert_eq!(engine.audio.with_mixer(|m| m.voice_count()), 0, "the last level is still playing");
    let _ = std::fs::remove_dir_all(&dir);
}
