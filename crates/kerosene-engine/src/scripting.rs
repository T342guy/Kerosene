// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Wiring the script VM to the running game.
//!
//! `kerosene-script` deliberately knows nothing about the engine: it reads a
//! [`WorldView`] and returns [`ScriptAction`]s. This is the translation on
//! both sides, and it is the only place a script's reach is decided.
//!
//! Two details are load-bearing.
//!
//! A script's handle on an entity carries the slot's *generation* as well as
//! its index, so a handle held across a death cannot come back pointing at
//! whatever moved into the slot. That is the same protection the engine's own
//! [`EntityId`] gives every queued event, extended to the one place a
//! long-lived reference can escape into.
//!
//! And actions are applied through the paths a map file would take. Firing an
//! input goes through the event queue, so a script fires things in the same
//! order, with the same delays, as an output wired in the editor. Setting a
//! convar goes through the console, so cheat protection is enforced in one
//! place. Nothing here is a back door around the rules the rest of the engine
//! plays by.

use crate::engine::Engine;
use kerosene_entity::{EntityId, Target};
use kerosene_script::{EntityView, ScriptAction, ScriptLevel, WorldView};

/// Where a map's script lives, given the map's name.
pub fn script_path(name: &str) -> String {
    format!("scripts/{name}.{}", kerosene_script::EXTENSION)
}

/// Pack an entity handle into something a script can hold.
pub fn pack(id: EntityId) -> u64 {
    ((id.generation as u64) << 32) | id.index as u64
}

/// Read a handle back. The generation is checked by the caller, through
/// [`kerosene_entity::EntityWorld::exists`].
pub fn unpack(packed: u64) -> EntityId {
    EntityId { index: packed as u32, generation: (packed >> 32) as u32 }
}

impl Engine {
    /// A snapshot of the world for a script to read.
    ///
    /// Built fresh for each run rather than kept up to date, because a script
    /// run is meant to be a pure function of the world at one instant. It is
    /// O(entities), which is why it happens when a script actually runs and
    /// not every tick regardless.
    pub fn script_view(&self) -> WorldView {
        let mut view = WorldView {
            time: self.time,
            tick: self.tick_count,
            map: self.level.as_ref().map(|l| l.name.clone()).unwrap_or_default(),
            ..Default::default()
        };

        for entity in self.entities.iter() {
            view.entities.push(entity_view(entity));
        }
        for cvar in self.console.cvars() {
            view.cvars.insert(cvar.name.clone(), cvar.string().to_string());
        }
        view.player = self
            .player
            .entity
            .and_then(|id| self.entities.get(id))
            .map(entity_view);

        view
    }

    /// Run script source against the current world, and apply what it asks
    /// for.
    pub fn run_script(&mut self, source: &str) -> Result<Option<String>, kerosene_script::ScriptError> {
        let view = self.script_view();
        self.script.set_view(view);
        let result = self.script.run(source);
        // Actions are applied whether or not the script finished: a script
        // that fires a door and then throws has already fired the door as far
        // as anyone watching is concerned, and swallowing that would be a
        // stranger rule than applying it.
        let actions = self.script.take_actions();
        self.apply_script_actions(actions);
        result
    }

    /// Call a script function, if it is defined, against the current world.
    pub fn call_script_hook(&mut self, name: &str, args: Vec<rhai::Dynamic>) {
        if !self.script.has_function(name) { return }
        let view = self.script_view();
        self.script.set_view(view);
        let result = self.script.call_hook(name, args);
        let actions = self.script.take_actions();
        self.apply_script_actions(actions);
        if let Err(e) = result {
            self.console.error(format!("script: {e}"));
        }
    }

    /// Load a script file from the content tree.
    pub fn load_script(&mut self, name: &str) -> anyhow::Result<()> {
        let path = if name.contains('/') || name.contains('.') {
            name.to_string()
        } else {
            script_path(name)
        };
        let source = self
            .vfs
            .read_string(&path)
            .map_err(|e| anyhow::anyhow!("could not read {path}: {e}"))?;
        self.script
            .load(&path, &source)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        // Loading runs the file's top level, which may already have asked for
        // things.
        let actions = self.script.take_actions();
        self.apply_script_actions(actions);
        self.console.print(format!("loaded {path}"));
        Ok(())
    }

    /// Forget every loaded script and load the same files again.
    pub fn reload_scripts(&mut self) {
        let files: Vec<String> = self.script.loaded().to_vec();
        self.script.clear();
        for file in files {
            if let Err(e) = self.load_script(&file) {
                self.console.error(format!("script_reload: {e}"));
            }
        }
    }

    /// Load the script belonging to a map, if it has one.
    ///
    /// A map without one is the normal case and says nothing.
    pub fn load_map_script(&mut self, map: &str) {
        self.script.clear();
        let path = script_path(map);
        let Ok(source) = self.vfs.read_string(&path) else { return };
        match self.script.load(&path, &source) {
            Ok(()) => {
                let actions = self.script.take_actions();
                self.apply_script_actions(actions);
                self.console.print(format!("loaded {path}"));
            }
            Err(e) => self.console.error(format!("script: {e}")),
        }
    }

    /// Do what a script asked for.
    pub fn apply_script_actions(&mut self, actions: Vec<ScriptAction>) {
        for action in actions {
            match action {
                ScriptAction::Log(level, text) => match level {
                    ScriptLevel::Print => self.console.print(text),
                    ScriptLevel::Warn => self.console.warn(text),
                    ScriptLevel::Error => self.console.error(text),
                },
                // Queued rather than executed: a script running inside a
                // command must not run more commands underneath it.
                ScriptAction::Command(text) => self.console.enqueue(text),
                ScriptAction::FireInput { target, input, parameter, delay } => {
                    let target = match kerosene_script::parse_id_target(&target) {
                        Some(packed) => Target::Handle(unpack(packed)),
                        None => Target::Named(target),
                    };
                    self.entities.queue_input(target, &input, &parameter, delay, None, None);
                }
                ScriptAction::SetField { entity, key, value } => {
                    let id = unpack(entity);
                    if let Some(e) = self.entities.get_mut(id) {
                        e.fields.set(&key, kerosene_entity::Value::Text(value));
                    }
                }
                ScriptAction::SetOrigin { entity, origin } => {
                    let id = unpack(entity);
                    if let Some(e) = self.entities.get_mut(id) {
                        e.origin = origin;
                        e.fields.set("origin", kerosene_entity::Value::Vector(origin));
                    }
                }
                ScriptAction::Kill { entity } => self.entities.remove(unpack(entity)),
                ScriptAction::PlaySound { name, position, volume } => {
                    let vfs = self.vfs.clone();
                    if self.audio.play(&vfs, &name, position, volume).is_none() {
                        self.console.warn(format!("script: could not play `{name}`"));
                    }
                }
                ScriptAction::StopAllSounds => self.audio.stop_all(),
            }
        }
    }
}

impl Engine {
    /// Act on everything entities asked for this tick.
    ///
    /// Drained after `EntityWorld::run` rather than during it, so a script
    /// cannot run in the middle of event dispatch and see the world half way
    /// through a tick.
    pub fn take_entity_requests(&mut self) {
        for request in self.entities.take_requests() {
            let caller = self
                .entities
                .get(request.caller)
                .and_then(|e| e.targetname())
                .unwrap_or_default()
                .to_string();

            match request.kind.as_str() {
                kerosene_entity::host_requests::SCRIPT => {
                    if let Err(e) = self.run_script(&request.payload) {
                        self.console.error(format!("logic_script: {e}"));
                    }
                }
                kerosene_entity::host_requests::SCRIPT_CALL => {
                    let name = request.payload;
                    if !self.script.has_function(&name) {
                        self.console.error(format!("logic_script: no function named `{name}`"));
                        continue;
                    }
                    // A hook may take the caller's name or take nothing;
                    // both spellings work.
                    let args = match self.script.function_arity(&name) {
                        Some(1) => vec![rhai::Dynamic::from(caller)],
                        _ => vec![],
                    };
                    self.call_script_hook(&name, args);
                }
                kerosene_entity::host_requests::SCRIPT_FILE => {
                    if let Err(e) = self.load_script(&request.payload) {
                        self.console.error(format!("logic_script: {e}"));
                    }
                }
                kerosene_entity::host_requests::PLAY_SOUND => {
                    self.play_entity_sound(request.caller, &request.payload);
                }
                kerosene_entity::host_requests::STOP_SOUND => {
                    self.stop_entity_sound(request.caller);
                }
                kerosene_entity::host_requests::PHYS_WAKE => {
                    if !self.physics.wake(request.caller) {
                        self.console.warn("Wake: not a physics prop");
                    }
                }
                kerosene_entity::host_requests::PHYS_SLEEP => {
                    if !self.physics.sleep(request.caller) {
                        self.console.warn("Sleep: not a physics prop");
                    }
                }
                other => self.console.warn(format!("unknown entity request `{other}`")),
            }
        }
    }
}

impl Engine {
    /// Start a sound belonging to an entity, at wherever that entity is.
    ///
    /// The handle is remembered on the entity so `StopSound` can find it
    /// again. Without that, a looping ambience could be started but never
    /// silenced, which is the worst of the two failure modes.
    pub fn play_entity_sound(&mut self, id: kerosene_entity::EntityId, name: &str) {
        let Some(entity) = self.entities.get(id) else { return };
        let everywhere = entity.has_spawnflag(kerosene_game::sound::SF_EVERYWHERE);
        let origin = entity.origin;
        // `volume` is what it is called. `health` is what Source calls it, and
        // is still read so a map that says so is not silently ignored.
        let volume = entity
            .fields
            .f32("volume", entity.fields.f32("health", 1.0))
            .clamp(0.0, 1.0);
        let radius = entity.fields.f32("radius", 0.0);
        let pitch = entity.fields.f32("pitch", 1.0).max(0.01);
        let looping = entity.fields.bool("looping", true);

        let vfs = self.vfs.clone();
        let Some(sound) = self.audio.sound(&vfs, name) else { return };
        let (_, mut params) = self.audio.bank.resolve(name);
        params.position = (!everywhere).then_some(origin);
        params.volume *= volume;
        params.pitch *= pitch;
        params.looping = looping;
        if radius > 0.0 {
            params.max_distance = radius;
            params.reference_distance = (radius * 0.1).max(16.0);
        }

        let handle = self.audio.with_mixer(|mixer| mixer.play(sound, params));
        if let Some(e) = self.entities.get_mut(id) {
            e.fields.set("__voice", kerosene_entity::Value::Int(handle.0 as i32));
        }
    }

    /// Stop whatever an entity started.
    pub fn stop_entity_sound(&mut self, id: kerosene_entity::EntityId) {
        let handle = self.entities.get(id).map(|e| e.fields.i32("__voice", 0)).unwrap_or(0);
        if handle > 0 {
            self.audio.stop(kerosene_audio::SoundHandle(handle as u64));
        }
        if let Some(e) = self.entities.get_mut(id) {
            e.fields.set("__voice", kerosene_entity::Value::Int(0));
        }
    }
}

fn entity_view(entity: &kerosene_entity::Entity) -> EntityView {
    let mut view = EntityView {
        id: pack(entity.id),
        classname: entity.classname.clone(),
        targetname: entity.targetname().unwrap_or_default().to_string(),
        origin: entity.origin,
        ..Default::default()
    };
    for (key, value) in entity.fields.iter() {
        view.fields.insert(key.to_string(), value.to_string());
    }
    view
}
