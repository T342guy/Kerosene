// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Logic entities: the glue a designer wires everything else together with.

use crate::components::{Switchable, is_disabled, set_disabled, toggle_disabled};
use kerosene_ecs::prelude::*;
use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, host_requests};

/// A `math_counter`: its value and the limits it is held between.
#[derive(Component, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct Counter {
    /// What it holds. Starts as the map's `startvalue`.
    #[reflect(@Key("startvalue"), @Label("Starting value"))]
    pub value: f32,
    /// Its floor, if it has one.
    #[reflect(@Key("min"), @Label("Minimum"), @Help("Left blank, there is no minimum."))]
    pub min: Option<f32>,
    /// Its ceiling, if it has one.
    #[reflect(@Key("max"), @Label("Maximum"), @Help("Left blank, there is no maximum."))]
    pub max: Option<f32>,
}

/// A `logic_branch`: the yes or no it remembers.
#[derive(Component, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct Branch {
    /// What it holds. Starts as the map's `initialvalue`.
    #[reflect(
        @Key("initialvalue"),
        @Label("Starts true"),
        @Help("What it remembers before anything sets it.")
    )]
    pub value: bool,
}

/// A `logic_timer`'s interval.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Timer {
    /// Seconds between firings.
    #[reflect(@Key("refiretime"), @Label("Interval"), @Help("Seconds between firings."))]
    pub refiretime: f32,
}

impl Default for Timer {
    fn default() -> Self {
        Timer { refiretime: 1.0 }
    }
}

/// A `point_message`'s text.
#[derive(Component, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct Message {
    /// What it prints.
    #[reflect(@Key("message"), @Label("Text"))]
    pub message: String,
}

/// A `logic_autosave`'s save name.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Autosave {
    /// The name the save is written under.
    #[reflect(
        @Key("savename"),
        @Label("Save name"),
        @Help("The name the save is written under. Letters, digits, - and _.")
    )]
    pub savename: String,
}

impl Default for Autosave {
    fn default() -> Self {
        Autosave {
            savename: "auto".into(),
        }
    }
}

/// Register the logic classes: `logic_relay`, `logic_auto`,
/// `logic_autosave`, `math_counter`, `point_message`, `logic_branch` and
/// `logic_timer`.
pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("logic_relay")
            .component::<Switchable>()
            .input("Trigger", input_relay_trigger)
            .input("Enable", |w, id, _| set_disabled(w, id, false))
            .input("Disable", |w, id, _| set_disabled(w, id, true))
            .input("Toggle", |w, id, _| toggle_disabled(w, id))
            .output("OnTrigger"),
    );

    // Fires once when the map starts. How a level does anything at all before
    // the player touches something.
    registry.register(
        ClassDef::new("logic_auto")
            .on_spawn(spawn_auto)
            .on_think(think_auto)
            .output("OnMapSpawn"),
    );

    // A checkpoint: saves the game when told to.
    registry.register(
        ClassDef::new("logic_autosave")
            .component::<Autosave>()
            .input("Save", input_autosave),
    );

    registry.register(
        ClassDef::new("math_counter")
            .component::<Counter>()
            .input("Add", |w, id, e| {
                adjust(w, id, e.parameter_f32().unwrap_or(1.0))
            })
            .input("Subtract", |w, id, e| {
                adjust(w, id, -e.parameter_f32().unwrap_or(1.0))
            })
            .input("SetValue", input_set_value)
            .input("GetValue", input_get_value)
            .output("OutValue")
            .output("OnHitMax")
            .output("OnHitMin"),
    );

    registry.register(
        ClassDef::new("point_message")
            .component::<Message>()
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
            .component::<Branch>()
            .input("SetValue", |w, id, e| {
                set_branch(w, id, truth(e), false);
                true
            })
            .input("SetValueTest", |w, id, e| {
                set_branch(w, id, truth(e), true);
                true
            })
            .input("Toggle", |w, id, _| {
                toggle_branch(w, id, false);
                true
            })
            .input("ToggleTest", |w, id, _| {
                toggle_branch(w, id, true);
                true
            })
            .input("Test", |w, id, _| {
                test_branch(w, id);
                true
            })
            .output("OnTrue")
            .output("OnFalse"),
    );

    registry.register(
        ClassDef::new("logic_timer")
            .component::<Switchable>()
            .component::<Timer>()
            .on_spawn(spawn_timer)
            .on_think(think_timer)
            .input("Enable", |w, id, _| {
                set_disabled(w, id, false);
                w.set_think_delay(id, 0.0);
                true
            })
            .input("Disable", |w, id, _| {
                set_disabled(w, id, true);
                w.clear_think(id);
                true
            })
            .input("Toggle", |w, id, _| {
                let was_off = is_disabled(w, id);
                toggle_disabled(w, id);
                if was_off {
                    w.set_think_delay(id, 0.0)
                } else {
                    w.clear_think(id)
                }
                true
            })
            .output("OnTimer"),
    );
}

/// What an input carries as a truth value.
///
/// A parameter if it has one, so `SetValue` can be wired from something that
/// computes a number; otherwise true, because firing `SetValue` with nothing
/// attached reads as "make it so".
fn truth(event: &InputEvent) -> bool {
    if let Some(v) = event.parameter_f32() {
        return v != 0.0;
    }
    let p = event.parameter.trim();
    !(p.eq_ignore_ascii_case("false") || p == "0")
}

fn set_branch(world: &mut EntityWorld, id: EntityId, value: bool, then_test: bool) {
    if let Some(b) = world.component_mut::<Branch>(id) {
        b.value = value;
    }
    if then_test {
        test_branch(world, id)
    }
}

fn toggle_branch(world: &mut EntityWorld, id: EntityId, then_test: bool) {
    let now = world.component::<Branch>(id).is_some_and(|b| b.value);
    set_branch(world, id, !now, then_test);
}

/// Fire one side or the other. Never both, which is the whole point.
fn test_branch(world: &mut EntityWorld, id: EntityId) {
    let value = world.component::<Branch>(id).is_some_and(|b| b.value);
    let output = if value { "OnTrue" } else { "OnFalse" };
    world.fire_output(id, output, None, None);
}

fn input_relay_trigger(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    if is_disabled(world, id) {
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

fn adjust(world: &mut EntityWorld, id: EntityId, delta: f32) -> bool {
    let current = world.component::<Counter>(id).map_or(0.0, |c| c.value);
    set_value(world, id, current + delta);
    true
}

fn input_set_value(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let Some(v) = event.parameter_f32() else {
        return false;
    };
    set_value(world, id, v);
    true
}

fn input_get_value(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    let value = world.component::<Counter>(id).map_or(0.0, |c| c.value);
    world.fire_output(id, "OutValue", None, Some(&value.to_string()));
    true
}

/// A counter's limits, made usable: a limit that is not set, or not a
/// number, is no limit, and a pair typed the wrong way round is swapped.
fn limits(min: Option<f32>, max: Option<f32>) -> (f32, f32) {
    let min = min.filter(|m| !m.is_nan()).unwrap_or(f32::NEG_INFINITY);
    let max = max.filter(|m| !m.is_nan()).unwrap_or(f32::INFINITY);
    if min > max { (max, min) } else { (min, max) }
}

/// Set a counter, clamping to its limits and firing when it reaches one.
fn set_value(world: &mut EntityWorld, id: EntityId, raw: f32) {
    let Some(counter) = world.component_mut::<Counter>(id) else {
        return;
    };
    let (min, max) = limits(counter.min, counter.max);
    let previous = counter.value;

    // Not `clamp`: that panics on limits a mapper typed the wrong way round.
    let value = raw.max(min).min(max);
    counter.value = value;
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
        .component::<Message>(id)
        .map(|m| m.message.clone())
        .unwrap_or_default();
    if !text.is_empty() {
        log::info!("{text}");
    }
    world.fire_output(id, "OnShowMessage", None, None);
    true
}

fn spawn_timer(world: &mut EntityWorld, id: EntityId) {
    if !is_disabled(world, id) {
        world.set_think_delay(id, interval(world, id));
    }
}

fn think_timer(world: &mut EntityWorld, id: EntityId) {
    if is_disabled(world, id) {
        return;
    }
    world.fire_output(id, "OnTimer", None, None);
    world.set_think_delay(id, interval(world, id));
}

/// A timer's interval, never so short it fires every tick by accident.
fn interval(world: &EntityWorld, id: EntityId) -> f32 {
    world
        .component::<Timer>(id)
        .map_or(1.0, |t| t.refiretime)
        .max(0.01)
}

/// `Save`: save the game under `savename`, `auto` if it names none.
fn input_autosave(world: &mut EntityWorld, id: EntityId, e: &InputEvent) -> bool {
    let name = world
        .component::<Autosave>(id)
        .map(|a| a.savename.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "auto".to_string());
    world.request(host_requests::SAVE, name, id, e.activator);
    true
}
