// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Components more than one class carries: the schema's bases, as data.
//!
//! A class declares the ones it has with
//! [`ClassDef::component`](kerosene_entity::ClassDef::component); the map
//! fills their keyed fields, saves keep them, and the editor offers their
//! keys. The engine, which does not know these types, reads the same fields
//! by keyvalue name through
//! [`EntityWorld::keyvalue`](kerosene_entity::EntityWorld::keyvalue).

use kerosene_ecs::prelude::*;
use kerosene_entity::{EntityId, EntityWorld};

/// Switched off and on by `Disable`, `Enable` and `Toggle`: the schema's
/// `Switchable` base.
///
/// The map's `startdisabled` fills `disabled` directly, so an entity starts
/// as the map says and nothing has to copy one into the other at spawn.
#[derive(Component, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct Switchable {
    /// Whether it is switched off: neither drawn nor solid, for a brush;
    /// deaf to what enters it, for a trigger.
    #[reflect(@Key("startdisabled"))]
    pub disabled: bool,
}

/// Switch an entity off or on. Always handled, so the input is never
/// reported as unknown.
pub fn set_disabled(world: &mut EntityWorld, id: EntityId, disabled: bool) -> bool {
    if let Some(s) = world.component_mut::<Switchable>(id) {
        s.disabled = disabled;
    }
    true
}

/// Switch an entity the other way.
pub fn toggle_disabled(world: &mut EntityWorld, id: EntityId) -> bool {
    if let Some(s) = world.component_mut::<Switchable>(id) {
        s.disabled = !s.disabled;
    }
    true
}

/// Whether an entity is switched off: its [`Switchable`], or for a class
/// without one, never.
pub fn is_disabled(world: &EntityWorld, id: EntityId) -> bool {
    world
        .component::<Switchable>(id)
        .is_some_and(|s| s.disabled)
}

/// Something the player picks up by walking into it.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Pickup {
    /// How big a cube around it the player has to walk into. The engine
    /// reads it by name, for every class with a touch handler.
    #[reflect(
        @Key("touch_size"),
        @Label("Touch size"),
        @Help("How big a cube around it the player has to walk into.")
    )]
    pub touch_size: f32,
}

impl Default for Pickup {
    fn default() -> Self {
        Pickup { touch_size: 32.0 }
    }
}

/// What a rigid body is made of: the schema's `PhysicsBody` base.
///
/// The engine reads these by key when it gives a prop its body (see
/// `kerosene_engine::physics`), and a spawner copies its own onto every prop
/// it drops.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct PhysicsBody {
    /// Kilograms. Blank derives it from the model's size at a wood-like
    /// density.
    #[reflect(@Key("mass"), @Label("Mass (kg)"))]
    pub mass: Option<f32>,
    /// How much it resists sliding.
    #[reflect(@Key("friction"), @Label("Friction"))]
    pub friction: f32,
    /// How much it bounces.
    #[reflect(@Key("elasticity"), @Label("Bounciness"))]
    pub elasticity: f32,
    /// Whether the pick-up tool can grab it.
    #[reflect(@Key("pickable"), @Label("Can be picked up"))]
    pub pickable: bool,
}

impl Default for PhysicsBody {
    fn default() -> Self {
        PhysicsBody {
            mass: None,
            friction: 0.8,
            elasticity: 0.1,
            pickable: true,
        }
    }
}
