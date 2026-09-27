// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
use super::*;
use crate::engine::EngineConfig;
use crate::input::InputState;

/// The base content's room: 768 x 512 x 256, a crate at (320, 128).
fn room() -> Engine {
    let mut engine = Engine::new(&EngineConfig::default());
    engine
        .load_map(crate::base::FALLBACK_MAP)
        .expect("the base room loads");
    // Props get their bodies on the first tick.
    engine.tick(1.0 / 64.0, &InputState::default());
    engine
}

#[test]
fn a_ray_at_a_wall_hits_the_world() {
    let engine = room();
    let hit = engine
        .trace_ray(Vec3::new(96.0, 256.0, 64.0), Vec3::new(2000.0, 256.0, 64.0))
        .expect("the far wall is in the way");
    assert!((hit.pos.x - 768.0).abs() < 1.0, "{hit:?}");
    assert!(hit.normal.x < -0.9, "facing back along the ray: {hit:?}");
    assert_eq!(hit.entity, None);
    assert!((hit.distance - (768.0 - 96.0)).abs() < 1.0);
}

#[test]
fn a_ray_at_a_prop_says_which_entity_it_hit() {
    let engine = room();
    let crate_id = engine.entities.find_by_class("prop_physics")[0];
    let at = engine.entities.get(crate_id).unwrap().origin;
    let hit = engine
        .trace_ray(
            at + Vec3::new(0.0, 0.0, 200.0),
            at - Vec3::new(0.0, 0.0, 200.0),
        )
        .expect("the crate is under the ray");
    assert_eq!(hit.entity, Some(crate_id));
    assert!(hit.normal.z > 0.9, "the top face: {hit:?}");
    assert!(
        hit.pos.z > at.z - 1.0,
        "stopped on the crate, not the floor"
    );
}

#[test]
fn a_clear_line_hits_nothing() {
    let engine = room();
    assert_eq!(
        engine.trace_ray(Vec3::new(96.0, 256.0, 64.0), Vec3::new(160.0, 256.0, 64.0)),
        None
    );
}

#[test]
fn no_map_means_no_hit() {
    let engine = Engine::new(&EngineConfig::default());
    assert_eq!(engine.trace_ray(Vec3::ZERO, Vec3::X * 100.0), None);
}
