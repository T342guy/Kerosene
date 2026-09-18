// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Streamed sections, end to end: a map with a far room in a streamed
//! visgroup is compiled with vis, loaded, and the section comes and goes as
//! the player approaches and leaves.

mod common;

use cleave::{CompileOptions, compile};
use kerosene_bsp::{Bsp, VisBuilder};
use kerosene_engine::engine::{Engine, EngineConfig};
use kerosene_engine::input::InputState;
use kerosene_engine::streaming::SectionState;
use kerosene_map::{Entity, Map, Solid};
use kerosene_math::{Aabb, Vec3};

const TICK: f32 = 1.0 / 64.0;

/// A long hall cut into four rooms by walls with offset doorways, so the far
/// end is out of the PAS from the start. The crates in the far room are in a
/// streamed visgroup.
fn hall() -> Map {
    let mut map = Map::new();
    let t = 16.0;
    let (wide, tall, len) = (256.0f32, 192.0f32, 1536.0f32);
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
    // Three walls with offset doorways: the PAS reaches one doorway past
    // what can be seen, so the far end has to be two doorways past that.
    for (x, door_lo, door_hi) in [
        (384.0, 0.0, 64.0),
        (768.0, 192.0, 256.0),
        (1152.0, 0.0, 64.0),
    ] {
        for slab in [
            Aabb::new(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 16.0, door_lo, tall)),
            Aabb::new(Vec3::new(x, door_hi, 0.0), Vec3::new(x + 16.0, wide, tall)),
            Aabb::new(
                Vec3::new(x, door_lo, 96.0),
                Vec3::new(x + 16.0, door_hi, tall),
            ),
        ] {
            if slab.size().min_element() > 0.0 {
                map.add_world_solid(Solid::cube(slab, "dev/grid"));
            }
        }
    }
    let far = map.add_visgroup("Far", None);
    map.visgroup_mut(far).unwrap().stream = true;
    for slab in [
        Aabb::new(Vec3::new(1200.0, 96.0, 0.0), Vec3::new(1264.0, 160.0, 64.0)),
        Aabb::new(Vec3::new(1400.0, 32.0, 0.0), Vec3::new(1464.0, 96.0, 48.0)),
    ] {
        let mut crate_ = Solid::cube(slab, "dev/grid");
        crate_.editor.add_to_visgroup(far);
        map.add_world_solid(crate_);
    }
    let id = map.next_id();
    let mut spawn = Entity::new(id, "info_player_start");
    spawn.set_origin(Vec3::new(64.0, 128.0, 8.0));
    map.entities.push(spawn);
    map
}

fn compiled() -> Bsp {
    let out = compile(&hall(), &CompileOptions::default()).expect("should compile");
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
    Bsp::from_bytes(&bsp.to_bytes(), "test.kerobsp").expect("round-trips")
}

fn engine_on(bsp: &Bsp) -> Engine {
    let dir = std::env::temp_dir().join(format!("kerosene-stream-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("maps/hall.kerobsp"), bsp.to_bytes()).unwrap();
    let mut engine = common::stock(&EngineConfig {
        content_paths: vec![dir],
        ..Default::default()
    });
    engine.load_map("hall").expect("loads");
    engine
}

fn idle(engine: &mut Engine, seconds: f32) {
    let input = InputState::default();
    for _ in 0..(seconds / TICK) as usize {
        engine.tick(TICK, &input);
    }
}

#[test]
fn the_far_section_loads_as_the_player_approaches_and_unloads_after_they_leave() {
    let bsp = compiled();
    assert_eq!(bsp.section_count(), 2, "the far crates are a section");
    let mut engine = engine_on(&bsp);
    engine.console.set("sv_stream_linger", "0.5");

    idle(&mut engine, 0.5);
    let far = 1;
    assert_eq!(
        engine.streaming().unwrap().state(far),
        SectionState::Unloaded,
        "three doorways away, nothing of the far room is wanted"
    );
    let hulls_before = engine.physics.static_body_count();

    // Teleport into the far room; the next tick wants it.
    engine.player.movement.origin = Vec3::new(1300.0, 128.0, 8.0);
    idle(&mut engine, 0.1);
    assert_eq!(engine.streaming().unwrap().state(far), SectionState::Wanted);
    assert_eq!(
        engine.physics.static_body_count(),
        hulls_before + 2,
        "the crates' hulls arrive with the section"
    );

    // The host reports the upload; the section is loaded.
    engine.section_loaded(far);
    assert!(engine.streaming().unwrap().is_loaded(far));

    // Back to the start: it lingers, then goes, hulls and all.
    engine.player.movement.origin = Vec3::new(64.0, 128.0, 8.0);
    idle(&mut engine, 0.2);
    assert!(
        engine.streaming().unwrap().is_loaded(far),
        "still lingering"
    );
    idle(&mut engine, 1.0);
    assert_eq!(
        engine.streaming().unwrap().state(far),
        SectionState::Unloaded
    );
    assert_eq!(engine.physics.static_body_count(), hulls_before);
}

#[test]
fn streaming_off_wants_every_section() {
    let bsp = compiled();
    let mut engine = engine_on(&bsp);
    engine.console.set("sv_stream", "0");
    idle(&mut engine, 0.1);
    assert_eq!(engine.streaming().unwrap().state(1), SectionState::Wanted);
}
