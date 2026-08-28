// SPDX-License-Identifier: LGPL-3.0-or-later
//! Logic entities: the glue a designer wires everything else together with.

use crate::set_field;
use void_entity::io::InputEvent;
use void_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value};

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("logic_relay")
            .on_spawn(spawn_relay)
            .input("Trigger", input_relay_trigger)
            .input("Enable", |w, id, _| { set_field(w, id, "disabled", Value::Bool(false)); true })
            .input("Disable", |w, id, _| { set_field(w, id, "disabled", Value::Bool(true)); true })
            .input("Toggle", |w, id, _| {
                let off = w.get(id).map(|e| e.fields.bool("disabled", false)).unwrap_or(false);
                set_field(w, id, "disabled", Value::Bool(!off));
                true
            })
            .output("OnTrigger"),
    );

    // Fires once when the map starts. How a level does anything at all before
    // the player touches something.
    registry.register(
        ClassDef::new("logic_auto").on_spawn(spawn_auto).on_think(think_auto).output("OnMapSpawn"),
    );

    registry.register(
        ClassDef::new("math_counter")
            .on_spawn(spawn_counter)
            .input("Add", |w, id, e| adjust(w, id, e.parameter_f32().unwrap_or(1.0)))
            .input("Subtract", |w, id, e| adjust(w, id, -e.parameter_f32().unwrap_or(1.0)))
            .input("SetValue", input_set_value)
            .input("GetValue", input_get_value)
            .output("OutValue")
            .output("OnHitMax")
            .output("OnHitMin"),
    );

    registry.register(
        ClassDef::new("point_message")
            .input("Show", input_show_message)
            .input("Display", input_show_message)
            .output("OnShowMessage"),
    );

    // The alternative. Everything else here fires one list of outputs; this
    // is the only class that answers "and if not?" -- without it a map can
    // say "when X, do Y" and has no way at all to say "otherwise do Z", which
    // is a hole you feel the moment you try to build a locked door.
    registry.register(
        ClassDef::new("logic_branch")
            .on_spawn(spawn_branch)
            .input("SetValue", |w, id, e| { set_branch(w, id, truth(e), false); true })
            .input("SetValueTest", |w, id, e| { set_branch(w, id, truth(e), true); true })
            .input("Toggle", |w, id, _| { toggle_branch(w, id, false); true })
            .input("ToggleTest", |w, id, _| { toggle_branch(w, id, true); true })
            .input("Test", |w, id, _| { test_branch(w, id); true })
            .output("OnTrue")
            .output("OnFalse"),
    );

    registry.register(
        ClassDef::new("logic_timer")
            .on_spawn(spawn_timer)
            .on_think(think_timer)
            .input("Enable", |w, id, _| { set_field(w, id, "disabled", Value::Bool(false)); w.set_think_delay(id, 0.0); true })
            .input("Disable", |w, id, _| { set_field(w, id, "disabled", Value::Bool(true)); w.clear_think(id); true })
            .input("Toggle", |w, id, _| {
                let was_off = w.get(id).map(|e| e.fields.bool("disabled", false)).unwrap_or(false);
                set_field(w, id, "disabled", Value::Bool(!was_off));
                if was_off { w.set_think_delay(id, 0.0) } else { w.clear_think(id) }
                true
            })
            .output("OnTimer"),
    );
}

fn spawn_branch(world: &mut EntityWorld, id: EntityId) {
    let initial = world
        .get(id)
        .map(|e| e.fields.bool("initialvalue", false))
        .unwrap_or(false);
    set_field(world, id, "value", Value::Bool(initial));
}

/// What an input carries as a truth value.
///
/// A parameter if it has one, so `SetValue` can be wired from something that
/// computes a number; otherwise true, because firing `SetValue` with nothing
/// attached reads as "make it so".
fn truth(event: &InputEvent) -> bool {
    if let Some(v) = event.parameter_f32() { return v != 0.0 }
    let p = event.parameter.trim();
    !(p.eq_ignore_ascii_case("false") || p == "0")
}

fn set_branch(world: &mut EntityWorld, id: EntityId, value: bool, then_test: bool) {
    set_field(world, id, "value", Value::Bool(value));
    if then_test { test_branch(world, id) }
}

fn toggle_branch(world: &mut EntityWorld, id: EntityId, then_test: bool) {
    let now = world.get(id).map(|e| e.fields.bool("value", false)).unwrap_or(false);
    set_branch(world, id, !now, then_test);
}

/// Fire one side or the other. Never both, which is the whole point.
fn test_branch(world: &mut EntityWorld, id: EntityId) {
    let value = world.get(id).map(|e| e.fields.bool("value", false)).unwrap_or(false);
    let output = if value { "OnTrue" } else { "OnFalse" };
    world.fire_output(id, output, None, None);
}

fn spawn_relay(world: &mut EntityWorld, id: EntityId) {
    let disabled = world.get(id).map(|e| e.fields.bool("startdisabled", false)).unwrap_or(false);
    set_field(world, id, "disabled", Value::Bool(disabled));
}

fn input_relay_trigger(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    if world.get(id).map(|e| e.fields.bool("disabled", false)).unwrap_or(false) {
        return true;
    }
    // The activator is passed along, so `!activator` still resolves to the
    // player several relays down a chain.
    world.fire_output(id, "OnTrigger", event.activator, None);

    // Spawnflag 1 is "remove on fire", matching Source.
    if world.get(id).is_some_and(|e| e.has_spawnflag(1)) {
        world.remove(id);
    }
    true
}

fn spawn_auto(world: &mut EntityWorld, id: EntityId) {
    // Deferred by a tick rather than fired during spawn, so that every other
    // entity in the map exists by the time it goes off.
    world.set_think_delay(id, 0.0);
}

fn think_auto(world: &mut EntityWorld, id: EntityId) {
    world.fire_output(id, "OnMapSpawn", None, None);
    world.remove(id);
}

fn spawn_counter(world: &mut EntityWorld, id: EntityId) {
    let start = world.get(id).map(|e| e.fields.f32("startvalue", 0.0)).unwrap_or(0.0);
    set_field(world, id, "value", Value::Float(start));
}

fn adjust(world: &mut EntityWorld, id: EntityId, delta: f32) -> bool {
    let current = world.get(id).map(|e| e.fields.f32("value", 0.0)).unwrap_or(0.0);
    set_value(world, id, current + delta);
    true
}

fn input_set_value(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let Some(v) = event.parameter_f32() else { return false };
    set_value(world, id, v);
    true
}

fn input_get_value(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    let value = world.get(id).map(|e| e.fields.f32("value", 0.0)).unwrap_or(0.0);
    world.fire_output(id, "OutValue", None, Some(&value.to_string()));
    true
}

/// Set a counter, clamping to its limits and firing when it reaches one.
fn set_value(world: &mut EntityWorld, id: EntityId, raw: f32) {
    let Some(entity) = world.get(id) else { return };
    let min = entity.fields.f32("min", f32::NEG_INFINITY);
    let max = entity.fields.f32("max", f32::INFINITY);
    let previous = entity.fields.f32("value", 0.0);

    let value = raw.clamp(min, max);
    set_field(world, id, "value", Value::Float(value));
    world.fire_output(id, "OutValue", None, Some(&value.to_string()));

    // Fire on the transition only, so holding at the limit does not fire
    // every time something adds to it.
    if value >= max && previous < max && max.is_finite() {
        world.fire_output(id, "OnHitMax", None, None);
    }
    if value <= min && previous > min && min.is_finite() {
        world.fire_output(id, "OnHitMin", None, None);
    }
}

fn input_show_message(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    let text = world
        .get(id)
        .and_then(|e| e.fields.text("message").map(str::to_string))
        .unwrap_or_default();
    if !text.is_empty() { log::info!("{text}"); }
    world.fire_output(id, "OnShowMessage", None, None);
    true
}

fn spawn_timer(world: &mut EntityWorld, id: EntityId) {
    let disabled = world.get(id).map(|e| e.fields.bool("startdisabled", false)).unwrap_or(false);
    set_field(world, id, "disabled", Value::Bool(disabled));
    if !disabled {
        let interval = world.get(id).map(|e| e.fields.f32("refiretime", 1.0)).unwrap_or(1.0);
        world.set_think_delay(id, interval.max(0.01));
    }
}

fn think_timer(world: &mut EntityWorld, id: EntityId) {
    if world.get(id).map(|e| e.fields.bool("disabled", false)).unwrap_or(false) { return; }
    world.fire_output(id, "OnTimer", None, None);
    let interval = world.get(id).map(|e| e.fields.f32("refiretime", 1.0)).unwrap_or(1.0);
    world.set_think_delay(id, interval.max(0.01));
}
