//! Trigger volumes.
//!
//! A trigger is a brush that is not solid but that traces can find. When
//! something enters it, it fires `OnStartTouch`; when the last thing leaves,
//! `OnEndTouch`. That is the whole mechanism behind almost every scripted
//! moment in a Source game.

use crate::set_field;
use void_entity::io::InputEvent;
use void_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value};

pub fn register(registry: &mut ClassRegistry) {
    for name in ["trigger_multiple", "trigger_once", "trigger_hurt"] {
        registry.register(
            ClassDef::new(name)
                .on_spawn(spawn_trigger)
                .input("Enable", |w, id, _| { set_field(w, id, "disabled", Value::Bool(false)); true })
                .input("Disable", input_disable)
                .input("Toggle", |w, id, _| {
                    let off = w.get(id).map(|e| e.fields.bool("disabled", false)).unwrap_or(false);
                    set_field(w, id, "disabled", Value::Bool(!off));
                    true
                }),
        );
    }
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
    if world.get(id).map(|e| e.fields.bool("occupied", false)).unwrap_or(false) {
        set_field(world, id, "occupied", Value::Bool(false));
        world.fire_output(id, "OnEndTouch", None, None);
    }
    true
}

/// Tell a trigger whether something is inside it this tick.
///
/// The engine calls this for every trigger each tick; the edge detection lives
/// here so that a trigger fires on the *transition* rather than continuously.
/// `trigger_once` removes itself after firing, which is the only difference
/// between it and `trigger_multiple`.
pub fn update_touch(world: &mut EntityWorld, id: EntityId, inside: bool, activator: Option<EntityId>) {
    let Some(entity) = world.get(id) else { return };
    if entity.fields.bool("disabled", false) { return; }

    let was_inside = entity.fields.bool("occupied", false);
    if inside == was_inside { return; }

    set_field(world, id, "occupied", Value::Bool(inside));

    if inside {
        world.fire_output(id, "OnStartTouch", activator, None);
        world.fire_output(id, "OnTrigger", activator, None);

        let once = world
            .get(id)
            .map(|e| e.classname.eq_ignore_ascii_case("trigger_once"))
            .unwrap_or(false);
        if once { world.remove(id); }
    } else {
        world.fire_output(id, "OnEndTouch", activator, None);
    }
}

/// Damage a `trigger_hurt` deals per second, if any.
pub fn hurt_per_second(world: &EntityWorld, id: EntityId) -> f32 {
    world.get(id).map_or(0.0, |e| {
        if e.classname.eq_ignore_ascii_case("trigger_hurt") {
            e.fields.f32("damage", 10.0)
        } else {
            0.0
        }
    })
}
