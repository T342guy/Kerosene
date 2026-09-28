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
//! classes with the inputs that flip `disabled`, the outputs the engine
//! fires, and the components holding the keys the engine reads by name.

use crate::components::{Switchable, set_disabled, toggle_disabled};
use kerosene_ecs::prelude::*;
use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};
use kerosene_math::Vec3;

/// A `trigger_hurt`'s damage.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct TriggerHurt {
    /// Per second, while the player stands in it.
    #[reflect(@Key("damage"), @Label("Damage per second"))]
    pub damage: f32,
}

impl Default for TriggerHurt {
    fn default() -> Self {
        TriggerHurt { damage: 10.0 }
    }
}

/// A `trigger_push`'s shove.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct TriggerPush {
    /// Which way, normalised when used.
    #[reflect(
        @Key("pushdir"),
        @Label("Push direction"),
        @Help("Normalised. 0 0 1 throws straight up.")
    )]
    pub pushdir: Vec3,
    /// Units per second added along it.
    #[reflect(
        @Key("speed"),
        @Label("Speed"),
        @Help("Units per second added along the push direction.")
    )]
    pub speed: f32,
}

impl Default for TriggerPush {
    fn default() -> Self {
        TriggerPush {
            pushdir: Vec3::Z,
            speed: 400.0,
        }
    }
}

/// A `trigger_teleport`'s destination.
#[derive(Component, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct TriggerTeleport {
    /// The targetname of where it sends things.
    #[reflect(
        @Key("target"),
        @Label("Destination"),
        @Widget::TargetDestination,
        @Help("The targetname of an info_target, or anything else with a position.")
    )]
    pub target: String,
}

/// A `trigger_changelevel`'s next map.
#[derive(Component, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct ChangeLevel {
    /// The map, by name.
    #[reflect(
        @Key("map"),
        @Label("Map"),
        @Help("The map to go to, by name: kero_start, not maps/kero_start.kbsp.")
    )]
    pub map: String,
    /// The `info_landmark` both maps share, if any.
    #[reflect(
        @Key("landmark"),
        @Label("Landmark"),
        @Widget::TargetDestination,
        @Help(
            "The targetname of an info_landmark placed at the same spot in both maps. Empty starts the player at the next map's spawn point."
        )
    )]
    pub landmark: String,
}

/// Register the triggers: `trigger_multiple`, `trigger_once`,
/// `trigger_hurt`, `trigger_push`, `trigger_teleport` and
/// `trigger_changelevel`. Their inputs and outputs are here; noticing the
/// player walk in is the engine's.
pub fn register(registry: &mut ClassRegistry) {
    registry.register(trigger("trigger_multiple"));
    registry.register(trigger("trigger_once"));
    registry.register(trigger("trigger_hurt").component::<TriggerHurt>());
    registry.register(trigger("trigger_push").component::<TriggerPush>());
    registry.register(trigger("trigger_teleport").component::<TriggerTeleport>());
    // Walking in is handled by the engine, which owns the player it carries
    // across; the input is for a level change something else decides on.
    registry.register(
        trigger("trigger_changelevel")
            .component::<ChangeLevel>()
            .input("ChangeLevel", input_change_level),
    );
}

/// A trigger class: the inputs that flip `disabled`, the outputs the engine
/// fires.
fn trigger(name: &'static str) -> ClassDef {
    ClassDef::new(name)
        .component::<Switchable>()
        .input("Enable", |w, id, _| set_disabled(w, id, false))
        .input("Disable", input_disable)
        .input("Toggle", |w, id, _| toggle_disabled(w, id))
        .output("OnStartTouch")
        .output("OnEndTouch")
        .output("OnTrigger")
}

/// Go now, without waiting for the player to walk in.
fn input_change_level(world: &mut EntityWorld, id: EntityId, e: &InputEvent) -> bool {
    let Some(to) = world.component::<ChangeLevel>(id) else {
        return false;
    };
    let map = to.map.trim().to_string();
    if map.is_empty() {
        log::warn!("trigger_changelevel: ChangeLevel with no `map` set");
        return false;
    }
    let landmark = to.landmark.trim().to_string();
    world.request(
        host_requests::CHANGE_LEVEL,
        format!("{map} {landmark}").trim_end().to_string(),
        id,
        e.activator,
    );
    true
}

fn input_disable(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    set_disabled(world, id, true);
    // Anything standing in it has effectively left, so the end-touch fires.
    // Without this, disabling a trigger with the player inside leaves it
    // permanently believing it is occupied. `occupied` is the engine's
    // record, kept on the entity by name.
    if world.keyvalue_bool(id, "occupied", false) {
        world.set_keyvalue(id, "occupied", Value::Bool(false));
        world.fire_output(id, "OnEndTouch", None, None);
    }
    true
}
