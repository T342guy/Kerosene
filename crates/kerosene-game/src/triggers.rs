// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Trigger volumes.
//!
//! A trigger is a brush that is not solid but that traces can find. When
//! something enters it, it fires `OnStartTouch`; when the last thing leaves,
//! `OnEndTouch`. That is the whole mechanism behind almost every scripted
//! moment in a Source game.
//!
//! Only the classes are here. What a trigger *does* when the player is in it
//! -- the touch edge, the hurt, the push, the teleport -- is an engine
//! convention (`kerosene_engine::triggers`), because every consequence is to
//! the player and the engine owns the player. This crate just declares the
//! classes with the inputs that flip `disabled` and the outputs the engine
//! fires.

use crate::set_field;
use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};

pub fn register(registry: &mut ClassRegistry) {
    for name in [
        "trigger_multiple",
        "trigger_once",
        "trigger_hurt",
        "trigger_push",
        "trigger_teleport",
    ] {
        registry.register(trigger(name));
    }
    // Walking in is handled by the engine, which owns the player it carries
    // across; the input is for a level change something else decides on.
    registry.register(trigger("trigger_changelevel").input("ChangeLevel", input_change_level));
}

/// A trigger class: the inputs that flip `disabled`, the outputs the engine
/// fires.
fn trigger(name: &'static str) -> ClassDef {
    ClassDef::new(name)
        .on_spawn(spawn_trigger)
        .input("Enable", |w, id, _| {
            set_field(w, id, "disabled", Value::Bool(false));
            true
        })
        .input("Disable", input_disable)
        .input("Toggle", |w, id, _| {
            let off = w
                .get(id)
                .map(|e| e.fields.bool("disabled", false))
                .unwrap_or(false);
            set_field(w, id, "disabled", Value::Bool(!off));
            true
        })
        .output("OnStartTouch")
        .output("OnEndTouch")
        .output("OnTrigger")
}

/// Go now, without waiting for the player to walk in.
fn input_change_level(world: &mut EntityWorld, id: EntityId, e: &InputEvent) -> bool {
    let Some(entity) = world.get(id) else {
        return false;
    };
    let map = entity
        .fields
        .text("map")
        .map(|m| m.trim().to_string())
        .unwrap_or_default();
    if map.is_empty() {
        log::warn!("trigger_changelevel: ChangeLevel with no `map` set");
        return false;
    }
    let landmark = entity
        .fields
        .text("landmark")
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    world.request(
        host_requests::CHANGE_LEVEL,
        format!("{map} {landmark}").trim_end().to_string(),
        id,
        e.activator,
    );
    true
}

fn spawn_trigger(world: &mut EntityWorld, id: EntityId) {
    let start_disabled = world
        .get(id)
        .map(|e| e.fields.bool("startdisabled", false))
        .unwrap_or(false);
    set_field(world, id, "disabled", Value::Bool(start_disabled));
    set_field(world, id, "occupied", Value::Bool(false));
}

fn input_disable(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    set_field(world, id, "disabled", Value::Bool(true));
    // Anything standing in it has effectively left, so the end-touch fires.
    // Without this, disabling a trigger with the player inside leaves it
    // permanently believing it is occupied.
    if world
        .get(id)
        .map(|e| e.fields.bool("occupied", false))
        .unwrap_or(false)
    {
        set_field(world, id, "occupied", Value::Bool(false));
        world.fire_output(id, "OnEndTouch", None, None);
    }
    true
}
