// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The entity world, written down and read back: the heart of a saved game.
//!
//! Everything an entity knows lives in its fields, its wires and its place in
//! the I/O queue -- that was the point of making entities a bag of fields
//! rather than typed structs -- so a snapshot is those, for every slot, and
//! nothing that would need a class to explain itself. A door half open is a
//! door whose `origin` is half way and whose think is due; a `math_counter`
//! is its `value`; a wire already fired once is a wire with `times_to_fire`
//! of zero.
//!
//! Handles survive: each entity goes back into the slot it came out of, at
//! the generation it had, so an `EntityId` held in a queued event, a script
//! variable or the game's own state still names the same entity afterwards.
//! That is also why nothing here renumbers anything.
//!
//! Restoring does not run spawn handlers -- a `logic_auto` would fire again,
//! a `math_counter` would reset -- but runs each class's
//! [`on_restore`](crate::ClassDef::on_restore) instead, for the little that
//! lives outside the fields.

use crate::io::{Connection, PendingEvent, Target};
use crate::value::{Fields, Value};
use crate::world::{Entity, EntityId, EntityWorld};
use kerosene_math::{Angles, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A field's value, as a save file spells it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SavedValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Text(String),
    Vector([f32; 3]),
    Angle([f32; 3]),
}

/// Who a queued event is for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SavedTarget {
    Named(String),
    Activator,
    Caller,
    #[serde(rename = "self")]
    Myself,
    Player,
    Handle([u32; 2]),
}

/// One entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedEntity {
    /// Slot and generation: the entity's handle.
    pub id: [u32; 2],
    pub classname: String,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brush_model: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_think: Option<f32>,
    /// Sorted, so the same world always writes the same file.
    pub fields: BTreeMap<String, SavedValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<Connection>,
}

/// An input on its way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedEvent {
    pub fire_at: f32,
    pub target: SavedTarget,
    pub input: String,
    #[serde(default)]
    pub parameter: String,
    #[serde(default)]
    pub activator: Option<[u32; 2]>,
    #[serde(default)]
    pub caller: Option<[u32; 2]>,
    pub sequence: u64,
}

/// Every entity, and everything waiting to happen to them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub time: f32,
    pub sequence: u64,
    #[serde(default)]
    pub player: Option<[u32; 2]>,
    /// Every slot's generation, occupied or not, so a handle to something
    /// already gone stays gone.
    pub generations: Vec<u32>,
    /// Empty slots in the order they will be reused, so a restored world
    /// hands out the same handles the saved one would have.
    pub free: Vec<u32>,
    pub entities: Vec<SavedEntity>,
    /// In firing order.
    pub queue: Vec<SavedEvent>,
}

/// A float JSON can hold. NaN and infinity have no spelling in it, and one
/// bad field should not make the whole game unsaveable.
fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

fn vec3(v: Vec3) -> [f32; 3] {
    [finite(v.x), finite(v.y), finite(v.z)]
}

fn id(id: EntityId) -> [u32; 2] {
    [id.index, id.generation]
}

fn handle([index, generation]: [u32; 2]) -> EntityId {
    EntityId { index, generation }
}

impl From<&Value> for SavedValue {
    fn from(v: &Value) -> SavedValue {
        match v {
            Value::Bool(b) => SavedValue::Bool(*b),
            Value::Int(i) => SavedValue::Int(*i),
            Value::Float(f) => SavedValue::Float(finite(*f)),
            Value::Text(t) => SavedValue::Text(t.clone()),
            Value::Vector(v) => SavedValue::Vector(vec3(*v)),
            Value::Angle(a) => SavedValue::Angle([finite(a.pitch), finite(a.yaw), finite(a.roll)]),
        }
    }
}

impl From<&SavedValue> for Value {
    fn from(v: &SavedValue) -> Value {
        match v {
            SavedValue::Bool(b) => Value::Bool(*b),
            SavedValue::Int(i) => Value::Int(*i),
            SavedValue::Float(f) => Value::Float(*f),
            SavedValue::Text(t) => Value::Text(t.clone()),
            SavedValue::Vector(v) => Value::Vector(Vec3::from_array(*v)),
            SavedValue::Angle([p, y, r]) => Value::Angle(Angles::new(*p, *y, *r)),
        }
    }
}

impl From<&Target> for SavedTarget {
    fn from(t: &Target) -> SavedTarget {
        match t {
            Target::Named(n) => SavedTarget::Named(n.clone()),
            Target::Activator => SavedTarget::Activator,
            Target::Caller => SavedTarget::Caller,
            Target::Myself => SavedTarget::Myself,
            Target::Player => SavedTarget::Player,
            Target::Handle(h) => SavedTarget::Handle(id(*h)),
        }
    }
}

impl From<&SavedTarget> for Target {
    fn from(t: &SavedTarget) -> Target {
        match t {
            SavedTarget::Named(n) => Target::Named(n.clone()),
            SavedTarget::Activator => Target::Activator,
            SavedTarget::Caller => Target::Caller,
            SavedTarget::Myself => Target::Myself,
            SavedTarget::Player => Target::Player,
            SavedTarget::Handle(h) => Target::Handle(handle(*h)),
        }
    }
}

impl EntityWorld {
    /// Write the world down.
    ///
    /// Taken between ticks. An entity already marked for removal is left
    /// out, as though its slot had been reclaimed.
    pub fn snapshot(&self) -> WorldSnapshot {
        let mut generations = self.generations.clone();
        let mut free = self.free.clone();
        let mut entities = Vec::new();
        for e in self.slots.iter().flatten() {
            if e.pending_removal {
                let i = e.id.index as usize;
                generations[i] = generations[i].wrapping_add(1);
                free.push(e.id.index);
                continue;
            }
            entities.push(SavedEntity {
                id: id(e.id),
                classname: e.classname.clone(),
                origin: vec3(e.origin),
                angles: [
                    finite(e.angles.pitch),
                    finite(e.angles.yaw),
                    finite(e.angles.roll),
                ],
                brush_model: e.brush_model,
                next_think: e.next_think.map(finite),
                fields: e
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.into()))
                    .collect(),
                connections: e.connections.clone(),
            });
        }

        let mut queue: Vec<&PendingEvent> = self.queue.iter().collect();
        queue.sort_by(|a, b| b.cmp(a));
        let alive = |h: Option<EntityId>| h.filter(|h| self.exists(*h)).map(id);
        WorldSnapshot {
            time: finite(self.time),
            sequence: self.sequence,
            player: alive(self.player),
            generations,
            free,
            entities,
            queue: queue
                .into_iter()
                .map(|e| SavedEvent {
                    fire_at: finite(e.fire_at),
                    target: (&e.target).into(),
                    input: e.input.clone(),
                    parameter: e.parameter.clone(),
                    activator: e.activator.map(id),
                    caller: e.caller.map(id),
                    sequence: e.sequence,
                })
                .collect(),
        }
    }

    /// Put a written-down world back, replacing everything here, and run
    /// each class's restore handler. Returns how many entities came back.
    ///
    /// The registry, the trace setting and any requests not yet taken stay
    /// as they are: they belong to the engine, not to the level.
    pub fn restore(&mut self, snapshot: &WorldSnapshot) -> Result<usize, String> {
        let slots = snapshot.generations.len();
        let mut restored: Vec<Option<Entity>> = vec![None; slots];
        for saved in &snapshot.entities {
            let at = saved.id[0] as usize;
            if at >= slots {
                return Err(format!(
                    "entity {} ({}) is outside the {slots} slots the save lists",
                    saved.id[0], saved.classname
                ));
            }
            if restored[at].is_some() {
                return Err(format!("two entities in slot {at}"));
            }
            if snapshot.generations[at] != saved.id[1] {
                return Err(format!(
                    "slot {at} is at generation {} but its entity says {}",
                    snapshot.generations[at], saved.id[1]
                ));
            }
            let mut fields = Fields::new();
            for (key, value) in &saved.fields {
                fields.set(key, value.into());
            }
            let [p, y, r] = saved.angles;
            restored[at] = Some(Entity {
                id: handle(saved.id),
                classname: saved.classname.clone(),
                fields,
                origin: Vec3::from_array(saved.origin),
                angles: Angles::new(p, y, r),
                connections: saved.connections.clone(),
                next_think: saved.next_think,
                brush_model: saved.brush_model,
                pending_removal: false,
            });
        }
        if let Some(bad) = snapshot
            .free
            .iter()
            .find(|&&i| restored.get(i as usize).is_none_or(Option::is_some))
        {
            return Err(format!("slot {bad} is listed free but is not"));
        }

        self.slots = restored;
        self.generations = snapshot.generations.clone();
        self.free = snapshot.free.clone();
        self.by_name.clear();
        for e in self.slots.iter().flatten() {
            if let Some(name) = e.targetname() {
                self.by_name
                    .entry(name.to_lowercase())
                    .or_default()
                    .push(e.id);
            }
        }
        self.queue.clear();
        for e in &snapshot.queue {
            self.queue.push(PendingEvent {
                fire_at: e.fire_at,
                target: (&e.target).into(),
                input: e.input.clone(),
                parameter: e.parameter.clone(),
                activator: e.activator.map(handle),
                caller: e.caller.map(handle),
                sequence: e.sequence,
            });
        }
        self.sequence = snapshot.sequence;
        self.time = snapshot.time;
        self.player = snapshot.player.map(handle).filter(|&h| self.exists(h));

        let registry = self.registry.clone();
        let ids = self.ids();
        for &id in &ids {
            let Some(classname) = self.get(id).map(|e| e.classname.clone()) else {
                continue;
            };
            if let Some(restore) = registry.restore_handler(&classname) {
                restore(self, id);
            }
        }
        Ok(ids.len())
    }
}

#[cfg(test)]
mod tests;
