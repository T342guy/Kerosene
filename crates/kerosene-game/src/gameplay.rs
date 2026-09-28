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

use crate::{field_f32, set_field};
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};

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
            .on_spawn(spawn_breakable)
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
                let now = field_f32(w, id, "health", 0.0);
                set_health(w, id, now + e.parameter_f32().unwrap_or(0.0), e.activator);
                true
            })
            .input("RemoveHealth", |w, id, e| {
                let now = field_f32(w, id, "health", 0.0);
                set_health(w, id, now - e.parameter_f32().unwrap_or(0.0), e.activator);
                true
            })
            .output("OnBreak")
            .output("OnHealthChanged"),
    );

    registry.register(
        ClassDef::new("func_wall_toggle")
            .on_spawn(|w, id| {
                let hidden = w
                    .get(id)
                    .is_some_and(|e| e.has_spawnflag(SF_START_INVISIBLE));
                set_field(w, id, "disabled", Value::Bool(hidden));
            })
            .input("Toggle", |w, id, _| {
                let off = w.get(id).is_some_and(|e| e.fields.bool("disabled", false));
                set_field(w, id, "disabled", Value::Bool(!off));
                true
            })
            .input("Show", |w, id, _| {
                set_field(w, id, "disabled", Value::Bool(false));
                true
            })
            .input("Hide", |w, id, _| {
                set_field(w, id, "disabled", Value::Bool(true));
                true
            }),
    );

    registry.register(
        ClassDef::new("point_hurt")
            .on_spawn(|w, id| {
                if w.get(id).is_some_and(|e| e.has_spawnflag(SF_HURT_START_ON)) {
                    set_field(w, id, "hurting", Value::Bool(true));
                    w.set_think_delay(id, 0.0);
                }
            })
            .on_think(think_hurt)
            .input("Hurt", |w, id, e| {
                hurt_once(w, id, e.activator);
                true
            })
            .input("TurnOn", |w, id, _| {
                set_field(w, id, "hurting", Value::Bool(true));
                w.set_think_delay(id, 0.0);
                true
            })
            .input("TurnOff", |w, id, _| {
                set_field(w, id, "hurting", Value::Bool(false));
                w.clear_think(id);
                true
            })
            .input("Toggle", |w, id, _| {
                let on = w.get(id).is_some_and(|e| e.fields.bool("hurting", false));
                set_field(w, id, "hurting", Value::Bool(!on));
                if on {
                    w.clear_think(id);
                } else {
                    w.set_think_delay(id, 0.0);
                }
                true
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
            .on_touch(|w, id, player| {
                let amount = field_f32(w, id, "health", 25.0);
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
                    set_field(w, player, "speed_scale", Value::Float(scale));
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

fn spawn_breakable(world: &mut EntityWorld, id: EntityId) {
    let health = field_f32(world, id, "health", 1.0);
    set_field(world, id, "health", Value::Float(health));
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
    let health = entity.fields.f32("health", 0.0);
    if entity.has_spawnflag(SF_BREAK_ON_TRIGGER_ONLY) || health <= 0.0 {
        return false;
    }
    set_health(world, id, health - amount, attacker);
    true
}

fn set_health(world: &mut EntityWorld, id: EntityId, health: f32, activator: Option<EntityId>) {
    let health = if health.is_finite() { health } else { 0.0 };
    set_field(world, id, "health", Value::Float(health.max(0.0)));
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
    if world.get(id).is_none_or(|e| e.fields.bool("broken", false)) {
        return;
    }
    set_field(world, id, "broken", Value::Bool(true));
    let sound = world
        .get(id)
        .and_then(|e| e.fields.text("breaksound").map(|s| s.into_owned()))
        .unwrap_or_default();
    if !sound.trim().is_empty() {
        world.request(host_requests::PLAY_SOUND, sound.trim(), id, activator);
    }
    world.fire_output(id, "OnBreak", activator, None);
    world.remove(id);
}

fn hurt_once(world: &mut EntityWorld, id: EntityId, activator: Option<EntityId>) {
    let damage = field_f32(world, id, "damage", 10.0);
    let radius = field_f32(world, id, "damageradius", 256.0);
    world.request(
        host_requests::HURT_PLAYER,
        format!("{damage} {radius}"),
        id,
        activator,
    );
    world.fire_output(id, "OnHurtPlayer", activator, None);
}

fn think_hurt(world: &mut EntityWorld, id: EntityId) {
    if !world
        .get(id)
        .is_some_and(|e| e.fields.bool("hurting", false))
    {
        return;
    }
    hurt_once(world, id, None);
    let interval = field_f32(world, id, "damagedelay", 0.5).max(0.05);
    world.set_think_delay(id, interval);
}
