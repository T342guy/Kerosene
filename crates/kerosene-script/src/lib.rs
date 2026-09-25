// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
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
//! # use kerosene_script::{ScriptHost, WorldView, EntityView, ScriptAction};
//! let mut host = ScriptHost::new();
//! let mut view = WorldView::default();
//! view.entities.push(EntityView::new(1, "func_door").with_name("gate"));
//!
//! host.set_view(view);
//! host.run(r#" ent_fire("gate", "Open"); "#).unwrap();
//!
//! assert!(matches!(host.take_actions().as_slice(), [ScriptAction::FireInput { .. }]));
//! ```

use kerosene_math::Vec3;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

mod bindings;
mod view;

pub use bindings::{ID_TARGET_PREFIX, parse_id_target};
pub use view::{EntityView, WorldView};

/// The extension a script file uses.
pub const EXTENSION: &str = "keroscript";

/// Well-known entry points the engine calls when a script defines them.
pub mod hooks {
    /// Called once, after every entity in the map has spawned.
    pub const MAP_START: &str = "on_map_start";
    /// Called every tick, with the tick length in seconds.
    pub const TICK: &str = "on_tick";
    /// Called for each store event -- an achievement unlocked, a score
    /// posted, the overlay opened -- with its name and data.
    pub const PLATFORM_EVENT: &str = "on_platform_event";
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
    SetField {
        entity: u64,
        key: String,
        value: String,
    },
    /// Move an entity.
    SetOrigin { entity: u64, origin: Vec3 },
    /// Remove an entity.
    Kill { entity: u64 },
    /// Play a sound, at a position or heard flat.
    PlaySound {
        name: String,
        position: Option<Vec3>,
        volume: f32,
    },
    /// Stop every sound.
    StopAllSounds,
    /// Publish a value to the game UI.
    UiSet { key: String, value: String },
    /// Send the game UI an event.
    UiEvent { name: String, data: String },
    /// Show a layout on a UI layer, or hide the layer (`path` empty).
    UiLayer { layer: String, path: String },
    /// Project a decal onto the surface at `origin`, facing along `normal`.
    PlaceDecal {
        material: String,
        origin: Vec3,
        normal: Vec3,
        size: f32,
    },
    /// Something for the store: an achievement, a stat, a score. Made by
    /// the `platform` (or `steam`) object.
    Platform(kerosene_platform::PlatformAction),
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

/// A Rhai engine with the bounds every Kerosene script runs under.
///
/// Shared with the UI's scripts (`kerosene-ui`), which are content in exactly
/// the same sense a level's are and get exactly the same limits.
pub fn sandboxed_engine() -> rhai::Engine {
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
    engine
}

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
    fn default() -> Self {
        ScriptHost::new()
    }
}

impl ScriptHost {
    pub fn new() -> ScriptHost {
        let shared = Rc::new(RefCell::new(Shared::default()));
        let mut engine = sandboxed_engine();
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
    pub fn loaded(&self) -> &[String] {
        &self.loaded
    }

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
        self.module
            .iter_functions()
            .find(|f| f.name == name)
            .map(|f| f.params.len())
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

        // Functions only: the statements just ran, and `AST::merge` would
        // otherwise carry them along to be run again by every later call
        // and every console `script` line -- a top-level `print` firing on
        // each tick. Later definitions win, which is what makes a reload a
        // reload.
        self.module = self.module.merge(&ast.clone_functions_only());
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
        // `eval_ast` off: the module holds no statements (see `load`), and
        // the default would run them before the call if it did.
        let options = rhai::CallFnOptions::new().eval_ast(false);
        self.engine
            .call_fn_with_options::<rhai::Dynamic>(
                options,
                &mut self.scope,
                &self.module,
                name,
                args,
            )
            .map(|_| ())
            .map_err(|e| ScriptError::Runtime(format!("{name}: {e}")))
    }

    /// The scripts' top-level variables, for a saved game.
    ///
    /// Only what is plain data -- numbers, text, booleans, and arrays and
    /// maps of them -- because that is what a save file can hold and what a
    /// level script keeps its state in. Anything else (a function pointer,
    /// an engine object) is left out and comes back as whatever the file's
    /// top level sets it to. Shadowed variables are written once, newest.
    pub fn variables(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut out = serde_json::Map::new();
        for (name, _, value) in self.scope.iter_raw() {
            if out.contains_key(name) {
                continue;
            }
            if let Some(v) = to_json(value) {
                out.insert(name.to_string(), v);
            }
        }
        out
    }

    /// Put saved variables back, over whatever the scripts' top level set.
    /// A variable the scripts no longer declare is added anyway, so a
    /// function that reads it still finds it.
    pub fn set_variables(&mut self, vars: &serde_json::Map<String, serde_json::Value>) {
        for (name, value) in vars {
            let value = from_json(value);
            if self.scope.is_constant(name).unwrap_or(false) {
                continue;
            }
            self.scope.set_or_push(name.clone(), value);
        }
    }

    /// Call a hook if the scripts defined one. Missing hooks are normal.
    pub fn call_hook(&mut self, name: &str, args: Vec<rhai::Dynamic>) -> Result<(), ScriptError> {
        match self.call(name, args) {
            Err(ScriptError::NoSuchFunction(_)) => Ok(()),
            other => other,
        }
    }
}

/// A script value as JSON, if it is plain data.
fn to_json(value: &rhai::Dynamic) -> Option<serde_json::Value> {
    use serde_json::Value as J;
    if value.is_unit() {
        return Some(J::Null);
    }
    if let Ok(b) = value.as_bool() {
        return Some(J::Bool(b));
    }
    if let Ok(i) = value.as_int() {
        return Some(J::from(i));
    }
    if let Ok(f) = value.as_float() {
        // NaN has no JSON spelling; nor does a variable anyone meant.
        return serde_json::Number::from_f64(f).map(J::Number);
    }
    if let Ok(c) = value.as_char() {
        return Some(J::String(c.to_string()));
    }
    if value.is_string() {
        return Some(J::String(value.clone().into_string().ok()?));
    }
    if value.is_array() {
        let array = value.read_lock::<rhai::Array>()?;
        return array
            .iter()
            .map(to_json)
            .collect::<Option<Vec<_>>>()
            .map(J::Array);
    }
    if value.is_map() {
        let map = value.read_lock::<rhai::Map>()?;
        let mut out = serde_json::Map::new();
        for (k, v) in map.iter() {
            out.insert(k.to_string(), to_json(v)?);
        }
        return Some(J::Object(out));
    }
    None
}

fn from_json(value: &serde_json::Value) -> rhai::Dynamic {
    use serde_json::Value as J;
    match value {
        J::Null => rhai::Dynamic::UNIT,
        J::Bool(b) => (*b).into(),
        J::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64().unwrap_or_default().into(),
        },
        J::String(s) => s.clone().into(),
        J::Array(a) => a.iter().map(from_json).collect::<rhai::Array>().into(),
        J::Object(o) => o
            .iter()
            .map(|(k, v)| (k.as_str().into(), from_json(v)))
            .collect::<rhai::Map>()
            .into(),
    }
}

/// Fields on a snapshot entity, in a stable order so iteration is repeatable.
pub type Fields = BTreeMap<String, String>;

#[cfg(test)]
mod tests;
