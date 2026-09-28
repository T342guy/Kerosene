// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Lights that exist at runtime.
//!
//! `light`, `light_spot` and `light_environment` are baked by Radiance and
//! gone by the time the map loads. `light_dynamic` is the one that stays: it
//! is drawn live, every frame, so it can be switched, moved and parented --
//! a lamp that goes out when a fuse blows, a warning beacon, a searchlight.
//! It costs a light in the renderer's budget and, if it casts shadows, some
//! shadow layers; use a baked light for anything that does not change.
//!
//! The component holds whether it is on and what it looks like -- `_light`,
//! the falloff, the cone. The engine reads the keys by name when it draws,
//! and they are the ones Radiance reads for a baked light, so one can be
//! swapped for the other without retyping them.

use kerosene_ecs::prelude::*;
use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld};

// The engine reads these itself when it draws the light, so the values are
// fixed there (`kerosene_engine::lights`); these copies are for this crate and
// the schema.

/// Spawnflag 1: start switched off.
pub const SF_START_OFF: u32 = 1;
/// Spawnflag 2: cast no shadows, even when a shadow layer is free.
pub const SF_NO_SHADOWS: u32 = 2;

/// A `light_dynamic`: how it shines, and whether it is shining.
///
/// Keys are worded in the schema; the types and defaults are here.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct DynamicLight {
    /// Red green blue, then brightness, as a baked light's.
    #[reflect(@Key("_light"), @Widget::Color)]
    pub light: String,
    /// Stops the light short of where its falloff would carry it; 0 lets the
    /// falloff decide.
    #[reflect(@Key("distance"))]
    pub distance: f32,
    /// Above 0 it is a spot; 0 shines every way.
    #[reflect(@Key("_cone"))]
    pub cone: f32,
    /// Where the spot's full brightness ends; blank is half the cone.
    #[reflect(@Key("_inner_cone"))]
    pub inner_cone: Option<f32>,
    /// Edge falloff of a spot.
    #[reflect(@Key("_exponent"))]
    pub exponent: f32,
    /// Overrides the pitch in the angles when non-zero.
    #[reflect(@Key("pitch"))]
    pub pitch: f32,
    /// Constant term of the falloff.
    #[reflect(@Key("_constant_attn"))]
    pub constant_attn: f32,
    /// Linear term of the falloff.
    #[reflect(@Key("_linear_attn"))]
    pub linear_attn: f32,
    /// Quadratic term of the falloff.
    #[reflect(@Key("_quadratic_attn"))]
    pub quadratic_attn: f32,
    /// Whether it is shining. State, not a key: the spawnflag sets it.
    pub on: bool,
}

impl Default for DynamicLight {
    fn default() -> Self {
        DynamicLight {
            light: "255 255 255 200".into(),
            distance: 0.0,
            cone: 0.0,
            inner_cone: None,
            exponent: 1.0,
            pitch: 0.0,
            constant_attn: 0.0,
            linear_attn: 0.0,
            quadratic_attn: 1.0,
            on: false,
        }
    }
}

/// Register `light_dynamic`, the one light that shines at run time: the
/// others are baked by Radiance and need no class here.
pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("light_dynamic")
            .component::<DynamicLight>()
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
    world.component::<DynamicLight>(id).is_some_and(|l| l.on)
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
    if let Some(l) = world.component_mut::<DynamicLight>(id) {
        l.on = on;
    }
}
