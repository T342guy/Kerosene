use super::*;
use crate::io::Connection;
use crate::registry::ClassDef;

/// Records deliveries on the receiving entity rather than in a global.
///
/// Tests run in parallel, so a shared counter would have them interfering
/// with each other -- which is exactly what happened the first time.
fn counter_hit(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    if let Some(e) = world.get_mut(id) {
        let n = e.fields.i32("hits", 0) + 1;
        e.fields.set("hits", Value::Int(n));
    }
    // Passing the signal on lets chains be tested.
    world.fire_output(id, "OnTrigger", None, None);
    true
}

fn set_value(world: &mut EntityWorld, id: EntityId, e: &InputEvent) -> bool {
    let v = e.parameter_f32().unwrap_or(0.0);
    if let Some(ent) = world.get_mut(id) { ent.fields.set("value", Value::Float(v)); }
    true
}

fn kill(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    world.remove(id);
    true
}

fn tick_think(world: &mut EntityWorld, id: EntityId) {
    if let Some(e) = world.get_mut(id) {
        let n = e.fields.i32("ticks", 0) + 1;
        e.fields.set("ticks", Value::Int(n));
    }
    // Reschedule, so the "stops when not rescheduled" test means something.
    if world.get(id).map(|e| e.fields.i32("ticks", 0)).unwrap_or(0) < 3 {
        world.set_think_delay(id, 0.1);
    }
}

fn registry() -> Arc<ClassRegistry> {
    let mut r = ClassRegistry::new();
    r.register(
        ClassDef::new("logic_relay")
            .input("Trigger", counter_hit)
            .input("SetValue", set_value),
    );
    r.register(ClassDef::new("ticker").on_think(tick_think));
    r.register(ClassDef::new("info_player_start"));
    r.register_common_input("Kill", kill);
    Arc::new(r)
}

fn world() -> EntityWorld { EntityWorld::new(registry()) }

/// Total deliveries across every entity in a world.
fn hits(w: &EntityWorld) -> i32 {
    w.iter().map(|e| e.fields.i32("hits", 0)).sum()
}

#[test]
fn spawning_and_looking_up_by_name() {
    let mut w = world();
    let a = w.spawn("logic_relay");
    w.set_targetname(a, "relay1");
    assert_eq!(w.find_by_name("relay1"), vec![a]);
    // Names in map files are inconsistently cased.
    assert_eq!(w.find_by_name("RELAY1"), vec![a]);
    assert!(w.find_by_name("nothing").is_empty());
}

#[test]
fn several_entities_can_share_a_name() {
    // How a designer opens six doors with one wire.
    let mut w = world();
    let ids: Vec<_> = (0..3).map(|_| w.spawn("logic_relay")).collect();
    for id in &ids { w.set_targetname(*id, "gates"); }
    assert_eq!(w.find_by_name("gates").len(), 3);
}

#[test]
fn an_output_fires_its_input() {
    let mut w = world();
    let source = w.spawn("logic_relay");
    let target = w.spawn("logic_relay");
    w.set_targetname(target, "target");
    w.get_mut(source).unwrap().connections.push(Connection::new("OnUse", "target", "Trigger"));

    assert_eq!(w.fire_output(source, "OnUse", None, None), 1);
    assert_eq!(hits(&w), 0, "nothing is delivered before the queue runs");
    w.run(0.0);
    assert_eq!(hits(&w), 1);
}

#[test]
fn a_delay_holds_the_input_back() {
    let mut w = world();
    let source = w.spawn("logic_relay");
    let target = w.spawn("logic_relay");
    w.set_targetname(target, "target");
    w.get_mut(source)
        .unwrap()
        .connections
        .push(Connection::new("OnUse", "target", "Trigger").with_delay(0.5));

    w.fire_output(source, "OnUse", None, None);
    w.run(0.2);
    assert_eq!(hits(&w), 0, "not yet");
    w.run(0.2);
    assert_eq!(hits(&w), 0, "still not yet");
    w.run(0.2);
    assert_eq!(hits(&w), 1, "0.6 seconds have passed");
}

#[test]
fn an_only_once_output_fires_once() {
    let mut w = world();
    let source = w.spawn("logic_relay");
    let target = w.spawn("logic_relay");
    w.set_targetname(target, "target");
    w.get_mut(source)
        .unwrap()
        .connections
        .push(Connection::new("OnUse", "target", "Trigger").once());

    for _ in 0..5 {
        w.fire_output(source, "OnUse", None, None);
        w.run(0.1);
    }
    assert_eq!(hits(&w), 1);
}

#[test]
fn an_only_once_output_cannot_double_fire_while_in_flight() {
    // The count is spent when the output fires, not when the input lands, so
    // firing twice inside the delay window still only delivers once.
    let mut w = world();
    let source = w.spawn("logic_relay");
    let target = w.spawn("logic_relay");
    w.set_targetname(target, "target");
    w.get_mut(source)
        .unwrap()
        .connections
        .push(Connection::new("OnUse", "target", "Trigger").once().with_delay(1.0));

    w.fire_output(source, "OnUse", None, None);
    w.fire_output(source, "OnUse", None, None);
    w.run(2.0);
    assert_eq!(hits(&w), 1);
}

#[test]
fn parameters_reach_the_input() {
    let mut w = world();
    let source = w.spawn("logic_relay");
    let target = w.spawn("logic_relay");
    w.set_targetname(target, "target");
    w.get_mut(source)
        .unwrap()
        .connections
        .push(Connection::new("OnUse", "target", "SetValue").with_parameter("42"));

    w.fire_output(source, "OnUse", None, None);
    w.run(0.0);
    assert_eq!(w.get(target).unwrap().fields.f32("value", 0.0), 42.0);
}

#[test]
fn a_firing_entity_can_override_the_parameter() {
    let mut w = world();
    let source = w.spawn("logic_relay");
    let target = w.spawn("logic_relay");
    w.set_targetname(target, "target");
    w.get_mut(source)
        .unwrap()
        .connections
        .push(Connection::new("OnUse", "target", "SetValue").with_parameter("1"));

    w.fire_output(source, "OnUse", None, Some("99"));
    w.run(0.0);
    assert_eq!(w.get(target).unwrap().fields.f32("value", 0.0), 99.0);
}

#[test]
fn activator_and_caller_are_addressable() {
    let mut w = world();
    let player = w.spawn("info_player_start");
    let trigger = w.spawn("logic_relay");
    let sink = w.spawn("logic_relay");
    w.set_targetname(sink, "sink");

    w.get_mut(trigger).unwrap().connections.push(Connection::new("OnTouch", "!activator", "Kill"));
    w.fire_output(trigger, "OnTouch", Some(player), None);
    w.run(0.0);
    assert!(!w.exists(player), "!activator should have resolved to the player");
    assert!(w.exists(sink));
}

#[test]
fn a_chain_of_relays_propagates() {
    // a -> b -> c, which is most of what logic_relay is for.
    let mut w = world();
    let a = w.spawn("logic_relay");
    let b = w.spawn("logic_relay");
    let c = w.spawn("logic_relay");
    w.set_targetname(b, "b");
    w.set_targetname(c, "c");
    w.get_mut(a).unwrap().connections.push(Connection::new("Go", "b", "Trigger"));
    w.get_mut(b).unwrap().connections.push(Connection::new("OnTrigger", "c", "Trigger"));

    w.fire_output(a, "Go", None, None);
    for _ in 0..4 { w.run(0.0); }
    assert_eq!(hits(&w), 2, "both b and c should have been triggered");
}

#[test]
fn a_wiring_loop_is_broken_rather_than_hanging() {
    // Two relays firing each other with no delay. Source guards this the same
    // way, because it is an easy mistake to make in the editor.
    let mut w = world();
    let a = w.spawn("logic_relay");
    let b = w.spawn("logic_relay");
    w.set_targetname(a, "a");
    w.set_targetname(b, "b");
    w.get_mut(a).unwrap().connections.push(Connection::new("OnTrigger", "b", "Trigger"));
    w.get_mut(b).unwrap().connections.push(Connection::new("OnTrigger", "a", "Trigger"));

    w.fire_output(a, "OnTrigger", None, None);
    // Must return rather than spin.
    let delivered = w.run(0.0);
    assert!(delivered <= MAX_EVENTS_PER_TICK);
    assert_eq!(w.pending_event_count(), 0, "the queue should have been cleared");
}

#[test]
fn a_stale_handle_does_not_address_a_recycled_slot() {
    // The bug generations exist to prevent: a queued event naming a dead
    // entity must not land on whoever takes its place.
    let mut w = world();
    let old = w.spawn("logic_relay");
    w.remove(old);
    w.run(0.0);
    assert!(!w.exists(old));

    let new = w.spawn("logic_relay");
    assert_eq!(new.index, old.index, "the slot should have been reused");
    assert_ne!(new.generation, old.generation);
    assert!(w.get(old).is_none(), "the stale handle must not resolve");
    assert!(w.get(new).is_some());
}

#[test]
fn an_event_aimed_at_a_dead_entity_is_dropped() {
    let mut w = world();
    let source = w.spawn("logic_relay");
    let target = w.spawn("logic_relay");
    w.set_targetname(target, "target");
    w.get_mut(source)
        .unwrap()
        .connections
        .push(Connection::new("OnUse", "target", "Trigger").with_delay(1.0));

    w.fire_output(source, "OnUse", None, None);
    w.remove(target);
    w.run(2.0);
    assert_eq!(hits(&w), 0, "the target was gone before the event landed");
}

#[test]
fn removal_is_deferred_until_the_end_of_the_tick() {
    // A handler that removes its own entity must not have the ground pulled
    // out from under the rest of the dispatch.
    let mut w = world();
    let a = w.spawn("logic_relay");
    w.set_targetname(a, "a");
    w.get_mut(a).unwrap().connections.push(Connection::new("OnTrigger", "a", "Kill"));
    w.fire_output(a, "OnTrigger", None, None);
    w.run(0.0);
    assert!(!w.exists(a));
}

#[test]
fn thinks_run_when_scheduled_and_stop_when_not_rescheduled() {
    let mut w = world();
    let t = w.spawn("ticker");
    w.set_think_delay(t, 0.1);

    for _ in 0..20 { w.run(0.05); }
    // The handler reschedules until it has ticked three times.
    assert_eq!(w.get(t).unwrap().fields.i32("ticks", 0), 3);
    assert!(w.get(t).unwrap().next_think.is_none());
}

#[test]
fn clearing_a_think_stops_it() {
    let mut w = world();
    let t = w.spawn("ticker");
    w.set_think_delay(t, 0.1);
    w.clear_think(t);
    for _ in 0..10 { w.run(0.05); }
    assert_eq!(w.get(t).unwrap().fields.i32("ticks", 0), 0);
}

#[test]
fn simultaneous_events_are_delivered_in_the_order_they_were_queued() {
    let mut w = world();
    let source = w.spawn("logic_relay");
    let first = w.spawn("logic_relay");
    let second = w.spawn("logic_relay");
    w.set_targetname(first, "first");
    w.set_targetname(second, "second");
    {
        let e = w.get_mut(source).unwrap();
        e.connections.push(Connection::new("Go", "first", "SetValue").with_parameter("1"));
        e.connections.push(Connection::new("Go", "second", "SetValue").with_parameter("2"));
    }
    w.fire_output(source, "Go", None, None);
    w.run(0.0);
    assert_eq!(w.get(first).unwrap().fields.f32("value", 0.0), 1.0);
    assert_eq!(w.get(second).unwrap().fields.f32("value", 0.0), 2.0);
}

#[test]
fn a_map_entity_lump_loads() {
    let src = r#"
entity { "classname" "worldspawn" "model" "*0" }
entity
{
    "classname" "logic_relay"
    "targetname" "gate_relay"
    "origin" "0 64 128"
    "angles" "0 90 0"
    "spawnflags" "3"
    connections { "OnTrigger" "gate,Open,,0.25,-1" }
}
entity { "classname" "func_door" "targetname" "gate" "model" "*1" }
"#;
    let mut w = world();
    let kv = KeyValues::parse(src).unwrap();
    let count = w.load_from_kv(&kv).unwrap();
    assert_eq!(count, 3);

    let relay = w.find_by_name("gate_relay")[0];
    let e = w.get(relay).unwrap();
    assert_eq!(e.origin, Vec3::new(0.0, 64.0, 128.0));
    assert_eq!(e.angles.yaw, 90.0);
    assert!(e.has_spawnflag(1) && e.has_spawnflag(2));
    assert_eq!(e.connections.len(), 1);
    assert_eq!(e.connections[0].delay, 0.25);

    let door = w.find_by_name("gate")[0];
    assert_eq!(w.get(door).unwrap().brush_model, Some(1));
    assert_eq!(w.get(w.first_of_class("worldspawn").unwrap()).unwrap().brush_model, Some(0));
}

#[test]
fn an_unregistered_class_loads_inert_rather_than_failing() {
    // A map may reference entities a given game does not implement. It should
    // still load and play, minus that entity's behaviour.
    let kv = KeyValues::parse(r#"entity { "classname" "prop_from_another_mod" "origin" "0 0 0" }"#).unwrap();
    let mut w = world();
    assert_eq!(w.load_from_kv(&kv).unwrap(), 1);
    assert_eq!(w.len(), 1);
}

#[test]
fn the_io_trace_records_what_fired_what() {
    let mut w = world();
    w.set_trace(true);
    let source = w.spawn("logic_relay");
    let target = w.spawn("logic_relay");
    w.set_targetname(target, "target");
    w.get_mut(source)
        .unwrap()
        .connections
        .push(Connection::new("OnUse", "target", "Trigger").with_delay(0.5));

    w.fire_output(source, "OnUse", None, None);
    let trace = w.trace_lines();
    assert_eq!(trace.len(), 1);
    assert!(trace[0].contains("OnUse"), "{}", trace[0]);
    assert!(trace[0].contains("Trigger"), "{}", trace[0]);
    assert!(trace[0].contains("+0.50s"), "{}", trace[0]);
}

#[test]
fn firing_an_output_nothing_is_wired_to_is_harmless() {
    let mut w = world();
    let a = w.spawn("logic_relay");
    assert_eq!(w.fire_output(a, "OnNothing", None, None), 0);
    w.run(0.0);
}

#[test]
fn an_unhandled_input_is_reported_rather_than_panicking() {
    let mut w = world();
    let a = w.spawn("logic_relay");
    assert!(!w.accept_input(a, &InputEvent::new("NoSuchInput")));
}
