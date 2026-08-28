// SPDX-License-Identifier: LGPL-3.0-or-later
//! `ambient_generic` -- a sound placed in the world.
//!
//! The entity every level uses more than any other sound mechanism: a hum in
//! a generator room, a drip in a cave, a door's own noise. It can be
//! positioned or heard flat, started with the map or waited for, and looped or
//! fired once.
//!
//! Like `logic_script`, it holds nothing of its own. It leaves a request on
//! the entity world and the engine, which owns the mixer, acts on it -- so
//! this crate stays free of the audio system entirely.

use void_entity::io::InputEvent;
use void_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};

/// Spawnflag 1: start playing as soon as the map does.
pub const SF_START_SILENT: u32 = 1;
/// Spawnflag 2: heard flat, wherever the listener is.
pub const SF_EVERYWHERE: u32 = 2;

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("ambient_generic")
            .on_spawn(spawn)
            .input("PlaySound", play)
            .input("StopSound", stop)
            .input("Toggle", toggle)
            .input("Volume", set_volume)
            .output("OnPlay"),
    );
}

fn spawn(world: &mut EntityWorld, id: EntityId) {
    let start_silent = world.get(id).is_some_and(|e| e.has_spawnflag(SF_START_SILENT));
    set(world, id, "playing", Value::Bool(false));
    if !start_silent {
        start(world, id, None);
    }
}

fn play(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    start(world, id, event.activator);
    true
}

fn stop(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    silence(world, id);
    true
}

fn toggle(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let playing = world.get(id).is_some_and(|e| e.fields.bool("playing", false));
    if playing { silence(world, id) } else { start(world, id, event.activator) }
    true
}

fn set_volume(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let Some(volume) = event.parameter_f32() else { return false };
    set(world, id, "health", Value::Float(volume.clamp(0.0, 1.0)));
    // Restart so the change is heard: the mixer sets a voice's gain when it
    // starts, and a running one is not re-read.
    let playing = world.get(id).is_some_and(|e| e.fields.bool("playing", false));
    if playing {
        silence(world, id);
        start(world, id, None);
    }
    true
}

fn start(world: &mut EntityWorld, id: EntityId, activator: Option<EntityId>) {
    let Some(entity) = world.get(id) else { return };
    let name = entity.fields.text("message").unwrap_or_default().to_string();
    if name.trim().is_empty() {
        log::warn!("ambient_generic at {:?} names no sound", entity.origin);
        return;
    }
    set(world, id, "playing", Value::Bool(true));
    world.request(host_requests::PLAY_SOUND, name, id, activator);
    world.fire_output(id, "OnPlay", activator, None);
}

fn silence(world: &mut EntityWorld, id: EntityId) {
    set(world, id, "playing", Value::Bool(false));
    world.request(host_requests::STOP_SOUND, "", id, None);
}

fn set(world: &mut EntityWorld, id: EntityId, key: &str, value: Value) {
    if let Some(e) = world.get_mut(id) { e.fields.set(key, value); }
}
