//! Entity classes and their handlers.
//!
//! Source splits its engine from its game DLL: the engine routes inputs and
//! runs think functions, and the game decides what `func_door` means. The same
//! split here means `void-entity` never mentions a game concept, and a mod can
//! register its own classes without touching the engine.

use crate::world::{EntityId, EntityWorld};
use std::collections::HashMap;

/// Called once when an entity is created from the map.
pub type SpawnHandler = fn(&mut EntityWorld, EntityId);

/// Called when the entity's scheduled think time arrives.
pub type ThinkHandler = fn(&mut EntityWorld, EntityId);

/// Called when an input is delivered. Returns whether it was handled, so that
/// an unhandled input can be reported rather than silently swallowed.
pub type InputHandler = fn(&mut EntityWorld, EntityId, &crate::io::InputEvent) -> bool;

/// Everything the engine needs to know about one entity class.
pub struct ClassDef {
    pub classname: &'static str,
    pub spawn: Option<SpawnHandler>,
    pub think: Option<ThinkHandler>,
    /// Input name to handler. Names are matched case-insensitively, because
    /// map files spell them inconsistently.
    pub inputs: Vec<(&'static str, InputHandler)>,
}

impl ClassDef {
    pub fn new(classname: &'static str) -> Self {
        ClassDef { classname, spawn: None, think: None, inputs: Vec::new() }
    }

    pub fn on_spawn(mut self, f: SpawnHandler) -> Self {
        self.spawn = Some(f);
        self
    }

    pub fn on_think(mut self, f: ThinkHandler) -> Self {
        self.think = Some(f);
        self
    }

    pub fn input(mut self, name: &'static str, f: InputHandler) -> Self {
        self.inputs.push((name, f));
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
}

impl ClassRegistry {
    pub fn new() -> Self { Self::default() }

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

    pub fn spawn_handler(&self, classname: &str) -> Option<SpawnHandler> {
        self.get(classname).and_then(|c| c.spawn)
    }

    pub fn think_handler(&self, classname: &str) -> Option<ThinkHandler> {
        self.get(classname).and_then(|c| c.think)
    }
}
