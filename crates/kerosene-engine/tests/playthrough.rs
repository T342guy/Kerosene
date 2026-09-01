// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! End-to-end tests: build a map, compile it, load it, and play it.
//!
//! These go through the whole stack -- `.keromap` source, Cleave's compile, the
//! `.kerobsp` loader, entity spawning, movement and collision -- because that is
//! the only place the seams between them show. Every crate can pass its own
//! tests and still not add up to a level you can walk around.

use cleave::{CompileOptions, compile};
use kerosene_bsp::Bsp;
use kerosene_engine::collision::LevelCollision;
use kerosene_engine::input::InputState;
use kerosene_entity::{EntityWorld, InputEvent};
use kerosene_map::{Connection, Entity, Map, Solid};
use kerosene_math::{Aabb, Angles, Vec3};
use kerosene_physics::{MoveInput, MoveParams, MoveState, player_move};

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
    Bsp::from_bytes(&bytes, "test.kerobsp").expect("the compiled map should reload")
}

fn spawned_world(bsp: &Bsp) -> EntityWorld {
    let mut world = EntityWorld::new(kerosene_game::registry());
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
        kerosene_game::triggers::update_touch(&mut entities, trigger, inside, None);

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
    use kerosene_engine::engine::{Engine, EngineConfig};

    // Write the compiled map somewhere the engine's VFS can find it.
    let dir = std::env::temp_dir().join(format!("kerosene-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    let bsp = build(&corridor_map(true, true));
    std::fs::write(dir.join("maps/testmap.kerobsp"), bsp.to_bytes()).unwrap();

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
    use kerosene_console::{LogLevel, LogRelay};
    use kerosene_engine::engine::{Engine, EngineConfig};

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
            .target("kerosene_game")
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
    use kerosene_console::LogRelay;
    use kerosene_engine::engine::{Engine, EngineConfig};

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
fn engine_with_script(script: &str) -> (kerosene_engine::engine::Engine, std::path::PathBuf) {
    use kerosene_engine::engine::{Engine, EngineConfig};

    let dir = std::env::temp_dir().join(format!(
        "kerosene-script-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let bsp = build(&corridor_map(true, true));
    std::fs::write(dir.join("maps/testmap.kerobsp"), bsp.to_bytes()).unwrap();
    std::fs::write(dir.join("scripts/testmap.keroscript"), script).unwrap();

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
    use kerosene_engine::engine::{Engine, EngineConfig};
    let dir = std::env::temp_dir().join(format!("kerosene-noscript-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    let bsp = build(&corridor_map(true, true));
    std::fs::write(dir.join("maps/testmap.kerobsp"), bsp.to_bytes()).unwrap();

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
    use kerosene_engine::scripting::{pack, unpack};
    use kerosene_script::ScriptAction;

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
        engine.console.log().any(|l| l.level == kerosene_console::LogLevel::Error),
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
    use kerosene_entity::{InputEvent, Target};

    let (mut engine, dir) = engine_with_script(
        r#" fn on_used(who) { print(`used by ${who}`); } "#,
    );
    engine.load_map("testmap").unwrap();

    let id = engine.entities.spawn("logic_script");
    engine.entities.set_targetname(id, "brain");
    engine.entities.get_mut(id).unwrap().fields.set(
        "function",
        kerosene_entity::Value::Text("on_used".into()),
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
    use kerosene_entity::InputEvent;

    let (mut engine, dir) = engine_with_script("");
    engine.load_map("testmap").unwrap();

    let id = engine.entities.spawn("logic_script");
    engine.entities.get_mut(id).unwrap().fields.set(
        "code",
        kerosene_entity::Value::Text(r#" print("inline"); "#.into()),
    );
    engine.entities.accept_input(id, &InputEvent::new("RunScriptCode"));
    engine.take_entity_requests();

    assert!(engine.console.log().any(|l| l.text == "inline"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_script_console_command_runs_and_shows_its_value() {
    use kerosene_engine::engine::take_console_requests;

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
    kerosene_engine::engine::take_console_requests(&mut engine);
    assert!(!engine.console.log().any(|l| l.text == "should not run"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- sound ----------------------------------------------------------------

#[test]
fn the_mixer_runs_whether_or_not_there_is_a_sound_card() {
    // If audio only existed when a device opened, then how many voices a
    // trigger starts would differ between a machine with sound and one
    // without -- and only one of those would ever be tested.
    use kerosene_engine::audio::AudioSystem;
    let audio = AudioSystem::silent();
    assert!(!audio.is_audible());
    audio.with_mixer(|mixer| {
        let sound = std::sync::Arc::new(kerosene_audio::Sound {
            channels: 1,
            sample_rate: 48_000,
            samples: vec![1.0; 4800],
        });
        mixer.play(sound, kerosene_audio::SoundParams::default());
        assert_eq!(mixer.voice_count(), 1);
        let mut out = vec![0.0; 256 * 2];
        mixer.mix(&mut out);
        assert!(out.iter().any(|s| *s != 0.0), "silent mode still has to mix");
    });
}

/// An engine whose content tree has a sound in it.
fn engine_with_sound() -> (kerosene_engine::engine::Engine, std::path::PathBuf) {
    use kerosene_engine::engine::{Engine, EngineConfig};

    let dir = std::env::temp_dir().join(format!(
        "kerosene-audio-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::create_dir_all(dir.join("sound/test")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();

    let bsp = build(&corridor_map(true, true));
    std::fs::write(dir.join("maps/testmap.kerobsp"), bsp.to_bytes()).unwrap();
    std::fs::write(dir.join("sound/test/beep.wav"), &wav_bytes(4800)).unwrap();
    std::fs::write(
        dir.join("scripts/test.kerosnd"),
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
    use kerosene_engine::engine::take_console_requests;
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
    use kerosene_entity::InputEvent;
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();

    let id = engine.entities.spawn("ambient_generic");
    engine.entities.get_mut(id).unwrap().fields.set(
        "message",
        kerosene_entity::Value::Text("test/beep".into()),
    );
    // Spawn runs the class's own start, which is what a map load does.
    kerosene_game::sound::register(&mut kerosene_entity::ClassRegistry::new());
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
    use kerosene_entity::InputEvent;
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();

    let id = engine.entities.spawn("ambient_generic");
    {
        let e = engine.entities.get_mut(id).unwrap();
        e.fields.set("message", kerosene_entity::Value::Text("test/beep".into()));
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

// ---- the use key ----------------------------------------------------------
//
// A player pressing a button is the one interaction that cannot be tested
// anywhere below the engine: it needs the trace, the entity world and the map
// geometry all agreeing about where things are.

/// Add a brush entity to a map, with fresh ids for it and its solid.
fn add_brush_entity(map: &mut Map, classname: &str, bounds: Aabb, material: &str) -> usize {
    let id = map.next_id();
    let solid_id = map.next_id();
    let side_ids: Vec<u32> = (0..6).map(|_| map.next_id()).collect();
    let mut solid = Solid::cube(bounds, material);
    solid.id = solid_id;
    for (s, sid) in solid.sides.iter_mut().zip(side_ids) { s.id = sid; }

    let mut entity = Entity::new(id, classname);
    entity.solids.push(solid);
    map.entities.push(entity);
    map.entities.len() - 1
}

/// The corridor, with a button on the player's eye line wired to the door.
fn corridor_with_button() -> Map {
    let mut map = corridor_map(true, false);
    // 64 units in front of the spawn, which is inside the default 80-unit
    // reach and outside anything the player is standing in.
    let at = add_brush_entity(
        &mut map,
        "func_button",
        Aabb::new(Vec3::new(96.0, 48.0, 32.0), Vec3::new(112.0, 80.0, 96.0)),
        "dev/grid",
    );
    map.entities[at].set("targetname", "switch");
    map.entities[at].set("movedir", "1 0 0");
    map.entities[at].set("speed", "40");
    map.entities[at].set("wait", "-1");
    map.entities[at].connect(Connection::new("OnPressed", "gate", "Open"));
    map
}

fn engine_with(map: &Map, name: &str) -> (kerosene_engine::engine::Engine, std::path::PathBuf) {
    use kerosene_engine::engine::{Engine, EngineConfig};

    let dir = std::env::temp_dir().join(format!(
        "kerosene-use-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("maps/testmap.kerobsp"), build(map).to_bytes()).unwrap();

    let mut engine = Engine::new(&EngineConfig {
        content_paths: vec![dir.clone()],
        ..Default::default()
    });
    engine.load_map("testmap").expect("the test map should load");
    (engine, dir)
}

fn door_progress(engine: &kerosene_engine::engine::Engine) -> f32 {
    let id = *engine.entities.find_by_name("gate").first().expect("the map has a door");
    kerosene_game::doors::door_progress(&engine.entities, id)
}

/// Hold the use key down for a moment while facing `yaw`, then let go.
fn press_use(engine: &mut kerosene_engine::engine::Engine, yaw: f32, seconds: f32) {
    let looking = Angles { pitch: 0.0, yaw, roll: 0.0 };
    let held = InputState { use_key: true, view_angles: looking, ..Default::default() };
    for _ in 0..(seconds / TICK).ceil() as usize {
        engine.tick(TICK, &held);
    }
    engine.tick(TICK, &InputState { view_angles: looking, ..Default::default() });
}

#[test]
fn pressing_use_while_looking_at_a_button_works_it() {
    let (mut engine, dir) = engine_with(&corridor_with_button(), "button");
    assert_eq!(door_progress(&engine), 0.0, "the door starts shut");

    press_use(&mut engine, 0.0, 0.1);
    for _ in 0..(1.0 / TICK) as usize {
        engine.tick(TICK, &InputState { view_angles: Angles::ZERO, ..Default::default() });
    }

    assert!(
        door_progress(&engine) > 0.0,
        "the button should have fired OnPressed and opened the door"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn holding_use_presses_once_rather_than_every_tick() {
    // Using is an edge. Holding the key against a door has to open it, not
    // toggle it sixty-four times a second -- which would leave the door in
    // whichever state the last tick happened to land on.
    let (mut engine, dir) = engine_with(&corridor_map(true, false), "held");

    // Look at the door: it is 224 units away, well beyond reach, so nothing
    // should happen at all from here.
    press_use(&mut engine, 0.0, 0.5);
    assert_eq!(door_progress(&engine), 0.0, "nothing is within reach");

    // Walk up to it, then hold use for half a second.
    let walking = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(2.0 / TICK) as usize {
        engine.tick(TICK, &walking);
    }
    press_use(&mut engine, 0.0, 0.5);
    for _ in 0..(2.0 / TICK) as usize {
        engine.tick(TICK, &InputState { view_angles: Angles::ZERO, ..Default::default() });
    }

    assert_eq!(
        door_progress(&engine), 1.0,
        "one press, so the door opened and stayed open"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn using_a_wall_does_nothing_and_says_nothing() {
    // The common case is a missed press. It has to be silent: a message on
    // every use that hit geometry would bury everything else in the console.
    let (mut engine, dir) = engine_with(&corridor_map(false, false), "wall");
    let before = engine.console.log().count();

    press_use(&mut engine, 0.0, 0.2);

    assert_eq!(engine.console.log().count(), before, "a missed use must not chatter");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_button_can_switch_a_brush_out_of_the_world() {
    // The wiring the sample level uses: press the switch, the shutter stops
    // being there. Worth an end-to-end test rather than only a unit one,
    // because "stops being solid" is a fact about the collision world, and
    // the collision world is rebuilt from the entity world every tick.
    let mut map = corridor_map(false, false);
    // Set into the side wall beside the spawn, which is where a button
    // actually goes: in the corridor proper it would be something to walk into.
    let switch = add_brush_entity(
        &mut map,
        "func_button",
        Aabb::new(Vec3::new(24.0, 112.0, 48.0), Vec3::new(40.0, 128.0, 96.0)),
        "dev/grid",
    );
    map.entities[switch].set("targetname", "switch");
    map.entities[switch].set("movedir", "0 1 0");
    map.entities[switch].set("wait", "-1");
    map.entities[switch].connect(Connection::new("OnPressed", "shutter", "Toggle"));

    let shutter = add_brush_entity(
        &mut map,
        "func_brush",
        Aabb::new(Vec3::new(300.0, 0.0, 0.0), Vec3::new(316.0, 128.0, 128.0)),
        "dev/grid",
    );
    map.entities[shutter].set("targetname", "shutter");

    let (mut engine, dir) = engine_with(&map, "shutter");
    let walking = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };

    // The shutter blocks the corridor, so walking runs into it.
    for _ in 0..(4.0 / TICK) as usize { engine.tick(TICK, &walking); }
    let stopped_at = engine.player.movement.origin.x;
    assert!(stopped_at < 300.0, "the shutter should be in the way, stopped at {stopped_at}");

    // Start again, press the switch from the spawn -- which is where it is,
    // set into the wall beside it -- and walk the same corridor.
    engine.load_map("testmap").expect("reloading the map should work");
    press_use(&mut engine, 90.0, 0.1);
    for _ in 0..(6.0 / TICK) as usize { engine.tick(TICK, &walking); }

    assert!(
        engine.player.movement.origin.x > 320.0,
        "with the shutter switched off the corridor should be open, reached {}",
        engine.player.movement.origin.x
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- rotation ---------------------------------------------------------------

/// The corridor with a bar across half of it, turned by `yaw`.
///
/// At rest the bar covers the half of the corridor the player walks down; a
/// quarter turn about its own centre swings it out of the way. Which is the
/// whole test: the renderer and the collision have to agree about where a
/// turned model is, and a bar that blocks where it is *not* drawn is the
/// failure worth catching.
fn corridor_with_bar(yaw: f32) -> Map {
    let mut map = corridor_map(false, false);
    let at = add_brush_entity(
        &mut map,
        "func_rotating",
        Aabb::new(Vec3::new(250.0, 0.0, 0.0), Vec3::new(266.0, 64.0, 128.0)),
        "dev/grid",
    );
    map.entities[at].set("targetname", "bar");
    map.entities[at].set("maxspeed", "90");
    map.entities[at].set("angles", &format!("0 {yaw} 0"));
    map
}

fn walked_to(engine: &mut kerosene_engine::engine::Engine, seconds: f32) -> f32 {
    let walking = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(seconds / TICK) as usize { engine.tick(TICK, &walking); }
    engine.player.movement.origin.x
}

#[test]
fn an_unturned_bar_blocks_the_corridor() {
    let (mut engine, dir) = engine_with(&corridor_with_bar(0.0), "bar-0");
    let reached = walked_to(&mut engine, 3.0);
    assert!(reached < 250.0, "the bar should be in the way, walked to {reached}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bar_turned_a_quarter_turn_is_no_longer_in_the_way() {
    // The same geometry, the same position, ninety degrees about its own
    // centre. If the collision code ignored the angles this would still block.
    let (mut engine, dir) = engine_with(&corridor_with_bar(90.0), "bar-90");
    let reached = walked_to(&mut engine, 3.0);
    assert!(reached > 300.0, "the bar should have swung clear, walked to {reached}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_turned_model_is_drawn_where_it_blocks() {
    // Renderer and collision take their transform from the same function, and
    // this is the assertion that keeps it that way: the pose the renderer is
    // handed must be the pose the collision world was built from.
    use kerosene_engine::collision::LevelCollision;

    let (engine, dir) = engine_with(&corridor_with_bar(90.0), "bar-agree");
    let bsp = &engine.level.as_ref().unwrap().bsp;
    let drawn: Vec<_> = engine.brush_model_poses();
    let world = LevelCollision::new(bsp, &engine.entities);

    assert_eq!(drawn.len(), 1, "one brush entity in this map");
    let (model, pose) = drawn[0];
    assert_eq!(world.mover_count(), 1);

    // The bar is x 250..266, y 0..64, turning about (258, 32). A quarter turn
    // puts it at x 226..290, y 24..40. Both probes avoid the corridor's own
    // dividing wall (x 256..272 outside the y 32..96 doorway), so the only
    // thing either can be hitting is the bar.
    let solid = Vec3::new(240.0, 32.0, 64.0);      // in the turned bar
    let clear = Vec3::new(260.0, 50.0, 64.0);      // in the unturned one, in the doorway
    let probe = |at: Vec3| {
        world
            .trace(at, at, Vec3::ZERO, Vec3::ZERO, kerosene_bsp::contents::MASK_PLAYER_SOLID)
            .start_solid
    };

    assert!(probe(solid), "the turned bar should be solid where it is drawn, at {solid:?}");
    assert!(!probe(clear), "and not solid where it used to be, at {clear:?}");

    // And the pose the renderer is handed is the one that produced those
    // answers, which is the invariant the two probes are evidence for.
    assert_eq!(pose.angles.yaw, 90.0);
    assert!(pose.is_rotated());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_rotating_brush_actually_turns_while_the_engine_runs() {
    let mut map = corridor_with_bar(0.0);
    let at = map.entities.len() - 1;
    map.entities[at].set("spawnflags", "1");

    let (mut engine, dir) = engine_with(&map, "bar-spin");
    let idle = InputState { view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(1.0 / TICK) as usize { engine.tick(TICK, &idle); }

    let bar = *engine.entities.find_by_name("bar").first().unwrap();
    let turned = engine.entities.get(bar).unwrap().angles.yaw;
    assert!(turned > 60.0, "90 degrees a second should have turned it, got {turned}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- push and teleport ------------------------------------------------------

#[test]
fn walking_into_a_trigger_push_throws_the_player() {
    let mut map = corridor_map(false, false);
    let at = add_brush_entity(
        &mut map,
        "trigger_push",
        Aabb::new(Vec3::new(96.0, 0.0, 0.0), Vec3::new(160.0, 128.0, 64.0)),
        "tools/trigger",
    );
    map.entities[at].set("targetname", "pad");
    map.entities[at].set("pushdir", "0 0 1");
    map.entities[at].set("speed", "500");

    let (mut engine, dir) = engine_with(&map, "push");
    let walking = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };

    let mut highest: f32 = 0.0;
    for _ in 0..(2.0 / TICK) as usize {
        engine.tick(TICK, &walking);
        highest = highest.max(engine.player.movement.origin.z);
    }

    assert!(highest > 40.0, "should have been launched, only reached {highest}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_push_adds_to_the_speed_the_player_arrives_with() {
    // Rather than replacing it: running onto a pad should carry you further
    // than stepping onto one, which is the whole appeal of them.
    let mut map = corridor_map(false, false);
    let at = add_brush_entity(
        &mut map,
        "trigger_push",
        Aabb::new(Vec3::new(96.0, 0.0, 0.0), Vec3::new(160.0, 128.0, 64.0)),
        "tools/trigger",
    );
    map.entities[at].set("pushdir", "1 0 0");
    map.entities[at].set("speed", "400");

    let (mut engine, dir) = engine_with(&map, "push-add");
    let walking = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };

    // The peak, not the end: a horizontal shove along the floor is braked by
    // ground friction within a few ticks, which is correct and is why a pad
    // that means to launch you points upward.
    let mut fastest: f32 = 0.0;
    for _ in 0..(0.6 / TICK) as usize {
        engine.tick(TICK, &walking);
        fastest = fastest.max(engine.player.movement.velocity.x);
    }

    assert!(
        fastest > 400.0,
        "arrived at running speed and should have been shoved faster still, peaked at {fastest}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_trigger_teleport_moves_the_player_to_what_it_targets() {
    let mut map = corridor_map(false, false);
    let at = add_brush_entity(
        &mut map,
        "trigger_teleport",
        Aabb::new(Vec3::new(96.0, 0.0, 0.0), Vec3::new(160.0, 128.0, 128.0)),
        "tools/trigger",
    );
    map.entities[at].set("target", "arrival");

    let id = map.next_id();
    let mut destination = Entity::new(id, "info_target");
    destination.set_origin(Vec3::new(448.0, 64.0, 8.0));
    destination.set("targetname", "arrival");
    map.entities.push(destination);

    let (mut engine, dir) = engine_with(&map, "teleport");
    let walking = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(1.0 / TICK) as usize { engine.tick(TICK, &walking); }

    assert!(
        engine.player.movement.origin.x > 400.0,
        "should have been teleported past the wall, at {:?}",
        engine.player.movement.origin
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_teleport_with_no_destination_says_so_rather_than_moving_nowhere() {
    // Silently doing nothing is the worst option: a designer who mistyped a
    // targetname would walk through the volume and conclude teleports are
    // broken.
    let mut map = corridor_map(false, false);
    let at = add_brush_entity(
        &mut map,
        "trigger_teleport",
        Aabb::new(Vec3::new(96.0, 0.0, 0.0), Vec3::new(160.0, 128.0, 128.0)),
        "tools/trigger",
    );
    map.entities[at].set("target", "nowhere_at_all");

    let (mut engine, dir) = engine_with(&map, "teleport-broken");
    let walking = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(1.0 / TICK) as usize { engine.tick(TICK, &walking); }

    assert!(
        engine.console.log().any(|l| l.text.contains("nowhere_at_all")),
        "the broken target should be named in the console"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- the sky ---------------------------------------------------------------

#[test]
fn the_sky_takes_its_tint_from_the_maps_sun() {
    // The value was plumbed to the shader and never fed: `set_sky_color`
    // existed with no callers, so every sky rendered at full white however
    // the map was lit.
    let mut map = corridor_map(false, false);
    let id = map.next_id();
    let mut sun = Entity::new(id, "light_environment");
    sun.set_origin(Vec3::new(256.0, 64.0, 96.0));
    sun.set("_light", "255 128 64 200");
    map.entities.push(sun);

    let (engine, dir) = engine_with(&map, "sky");
    let sky = engine.level.as_ref().unwrap().sky_color;

    assert!((sky.x - 1.0).abs() < 0.01, "{sky:?}");
    assert!((sky.y - 128.0 / 255.0).abs() < 0.01, "{sky:?}");
    assert!((sky.z - 64.0 / 255.0).abs() < 0.01, "{sky:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_with_no_sun_gets_an_untinted_sky() {
    // White, not black: a tint of nothing has to mean no tint, or a map
    // without a light_environment would render its sky as a silhouette.
    let (engine, dir) = engine_with(&corridor_map(false, false), "no-sky");
    assert_eq!(engine.level.as_ref().unwrap().sky_color, Vec3::ONE);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_suns_brightness_does_not_leak_into_the_sky_tint() {
    // `_light` is "r g b brightness", and the brightness belongs to the
    // lighting compile. Taking all four would blow the sky out to white.
    let mut map = corridor_map(false, false);
    let id = map.next_id();
    let mut sun = Entity::new(id, "light_environment");
    sun.set_origin(Vec3::new(256.0, 64.0, 96.0));
    sun.set("_light", "128 128 128 4000");
    map.entities.push(sun);

    let (engine, dir) = engine_with(&map, "sky-bright");
    let sky = engine.level.as_ref().unwrap().sky_color;
    assert!((sky.x - 0.5).abs() < 0.01, "brightness leaked into the tint: {sky:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- ladders --------------------------------------------------------------

#[test]
fn a_func_ladder_can_be_climbed_in_a_compiled_map() {
    // End to end because that is where this could break: Cleave has to give
    // the brush ladder contents, the lump has to survive the round trip, and
    // the collision world has to report a volume that nothing collides with.
    let mut map = corridor_map(false, false);
    let at = add_brush_entity(
        &mut map,
        "func_ladder",
        Aabb::new(Vec3::new(16.0, 32.0, 0.0), Vec3::new(80.0, 96.0, 120.0)),
        "dev/grid",
    );
    map.entities[at].set("targetname", "climb");

    let (mut engine, dir) = engine_with(&map, "ladder");
    let start = engine.player.movement.origin.z;

    // Only a fifth of a second: the corridor ceiling is at 128 and the player
    // is 72 tall, so there are 47 units of headroom and a full second of
    // climbing would spend most of it pressed against the roof.
    let climbing = InputState { jump: true, view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(0.2 / TICK) as usize { engine.tick(TICK, &climbing); }

    assert!(engine.player.movement.on_ladder, "the player should be on the ladder");
    let climbed = engine.player.movement.origin.z - start;
    assert!(
        climbed > 30.0,
        "a fifth of a second at 200 ku/s should be about 40 units, climbed {climbed}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_ladder_does_not_block_the_player() {
    // It is a volume, not a wall. If it were solid the corridor would be
    // closed and nothing above would have caught it, because climbing works
    // just as well inside a box.
    let mut map = corridor_map(false, false);
    let at = add_brush_entity(
        &mut map,
        "func_ladder",
        Aabb::new(Vec3::new(96.0, 0.0, 0.0), Vec3::new(128.0, 128.0, 128.0)),
        "dev/grid",
    );
    map.entities[at].set("targetname", "climb");

    let (mut engine, dir) = engine_with(&map, "ladder-solid");
    let walking = InputState { forward: 1.0, view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(3.0 / TICK) as usize { engine.tick(TICK, &walking); }

    assert!(
        engine.player.movement.origin.x > 200.0,
        "walked into the ladder and stopped at {}",
        engine.player.movement.origin.x
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- trigger_hurt ---------------------------------------------------------

fn corridor_with_hurt(damage: &str) -> Map {
    let mut map = corridor_map(false, false);
    // Across the corridor where the player spawns, so it applies at once.
    let at = add_brush_entity(
        &mut map,
        "trigger_hurt",
        Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(128.0, 128.0, 128.0)),
        "tools/trigger",
    );
    map.entities[at].set("targetname", "burn");
    map.entities[at].set("damage", damage);
    map
}

#[test]
fn standing_in_a_trigger_hurt_costs_health_over_time() {
    // The class and its damage keyvalue both existed; nothing read them, so a
    // trigger_hurt fired its outputs and did no harm at all.
    let (mut engine, dir) = engine_with(&corridor_with_hurt("10"), "hurt");
    assert_eq!(engine.player.health, 100.0);

    let idle = InputState { view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(1.0 / TICK) as usize {
        engine.tick(TICK, &idle);
    }

    let lost = 100.0 - engine.player.health;
    assert!(
        (lost - 10.0).abs() < 1.0,
        "a second at 10 damage a second should cost about 10 health, lost {lost}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_trigger_hurt_that_kills_respawns_the_player_rather_than_leaving_them_dead() {
    let (mut engine, dir) = engine_with(&corridor_with_hurt("400"), "kill");

    let idle = InputState { view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(0.5 / TICK) as usize {
        engine.tick(TICK, &idle);
    }

    assert!(engine.console.log().any(|l| l.text.contains("died")), "death should be reported");
    assert!(engine.player.health > 0.0, "and the player should be alive again");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_disabled_trigger_hurt_does_no_damage() {
    let mut map = corridor_with_hurt("50");
    let at = map.entities.len() - 1;
    map.entities[at].set("startdisabled", "1");
    let (mut engine, dir) = engine_with(&map, "hurt-off");

    let idle = InputState { view_angles: Angles::ZERO, ..Default::default() };
    for _ in 0..(1.0 / TICK) as usize {
        engine.tick(TICK, &idle);
    }

    assert_eq!(engine.player.health, 100.0, "a disabled trigger must not hurt");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- compiled sounds ------------------------------------------------------

/// A `.keroaud` holding a short tone, written by the same code Timbre uses.
fn compiled_bytes(frames: usize) -> Vec<u8> {
    let samples = (0..frames)
        .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / 44100.0).sin())
        .collect();
    let sound = kerosene_audio::wav::Sound { channels: 1, sample_rate: 44100, samples };
    kerosene_audio::compiled::encode(
        &sound,
        kerosene_audio::compiled::Encoding::Adpcm,
        kerosene_audio::compiled::Loop::default(),
    )
}

#[test]
fn a_compiled_sound_loads_and_plays() {
    let (mut engine, dir) = engine_with_sound();
    // Replace the source with its compiled form, as a shipped game has.
    std::fs::remove_file(dir.join("sound/test/beep.wav")).unwrap();
    std::fs::write(dir.join("sound/test/beep.keroaud"), compiled_bytes(4800)).unwrap();
    engine.load_map("testmap").unwrap();

    let handle = engine.audio.play(&engine.vfs.clone(), "test/beep", None, 1.0);
    assert!(handle.is_some(), "a .keroaud should load where a .wav would");
    assert!(engine.console.log().all(|l| !l.text.contains("beep")), "and quietly");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_compiled_form_is_preferred_over_the_source() {
    // A checkout has both. Loading the `.wav` would work and would silently
    // undo the build -- different samples, and none of the loop points.
    let (mut engine, dir) = engine_with_sound();
    // A compiled file of a different length, so which one loaded is visible.
    std::fs::write(dir.join("sound/test/beep.keroaud"), compiled_bytes(1000)).unwrap();
    engine.load_map("testmap").unwrap();

    let sound = engine.audio.sound(&engine.vfs.clone(), "test/beep").expect("it should load");
    assert_eq!(sound.frames(), 1000, "the .keroaud should have won over the .wav");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_source_wav_still_plays_when_nothing_has_been_compiled_yet() {
    // Mid-edit, a designer who has just dropped a .wav in should hear it
    // without running a build first to find out whether it is the right one.
    let (mut engine, dir) = engine_with_sound();
    engine.load_map("testmap").unwrap();

    let sound = engine.audio.sound(&engine.vfs.clone(), "test/beep");
    assert!(sound.is_some(), "the source should still play");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_compiled_sound_is_reported_rather_than_silently_skipped() {
    let (mut engine, dir) = engine_with_sound();
    std::fs::write(dir.join("sound/test/beep.keroaud"), b"not a keroaud at all").unwrap();
    engine.load_map("testmap").unwrap();

    assert!(engine.audio.sound(&engine.vfs.clone(), "test/beep").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sound_that_is_there_but_uncompiled_says_so_rather_than_missing() {
    // The report this fixes: "the engine cannot find the audio files, even
    // though the path is correct." It was -- the engine guessed `.wav`, and
    // then reported the path it had guessed as missing.
    let (mut engine, dir) = engine_with_sound();
    std::fs::remove_file(dir.join("sound/test/beep.wav")).unwrap();
    std::fs::write(dir.join("sound/test/beep.flac"), b"fLaC not really").unwrap();
    engine.load_map("testmap").unwrap();

    assert!(engine.audio.sound(&engine.vfs.clone(), "test/beep").is_none());

    // Asserted on the message itself: it reaches the console through the log
    // relay, which a test does not install.
    let candidates = engine.audio.bank.candidates("test/beep");
    let said = kerosene_engine::audio::explain_missing(&engine.vfs, &candidates);
    assert!(said.contains("beep.flac"), "the file that is there should be named: {said}");
    assert!(said.contains("timbre build"), "and what to do about it: {said}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sound_named_with_capitals_is_found_on_a_case_sensitive_filesystem() {
    // The other half of the same report. Every VFS path was folded to lower
    // case, so an asset named `FINALSmusic.flac` was invisible on Linux --
    // and not only to audio: any texture, model or map with a capital in its
    // name had the same problem.
    let (mut engine, dir) = engine_with_sound();
    std::fs::rename(dir.join("sound/test/beep.wav"), dir.join("sound/test/BeepLOUD.wav")).unwrap();
    engine.load_map("testmap").unwrap();

    assert!(
        engine.audio.sound(&engine.vfs.clone(), "test/BeepLOUD").is_some(),
        "a name with capitals must resolve to the file that has them"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_in_the_wrong_case_still_finds_the_file() {
    // Content paths are case-insensitive by contract, because an archive
    // makes them so for free. A loose tree has to agree, or a game works from
    // a checkout and breaks the moment it is packed.
    let (mut engine, dir) = engine_with_sound();
    std::fs::rename(dir.join("sound/test/beep.wav"), dir.join("sound/test/BeepLOUD.wav")).unwrap();
    engine.load_map("testmap").unwrap();

    assert!(engine.audio.sound(&engine.vfs.clone(), "test/beeploud").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
