// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Entity storage, spawning, and the tick that drives them.

use crate::MAX_EVENTS_PER_TICK;
use crate::io::{Connection, InputEvent, PendingEvent, Target};
use crate::registry::{ClassComponent, ClassRegistry, ComponentDecl};
use crate::value::{Fields, Value};
use kerosene_ecs::{Component, Entity as Handle, World};
use kerosene_kv::KeyValues;
use kerosene_math::{Aabb, Angles, Vec3};
use kerosene_reflect::Struct;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use thiserror::Error;

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
#[non_exhaustive]
pub enum SpawnError {
    #[error("the map's entity lump did not parse: {0}")]
    BadEntityLump(#[from] kerosene_kv::ParseError),
}

/// One entity.
#[derive(Clone, Debug)]
pub struct Entity {
    pub id: EntityId,
    pub classname: String,
    /// Keyvalues no component of its class claims: a mod's own keys, and the
    /// engine's for classes not yet described by components. See
    /// [`EntityWorld::keyvalue`], which looks in both.
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
    /// Where its components live in [`EntityWorld`]'s ECS world.
    pub(crate) handle: Handle,
}

impl Entity {
    pub fn targetname(&self) -> Option<&str> {
        // Always stored as text -- `set_targetname` sees to that -- so this
        // can hand out a borrow.
        self.fields.get("targetname").and_then(Value::as_str)
    }

    pub fn spawnflags(&self) -> u32 {
        self.fields.i32("spawnflags", 0) as u32
    }
    pub fn has_spawnflag(&self, bit: u32) -> bool {
        self.spawnflags() & bit != 0
    }

    /// Outputs matching a name, case-insensitively.
    pub fn outputs<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Connection> + 'a {
        self.connections
            .iter()
            .filter(move |c| c.output.eq_ignore_ascii_case(name))
    }
}

/// Every entity in the running level, plus the queue that drives their I/O.
pub struct EntityWorld {
    pub(crate) slots: Vec<Option<Entity>>,
    /// Each entity's components, by its [`Entity::handle`].
    pub(crate) ecs: World,
    pub(crate) generations: Vec<u32>,
    pub(crate) free: Vec<u32>,
    pub(crate) by_name: HashMap<String, Vec<EntityId>>,
    pub(crate) queue: BinaryHeap<PendingEvent>,
    pub(crate) sequence: u64,
    /// Game time in seconds.
    pub time: f32,
    pub registry: Arc<ClassRegistry>,
    /// The local player, for `!player` targets.
    pub player: Option<EntityId>,
    /// Recent I/O, for a `developer 2`-style trace of what fired what.
    trace: Vec<String>,
    trace_enabled: bool,
    /// Things entities have asked the host to do.
    ///
    /// The same shape as the console's outbox, and for the same reason: a
    /// class handler gets `&mut EntityWorld` and nothing else, deliberately,
    /// so that the game DLL cannot reach into the engine. Anything needing
    /// more than the entity world -- running a script, loading a sound --
    /// leaves a request here and the engine picks it up at the end of the
    /// tick, in order.
    requests: Vec<HostRequest>,
}

/// Something an entity asked the engine to do.
#[derive(Clone, Debug, PartialEq)]
pub struct HostRequest {
    pub kind: String,
    pub payload: String,
    /// The entity that asked, so the engine can answer in its context.
    pub caller: EntityId,
    /// Whatever set the chain off, if anything.
    pub activator: Option<EntityId>,
}

/// Request kinds the engine understands.
pub mod host_requests {
    /// Run script source. The payload is the source.
    pub const SCRIPT: &str = "script";
    /// Call a function a loaded script defined. The payload is its name.
    pub const SCRIPT_CALL: &str = "script_call";
    /// Load a script file. The payload is its name.
    pub const SCRIPT_FILE: &str = "script_file";
    /// Play a sound. The payload is its name; the caller says where it is.
    pub const PLAY_SOUND: &str = "play_sound";
    /// Stop whatever the caller started.
    pub const STOP_SOUND: &str = "stop_sound";
    /// Nudge a physics prop awake. The payload is ignored.
    pub const PHYS_WAKE: &str = "phys_wake";
    /// Put a physics prop to sleep. The payload is ignored.
    pub const PHYS_SLEEP: &str = "phys_sleep";
    /// Send the game UI an event. The payload is `name [data]`.
    pub const UI_EMIT: &str = "ui_emit";
    /// Publish a value to the game UI. The payload is `key value`.
    pub const UI_SET: &str = "ui_set";
    /// Show a UI layer. The payload is `layer file`.
    pub const UI_SHOW: &str = "ui_show";
    /// Hide a UI layer. The payload is its name.
    pub const UI_HIDE: &str = "ui_hide";
    /// Project a decal onto the nearest surface to the caller. The payload
    /// is `material size`.
    pub const PLACE_DECAL: &str = "place_decal";
    /// Something for the store -- Steam, or nothing. The payload is a
    /// platform action in its one-line form: `unlock ACH_X`,
    /// `add_stat kills 1`, `score best_time 5230 asc`.
    pub const PLATFORM: &str = "platform";
    /// A `logic_platform` spawned or was asked to `Refresh`: the engine
    /// answers by firing `OnAvailable` or `OnUnavailable` on the caller.
    pub const PLATFORM_STATUS: &str = "platform_status";
    /// Move to another map, carrying the player across. The payload is
    /// `map [landmark]`.
    pub const CHANGE_LEVEL: &str = "changelevel";
    /// Save the game. The payload is the save's name.
    pub const SAVE: &str = "save";
    /// Move the player to the caller, facing the way it faces.
    pub const TELEPORT_PLAYER: &str = "teleport_player";
    /// Hurt the player. The payload is `damage [radius]`: with a radius,
    /// only if the player is that close to the caller, and less the further
    /// away they are.
    pub const HURT_PLAYER: &str = "hurt_player";
    /// Heal the player, up to their maximum. The payload is the amount. The
    /// engine fires `OnPlayerHealed` on the caller and removes it if any
    /// health was given, and `OnHealthFull` if none was needed.
    pub const HEAL_PLAYER: &str = "heal_player";
    /// End the game: back to the main menu. The payload is ignored.
    pub const END_GAME: &str = "end_game";
}

impl EntityWorld {
    pub fn new(registry: Arc<ClassRegistry>) -> Self {
        EntityWorld {
            slots: Vec::new(),
            ecs: World::new(),
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
            requests: Vec::new(),
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
        let id = EntityId {
            index,
            generation: self.generations[index as usize],
        };
        let handle = self.spawn_components(classname);
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
            handle,
        });
        id
    }

    /// A new ECS entity holding the class's components at their defaults.
    pub(crate) fn spawn_components(&mut self, classname: &str) -> Handle {
        let handle = self.ecs.spawn_empty().id();
        let registry = self.registry.clone();
        for decl in registry.components(classname) {
            decl.insert(&mut self.ecs, handle);
        }
        handle
    }

    // ---- components ------------------------------------------------------

    /// An entity's `T`, if its class carries one.
    pub fn component<T: Component>(&self, id: EntityId) -> Option<&T> {
        self.ecs.get::<T>(self.get(id)?.handle)
    }

    /// An entity's `T`, to change, if its class carries one.
    pub fn component_mut<T: ClassComponent>(&mut self, id: EntityId) -> Option<&mut T> {
        let handle = self.get(id)?.handle;
        self.ecs.get_mut::<T>(handle).map(|c| c.into_inner())
    }

    /// Every component an entity carries, by type name, for a script, the
    /// console or a debugger to look through.
    pub fn components(&self, id: EntityId) -> Vec<(&'static str, &dyn Struct)> {
        let Some(e) = self.get(id) else {
            return Vec::new();
        };
        self.registry
            .components(&e.classname)
            .iter()
            .filter_map(|d| (d.reflect)(&self.ecs, e.handle).map(|s| (d.name, s)))
            .collect()
    }

    /// The component of an entity's class that has a field for `key`: one
    /// declared with that keyvalue, or failing that, one with a field of that
    /// name -- how a saved game from before a field moved into a component,
    /// or a script, names a field that has no keyvalue.
    pub(crate) fn claiming(
        &self,
        id: EntityId,
        key: &str,
    ) -> Option<(ComponentDecl, &'static str)> {
        let e = self.get(id)?;
        let decls = self.registry.components(&e.classname);
        decls
            .iter()
            .find_map(|d| d.field_for_key(key).map(|f| (d.clone(), f.name)))
            .or_else(|| {
                decls
                    .iter()
                    .find_map(|d| d.field_named(key).map(|f| (d.clone(), f.name)))
            })
    }

    /// An entity's value for a keyvalue, wherever it lives: the component
    /// field that claims it, or its loose fields.
    ///
    /// What the engine, scripts and I/O use to read a key without knowing
    /// the game's types.
    pub fn keyvalue(&self, id: EntityId, key: &str) -> Option<Value> {
        if let Some((decl, field)) = self.claiming(id, key) {
            let handle = self.get(id)?.handle;
            return (decl.reflect)(&self.ecs, handle).and_then(|s| kerosene_reflect::get(s, field));
        }
        self.get(id)?.fields.get(key).cloned()
    }

    /// Every keyvalue an entity has, loose or in a component: what a script
    /// or a dump shows. A component's field is under its key, or its own
    /// name when it has none (state the game keeps).
    pub fn keyvalues(&self, id: EntityId) -> Vec<(String, Value)> {
        let Some(e) = self.get(id) else {
            return Vec::new();
        };
        let mut all: Vec<(String, Value)> = e
            .fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        for decl in self.registry.components(&e.classname) {
            let Some(target) = (decl.reflect)(&self.ecs, e.handle) else {
                continue;
            };
            for field in decl.fields.iter() {
                if let Some(v) = kerosene_reflect::get(target, field.name) {
                    all.push((field.key.unwrap_or(field.name).to_string(), v));
                }
            }
        }
        all
    }

    /// [`EntityWorld::keyvalue`] as a number, or `default` when the entity
    /// has no such key or it does not convert.
    pub fn keyvalue_f32(&self, id: EntityId, key: &str, default: f32) -> f32 {
        self.keyvalue(id, key)
            .and_then(|v| v.as_f32())
            .unwrap_or(default)
    }

    /// [`EntityWorld::keyvalue`] as an integer, or `default`.
    pub fn keyvalue_i32(&self, id: EntityId, key: &str, default: i32) -> i32 {
        self.keyvalue(id, key)
            .and_then(|v| v.as_i32())
            .unwrap_or(default)
    }

    /// [`EntityWorld::keyvalue`] as a flag, or `default`.
    pub fn keyvalue_bool(&self, id: EntityId, key: &str, default: bool) -> bool {
        self.keyvalue(id, key)
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }

    /// [`EntityWorld::keyvalue`] as text, however it is typed; `None` only
    /// when the entity has no such key.
    pub fn keyvalue_text(&self, id: EntityId, key: &str) -> Option<String> {
        self.keyvalue(id, key).map(|v| match v {
            Value::Text(t) => t,
            other => other.to_string(),
        })
    }

    /// Whether an entity is switched off: the `startdisabled` keyvalue,
    /// which is also where a `Disable` input leaves it. What the engine
    /// asks of a brush or trigger without knowing the game's components.
    pub fn is_disabled(&self, id: EntityId) -> bool {
        self.keyvalue_bool(id, "startdisabled", false)
    }

    /// Set a keyvalue wherever it lives. A value that does not fit the
    /// component field that claims it is refused, with a warning, and
    /// changes nothing. Returns whether it was set.
    pub fn set_keyvalue(&mut self, id: EntityId, key: &str, value: Value) -> bool {
        if let Some((decl, field)) = self.claiming(id, key) {
            let Some(handle) = self.get(id).map(|e| e.handle) else {
                return false;
            };
            let Some(target) = (decl.reflect_mut)(&mut self.ecs, handle) else {
                return false;
            };
            return match kerosene_reflect::set(target, field, &value) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("{}: {e}", decl.name);
                    false
                }
            };
        }
        match self.get_mut(id) {
            Some(e) => {
                e.fields.set(key, value);
                true
            }
            None => false,
        }
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        let slot = self.slots.get(id.index as usize)?.as_ref()?;
        (slot.id.generation == id.generation).then_some(slot)
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        let slot = self.slots.get_mut(id.index as usize)?.as_mut()?;
        (slot.id.generation == id.generation).then_some(slot)
    }

    pub fn exists(&self, id: EntityId) -> bool {
        self.get(id).is_some()
    }

    /// Mark an entity for removal. The slot is reclaimed after the tick, so
    /// handlers mid-dispatch never find it vanished underneath them.
    pub fn remove(&mut self, id: EntityId) {
        if let Some(e) = self.get_mut(id) {
            e.pending_removal = true;
        }
    }

    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.slots.iter().filter_map(|s| s.as_ref())
    }

    pub fn ids(&self) -> Vec<EntityId> {
        self.slots
            .iter()
            .filter_map(|s| s.as_ref().map(|e| e.id))
            .collect()
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
        self.iter()
            .find(|e| e.classname.eq_ignore_ascii_case(classname))
            .map(|e| e.id)
    }

    /// Give an entity a name, or change the one it has.
    pub fn set_targetname(&mut self, id: EntityId, name: &str) {
        if let Some(old) = self
            .get(id)
            .and_then(|e| e.targetname())
            .map(str::to_lowercase)
            && let Some(list) = self.by_name.get_mut(&old)
        {
            list.retain(|&x| x != id);
        }
        if let Some(e) = self.get_mut(id) {
            e.fields.set("targetname", Value::Text(name.to_string()));
        }
        self.by_name
            .entry(name.to_lowercase())
            .or_default()
            .push(id);
    }

    // ---- loading ---------------------------------------------------------

    /// Create every entity in a compiled map's entity lump and run its spawn
    /// handler.
    ///
    /// `brush_bounds` is the map's brush models' bounds, indexed the way an
    /// entity's `model` key (`*1`, `*2`...) indexes them. Brush entities are
    /// given theirs as fields *before* spawn handlers run, because a class
    /// like `door` needs to know how far it travels, and that comes from
    /// the geometry rather than from a keyvalue.
    ///
    /// The lump and the bounds are handed in, rather than read out of a
    /// `.kbsp` here, so that entities do not depend on the world format:
    /// the engine, which loads both, joins them.
    pub fn load_from_lump(
        &mut self,
        entities: &KeyValues,
        brush_bounds: &[Aabb],
    ) -> Result<usize, SpawnError> {
        let created = self.create_entities(entities);

        for &id in &created {
            let Some(index) = self.get(id).and_then(|e| e.brush_model) else {
                continue;
            };
            let Some(bounds) = brush_bounds.get(index) else {
                continue;
            };
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

    /// Set one key on an entity the way a map sets it: `origin`, `angles`,
    /// `model` and `targetname` go where the engine reads them, and anything
    /// else becomes a field.
    pub fn apply_keyvalue(&mut self, id: EntityId, key: &str, value: &str) {
        match key.to_lowercase().as_str() {
            "classname" => {}
            "origin" => {
                if let Some(v) = Value::from_keyvalue(value).as_vec3()
                    && let Some(e) = self.get_mut(id)
                {
                    e.origin = v;
                }
            }
            "angles" => {
                if let Some(v) = Value::from_keyvalue(value).as_vec3()
                    && let Some(e) = self.get_mut(id)
                {
                    e.angles = Angles::new(v.x, v.y, v.z);
                }
            }
            "model" => {
                // `"*3"` names brush model 3; anything else is a
                // studio model path, which stays a plain field.
                if let Some(rest) = value.strip_prefix('*')
                    && let Ok(index) = rest.parse::<usize>()
                    && let Some(e) = self.get_mut(id)
                {
                    e.brush_model = Some(index);
                }
                if let Some(e) = self.get_mut(id) {
                    e.fields.set("model", Value::Text(value.to_string()));
                }
            }
            "targetname" => self.set_targetname(id, value),
            other => {
                if let Some((decl, field)) = self.claiming(id, other) {
                    let Some(handle) = self.get(id).map(|e| e.handle) else {
                        return;
                    };
                    if let Some(target) = (decl.reflect_mut)(&mut self.ecs, handle)
                        && let Err(e) = kerosene_reflect::set_from_text(target, field, value)
                    {
                        let classname = self.get(id).map_or("", |e| e.classname.as_str());
                        log::warn!("{classname}: keyvalue `{other}`: {e}");
                    }
                } else if let Some(e) = self.get_mut(id) {
                    e.fields.set(other, Value::from_keyvalue(value));
                }
            }
        }
    }

    /// Create one entity from keyvalues, as if it had been in the map, and
    /// run its spawn handler: what a game spawns at run time.
    ///
    /// An unregistered class still makes an entity -- the map loader does
    /// the same -- and says so, since it will do nothing.
    pub fn spawn_with(&mut self, classname: &str, keys: &[(&str, &str)]) -> EntityId {
        if !self.registry.is_registered(classname) {
            log::warn!("spawning `{classname}`, which no class is registered for");
        }
        let id = self.spawn(classname);
        for (key, value) in keys {
            if !key.eq_ignore_ascii_case("classname") {
                self.apply_keyvalue(id, key, value);
            }
        }
        self.run_spawn_handlers(&[id]);
        id
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
                self.apply_keyvalue(id, key, value);
            }

            if let Some(conn) = block.block("connections") {
                for (output, raw) in conn.pairs() {
                    match kerosene_kv::Connection::parse(output, raw) {
                        Ok(c) => {
                            if let Some(e) = self.get_mut(id) {
                                e.connections.push(c.into());
                            }
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
            let Some(classname) = self.get(*id).map(|e| e.classname.clone()) else {
                continue;
            };
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
        let Some(entity) = self.get(caller) else {
            return 0;
        };

        let mut queued = Vec::new();
        for (i, c) in entity.connections.iter().enumerate() {
            if !c.output.eq_ignore_ascii_case(output) {
                continue;
            }
            if c.is_exhausted() {
                continue;
            }
            queued.push((
                i,
                Target::parse(&c.target),
                c.input.clone(),
                parameter_override.unwrap_or(&c.parameter).to_string(),
                c.delay,
            ));
        }
        if queued.is_empty() {
            return 0;
        }

        for (index, target, input, parameter, delay) in &queued {
            if self.trace_enabled {
                let name = self
                    .get(caller)
                    .map(|e| e.classname.clone())
                    .unwrap_or_default();
                self.trace.push(format!(
                    "[{:.2}] {name} :: {output} -> {:?} :: {input}{}",
                    self.time,
                    target,
                    if *delay > 0.0 {
                        format!(" (+{delay:.2}s)")
                    } else {
                        String::new()
                    }
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
            if let Some(e) = self.get_mut(caller)
                && let Some(c) = e.connections.get_mut(*index)
                && c.times_to_fire > 0
            {
                c.times_to_fire -= 1;
            }
        }

        queued.len()
    }

    /// Deliver an input to one entity immediately.
    pub fn accept_input(&mut self, target: EntityId, event: &InputEvent) -> bool {
        // One killed earlier in the tick is gone as far as anyone wiring to
        // it can tell, although its slot is not reclaimed until the end.
        let Some(classname) = self
            .get(target)
            .filter(|e| !e.pending_removal)
            .map(|e| e.classname.clone())
        else {
            return false;
        };
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
    fn resolve(
        &self,
        target: &Target,
        activator: Option<EntityId>,
        caller: Option<EntityId>,
    ) -> Vec<EntityId> {
        match target {
            // Several entities may share a name, and firing at it fires all of
            // them -- which is how one wire opens six doors.
            Target::Named(name) => self.find_by_name(name),
            Target::Activator => activator
                .into_iter()
                .filter(|&id| self.exists(id))
                .collect(),
            Target::Caller => caller.into_iter().filter(|&id| self.exists(id)).collect(),
            Target::Myself => caller.into_iter().filter(|&id| self.exists(id)).collect(),
            Target::Player => self
                .player
                .into_iter()
                .filter(|&id| self.exists(id))
                .collect(),
            Target::Handle(id) => std::iter::once(*id).filter(|&id| self.exists(id)).collect(),
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
            if next.fire_at > self.time {
                break;
            }
            let event = self.queue.pop().expect("just peeked");

            let receivers = self.resolve(&event.target, event.activator, event.caller);
            if receivers.is_empty()
                && matches!(event.target, Target::Named(_))
                && let Target::Named(name) = &event.target
            {
                log::debug!("nothing named '{name}' to receive '{}'", event.input);
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
            match self.get_mut(id) {
                Some(e) if !e.pending_removal => e.next_think = None,
                // Killed earlier in the tick, perhaps by the think before.
                _ => continue,
            }
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
            if let Some(name) = self
                .get(id)
                .and_then(|e| e.targetname())
                .map(str::to_lowercase)
                && let Some(list) = self.by_name.get_mut(&name)
            {
                list.retain(|&x| x != id);
            }
            let index = id.index as usize;
            if let Some(e) = self.slots[index].take() {
                self.ecs.despawn(e.handle);
            }
            // Bumping the generation is what makes stale handles fail to
            // resolve instead of addressing whoever moves into this slot.
            self.generations[index] = self.generations[index].wrapping_add(1);
            self.free.push(id.index);
            if self.player == Some(id) {
                self.player = None;
            }
        }
    }

    /// Schedule a think for `delay` seconds from now.
    pub fn set_think_delay(&mut self, id: EntityId, delay: f32) {
        let at = self.time + delay.max(0.0);
        if let Some(e) = self.get_mut(id) {
            e.next_think = Some(at);
        }
    }

    pub fn clear_think(&mut self, id: EntityId) {
        if let Some(e) = self.get_mut(id) {
            e.next_think = None;
        }
    }

    pub fn pending_event_count(&self) -> usize {
        self.queue.len()
    }

    // ---- host requests ---------------------------------------------------

    /// Ask the engine to do something the entity world cannot do itself.
    pub fn request(
        &mut self,
        kind: &str,
        payload: impl Into<String>,
        caller: EntityId,
        activator: Option<EntityId>,
    ) {
        self.requests.push(HostRequest {
            kind: kind.to_string(),
            payload: payload.into(),
            caller,
            activator,
        });
    }

    /// Take everything entities have asked the engine for.
    pub fn take_requests(&mut self) -> Vec<HostRequest> {
        std::mem::take(&mut self.requests)
    }

    pub fn pending_requests(&self) -> usize {
        self.requests.len()
    }

    pub fn set_trace(&mut self, on: bool) {
        self.trace_enabled = on;
        if !on {
            self.trace.clear();
        }
    }

    pub fn trace_lines(&self) -> &[String] {
        &self.trace
    }
    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "world/components_tests.rs"]
mod components_tests;
