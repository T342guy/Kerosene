// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `prop_dynamic`: a model that animates.
//!
//! What a designer uses for a turret that swivels, a fan, a door made of a
//! model rather than a brush, a character standing at a console. It plays
//! its `defaultanim` from the start, switches clip on `SetAnimation`, and
//! fires `OnAnimationDone` when a clip that does not loop reaches its end --
//! after which it goes back to its default, as Source's does.
//!
//! This class only keeps the playback state, in a component and in game
//! time, so it saves and replays like everything else. The engine reads the
//! fields to pose the model, and it is the engine, which has the model's
//! clips, that notices a clip ending (see `kerosene_engine::animation`).

use kerosene_ecs::prelude::*;
use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, ModelRole};

/// What a `prop_dynamic` is playing, and when it started, in game time.
///
/// The engine reads and writes these by field name (see
/// `kerosene_engine::animation`), so the names are fixed.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Animated {
    /// The clip it plays from the start and returns to.
    #[reflect(@Key("defaultanim"))]
    pub defaultanim: String,
    /// The clip playing now.
    pub animation: String,
    /// Game time the clip started.
    pub anim_start: f32,
    /// Playback speed: 1 as authored.
    pub anim_rate: f32,
    /// The clip being faded out of, while a new one fades in.
    pub anim_previous: String,
    /// Game time `anim_previous` started, so it carries on from where it was.
    pub anim_previous_start: f32,
    /// Game time the fade from `anim_previous` began.
    pub anim_fade_start: f32,
    /// Set once `OnAnimationDone` has fired for the current clip.
    pub anim_done: bool,
}

impl Default for Animated {
    fn default() -> Self {
        Animated {
            defaultanim: String::new(),
            animation: String::new(),
            anim_start: 0.0,
            anim_rate: 1.0,
            anim_previous: String::new(),
            anim_previous_start: 0.0,
            anim_fade_start: 0.0,
            anim_done: false,
        }
    }
}

/// Register `prop_dynamic`.
pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("prop_dynamic")
            .model(ModelRole::Animated)
            .component::<Animated>()
            .on_spawn(spawn)
            .input("SetAnimation", set_animation)
            .input("SetDefaultAnimation", set_default)
            .input("SetPlaybackRate", set_rate)
            .output("OnAnimationDone"),
    );
}

fn spawn(world: &mut EntityWorld, id: EntityId) {
    let now = world.time;
    let Some(a) = world.component_mut::<Animated>(id) else {
        return;
    };
    a.defaultanim = a.defaultanim.trim().to_string();
    a.animation = a.defaultanim.clone();
    a.anim_start = now;
}

/// Start `clip`, fading from whatever was playing. The engine has the same
/// in `kerosene_engine::animation`, to go back to the default when a
/// one-shot ends.
pub fn play(world: &mut EntityWorld, id: EntityId, clip: &str) {
    let now = world.time;
    let Some(a) = world.component_mut::<Animated>(id) else {
        return;
    };
    if !a.animation.is_empty() {
        a.anim_previous = std::mem::take(&mut a.animation);
        a.anim_previous_start = a.anim_start;
        a.anim_fade_start = now;
    }
    a.animation = clip.trim().to_string();
    a.anim_start = now;
    a.anim_done = false;
}

fn set_animation(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    play(world, id, &event.parameter);
    true
}

fn set_default(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    if let Some(a) = world.component_mut::<Animated>(id) {
        a.defaultanim = event.parameter.trim().to_string();
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
    if let Some(a) = world.component_mut::<Animated>(id) {
        a.anim_rate = rate;
    }
    true
}
