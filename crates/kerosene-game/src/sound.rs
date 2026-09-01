// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Sounds placed in the world.
//!
//! Two classes, because they are two jobs and one entity doing both is one
//! entity you have to configure correctly to get either:
//!
//! * [`ambient_generic`](register) is the *bed*: a hum in a generator room, a
//!   drip in a cave. It loops, it starts with the map unless told not to, and
//!   it can be stopped and started again.
//! * `point_sound` is the *event*: a chime when a button is pressed, a crash
//!   when something breaks. It plays once, when fired, and holds no state at
//!   all.
//!
//! Source makes the first do both, through a spawnflag and a keyvalue that
//! have to agree. That is a thing to know rather than a thing to see, and a
//! designer who wants a one-shot noise should not have to know it.
//!
//! Like `logic_script`, it holds nothing of its own. It leaves a request on
//! the entity world and the engine, which owns the mixer, acts on it -- so
//! this crate stays free of the audio system entirely.

use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};

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

    registry.register(
        ClassDef::new("point_sound")
            .on_spawn(spawn_one_shot)
            .input("Play", play_once)
            .input("PlaySound", play_once)
            .input("Volume", set_volume)
            .output("OnPlay"),
    );
}

/// The sound an entity names.
///
/// `sound` is what it should be called and what the schema advertises.
/// `message` is what Source calls it, for a reason that made sense in 1998
/// and none since; it is still read so that anyone who wrote it, or copied a
/// Source example, is not left wondering why nothing plays.
pub fn sound_name(world: &EntityWorld, id: EntityId) -> String {
    world
        .get(id)
        .and_then(|e| {
            e.fields
                .text("sound")
                .or_else(|| e.fields.text("message"))
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn spawn_one_shot(world: &mut EntityWorld, id: EntityId) {
    // The difference from an ambience, and the only one: it does not loop and
    // does not start on its own. Everything else about playing a sound is the
    // same, so it is the same request.
    set(world, id, "looping", Value::Bool(false));
}

fn play_once(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let name = sound_name(world, id);
    if name.is_empty() {
        let at = world.get(id).map(|e| e.origin).unwrap_or_default();
        log::warn!("point_sound at {at:?} names no sound");
        return false;
    }
    world.request(host_requests::PLAY_SOUND, name, id, event.activator);
    world.fire_output(id, "OnPlay", event.activator, None);
    true
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
    set(world, id, "volume", Value::Float(volume.clamp(0.0, 1.0)));
    // Restart so the change is heard: the mixer sets a voice's gain when it
    // starts, and a running one is not re-read. A one-shot has nothing to
    // restart, and `playing` is a field only an ambience keeps.
    let playing = world.get(id).is_some_and(|e| e.fields.bool("playing", false));
    if playing {
        silence(world, id);
        start(world, id, None);
    }
    true
}

fn start(world: &mut EntityWorld, id: EntityId, activator: Option<EntityId>) {
    let name = sound_name(world, id);
    if name.is_empty() {
        let at = world.get(id).map(|e| e.origin).unwrap_or_default();
        log::warn!("ambient_generic at {at:?} names no sound");
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
