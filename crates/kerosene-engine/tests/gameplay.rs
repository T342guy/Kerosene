// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The gameplay classes in a real level: breaking glass, walls that come and
//! go, hazards, pickups, teleports, and a trigger a crate sets off.
//!
//! Each builds a small room, compiles it with Cleave and plays it in the
//! engine with the stock game, because the half of each class that matters
//! -- the player's box meeting it, a shot finding it -- is the engine's.

mod common;

use cleave::{CompileOptions, compile};
use kerosene_engine::engine::{Engine, EngineConfig};
use kerosene_engine::input::InputState;
use kerosene_entity::{EntityId, InputEvent, Value};
use kerosene_map::{Connection, Entity, Map, Solid};
use kerosene_math::{Aabb, Angles, Vec3};

const TICK: f32 = 1.0 / 64.0;

/// A sealed room 512 x 256 x 128, the player at one end facing +x.
fn room() -> Map {
    let mut map = Map::new();
    let t = 16.0;
    let (len, wide, tall) = (512.0f32, 256.0f32, 128.0f32);
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
    let id = map.next_id();
    let mut spawn = Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(32.0, wide / 2.0, 8.0));
    map.entities.push(spawn);
    map
}

fn brush<'a>(map: &'a mut Map, classname: &str, bounds: Aabb, material: &str) -> &'a mut Entity {
    let id = map.next_id();
    let solid_id = map.next_id();
    let side_ids: Vec<u32> = (0..6).map(|_| map.next_id()).collect();
    let mut solid = Solid::cube(bounds, material);
    solid.id = solid_id;
    for (s, sid) in solid.sides.iter_mut().zip(side_ids) {
        s.id = sid;
    }
    let mut entity = Entity::new(id, classname);
    entity.solids.push(solid);
    map.entities.push(entity);
    map.entities.last_mut().unwrap()
}

fn point<'a>(map: &'a mut Map, classname: &str, origin: Vec3) -> &'a mut Entity {
    let id = map.next_id();
    let mut entity = Entity::new(id, classname);
    entity.set_origin(origin);
    map.entities.push(entity);
    map.entities.last_mut().unwrap()
}

fn play(name: &str, map: &Map) -> (Engine, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kerosene-gameplay-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    let out = compile(map, &CompileOptions::default()).expect("the room compiles");
    assert!(out.leak.is_none(), "the room leaks");
    std::fs::write(dir.join(format!("maps/{name}.kbsp")), out.bsp.to_bytes()).unwrap();
    let mut engine = common::stock(&EngineConfig::default().with_content(dir.clone()));
    engine.load_map(name).expect("the room loads");
    (engine, dir)
}

fn named(engine: &Engine, name: &str) -> EntityId {
    *engine
        .entities
        .find_by_name(name)
        .first()
        .unwrap_or_else(|| panic!("no {name}"))
}

fn fire(engine: &mut Engine, name: &str, input: &str, parameter: &str) {
    let id = named(engine, name);
    let player = engine.player.entity;
    engine.entities.accept_input(
        id,
        &InputEvent {
            name: input.into(),
            parameter: parameter.into(),
            activator: player,
            caller: player,
        },
    );
}

fn run(engine: &mut Engine, seconds: f32, input: &InputState) {
    for _ in 0..(seconds / TICK).ceil() as usize {
        engine.tick(TICK, input);
    }
}

fn still() -> InputState {
    InputState::default()
}

fn walk() -> InputState {
    InputState {
        forward: 1.0,
        view_angles: Angles::ZERO,
        ..Default::default()
    }
}

#[test]
fn shooting_glass_breaks_it_and_fires_on_break() {
    let mut map = room();
    brush(
        &mut map,
        "breakable",
        Aabb::new(Vec3::new(200.0, 0.0, 0.0), Vec3::new(208.0, 256.0, 128.0)),
        "dev/grid",
    )
    .set("targetname", "glass");
    map.entities.last_mut().unwrap().set("health", "30");
    point(&mut map, "math_counter", Vec3::new(8.0, 8.0, 8.0)).set("targetname", "broken");
    map.entities
        .iter_mut()
        .find(|e| e.get("targetname") == Some("glass"))
        .unwrap()
        .connect(Connection::new("OnBreak", "broken", "Add").with_parameter("1"));
    let (mut engine, dir) = play("glass", &map);

    let glass = named(&engine, "glass");
    let hit = engine
        .trace_view((0.0, 0.0), 1024.0)
        .expect("the glass is in front");
    assert_eq!(hit.entity, Some(glass), "the shot finds the glass");

    // 20 is not enough; 20 more is.
    let player = engine.player.entity;
    assert!(engine.damage_entity(glass, 20.0, player));
    run(&mut engine, 0.1, &still());
    assert!(engine.entities.exists(glass));
    assert_eq!(engine.entities.keyvalue_f32(glass, "health", 0.0), 10.0);
    engine.damage_entity(glass, 20.0, player);
    run(&mut engine, 0.1, &still());
    assert!(!engine.entities.exists(glass), "broken");
    let counter = named(&engine, "broken");
    assert_eq!(
        engine.entities.keyvalue_f32(counter, "startvalue", 0.0),
        1.0
    );

    // Gone from the world: the player walks through where it stood.
    run(&mut engine, 3.0, &walk());
    assert!(engine.player.movement.origin.x > 300.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_toggled_wall_stops_the_player_and_then_does_not() {
    let mut map = room();
    brush(
        &mut map,
        "wall_toggle",
        Aabb::new(Vec3::new(200.0, 0.0, 0.0), Vec3::new(216.0, 256.0, 128.0)),
        "dev/grid",
    )
    .set("targetname", "wall");
    let (mut engine, dir) = play("walltoggle", &map);

    run(&mut engine, 3.0, &walk());
    assert!(
        engine.player.movement.origin.x < 200.0,
        "stopped by the wall"
    );
    let model_count = engine.brush_model_poses().len();

    fire(&mut engine, "wall", "Hide", "");
    assert_eq!(
        engine.brush_model_poses().len(),
        model_count - 1,
        "and not drawn"
    );
    run(&mut engine, 3.0, &walk());
    assert!(engine.player.movement.origin.x > 300.0, "gone");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_healthkit_heals_a_hurt_player_and_waits_for_a_healthy_one() {
    let mut map = room();
    point(&mut map, "item_healthkit", Vec3::new(160.0, 128.0, 16.0)).set("targetname", "kit");
    let (mut engine, dir) = play("healthkit", &map);

    // At full health, walking over it leaves it be.
    run(&mut engine, 1.5, &walk());
    assert!(engine.player.movement.origin.x > 160.0);
    assert!(engine.entities.exists(named(&engine, "kit")));

    // Hurt, and back over it.
    engine.hurt_player(40.0, "test");
    let (origin, angles) = (Vec3::new(64.0, 128.0, 8.0), Angles::ZERO);
    engine.teleport_player(origin, Some(angles));
    run(&mut engine, 1.5, &walk());
    assert_eq!(engine.player.health, 85.0, "25 back");
    assert!(engine.entities.find_by_name("kit").is_empty(), "used up");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_pickup_fires_its_output_and_goes() {
    let mut map = room();
    point(&mut map, "item_generic", Vec3::new(160.0, 128.0, 16.0))
        .connect(Connection::new("OnPlayerTouch", "got", "Add").with_parameter("1"));
    point(&mut map, "math_counter", Vec3::new(8.0, 8.0, 8.0)).set("targetname", "got");
    let (mut engine, dir) = play("pickup", &map);
    run(&mut engine, 2.0, &walk());
    let got = named(&engine, "got");
    assert_eq!(engine.entities.keyvalue_f32(got, "startvalue", 0.0), 1.0);
    assert!(engine.entities.find_by_class("item_generic").is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn point_hurt_point_teleport_and_player_speedmod() {
    let mut map = room();
    point(&mut map, "point_hurt", Vec3::new(40.0, 128.0, 40.0)).set("targetname", "hurt");
    let there = Vec3::new(400.0, 128.0, 8.0);
    let tp = point(&mut map, "point_teleport", there);
    tp.set("targetname", "tp");
    tp.set("angles", "0 180 0");
    point(&mut map, "player_speedmod", Vec3::new(8.0, 8.0, 8.0)).set("targetname", "slow");
    let (mut engine, dir) = play("pointents", &map);

    fire(&mut engine, "hurt", "Hurt", "");
    run(&mut engine, 0.1, &still());
    let hurt = engine.player.health;
    assert!(hurt < 100.0 && hurt > 90.0, "close, so most of 10: {hurt}");

    fire(&mut engine, "tp", "Teleport", "");
    run(&mut engine, 0.1, &still());
    let at = engine.player.movement.origin;
    assert_eq!((at.x, at.y), (there.x, there.y));
    // Facing the way it faces: where the host's mouse carries on from. (This
    // test feeds its own fixed input, which a host would not.)
    assert_eq!(engine.input.view_angles.yaw.abs(), 180.0);

    // Far from the hurt now: nothing.
    fire(&mut engine, "hurt", "Hurt", "");
    run(&mut engine, 0.1, &still());
    assert_eq!(engine.player.health, hurt);

    // Half speed, and it stays on the player's entity.
    let start = engine.player.movement.origin.x;
    run(
        &mut engine,
        1.0,
        &InputState {
            forward: -1.0,
            ..walk()
        },
    );
    let full = start - engine.player.movement.origin.x;
    fire(&mut engine, "slow", "ModifySpeed", "0.5");
    let player = engine.player.entity.unwrap();
    assert_eq!(
        engine
            .entities
            .get(player)
            .unwrap()
            .fields
            .get("speed_scale"),
        Some(&Value::Float(0.5))
    );
    let start = engine.player.movement.origin.x;
    run(
        &mut engine,
        1.0,
        &InputState {
            forward: -1.0,
            ..walk()
        },
    );
    let slow = start - engine.player.movement.origin.x;
    assert!(slow < full * 0.7, "{slow} against {full}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn game_end_goes_back_to_no_map() {
    let mut map = room();
    point(&mut map, "game_end", Vec3::new(8.0, 8.0, 8.0)).set("targetname", "end");
    let (mut engine, dir) = play("gameend", &map);
    fire(&mut engine, "end", "EndGame", "");
    run(&mut engine, 0.1, &still());
    assert!(!engine.has_level());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_trigger_for_physics_objects_notices_a_falling_crate() {
    let mut map = room();
    let trigger = brush(
        &mut map,
        "trigger_multiple",
        Aabb::new(Vec3::new(300.0, 96.0, 0.0), Vec3::new(364.0, 160.0, 64.0)),
        "tools/trigger",
    );
    trigger.set("spawnflags", "8");
    trigger.connect(Connection::new("OnStartTouch", "count", "Add").with_parameter("1"));
    point(&mut map, "math_counter", Vec3::new(8.0, 8.0, 8.0)).set("targetname", "count");
    let (mut engine, dir) = play("physobjects", &map);
    // The base content's cube.
    engine.spawn_prop("props/cube", Vec3::new(332.0, 128.0, 100.0));
    run(&mut engine, 2.0, &still());
    let count = named(&engine, "count");
    assert_eq!(
        engine.entities.keyvalue_f32(count, "startvalue", 0.0),
        1.0,
        "the crate set it off"
    );
    let _ = std::fs::remove_dir_all(dir);
}
