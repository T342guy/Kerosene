// SPDX-License-Identifier: LGPL-3.0-or-later
//! Scripting: the layer above entity I/O.
//!
//! Entity outputs wired to inputs compose further than they have any right
//! to, and most of a level is built that way. But some things are not a
//! graph. Counting, arithmetic, "pick one of these three at random", "do this
//! only if the player still has the crowbar" -- expressing those as relays and
//! counters is possible and miserable, and it is the point at which every
//! engine in this lineage grew a script VM.
//!
//! This is that layer. It is deliberately *not* a second way to write the
//! engine: a script cannot allocate an entity slot, walk the BSP tree, or
//! touch the renderer. It reads a snapshot of the world and returns a list of
//! things it would like done.
//!
//! # Why a snapshot and a queue
//!
//! The obvious design hands the script a live `&mut EntityWorld`. It cannot
//! be done safely -- script functions outlive the call that registered them,
//! so the borrow would have to be `'static` -- and it should not be done
//! anyway. A script that mutates the world halfway through a frame can
//! observe the world in a state no other code ever sees, which is how the
//! hard-to-reproduce bugs get in. Reading a snapshot and returning
//! [`ScriptAction`]s means a script run is a pure function of the world, and
//! the same script run twice on the same world does the same thing.
//!
//! ```
//! # use void_script::{ScriptHost, WorldView, EntityView, ScriptAction};
//! let mut host = ScriptHost::new();
//! let mut view = WorldView::default();
//! view.entities.push(EntityView::new(1, "func_door").with_name("gate"));
//!
//! host.set_view(view);
//! host.run(r#" ent_fire("gate", "Open"); "#).unwrap();
//!
//! assert!(matches!(host.take_actions().as_slice(), [ScriptAction::FireInput { .. }]));
//! ```

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use void_math::Vec3;

mod bindings;
mod view;

pub use bindings::{ID_TARGET_PREFIX, parse_id_target};
pub use view::{EntityView, WorldView};

/// The extension a script file uses.
pub const EXTENSION: &str = "voidscript";

/// Well-known entry points the engine calls when a script defines them.
pub mod hooks {
    /// Called once, after every entity in the map has spawned.
    pub const MAP_START: &str = "on_map_start";
    /// Called every tick, with the tick length in seconds.
    pub const TICK: &str = "on_tick";
}

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("{0}")]
    Compile(String),
    #[error("{0}")]
    Runtime(String),
    #[error("no function named `{0}`")]
    NoSuchFunction(String),
}

/// Severity a script asked for when it logged something.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptLevel {
    Print,
    Warn,
    Error,
}

/// Something a script would like the engine to do.
///
/// Every effect a script can have is one of these. That is the whole security
/// and sanity story: the list is short, it is auditable, and adding to it is a
/// deliberate act rather than a consequence of exposing a type.
#[derive(Clone, PartialEq, Debug)]
pub enum ScriptAction {
    /// Write a line to the console.
    Log(ScriptLevel, String),
    /// Run console text, exactly as if typed.
    Command(String),
    /// Fire an entity input, the same path an output takes.
    FireInput {
        target: String,
        input: String,
        parameter: String,
        delay: f32,
    },
    /// Set a keyvalue on an entity.
    SetField { entity: u64, key: String, value: String },
    /// Move an entity.
    SetOrigin { entity: u64, origin: Vec3 },
    /// Remove an entity.
    Kill { entity: u64 },
}

/// The shared state script functions read and write.
///
/// `Rc<RefCell<_>>` rather than a lock: rhai is single-threaded here by
/// design, and a script that could run on another thread while the world
/// ticked would be a much larger promise than this makes.
#[derive(Default, Debug)]
pub(crate) struct Shared {
    pub view: WorldView,
    pub actions: Vec<ScriptAction>,
}

/// How many actions one script run may queue before it is cut off.
///
/// A runaway loop calling `ent_fire` is the script equivalent of an entity
/// I/O loop, and the engine already refuses to dispatch forever for the same
/// reason.
pub const MAX_ACTIONS: usize = 4096;

/// A script VM with the engine's bindings in it.
pub struct ScriptHost {
    engine: rhai::Engine,
    /// Compiled top-level scripts, kept so functions stay callable after the
    /// file that defined them has been run.
    module: rhai::AST,
    scope: rhai::Scope<'static>,
    shared: Rc<RefCell<Shared>>,
    /// Names of the files loaded into `module`, for `script_reload`.
    loaded: Vec<String>,
}

impl Default for ScriptHost {
    fn default() -> Self { ScriptHost::new() }
}

impl ScriptHost {
    pub fn new() -> ScriptHost {
        let shared = Rc::new(RefCell::new(Shared::default()));
        let mut engine = rhai::Engine::new();

        // Bounds, not trust. A level's scripts are content, and content is
        // edited by people who make mistakes; an infinite loop should stop the
        // script rather than the game.
        engine.set_max_operations(2_000_000);
        engine.set_max_call_levels(64);
        engine.set_max_expr_depths(128, 64);
        engine.set_max_string_size(64 * 1024);
        engine.set_max_array_size(16 * 1024);
        // No file system and no module loading from inside a script: `import`
        // would be a way around every bound above.
        engine.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());

        bindings::register(&mut engine, &shared);

        ScriptHost {
            engine,
            module: rhai::AST::empty(),
            scope: rhai::Scope::new(),
            shared,
            loaded: Vec::new(),
        }
    }

    /// Replace the world the next script run will see.
    pub fn set_view(&mut self, view: WorldView) {
        self.shared.borrow_mut().view = view;
    }

    /// Everything the scripts have asked for since the last call.
    pub fn take_actions(&mut self) -> Vec<ScriptAction> {
        std::mem::take(&mut self.shared.borrow_mut().actions)
    }

    /// Names of the files currently loaded.
    pub fn loaded(&self) -> &[String] { &self.loaded }

    /// Whether a function of this name is defined.
    pub fn has_function(&self, name: &str) -> bool {
        self.module.iter_functions().any(|f| f.name == name)
    }

    /// How many parameters a function takes, if it is defined.
    ///
    /// The engine uses this to decide whether to hand a hook the name of
    /// whatever called it. Writing `fn on_use()` and `fn on_use(who)` should
    /// both work, and requiring the unused parameter would be the kind of
    /// papercut that makes people stop writing scripts.
    pub fn function_arity(&self, name: &str) -> Option<usize> {
        self.module.iter_functions().find(|f| f.name == name).map(|f| f.params.len())
    }

    /// Forget every loaded script and everything they defined.
    pub fn clear(&mut self) {
        self.module = rhai::AST::empty();
        self.scope = rhai::Scope::new();
        self.loaded.clear();
    }

    /// Compile a script and keep its functions and top-level state.
    ///
    /// `name` is what errors are reported against. Loading a file twice
    /// replaces what it defined rather than stacking a second copy, so
    /// reloading during development does what it looks like it does.
    pub fn load(&mut self, name: &str, source: &str) -> Result<(), ScriptError> {
        let ast = self
            .engine
            .compile(source)
            .map_err(|e| ScriptError::Compile(format!("{name}: {e}")))?;

        self.engine
            .run_ast_with_scope(&mut self.scope, &ast)
            .map_err(|e| ScriptError::Runtime(format!("{name}: {e}")))?;

        // Later definitions win, which is what makes a reload a reload.
        self.module = self.module.merge(&ast);
        if !self.loaded.iter().any(|f| f == name) {
            self.loaded.push(name.to_string());
        }
        Ok(())
    }

    /// Evaluate a snippet, as typed at the console.
    ///
    /// Runs against the same scope and functions the loaded files created, so
    /// `script my_function()` works, and so does poking at a variable a script
    /// set up.
    pub fn run(&mut self, source: &str) -> Result<Option<String>, ScriptError> {
        let ast = self
            .engine
            .compile_with_scope(&self.scope, source)
            .map_err(|e| ScriptError::Compile(e.to_string()))?;
        let combined = self.module.merge(&ast);

        let value: rhai::Dynamic = self
            .engine
            .eval_ast_with_scope(&mut self.scope, &combined)
            .map_err(|e| ScriptError::Runtime(e.to_string()))?;

        Ok((!value.is_unit()).then(|| value.to_string()))
    }

    /// Call a function a loaded script defined.
    ///
    /// A missing function is [`ScriptError::NoSuchFunction`] rather than a
    /// silent no-op, because the engine calls hooks by name and "the hook did
    /// nothing" and "the hook is not there" need telling apart.
    pub fn call(&mut self, name: &str, args: Vec<rhai::Dynamic>) -> Result<(), ScriptError> {
        if !self.has_function(name) {
            return Err(ScriptError::NoSuchFunction(name.to_string()));
        }
        self.engine
            .call_fn::<rhai::Dynamic>(&mut self.scope, &self.module, name, args)
            .map(|_| ())
            .map_err(|e| ScriptError::Runtime(format!("{name}: {e}")))
    }

    /// Call a hook if the scripts defined one. Missing hooks are normal.
    pub fn call_hook(&mut self, name: &str, args: Vec<rhai::Dynamic>) -> Result<(), ScriptError> {
        match self.call(name, args) {
            Err(ScriptError::NoSuchFunction(_)) => Ok(()),
            other => other,
        }
    }
}

/// Fields on a snapshot entity, in a stable order so iteration is repeatable.
pub type Fields = BTreeMap<String, String>;

#[cfg(test)]
mod tests;
