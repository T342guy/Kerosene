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
