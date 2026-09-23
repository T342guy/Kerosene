// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Lights that exist at runtime.
//!
//! `light`, `light_spot` and `light_environment` are baked by Radiance and
//! gone by the time the map loads. `light_dynamic` is the one that stays: it
//! is drawn live, every frame, so it can be switched, moved and parented --
//! a lamp that goes out when a fuse blows, a warning beacon, a searchlight.
//! It costs a light in the renderer's budget and, if it casts shadows, some
//! shadow layers; use a baked light for anything that does not change.
//!
//! This class holds only whether it is on. What it looks like -- `_light`,
//! the falloff, the cone -- is read by the engine when it draws, from the
//! same keys Radiance reads for a baked light, so one can be swapped for the
//! other without retyping them.

use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value};

// The engine reads these itself when it draws the light, so the values are
// fixed there (`kerosene_engine::lights`); these copies are for this crate and
// the schema.

/// Spawnflag 1: start switched off.
pub const SF_START_OFF: u32 = 1;
/// Spawnflag 2: cast no shadows, even when a shadow layer is free.
pub const SF_NO_SHADOWS: u32 = 2;

/// The field the engine reads to know whether to draw it.
pub const ON_FIELD: &str = "on";

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("light_dynamic")
            .on_spawn(spawn)
            .input("TurnOn", turn_on)
            .input("TurnOff", turn_off)
            .input("Toggle", toggle)
            .output("OnTurnedOn")
            .output("OnTurnedOff"),
    );
}

/// Whether a `light_dynamic` is on.
pub fn is_on(world: &EntityWorld, id: EntityId) -> bool {
    world
        .get(id)
        .is_some_and(|e| e.fields.bool(ON_FIELD, false))
}

fn spawn(world: &mut EntityWorld, id: EntityId) {
    let off = world.get(id).is_some_and(|e| e.has_spawnflag(SF_START_OFF));
    set_on(world, id, !off);
}

fn turn_on(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    switch(world, id, true, event)
}

fn turn_off(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    switch(world, id, false, event)
}

fn toggle(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let on = is_on(world, id);
    switch(world, id, !on, event)
}

fn switch(world: &mut EntityWorld, id: EntityId, on: bool, event: &InputEvent) -> bool {
    if is_on(world, id) == on {
        return true;
    }
    set_on(world, id, on);
    let output = if on { "OnTurnedOn" } else { "OnTurnedOff" };
    world.fire_output(id, output, event.activator, None);
    true
}

fn set_on(world: &mut EntityWorld, id: EntityId, on: bool) {
    if let Some(e) = world.get_mut(id) {
        e.fields.set(ON_FIELD, Value::Bool(on));
    }
}
