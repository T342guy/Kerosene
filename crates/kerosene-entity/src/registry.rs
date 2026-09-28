// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Entity classes and their handlers.
//!
//! Source splits its engine from its game DLL: the engine routes inputs and
//! runs think functions, and the game decides what `func_door` means. The same
//! split here means `kerosene-entity` never mentions a game concept, and a mod can
//! register its own classes without touching the engine.

use crate::world::{EntityId, EntityWorld};
use std::collections::HashMap;

/// Called once when an entity is created from the map.
pub type SpawnHandler = fn(&mut EntityWorld, EntityId);

/// Called when the entity's scheduled think time arrives.
pub type ThinkHandler = fn(&mut EntityWorld, EntityId);

/// Called for each entity when a saved game puts it back, instead of its
/// spawn handler. Its fields are already what they were when the game was
/// saved; this is for anything that lived outside them -- a looping sound
/// that has to be asked for again, say.
pub type RestoreHandler = fn(&mut EntityWorld, EntityId);

/// Called when an input is delivered. Returns whether it was handled, so that
/// an unhandled input can be reported rather than silently swallowed.
pub type InputHandler = fn(&mut EntityWorld, EntityId, &crate::io::InputEvent) -> bool;

/// Called on the tick the player starts touching the entity: walking into a
/// pickup, brushing a hazard. The last argument is the player's entity.
///
/// Where a trigger fires outputs for a designer to wire, this is the
/// class's own reaction, for behaviour that belongs to the thing itself.
/// The engine tests the player's box against the entity's brush model, or
/// for a point entity against a cube `touch_size` units across (32 unless
/// the entity says).
pub type TouchHandler = fn(&mut EntityWorld, EntityId, EntityId);

/// Called when something damages the entity: a shot, an explosion, a
/// `point_hurt`. The amount is positive; the attacker, when there is one, is
/// who did it. Returns whether the entity took it, so a game can tell a hit
/// that mattered from one that did not.
pub type DamageHandler = fn(&mut EntityWorld, EntityId, f32, Option<EntityId>) -> bool;

/// What the engine does with an entity's `model` key.
///
/// The engine draws, animates and collides models, and the game decides
/// which of its classes have one. A class with no role has its `model` key
/// ignored, which is right for the many classes (a sound, a trigger) that
/// never have one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelRole {
    /// Drawn and collided with, and never moves: `prop_static`.
    Static,
    /// A rigid body the physics simulates: `prop_physics`.
    Physics,
    /// Drawn with its skeleton posed, and moved by entity I/O:
    /// `prop_dynamic`. Collided with as it stands.
    Animated,
}

impl ModelRole {
    /// The role of the stock prop classes, by name, for a registry that does
    /// not declare them -- an engine made with no game, in a test.
    pub fn of_stock_class(classname: &str) -> Option<ModelRole> {
        [
            ("prop_static", ModelRole::Static),
            ("prop_physics", ModelRole::Physics),
            ("prop_dynamic", ModelRole::Animated),
        ]
        .into_iter()
        .find(|(name, _)| classname.eq_ignore_ascii_case(name))
        .map(|(_, role)| role)
    }
}

/// Everything the engine needs to know about one entity class.
///
/// Built with [`ClassDef::new`] and its builder methods; new fields arrive
/// as new builder methods, so it cannot be written as a struct literal.
#[non_exhaustive]
pub struct ClassDef {
    pub classname: &'static str,
    pub spawn: Option<SpawnHandler>,
    pub think: Option<ThinkHandler>,
    pub restore: Option<RestoreHandler>,
    /// Input name to handler. Names are matched case-insensitively, because
    /// map files spell them inconsistently.
    pub inputs: Vec<(&'static str, InputHandler)>,
    /// The outputs this class fires.
    ///
    /// Declared rather than inferred, because an output is just a string
    /// passed to [`EntityWorld::fire_output`](crate::EntityWorld::fire_output)
    /// and nothing else would know the set. Listing them keeps the editor's
    /// schema honest: a test checks the two against each other, so adding an
    /// output to the game and forgetting to offer it in Chisel is a build
    /// failure rather than a wiring session that silently does nothing.
    pub outputs: Vec<&'static str>,
    /// What the engine does with the entity's `model` key. See
    /// [`ClassDef::model`].
    pub model: Option<ModelRole>,
    /// What it does when the player walks into it. See [`TouchHandler`].
    pub touch: Option<TouchHandler>,
    /// What it does when it is damaged. See [`DamageHandler`].
    pub damage: Option<DamageHandler>,
}

impl ClassDef {
    pub fn new(classname: &'static str) -> Self {
        ClassDef {
            classname,
            spawn: None,
            think: None,
            restore: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            model: None,
            touch: None,
            damage: None,
        }
    }

    /// React to the player walking into it. See [`TouchHandler`].
    pub fn on_touch(mut self, f: TouchHandler) -> Self {
        self.touch = Some(f);
        self
    }

    /// React to being damaged. See [`DamageHandler`].
    pub fn on_damage(mut self, f: DamageHandler) -> Self {
        self.damage = Some(f);
        self
    }

    /// Give the class a model the engine draws and collides with, in the
    /// given role: a game's `npc_*` or `item_*` with a `model` key is drawn
    /// the way a stock prop is.
    pub fn model(mut self, role: ModelRole) -> Self {
        self.model = Some(role);
        self
    }

    pub fn on_spawn(mut self, f: SpawnHandler) -> Self {
        self.spawn = Some(f);
        self
    }

    pub fn on_think(mut self, f: ThinkHandler) -> Self {
        self.think = Some(f);
        self
    }

    /// What to do when a saved game brings this entity back. Most classes
    /// need nothing: everything they know is in their fields.
    pub fn on_restore(mut self, f: RestoreHandler) -> Self {
        self.restore = Some(f);
        self
    }

    pub fn input(mut self, name: &'static str, f: InputHandler) -> Self {
        self.inputs.push((name, f));
        self
    }

    /// Declare an output this class fires.
    pub fn output(mut self, name: &'static str) -> Self {
        self.outputs.push(name);
        self
    }

    pub fn find_input(&self, name: &str) -> Option<InputHandler> {
        self.inputs
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, f)| *f)
    }
}

/// Every class the game has registered.
#[derive(Default)]
pub struct ClassRegistry {
    classes: HashMap<String, ClassDef>,
    /// Inputs every entity understands, whatever its class.
    common: Vec<(&'static str, InputHandler)>,
    /// Outputs every entity may fire.
    common_outputs: Vec<&'static str>,
}

impl ClassRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, def: ClassDef) -> &mut Self {
        self.classes.insert(def.classname.to_lowercase(), def);
        self
    }

    /// Register an input handled by every entity -- `Kill`, `AddOutput` and
    /// friends, which Source makes universal.
    pub fn register_common_input(&mut self, name: &'static str, f: InputHandler) -> &mut Self {
        self.common.push((name, f));
        self
    }

    /// Declare an output every entity may fire, whatever its class.
    pub fn register_common_output(&mut self, name: &'static str) -> &mut Self {
        self.common_outputs.push(name);
        self
    }

    /// Inputs handled by every entity, in registration order.
    pub fn common_inputs(&self) -> Vec<&'static str> {
        self.common.iter().map(|(n, _)| *n).collect()
    }

    /// Outputs every entity may fire, in registration order.
    pub fn common_outputs(&self) -> Vec<&'static str> {
        self.common_outputs.clone()
    }

    pub fn get(&self, classname: &str) -> Option<&ClassDef> {
        self.classes.get(&classname.to_lowercase())
    }

    pub fn is_registered(&self, classname: &str) -> bool {
        self.classes.contains_key(&classname.to_lowercase())
    }

    pub fn class_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.classes.values().map(|c| c.classname).collect();
        names.sort_unstable();
        names
    }

    /// Find the handler for an input, checking the class first and then the
    /// common set.
    pub fn find_input(&self, classname: &str, input: &str) -> Option<InputHandler> {
        if let Some(handler) = self.get(classname).and_then(|c| c.find_input(input)) {
            return Some(handler);
        }
        self.common
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(input))
            .map(|(_, f)| *f)
    }

    /// What the engine does with an entity of this class's `model` key:
    /// the role the class declared, or for a class nobody registered, the
    /// stock prop classes' by name.
    pub fn model_role(&self, classname: &str) -> Option<ModelRole> {
        match self.get(classname) {
            Some(def) => def.model.or_else(|| ModelRole::of_stock_class(classname)),
            None => ModelRole::of_stock_class(classname),
        }
    }

    pub fn spawn_handler(&self, classname: &str) -> Option<SpawnHandler> {
        self.get(classname).and_then(|c| c.spawn)
    }

    pub fn think_handler(&self, classname: &str) -> Option<ThinkHandler> {
        self.get(classname).and_then(|c| c.think)
    }

    pub fn restore_handler(&self, classname: &str) -> Option<RestoreHandler> {
        self.get(classname).and_then(|c| c.restore)
    }

    pub fn touch_handler(&self, classname: &str) -> Option<TouchHandler> {
        self.get(classname).and_then(|c| c.touch)
    }

    pub fn damage_handler(&self, classname: &str) -> Option<DamageHandler> {
        self.get(classname).and_then(|c| c.damage)
    }
}
