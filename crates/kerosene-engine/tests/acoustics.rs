// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The whole acoustic chain, end to end: a map built in memory, compiled
//! through Cleave, Umbra and Resonance, loaded by the engine, and listened
//! to. Each stage has its own tests; this is the seam between them -- that
//! the room the compiler wrote is the room the engine hears, and that a wall
//! the compiler saw is a wall the mixer muffles.

mod common;

use cleave::{CompileOptions, compile};
use kerosene_audio::{ReverbParams, SoundParams};
use kerosene_bsp::{Bsp, VisBuilder};
use kerosene_engine::acoustics::{self, Reach};
use kerosene_engine::engine::{Engine, EngineConfig};
use kerosene_engine::input::InputState;
use kerosene_map::{Entity, Map, Solid};
use kerosene_math::{Aabb, Angles, Vec3};

const WALL: f32 = 16.0;
const TICK: f32 = 1.0 / 64.0;

fn shell(map: &mut Map, inner: Aabb, materials: [&str; 6]) {
    let (lo, hi, t) = (inner.min, inner.max, WALL);
    let slabs = [
        Aabb::new(
            Vec3::new(lo.x - t, lo.y - t, lo.z - t),
            Vec3::new(hi.x + t, hi.y + t, lo.z),
        ),
        Aabb::new(
            Vec3::new(lo.x - t, lo.y - t, hi.z),
            Vec3::new(hi.x + t, hi.y + t, hi.z + t),
        ),
        Aabb::new(
            Vec3::new(lo.x - t, lo.y - t, lo.z),
            Vec3::new(lo.x, hi.y + t, hi.z),
        ),
        Aabb::new(
            Vec3::new(hi.x, lo.y - t, lo.z),
            Vec3::new(hi.x + t, hi.y + t, hi.z),
        ),
        Aabb::new(Vec3::new(lo.x, lo.y - t, lo.z), Vec3::new(hi.x, lo.y, hi.z)),
        Aabb::new(Vec3::new(lo.x, hi.y, lo.z), Vec3::new(hi.x, hi.y + t, hi.z)),
    ];
    for (slab, material) in slabs.into_iter().zip(materials) {
        map.add_world_solid(Solid::cube(slab, material));
    }
}

const HALL: Aabb = Aabb {
    min: Vec3::new(0.0, 0.0, 0.0),
    max: Vec3::new(768.0, 384.0, 256.0),
};
const CLOSET: Aabb = Aabb {
    min: Vec3::new(784.0, 128.0, 0.0),
    max: Vec3::new(912.0, 256.0, 128.0),
};
/// The doorway between them, in the wall at x = 768..784.
const DOOR_Y: (f32, f32) = (160.0, 224.0);
const DOOR_TOP: f32 = 96.0;
/// A box with no way in at all.
const VAULT: Aabb = Aabb {
    min: Vec3::new(0.0, 512.0, 0.0),
    max: Vec3::new(256.0, 768.0, 128.0),
};

/// A concrete hall and a carpeted closet joined by a doorway, plus a sealed
/// box nothing can reach.
fn hall_and_closet() -> Map {
    let mut map = Map::new();
    shell(
        &mut map,
        HALL,
        [
            "concrete/a",
            "concrete/a",
            "concrete/a",
            "tools/nodraw",
            "concrete/a",
            "concrete/a",
        ],
    );
    shell(
        &mut map,
        CLOSET,
        [
            "carpet/a",
            "carpet/a",
            "tools/nodraw",
            "carpet/a",
            "carpet/a",
            "carpet/a",
        ],
    );
    map.world
        .solids
        .retain(|s| s.sides.iter().all(|side| side.material != "tools/nodraw"));
    let (x0, x1) = (HALL.max.x, CLOSET.min.x);
    for slab in [
        Aabb::new(
            Vec3::new(x0, -WALL, -WALL),
            Vec3::new(x1, DOOR_Y.0, 256.0 + WALL),
        ),
        Aabb::new(
            Vec3::new(x0, DOOR_Y.1, -WALL),
            Vec3::new(x1, 384.0 + WALL, 256.0 + WALL),
        ),
        Aabb::new(
            Vec3::new(x0, DOOR_Y.0, DOOR_TOP),
            Vec3::new(x1, DOOR_Y.1, 256.0 + WALL),
        ),
        Aabb::new(Vec3::new(x0, DOOR_Y.0, -WALL), Vec3::new(x1, DOOR_Y.1, 0.0)),
    ] {
        map.add_world_solid(Solid::cube(slab, "concrete/a"));
    }
    shell(&mut map, VAULT, ["concrete/a"; 6]);

    let id = map.next_id();
    let mut spawn = Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(384.0, 192.0, 8.0));
    map.entities.push(spawn);
    map
}

/// Cleave, Umbra and Resonance, in memory.
fn compiled() -> Bsp {
    let out = compile(&hall_and_closet(), &CompileOptions::default()).expect("should compile");
    assert!(out.leak.is_none(), "the test map leaks");
    let mut bsp = Bsp::from_bytes(&out.bsp.to_bytes(), "test.kerobsp").unwrap();

    let graph = umbra::prt::PortalGraph::parse(&out.prt).unwrap();
    let vis = umbra::flow::compute(&graph, false);
    let mut builder = VisBuilder::new(graph.clusters);
    for from in 0..graph.clusters {
        for to in vis.cluster_vis[from].iter_set() {
            builder.set_visible(from, to);
        }
    }
    builder.derive_pas();
    bsp.visibility = builder.build();

    let absorption = resonance::Absorption::build(&bsp, |name| {
        let family = name.split('/').next().unwrap_or(name);
        Some(kerosene_asset::AcousticProfile::of(
            &kerosene_asset::SurfaceProperty::parse(family),
        ))
    });
    let (acoustics, _) =
        resonance::compute(&bsp, Some(&graph), &absorption, &resonance::Options::FAST);
    bsp.acoustics = Some(acoustics);
    Bsp::from_bytes(&bsp.to_bytes(), "test.kerobsp").expect("acoustics should round-trip")
}

#[test]
fn the_engine_hears_the_room_the_compiler_wrote() {
    let bsp = compiled();
    let hall = acoustics::surroundings(&bsp, HALL.center()).expect("the hall is a room");
    let closet = acoustics::surroundings(&bsp, CLOSET.center()).expect("the closet is a room");
    assert_ne!(hall.room, closet.room);
    assert!(hall.params.enabled && closet.params.enabled);
    assert!(
        hall.params.rt60[2] > closet.params.rt60[2] * 1.5,
        "hall {:?} closet {:?}",
        hall.params.rt60,
        closet.params.rt60
    );
    assert_eq!(
        hall.blending, None,
        "the middle of the hall is nowhere near a door"
    );
    assert_eq!(closet.blending, None);
}

#[test]
fn a_doorway_is_a_slide_between_rooms_not_a_switch() {
    let bsp = compiled();
    let mid_y = (DOOR_Y.0 + DOOR_Y.1) / 2.0;
    // In the doorway itself, a few units from the closet side of it. The
    // doorway belongs to one room or the other; either way the far side of
    // it is the other room, and standing in it should sound like both.
    let in_doorway = Vec3::new(HALL.max.x + 8.0, mid_y, 48.0);
    let hall = acoustics::surroundings(&bsp, HALL.center()).unwrap();
    let closet = acoustics::surroundings(&bsp, CLOSET.center()).unwrap();
    let here = acoustics::surroundings(&bsp, in_doorway).unwrap();
    assert!(
        here.room == hall.room || here.room == closet.room,
        "a doorway is part of one of the rooms it joins, not a room of its own"
    );
    let other = if here.room == hall.room {
        closet.room
    } else {
        hall.room
    };
    assert!(
        here.blending.is_some_and(|(o, t)| o == other && t > 0.3),
        "the doorway should blend towards room {other}: {:?}",
        here.blending
    );
    // Blended figures sit between the two rooms'.
    let (a, b) = (hall.params.rt60[2], closet.params.rt60[2]);
    let m = here.params.rt60[2];
    assert!(
        (m < a && m > b) || (m > a && m < b),
        "blend {m} should be between {a} and {b}"
    );
    // And well away from any door, nothing blends.
    assert_eq!(hall.blending, None);
}

#[test]
fn walls_occlude_and_a_sealed_box_is_unreachable() {
    let bsp = compiled();
    let eye = Vec3::new(384.0, 192.0, 64.0);
    let basis = Angles::ZERO.vectors();

    // Same room, nothing between: clear.
    assert_eq!(
        acoustics::reach(&bsp, eye, &basis, Vec3::new(600.0, 192.0, 64.0)),
        Reach::Clear
    );

    // In the closet, behind the solid part of the wall.
    let behind_wall = Vec3::new(850.0, 140.0, 64.0);
    match acoustics::reach(&bsp, eye, &basis, behind_wall) {
        Reach::Occluded(o) => assert!(o > 0.6, "a wall should mostly block: {o}"),
        other => panic!("a wall should occlude, got {other:?}"),
    }

    // Lined up with the doorway: seen straight through, so at most the
    // side lines are blocked.
    let eye_in_line = Vec3::new(384.0, (DOOR_Y.0 + DOOR_Y.1) / 2.0, 48.0);
    let through_door = Vec3::new(850.0, (DOOR_Y.0 + DOOR_Y.1) / 2.0, 48.0);
    match acoustics::reach(&bsp, eye_in_line, &basis, through_door) {
        Reach::Clear => {}
        Reach::Occluded(o) => assert!(o < 0.7, "a doorway should let most through: {o}"),
        Reach::Unreachable => panic!("a doorway is a way through"),
    }

    // The sealed box: no way sound could get there.
    assert_eq!(
        acoustics::reach(&bsp, eye, &basis, VAULT.center()),
        Reach::Unreachable
    );
}

/// A steady tone, so level is readable straight off the output.
fn tone() -> std::sync::Arc<kerosene_audio::Sound> {
    std::sync::Arc::new(kerosene_audio::Sound {
        channels: 1,
        sample_rate: 48_000,
        samples: vec![0.5; 48_000 * 4],
    })
}

fn engine_with(bsp: &Bsp) -> (Engine, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "kerosene-acoustics-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("maps/acoustics.kerobsp"), bsp.to_bytes()).unwrap();
    let mut engine = common::stock(&EngineConfig::default().with_content(dir.clone()));
    engine
        .load_map("acoustics")
        .expect("the engine should load it");
    (engine, dir)
}

/// Peak level of a block of mixed audio.
fn peak(engine: &Engine) -> f32 {
    engine.audio.with_mixer(|mixer| {
        let mut out = vec![0.0f32; 1024 * 2];
        // Several blocks so the per-voice ramps settle.
        for _ in 0..16 {
            mixer.mix(&mut out);
        }
        out.iter().fold(0.0f32, |a, s| a.max(s.abs()))
    })
}

#[test]
fn the_engine_sets_the_room_and_muffles_what_is_behind_a_wall() {
    let bsp = compiled();
    let (mut engine, dir) = engine_with(&bsp);
    let input = InputState::default();
    engine.tick(TICK, &input);

    // The player spawned in the hall, and the mixer was told so.
    let hall = acoustics::surroundings(&bsp, HALL.center()).unwrap();
    assert_eq!(engine.audio.room, Some(hall.room));
    let told = engine.audio.reverb().expect("a room was set");
    assert!(told.enabled);
    assert_eq!(told.rt60, hall.params.rt60);

    // Switching reverb off dries the mixer out -- and keeps the room's
    // tail out of the level comparison below.
    engine.console.execute("snd_reverb 0");
    engine.tick(TICK, &input);
    assert_eq!(engine.audio.reverb(), Some(ReverbParams::default()));

    // The same tone at the same distance, once in the open and once in the
    // closet behind the wall.
    let eye = engine.player.movement.eye_position();
    let behind_wall = Vec3::new(CLOSET.min.x + 16.0, 140.0, eye.z);
    let distance = behind_wall.distance(eye);
    let in_the_open = eye + Vec3::new(distance * 0.9, distance * 0.436, 0.0);
    assert!(HALL.contains_point(in_the_open), "{in_the_open}");
    assert!((in_the_open.distance(eye) - distance).abs() < 1.0);

    let clear = engine.audio.start(tone(), SoundParams::at(in_the_open));
    engine.tick(TICK, &input);
    let open_level = peak(&engine);
    engine.audio.stop(clear);
    assert_eq!(engine.audio.tracked_count(), 0);

    engine.audio.start(tone(), SoundParams::at(behind_wall));
    engine.tick(TICK, &input);
    assert_eq!(engine.audio.tracked_count(), 1);
    let muffled_level = peak(&engine);
    assert!(
        muffled_level < open_level * 0.5,
        "behind a wall {muffled_level} should be well under the open {open_level}"
    );

    // With occlusion off the wall is forgotten.
    engine.console.execute("snd_occlusion 0");
    engine.tick(TICK, &input);
    let ignored_level = peak(&engine);
    assert!(
        ignored_level > muffled_level * 2.0,
        "snd_occlusion 0 should lift the muffling: {ignored_level} vs {muffled_level}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_forced_preset_wins_over_the_map() {
    let bsp = compiled();
    let (mut engine, dir) = engine_with(&bsp);
    engine.console.execute("snd_reverb_preset cave");
    engine.tick(TICK, &InputState::default());
    assert_eq!(engine.audio.reverb(), ReverbParams::preset("cave"));
    engine.console.execute("snd_reverb_preset \"\"");
    engine.tick(TICK, &InputState::default());
    assert_ne!(engine.audio.reverb(), ReverbParams::preset("cave"));
    let _ = std::fs::remove_dir_all(&dir);
}
