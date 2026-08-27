// SPDX-License-Identifier: LGPL-3.0-or-later
//! Entity storage, spawning, and the tick that drives them.

use crate::io::{Connection, InputEvent, PendingEvent, Target};
use crate::registry::ClassRegistry;
use crate::value::{Fields, Value};
use crate::MAX_EVENTS_PER_TICK;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use thiserror::Error;
use void_bsp::Bsp;
use void_kv::KeyValues;
use void_math::{Angles, Vec3};

/// A handle to an entity.
///
/// Carries a generation alongside the slot index so that a handle to a removed
/// entity fails to resolve rather than silently addressing whatever was
/// created in its place. Entity references outlive entities constantly -- a
/// queued event naming an entity that dies before it fires is routine -- and
/// without the generation those become use-after-free bugs with no crash to
/// point at them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EntityId {
    pub index: u32,
    pub generation: u32,
}

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("the map's entity lump did not parse: {0}")]
    BadEntityLump(#[from] void_kv::ParseError),
}

/// One entity.
#[derive(Clone, Debug)]
pub struct Entity {
    pub id: EntityId,
    pub classname: String,
    pub fields: Fields,
    pub origin: Vec3,
    pub angles: Angles,
    pub connections: Vec<Connection>,
    /// Game time of the next think, if scheduled.
    pub next_think: Option<f32>,
    /// Index of the brush model this entity is, from a `model` key of `"*N"`.
    pub brush_model: Option<usize>,
    /// Set by [`EntityWorld::remove`]; the slot is reclaimed at end of tick.
    pub pending_removal: bool,
}

impl Entity {
    pub fn targetname(&self) -> Option<&str> { self.fields.text("targetname") }

    pub fn spawnflags(&self) -> u32 { self.fields.i32("spawnflags", 0) as u32 }
    pub fn has_spawnflag(&self, bit: u32) -> bool { self.spawnflags() & bit != 0 }

    /// Outputs matching a name, case-insensitively.
    pub fn outputs<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Connection> + 'a {
        self.connections.iter().filter(move |c| c.output.eq_ignore_ascii_case(name))
    }
}

/// Every entity in the running level, plus the queue that drives their I/O.
pub struct EntityWorld {
    slots: Vec<Option<Entity>>,
    generations: Vec<u32>,
    free: Vec<u32>,
    by_name: HashMap<String, Vec<EntityId>>,
    queue: BinaryHeap<PendingEvent>,
    sequence: u64,
    /// Game time in seconds.
    pub time: f32,
    pub registry: Arc<ClassRegistry>,
    /// The local player, for `!player` targets.
    pub player: Option<EntityId>,
    /// Recent I/O, for a `developer 2`-style trace of what fired what.
    trace: Vec<String>,
    trace_enabled: bool,
}

impl EntityWorld {
    pub fn new(registry: Arc<ClassRegistry>) -> Self {
        EntityWorld {
            slots: Vec::new(),
            generations: Vec::new(),
            free: Vec::new(),
            by_name: HashMap::new(),
            queue: BinaryHeap::new(),
            sequence: 0,
            time: 0.0,
            registry,
            player: None,
            trace: Vec::new(),
            trace_enabled: false,
        }
    }

    // ---- storage ---------------------------------------------------------

    pub fn spawn(&mut self, classname: &str) -> EntityId {
        let index = match self.free.pop() {
            Some(i) => i,
            None => {
                self.slots.push(None);
                self.generations.push(0);
                (self.slots.len() - 1) as u32
            }
        };
        let id = EntityId { index, generation: self.generations[index as usize] };
        self.slots[index as usize] = Some(Entity {
            id,
            classname: classname.to_string(),
            fields: Fields::new(),
            origin: Vec3::ZERO,
            angles: Angles::ZERO,
            connections: Vec::new(),
            next_think: None,
            brush_model: None,
            pending_removal: false,
        });
        id
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        let slot = self.slots.get(id.index as usize)?.as_ref()?;
        (slot.id.generation == id.generation).then_some(slot)
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        let slot = self.slots.get_mut(id.index as usize)?.as_mut()?;
        (slot.id.generation == id.generation).then_some(slot)
    }

    pub fn exists(&self, id: EntityId) -> bool { self.get(id).is_some() }

    /// Mark an entity for removal. The slot is reclaimed after the tick, so
    /// handlers mid-dispatch never find it vanished underneath them.
    pub fn remove(&mut self, id: EntityId) {
        if let Some(e) = self.get_mut(id) { e.pending_removal = true; }
    }

    pub fn len(&self) -> usize { self.slots.iter().filter(|s| s.is_some()).count() }
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.slots.iter().filter_map(|s| s.as_ref())
    }

    pub fn ids(&self) -> Vec<EntityId> {
        self.slots.iter().filter_map(|s| s.as_ref().map(|e| e.id)).collect()
    }

    pub fn find_by_name(&self, name: &str) -> Vec<EntityId> {
        self.by_name
            .get(&name.to_lowercase())
            .map(|v| v.iter().copied().filter(|&id| self.exists(id)).collect())
            .unwrap_or_default()
    }

    pub fn find_by_class(&self, classname: &str) -> Vec<EntityId> {
        self.iter()
            .filter(|e| e.classname.eq_ignore_ascii_case(classname))
            .map(|e| e.id)
            .collect()
    }

    pub fn first_of_class(&self, classname: &str) -> Option<EntityId> {
        self.iter().find(|e| e.classname.eq_ignore_ascii_case(classname)).map(|e| e.id)
    }

    /// Give an entity a name, or change the one it has.
    pub fn set_targetname(&mut self, id: EntityId, name: &str) {
        if let Some(old) = self.get(id).and_then(|e| e.targetname()).map(str::to_lowercase) {
            if let Some(list) = self.by_name.get_mut(&old) { list.retain(|&x| x != id); }
        }
        if let Some(e) = self.get_mut(id) {
            e.fields.set("targetname", Value::Text(name.to_string()));
        }
        self.by_name.entry(name.to_lowercase()).or_default().push(id);
    }

    // ---- loading ---------------------------------------------------------

    /// Create every entity in a compiled map and run its spawn handler.
    ///
    /// Brush entities are given their model's bounds as fields *before* spawn
    /// handlers run, because a class like `func_door` needs to know how far it
    /// travels, and that comes from the geometry rather than from a keyvalue.
    pub fn load_from_bsp(&mut self, bsp: &Bsp) -> Result<usize, SpawnError> {
        let kv = bsp.entities_kv()?;
        let created = self.create_entities(&kv);

        for &id in &created {
            let Some(index) = self.get(id).and_then(|e| e.brush_model) else { continue };
            let Some(model) = bsp.models.get(index) else { continue };
            let bounds = model.bounds();
            if let Some(e) = self.get_mut(id) {
                e.fields.set("model_mins", Value::Vector(bounds.min));
                e.fields.set("model_maxs", Value::Vector(bounds.max));
            }
        }

        self.run_spawn_handlers(&created);
        Ok(created.len())
    }

    /// Create entities from KeyValues and run their spawn handlers.
    pub fn load_from_kv(&mut self, kv: &KeyValues) -> Result<usize, SpawnError> {
        let created = self.create_entities(kv);
        self.run_spawn_handlers(&created);
        Ok(created.len())
    }

    /// Create entities without spawning them, so callers can fill in anything
    /// a spawn handler will need first.
    fn create_entities(&mut self, kv: &KeyValues) -> Vec<EntityId> {
        let mut created = Vec::new();

        for block in kv.blocks("entity") {
            let classname = block.get("classname").unwrap_or("").to_string();
            if classname.is_empty() {
                log::warn!("skipping an entity with no classname");
                continue;
            }
            let id = self.spawn(&classname);

            for (key, value) in block.pairs() {
                match key.to_lowercase().as_str() {
                    "classname" => {}
                    "origin" => {
                        if let Some(v) = Value::from_keyvalue(value).as_vec3() {
                            if let Some(e) = self.get_mut(id) { e.origin = v; }
                        }
                    }
                    "angles" => {
                        if let Some(v) = Value::from_keyvalue(value).as_vec3() {
                            if let Some(e) = self.get_mut(id) {
                                e.angles = Angles::new(v.x, v.y, v.z);
                            }
                        }
                    }
                    "model" => {
                        // `"*3"` names brush model 3; anything else is a
                        // studio model path, which stays a plain field.
                        if let Some(rest) = value.strip_prefix('*') {
                            if let Ok(index) = rest.parse::<usize>() {
                                if let Some(e) = self.get_mut(id) { e.brush_model = Some(index); }
                            }
                        }
                        if let Some(e) = self.get_mut(id) {
                            e.fields.set("model", Value::Text(value.to_string()));
                        }
                    }
                    "targetname" => self.set_targetname(id, value),
                    other => {
                        if let Some(e) = self.get_mut(id) {
                            e.fields.set(other, Value::from_keyvalue(value));
                        }
                    }
                }
            }

            if let Some(conn) = block.block("connections") {
                for (output, raw) in conn.pairs() {
                    match void_map::Connection::parse(output, raw) {
                        Ok(c) => {
                            if let Some(e) = self.get_mut(id) { e.connections.push(c.into()); }
                        }
                        Err(err) => log::warn!("{classname}: {err}"),
                    }
                }
            }

            created.push(id);
        }

        created
    }

    /// Run spawn handlers, once every entity exists.
    ///
    /// Deferred so that one entity can look another up by name during its own
    /// spawn -- a door finding the button that opens it, for instance.
    fn run_spawn_handlers(&mut self, created: &[EntityId]) {
        let registry = self.registry.clone();
        for id in created {
            let Some(classname) = self.get(*id).map(|e| e.classname.clone()) else { continue };
            if let Some(spawn) = registry.spawn_handler(&classname) {
                spawn(self, *id);
            } else if !registry.is_registered(&classname) {
                log::debug!("no class registered for '{classname}'; it will be inert");
            }
        }
    }

    // ---- entity I/O ------------------------------------------------------

    /// Fire an entity's output, queueing an input on everything it is wired to.
    ///
    /// Returns how many wires fired. Nothing is delivered immediately, even at
    /// zero delay: routing everything through the queue keeps an entity firing
    /// at itself from recursing into the stack, and makes ordering the same
    /// whether a delay is zero or not.
    pub fn fire_output(
        &mut self,
        caller: EntityId,
        output: &str,
        activator: Option<EntityId>,
        parameter_override: Option<&str>,
    ) -> usize {
        let Some(entity) = self.get(caller) else { return 0 };

        let mut queued = Vec::new();
        for (i, c) in entity.connections.iter().enumerate() {
            if !c.output.eq_ignore_ascii_case(output) { continue; }
            if c.is_exhausted() { continue; }
            queued.push((
                i,
                Target::parse(&c.target),
                c.input.clone(),
                parameter_override.unwrap_or(&c.parameter).to_string(),
                c.delay,
            ));
        }
        if queued.is_empty() { return 0; }

        for (index, target, input, parameter, delay) in &queued {
            if self.trace_enabled {
                let name = self.get(caller).map(|e| e.classname.clone()).unwrap_or_default();
                self.trace.push(format!(
                    "[{:.2}] {name} :: {output} -> {:?} :: {input}{}",
                    self.time,
                    target,
                    if *delay > 0.0 { format!(" (+{delay:.2}s)") } else { String::new() }
                ));
            }

            self.sequence += 1;
            self.queue.push(PendingEvent {
                fire_at: self.time + delay.max(0.0),
                target: target.clone(),
                input: input.clone(),
                parameter: parameter.clone(),
                activator,
                caller: Some(caller),
                sequence: self.sequence,
            });

            // Decrement the fire counter now rather than on delivery: an
            // "only once" output should not fire twice while the first is
            // still in flight.
            if let Some(e) = self.get_mut(caller) {
                if let Some(c) = e.connections.get_mut(*index) {
                    if c.times_to_fire > 0 { c.times_to_fire -= 1; }
                }
            }
        }

        queued.len()
    }

    /// Deliver an input to one entity immediately.
    pub fn accept_input(&mut self, target: EntityId, event: &InputEvent) -> bool {
        let Some(classname) = self.get(target).map(|e| e.classname.clone()) else { return false };
        let registry = self.registry.clone();
        match registry.find_input(&classname, &event.name) {
            Some(handler) => handler(self, target, event),
            None => {
                log::debug!("{classname} has no input named '{}'", event.name);
                false
            }
        }
    }

    /// Queue an input for later, as if an output had fired it.
    pub fn queue_input(
        &mut self,
        target: Target,
        input: &str,
        parameter: &str,
        delay: f32,
        activator: Option<EntityId>,
        caller: Option<EntityId>,
    ) {
        self.sequence += 1;
        self.queue.push(PendingEvent {
            fire_at: self.time + delay.max(0.0),
            target,
            input: input.to_string(),
            parameter: parameter.to_string(),
            activator,
            caller,
            sequence: self.sequence,
        });
    }

    /// Work out which entities an output is addressed to.
    fn resolve(&self, target: &Target, activator: Option<EntityId>, caller: Option<EntityId>) -> Vec<EntityId> {
        match target {
            // Several entities may share a name, and firing at it fires all of
            // them -- which is how one wire opens six doors.
            Target::Named(name) => self.find_by_name(name),
            Target::Activator => activator.into_iter().filter(|&id| self.exists(id)).collect(),
            Target::Caller => caller.into_iter().filter(|&id| self.exists(id)).collect(),
            Target::Myself => caller.into_iter().filter(|&id| self.exists(id)).collect(),
            Target::Player => self.player.into_iter().filter(|&id| self.exists(id)).collect(),
        }
    }

    // ---- the tick --------------------------------------------------------

    /// Advance time, deliver every event that has come due, and run thinks.
    ///
    /// Returns how many inputs were delivered.
    pub fn run(&mut self, dt: f32) -> usize {
        self.time += dt;
        let delivered = self.dispatch_due();
        self.run_thinks();
        self.reclaim_removed();
        delivered
    }

    fn dispatch_due(&mut self) -> usize {
        let mut delivered = 0usize;

        while delivered < MAX_EVENTS_PER_TICK {
            let Some(next) = self.queue.peek() else { break };
            if next.fire_at > self.time { break; }
            let event = self.queue.pop().expect("just peeked");

            let receivers = self.resolve(&event.target, event.activator, event.caller);
            if receivers.is_empty() && matches!(event.target, Target::Named(_)) {
                if let Target::Named(name) = &event.target {
                    log::debug!("nothing named '{name}' to receive '{}'", event.input);
                }
            }

            for id in receivers {
                let input = InputEvent {
                    name: event.input.clone(),
                    parameter: event.parameter.clone(),
                    activator: event.activator,
                    caller: event.caller,
                };
                self.accept_input(id, &input);
                delivered += 1;
            }
        }

        if delivered >= MAX_EVENTS_PER_TICK {
            // Two relays firing each other at zero delay would otherwise spin
            // forever. Dropping the rest of the queue breaks the loop and
            // leaves the level playable.
            log::error!(
                "entity I/O exceeded {MAX_EVENTS_PER_TICK} events in one tick; \
                 something is wired in a loop. Remaining events discarded."
            );
            self.queue.clear();
        }

        delivered
    }

    fn run_thinks(&mut self) {
        let registry = self.registry.clone();
        let due: Vec<(EntityId, String)> = self
            .iter()
            .filter(|e| e.next_think.is_some_and(|t| t <= self.time))
            .map(|e| (e.id, e.classname.clone()))
            .collect();

        for (id, classname) in due {
            // Clear it first so a handler that does not reschedule stops,
            // rather than being called every tick forever.
            if let Some(e) = self.get_mut(id) { e.next_think = None; }
            if let Some(think) = registry.think_handler(&classname) {
                think(self, id);
            }
        }
    }

    fn reclaim_removed(&mut self) {
        let doomed: Vec<EntityId> = self
            .iter()
            .filter(|e| e.pending_removal)
            .map(|e| e.id)
            .collect();

        for id in doomed {
            if let Some(name) = self.get(id).and_then(|e| e.targetname()).map(str::to_lowercase) {
                if let Some(list) = self.by_name.get_mut(&name) { list.retain(|&x| x != id); }
            }
            let index = id.index as usize;
            self.slots[index] = None;
            // Bumping the generation is what makes stale handles fail to
            // resolve instead of addressing whoever moves into this slot.
            self.generations[index] = self.generations[index].wrapping_add(1);
            self.free.push(id.index);
            if self.player == Some(id) { self.player = None; }
        }
    }

    /// Schedule a think for `delay` seconds from now.
    pub fn set_think_delay(&mut self, id: EntityId, delay: f32) {
        let at = self.time + delay.max(0.0);
        if let Some(e) = self.get_mut(id) { e.next_think = Some(at); }
    }

    pub fn clear_think(&mut self, id: EntityId) {
        if let Some(e) = self.get_mut(id) { e.next_think = None; }
    }

    pub fn pending_event_count(&self) -> usize { self.queue.len() }

    // ---- debugging -------------------------------------------------------

    /// Record every output that fires, for a `developer`-style I/O trace.
    pub fn set_trace(&mut self, on: bool) {
        self.trace_enabled = on;
        if !on { self.trace.clear(); }
    }

    pub fn trace_lines(&self) -> &[String] { &self.trace }
    pub fn clear_trace(&mut self) { self.trace.clear(); }
}

#[cfg(test)]
mod tests;
