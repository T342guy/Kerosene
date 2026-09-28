// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The pieces of a level that act on the player, or that the player acts on:
//! glass to break, walls that come and go, hazards, pickups, teleports, the
//! end of the game.
//!
//! | Class | What it does |
//! |---|---|
//! | `func_breakable` | A brush with health: shoot it, or fire `Break`, and it is gone |
//! | `func_wall_toggle` | A wall that is there, or is not |
//! | `point_hurt` | Hurts the player near it, once or over and over |
//! | `point_teleport` | Moves the player to itself |
//! | `item_healthkit` | Heals the player who walks into it, if they need it |
//! | `item_generic` | Anything else a player picks up: fires its outputs, and goes |
//! | `player_speedmod` | Makes the player faster or slower |
//! | `game_end` | Back to the main menu |
//!
//! What happens to the player is the engine's to do -- it owns their health
//! and where they are -- so these ask it, with the requests in
//! [`kerosene_entity::host_requests`].

use crate::components::Pickup;
use kerosene_ecs::prelude::*;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};

/// A `func_breakable`: how much more it takes, and what it sounds like
/// going.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Breakable {
    /// What is left of it. 0 from the start means it breaks only when told.
    #[reflect(
        @Key("health"),
        @Label("Health"),
        @Help("How much damage it takes to break. 0 breaks only when told.")
    )]
    pub health: f32,
    /// Played as it breaks.
    #[reflect(@Key("breaksound"), @Label("Break sound"), @Help("A sound to play as it breaks."))]
    pub breaksound: String,
    /// Already broken: a shotgun's pellets arrive in one tick, and the
    /// second must not break it again.
    pub broken: bool,
}

impl Default for Breakable {
    fn default() -> Self {
        Breakable {
            health: 1.0,
            breaksound: String::new(),
            broken: false,
        }
    }
}

/// A `func_wall_toggle`: whether it is there. No key: spawnflag 1 hides it
/// from the start. Named `disabled` because that is what the engine asks
/// every brush entity, to know whether to draw and collide with it.
#[derive(Component, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct WallToggle {
    /// Neither drawn nor solid.
    pub disabled: bool,
}

/// A `point_hurt`: how hard, how far and how often.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Hurt {
    /// At the centre.
    #[reflect(@Key("damage"), @Label("Damage"))]
    pub damage: f32,
    /// Where it reaches none.
    #[reflect(
        @Key("damageradius"),
        @Label("Radius"),
        @Help("Full damage at the centre, none at the edge. 0 reaches everywhere.")
    )]
    pub damageradius: f32,
    /// Seconds between hurts while on.
    #[reflect(@Key("damagedelay"), @Label("Interval"), @Help("Seconds between hurts while on."))]
    pub damagedelay: f32,
    /// Whether it is on.
    pub hurting: bool,
}

impl Default for Hurt {
    fn default() -> Self {
        Hurt {
            damage: 10.0,
            damageradius: 256.0,
            damagedelay: 0.5,
            hurting: false,
        }
    }
}

/// An `item_healthkit`: how much it gives.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Healthkit {
    /// Health given.
    #[reflect(@Key("health"), @Label("Health given"))]
    pub health: f32,
}

impl Default for Healthkit {
    fn default() -> Self {
        Healthkit { health: 25.0 }
    }
}

/// `func_breakable`: break only when told to, never from damage.
pub const SF_BREAK_ON_TRIGGER_ONLY: u32 = 1;
/// `func_wall_toggle`: start not there.
pub const SF_START_INVISIBLE: u32 = 1;
/// `point_hurt`: hurt on its own, every `interval`, from the start.
pub const SF_HURT_START_ON: u32 = 1;
/// `item_generic`: stay after being picked up, to be picked up again.
pub const SF_PICKUP_STAYS: u32 = 1;

/// Register the classes.
pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("func_breakable")
            .component::<Breakable>()
            .on_damage(damage_breakable)
            .input("Break", |w, id, e| {
                breakable_break(w, id, e.activator);
                true
            })
            .input("SetHealth", |w, id, e| {
                set_health(w, id, e.parameter_f32().unwrap_or(0.0), e.activator);
                true
            })
            .input("AddHealth", |w, id, e| {
                let now = health(w, id);
                set_health(w, id, now + e.parameter_f32().unwrap_or(0.0), e.activator);
                true
            })
            .input("RemoveHealth", |w, id, e| {
                let now = health(w, id);
                set_health(w, id, now - e.parameter_f32().unwrap_or(0.0), e.activator);
                true
            })
            .output("OnBreak")
            .output("OnHealthChanged"),
    );

    registry.register(
        ClassDef::new("func_wall_toggle")
            .component::<WallToggle>()
            .on_spawn(|w, id| {
                let hidden = w
                    .get(id)
                    .is_some_and(|e| e.has_spawnflag(SF_START_INVISIBLE));
                set_wall_hidden(w, id, |_| hidden);
            })
            .input("Toggle", |w, id, _| set_wall_hidden(w, id, |was| !was))
            .input("Show", |w, id, _| set_wall_hidden(w, id, |_| false))
            .input("Hide", |w, id, _| set_wall_hidden(w, id, |_| true)),
    );

    registry.register(
        ClassDef::new("point_hurt")
            .component::<Hurt>()
            .on_spawn(|w, id| {
                if w.get(id).is_some_and(|e| e.has_spawnflag(SF_HURT_START_ON)) {
                    set_hurting(w, id, true);
                }
            })
            .on_think(think_hurt)
            .input("Hurt", |w, id, e| {
                hurt_once(w, id, e.activator);
                true
            })
            .input("TurnOn", |w, id, _| set_hurting(w, id, true))
            .input("TurnOff", |w, id, _| set_hurting(w, id, false))
            .input("Toggle", |w, id, _| {
                let on = w.component::<Hurt>(id).is_some_and(|h| h.hurting);
                set_hurting(w, id, !on)
            })
            .output("OnHurtPlayer"),
    );

    registry.register(
        ClassDef::new("point_teleport")
            .input("Teleport", |w, id, e| {
                w.request(host_requests::TELEPORT_PLAYER, "", id, e.activator);
                w.fire_output(id, "OnTeleport", e.activator, None);
                true
            })
            .output("OnTeleport"),
    );

    registry.register(
        ClassDef::new("item_healthkit")
            .component::<Healthkit>()
            .component::<Pickup>()
            .on_touch(|w, id, player| {
                let amount = w.component::<Healthkit>(id).map_or(25.0, |h| h.health);
                w.request(
                    host_requests::HEAL_PLAYER,
                    amount.to_string(),
                    id,
                    Some(player),
                );
            })
            .output("OnPlayerHealed")
            .output("OnHealthFull"),
    );

    registry.register(
        ClassDef::new("item_generic")
            .component::<Pickup>()
            .on_touch(|w, id, player| {
                w.fire_output(id, "OnPlayerTouch", Some(player), None);
                if !w.get(id).is_some_and(|e| e.has_spawnflag(SF_PICKUP_STAYS)) {
                    w.remove(id);
                }
            })
            .output("OnPlayerTouch"),
    );

    registry.register(
        ClassDef::new("player_speedmod")
            .input("ModifySpeed", |w, id, e| {
                let scale = e
                    .parameter_f32()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .unwrap_or(1.0);
                // The player's own entity keeps it, so a save does too.
                let player = e.activator.or(w.player);
                if let Some(player) = player {
                    w.set_keyvalue(player, "speed_scale", Value::Float(scale));
                }
                w.fire_output(id, "OnModified", e.activator, Some(&scale.to_string()));
                true
            })
            .output("OnModified"),
    );

    registry.register(ClassDef::new("game_end").input("EndGame", |w, id, e| {
        w.request(host_requests::END_GAME, "", id, e.activator);
        true
    }));
}

fn health(world: &EntityWorld, id: EntityId) -> f32 {
    world.component::<Breakable>(id).map_or(0.0, |b| b.health)
}

fn set_wall_hidden(world: &mut EntityWorld, id: EntityId, hide: impl Fn(bool) -> bool) -> bool {
    if let Some(w) = world.component_mut::<WallToggle>(id) {
        w.disabled = hide(w.disabled);
    }
    true
}

fn set_hurting(world: &mut EntityWorld, id: EntityId, on: bool) -> bool {
    if let Some(h) = world.component_mut::<Hurt>(id) {
        h.hurting = on;
    }
    if on {
        world.set_think_delay(id, 0.0);
    } else {
        world.clear_think(id);
    }
    true
}

/// A hit: less health, and at none, broken. A breakable with no health to
/// start with, or the flag, only breaks when told.
fn damage_breakable(
    world: &mut EntityWorld,
    id: EntityId,
    amount: f32,
    attacker: Option<EntityId>,
) -> bool {
    let Some(entity) = world.get(id) else {
        return false;
    };
    let health = health(world, id);
    if entity.has_spawnflag(SF_BREAK_ON_TRIGGER_ONLY) || health <= 0.0 {
        return false;
    }
    set_health(world, id, health - amount, attacker);
    true
}

fn set_health(world: &mut EntityWorld, id: EntityId, health: f32, activator: Option<EntityId>) {
    let health = if health.is_finite() { health } else { 0.0 };
    if let Some(b) = world.component_mut::<Breakable>(id) {
        b.health = health.max(0.0);
    }
    world.fire_output(
        id,
        "OnHealthChanged",
        activator,
        Some(&health.max(0.0).to_string()),
    );
    if health <= 0.0 {
        breakable_break(world, id, activator);
    }
}

fn breakable_break(world: &mut EntityWorld, id: EntityId, activator: Option<EntityId>) {
    // Once: a shotgun's pellets arrive in one tick, and the second must not
    // break it again.
    let Some(b) = world.component_mut::<Breakable>(id) else {
        return;
    };
    if b.broken {
        return;
    }
    b.broken = true;
    let sound = b.breaksound.clone();
    if !sound.trim().is_empty() {
        world.request(host_requests::PLAY_SOUND, sound.trim(), id, activator);
    }
    world.fire_output(id, "OnBreak", activator, None);
    world.remove(id);
}

fn hurt_once(world: &mut EntityWorld, id: EntityId, activator: Option<EntityId>) {
    let hurt = world.component::<Hurt>(id).cloned().unwrap_or_default();
    let (damage, radius) = (hurt.damage, hurt.damageradius);
    world.request(
        host_requests::HURT_PLAYER,
        format!("{damage} {radius}"),
        id,
        activator,
    );
    world.fire_output(id, "OnHurtPlayer", activator, None);
}

fn think_hurt(world: &mut EntityWorld, id: EntityId) {
    let Some(hurt) = world.component::<Hurt>(id).cloned() else {
        return;
    };
    if !hurt.hurting {
        return;
    }
    hurt_once(world, id, None);
    let interval = hurt.damagedelay.max(0.05);
    world.set_think_delay(id, interval);
}
