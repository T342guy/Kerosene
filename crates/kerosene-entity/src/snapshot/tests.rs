// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
use super::*;
use crate::io::InputEvent;
use crate::registry::{ClassDef, ClassRegistry};
use std::sync::Arc;

fn count(world: &mut EntityWorld, id: EntityId, _: &InputEvent) -> bool {
    if let Some(e) = world.get_mut(id) {
        let n = e.fields.i32("hits", 0) + 1;
        e.fields.set("hits", Value::Int(n));
    }
    world.fire_output(id, "OnTrigger", None, None);
    true
}

fn spawned(world: &mut EntityWorld, id: EntityId) {
    if let Some(e) = world.get_mut(id) {
        e.fields
            .set("spawns", Value::Int(e.fields.i32("spawns", 0) + 1));
    }
}

fn restored(world: &mut EntityWorld, id: EntityId) {
    if let Some(e) = world.get_mut(id) {
        e.fields
            .set("restores", Value::Int(e.fields.i32("restores", 0) + 1));
    }
}

fn world() -> EntityWorld {
    let mut r = ClassRegistry::new();
    r.register(
        ClassDef::new("relay")
            .on_spawn(spawned)
            .on_restore(restored)
            .input("Trigger", count),
    );
    EntityWorld::new(Arc::new(r))
}

fn level() -> EntityWorld {
    let mut w = world();
    let kv = kerosene_kv::KeyValues::parse(
        r#"
        entity { "classname" "relay" "targetname" "a" "origin" "1 2 3" "angles" "0 90 0"
                 "note" "hello" "speed" "2.5"
                 connections { "OnTrigger" "b,Trigger,,0.5,1" } }
        entity { "classname" "relay" "targetname" "b" }
        entity { "classname" "relay" "targetname" "doomed" }
        "#,
    )
    .unwrap();
    w.load_from_kv(&kv).unwrap();
    w
}

fn json_round_trip(s: &WorldSnapshot) -> WorldSnapshot {
    let text = serde_json::to_string(s).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn a_world_comes_back_as_it_was_saved_and_carries_on_the_same() {
    let mut w = level();
    let a = w.find_by_name("a")[0];
    let doomed = w.find_by_name("doomed")[0];
    w.remove(doomed);
    w.run(0.0);
    w.queue_input(Target::Named("a".into()), "Trigger", "", 0.25, None, None);
    w.set_think_delay(a, 3.0);
    w.run(0.3); // a fires its once-only wire to b, due at 0.8

    let snap = json_round_trip(&w.snapshot());
    let mut back = world();
    assert_eq!(back.restore(&snap).unwrap(), 2);

    let e = back.get(a).expect("the same handle names the same entity");
    assert_eq!(e.origin, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(e.angles, Angles::new(0.0, 90.0, 0.0));
    assert_eq!(e.fields.text("note").as_deref(), Some("hello"));
    assert_eq!(e.fields.f32("speed", 0.0), 2.5);
    assert_eq!(e.fields.i32("hits", 0), 1);
    assert_eq!(e.connections[0].times_to_fire, 0, "the wire stays spent");
    assert_eq!(e.fields.i32("spawns", 0), 1, "no second spawn");
    assert_eq!(e.fields.i32("restores", 0), 1);
    assert!(!back.exists(doomed), "a dead handle stays dead");
    assert_eq!(back.find_by_name("b").len(), 1);
    assert_eq!(back.pending_event_count(), 1);

    // Both carry on identically: the queued event lands, and the next
    // entity spawned gets the same handle in each.
    w.run(0.6);
    back.run(0.6);
    let b = w.find_by_name("b")[0];
    assert_eq!(w.get(b).unwrap().fields.i32("hits", 0), 1);
    assert_eq!(back.get(b).unwrap().fields.i32("hits", 0), 1);
    assert_eq!(w.spawn("relay"), back.spawn("relay"));
    assert!((w.time - back.time).abs() < 1e-6);
}

#[test]
fn saving_twice_writes_the_same_text() {
    let w = level();
    let one = serde_json::to_string(&w.snapshot()).unwrap();
    let two = serde_json::to_string(&w.snapshot()).unwrap();
    assert_eq!(one, two);
    assert!(one.contains(r#""note":{"text":"hello"}"#), "{one}");
}

#[test]
fn a_float_json_cannot_spell_is_written_as_zero() {
    let mut w = level();
    let a = w.find_by_name("a")[0];
    w.get_mut(a)
        .unwrap()
        .fields
        .set("broken", Value::Float(f32::NAN));
    let snap = json_round_trip(&w.snapshot());
    let saved = snap.entities.iter().find(|e| e.id[0] == a.index).unwrap();
    assert_eq!(saved.fields["broken"], SavedValue::Float(0.0));
}

#[test]
fn a_damaged_snapshot_is_refused_and_the_world_left_alone() {
    let w = level();
    let mut snap = w.snapshot();
    snap.entities[0].id[1] += 7;
    let mut other = level();
    assert!(other.restore(&snap).unwrap_err().contains("generation"));
    assert_eq!(other.len(), 3, "untouched");

    let mut snap = w.snapshot();
    snap.free.push(0);
    assert!(level().restore(&snap).unwrap_err().contains("free"));
}
