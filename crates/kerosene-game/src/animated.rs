// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `prop_dynamic`: a model that animates.
//!
//! What a designer uses for a turret that swivels, a fan, a door made of a
//! model rather than a brush, a character standing at a console. It plays
//! its `defaultanim` from the start, switches clip on `SetAnimation`, and
//! fires `OnAnimationDone` when a clip that does not loop reaches its end --
//! after which it goes back to its default, as Source's does.
//!
//! This class only keeps the playback state, in plain fields and in game
//! time, so it saves and replays like everything else. The engine reads the
//! fields to pose the model, and it is the engine, which has the model's
//! clips, that notices a clip ending (see `kerosene_engine::animation`).

use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value};

// The fields, which the engine reads by the same names.
pub const ANIMATION: &str = "animation";
pub const STARTED: &str = "anim_start";
pub const RATE: &str = "anim_rate";
pub const PREVIOUS: &str = "anim_previous";
pub const PREVIOUS_STARTED: &str = "anim_previous_start";
pub const FADE_STARTED: &str = "anim_fade_start";
/// Set once `OnAnimationDone` has fired for the current clip.
pub const DONE: &str = "anim_done";

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("prop_dynamic")
            .on_spawn(spawn)
            .input("SetAnimation", set_animation)
            .input("SetDefaultAnimation", set_default)
            .input("SetPlaybackRate", set_rate)
            .output("OnAnimationDone"),
    );
}

fn spawn(world: &mut EntityWorld, id: EntityId) {
    let now = world.time;
    let Some(e) = world.get_mut(id) else { return };
    let default = e
        .fields
        .text("defaultanim")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    e.fields.set(ANIMATION, Value::Text(default));
    e.fields.set(STARTED, Value::Float(now));
    if !e.fields.contains(RATE) {
        e.fields.set(RATE, Value::Float(1.0));
    }
}

/// Start `clip`, fading from whatever was playing. The engine calls this
/// too, to go back to the default when a one-shot ends.
pub fn play(world: &mut EntityWorld, id: EntityId, clip: &str) {
    let now = world.time;
    let Some(e) = world.get_mut(id) else { return };
    let current = e
        .fields
        .text(ANIMATION)
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let started = e.fields.f32(STARTED, now);
    if !current.is_empty() {
        e.fields.set(PREVIOUS, Value::Text(current));
        e.fields.set(PREVIOUS_STARTED, Value::Float(started));
        e.fields.set(FADE_STARTED, Value::Float(now));
    }
    e.fields
        .set(ANIMATION, Value::Text(clip.trim().to_string()));
    e.fields.set(STARTED, Value::Float(now));
    e.fields.set(DONE, Value::Bool(false));
}

fn set_animation(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    play(world, id, &event.parameter);
    true
}

fn set_default(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    if let Some(e) = world.get_mut(id) {
        e.fields.set(
            "defaultanim",
            Value::Text(event.parameter.trim().to_string()),
        );
    }
    true
}

fn set_rate(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let rate = event
        .parameter
        .trim()
        .parse::<f32>()
        .unwrap_or(1.0)
        .max(0.0);
    if let Some(e) = world.get_mut(id) {
        e.fields.set(RATE, Value::Float(rate));
    }
    true
}
