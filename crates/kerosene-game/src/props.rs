// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Physics props and the thing that spawns them.
//!
//! `prop_physics` is a model the engine gives a rigid body: it falls, bounces
//! off walls, and settles. The class itself is thin on purpose -- the body,
//! the stepping and the pose write-back all live in the engine's
//! [`kerosene_engine::physics`] module, because a class handler only sees the
//! entity world and cannot reach the simulation. What lives here is the
//! designer-facing surface: the inputs a prop answers to.
//!
//! `prop_dynamic_spawner` is the counterpart for maps: a point entity that
//! spawns `prop_physics` props on demand, so a level can rain crates or drop
//! a barrel when a door opens without any scripting.

use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value};
use kerosene_math::Vec3;

/// Spawnflag: `prop_dynamic_spawner` fires once when the map starts.
pub const SF_SPAWN_ON_START: u32 = 1;

/// Spawnflag: `prop_physics` starts asleep until something wakes it.
pub const SF_START_ASLEEP: u32 = 1;

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("prop_physics")
            .input("Break", input_break)
            .input("Wake", input_wake)
            .input("Sleep", input_sleep)
            .output("OnBreak"),
    );

    registry.register(
        ClassDef::new("prop_dynamic_spawner")
            .on_spawn(spawn_spawner)
            .input("Trigger", input_trigger)
            .input("Spawn", input_trigger)
            .output("OnSpawned"),
    );
}

/// `Break` removes the prop, like Source's. The body follows it out of the
/// simulation on the next physics pass, which notices the entity is gone.
fn input_break(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    world.remove(id);
    world.fire_output(id, "OnBreak", None, None);
    true
}

/// `Wake` asks the engine to nudge the body, so a dozing prop reacts visibly.
fn input_wake(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    world.request(kerosene_entity::host_requests::PHYS_WAKE, "", id, None);
    true
}

/// `Sleep` asks the engine to stop the body dead.
fn input_sleep(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    world.request(kerosene_entity::host_requests::PHYS_SLEEP, "", id, None);
    true
}

/// A spawner that begins the map by spawning its first batch.
fn spawn_spawner(world: &mut EntityWorld, id: EntityId) {
    if world.get(id).is_some_and(|e| e.has_spawnflag(SF_SPAWN_ON_START)) {
        spawn_batch(world, id);
    }
}

/// `Trigger` (or `Spawn`) drops another batch of props.
fn input_trigger(world: &mut EntityWorld, id: EntityId, _e: &InputEvent) -> bool {
    spawn_batch(world, id);
    true
}

/// Spawn one batch of physics props at the spawner's position.
fn spawn_batch(world: &mut EntityWorld, id: EntityId) {
    let Some(spawner) = world.get(id).cloned() else { return };

    let model = spawner.fields.text("model").unwrap_or("props/cube").to_string();
    let batch = spawner.fields.i32("spawncount", 1).max(1) as usize;
    let max_total = spawner.fields.i32("maxprops", -1);
    let spread = spawner.fields.f32("spread", 0.0).max(0.0);
    let origin = spawner.origin;

    // How many it has produced so far, so `maxprops` can cap it and so stacked
    // props land apart rather than inside one another.
    let mut produced = spawner.fields.i32("_spawned", 0);

    for i in 0..batch {
        if max_total >= 0 && produced >= max_total { break; }

        // A deterministic jitter so a batch of crates drops as a loose pile
        // instead of a perfectly interpenetrating stack.
        let jitter = jitter(i, spread);
        let spawned = spawn_prop(world, &model, origin + jitter);

        if let Some(e) = world.get_mut(spawned) {
            // Slightly rotated so two cubes never settle into the same corner.
            e.angles = kerosene_math::Angles::new(0.0, (i as f32 * 37.0) % 360.0, 0.0);
        }
        produced += 1;
    }

    if let Some(e) = world.get_mut(id) {
        e.fields.set("_spawned", Value::Int(produced));
    }
    world.fire_output(id, "OnSpawned", None, None);
}

/// Create a `prop_physics` entity with a model, for the spawner.
fn spawn_prop(world: &mut EntityWorld, model: &str, origin: Vec3) -> EntityId {
    let id = world.spawn("prop_physics");
    if let Some(e) = world.get_mut(id) {
        e.origin = origin;
        e.fields.set("model", Value::Text(model.to_string()));
    }
    id
}

/// A small deterministic spread, in world units.
fn jitter(i: usize, spread: f32) -> Vec3 {
    if spread <= 0.0 {
        // No spread: still lift each one a little so a batch does not stack
        // inside itself.
        return Vec3::new(0.0, 0.0, i as f32 * 2.0);
    }
    let n = i as f32;
    Vec3::new(
        (n * 12.9898).sin() * spread,
        (n * 78.233).sin() * spread,
        i as f32 * 2.0 + (n * 39.425).sin().abs() * spread,
    )
}
