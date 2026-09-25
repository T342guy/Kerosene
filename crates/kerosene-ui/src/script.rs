// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! UI scripts: the Rhai a layout runs.
//!
//! Bindings cover showing state; a script is for *behaviour* -- a menu that
//! opens a sub-page, a kill feed that adds a line and removes it four seconds
//! later, a keypad that checks a code. Each document gets its own sandboxed
//! Rhai engine ([`kerosene_script::sandboxed_engine`], the same bounds a
//! level's scripts run under) and this API:
//!
//! | | |
//! |---|---|
//! | `panel(id)` | A handle to the element with that `id` |
//! | `p.add_class(c)` `p.remove_class(c)` `p.toggle_class(c)` `p.set_class(c, on)` | Classes |
//! | `p.trigger_class(c)` | Add a class and restart its animation |
//! | `p.set_text(t)` `p.set_attr(name, value)` `p.set_style(prop, value)` | Content |
//! | `p.show()` `p.hide()` `p.focus()` | |
//! | `store(key)` `store_or(key, default)` `set_store(key, value)` | The shared state |
//! | `emit(name, data)` | An event to the game and every document |
//! | `command(text)` `set_cvar(name, value)` `play_sound(name)` | The engine |
//! | `show_layer(layer, file)` `hide_layer(layer)` | Other documents |
//! | `schedule(seconds, "fn_name")` | Call a function later |
//! | `time()` `print(x)` `warn(x)` | |
//!
//! Hooks: `on_load()` once the layout is up, and `on_event(name, data)` for
//! every event in the store.
//!
//! As with level scripts, nothing here touches a live structure: calls queue
//! [`DocOp`]s and [`UiAction`]s, which the document applies after the script
//! returns. A handler therefore sees the document as it was when the handler
//! began, whatever it does part-way through.

use crate::UiAction;
use crate::store::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// A change a script asked for, applied to the document afterwards.
#[derive(Clone, PartialEq, Debug)]
pub enum DocOp {
    SetClass {
        node: usize,
        class: String,
        on: bool,
    },
    ToggleClass {
        node: usize,
        class: String,
    },
    TriggerClass {
        node: usize,
        class: String,
    },
    SetAttr {
        node: usize,
        name: String,
        value: String,
    },
    SetStyle {
        node: usize,
        prop: String,
        value: String,
    },
    Focus {
        node: usize,
    },
    Schedule {
        seconds: f32,
        function: String,
    },
}

/// What a script's functions read and write.
#[derive(Default)]
pub(crate) struct Shared {
    pub ids: HashMap<String, usize>,
    pub ops: Vec<DocOp>,
    pub actions: Vec<UiAction>,
    pub store_writes: Vec<(String, Value)>,
    /// A copy of the store as it was when the script began.
    pub store: BTreeMap<String, Value>,
    pub time: f64,
    /// Set when `panel()` named something that does not exist.
    pub missing: Vec<String>,
}

/// A handle to one element. Stale or missing handles do nothing.
#[derive(Clone, Copy, Debug)]
pub struct PanelRef {
    pub(crate) node: Option<usize>,
}

/// How many queued operations one script run may make.
const MAX_OPS: usize = 4096;

pub(crate) fn register(engine: &mut rhai::Engine, shared: &Rc<RefCell<Shared>>) {
    engine.register_type_with_name::<PanelRef>("Panel");

    let s = shared.clone();
    engine.register_fn("panel", move |id: &str| {
        let mut sh = s.borrow_mut();
        let node = sh.ids.get(id).copied();
        if node.is_none() && !sh.missing.iter().any(|m| m == id) {
            sh.missing.push(id.to_string());
        }
        PanelRef { node }
    });
    engine.register_get("valid", |p: &mut PanelRef| p.node.is_some());

    fn op(shared: &Rc<RefCell<Shared>>, op: DocOp) {
        let mut sh = shared.borrow_mut();
        if sh.ops.len() < MAX_OPS {
            sh.ops.push(op);
        }
    }

    macro_rules! method {
        ($name:literal, |$node:ident $(, $arg:ident : $ty:ty)*| $make:expr) => {{
            let s = shared.clone();
            engine.register_fn($name, move |p: &mut PanelRef $(, $arg: $ty)*| {
                if let Some($node) = p.node {
                    op(&s, $make);
                }
            });
        }};
    }
    method!("add_class", |node, class: &str| DocOp::SetClass {
        node,
        class: class.to_string(),
        on: true
    });
    method!("remove_class", |node, class: &str| DocOp::SetClass {
        node,
        class: class.to_string(),
        on: false
    });
    method!("set_class", |node, class: &str, on: bool| DocOp::SetClass {
        node,
        class: class.to_string(),
        on
    });
    method!("toggle_class", |node, class: &str| DocOp::ToggleClass {
        node,
        class: class.to_string()
    });
    method!("trigger_class", |node, class: &str| DocOp::TriggerClass {
        node,
        class: class.to_string()
    });
    method!("set_text", |node, text: rhai::Dynamic| DocOp::SetAttr {
        node,
        name: "text".to_string(),
        value: crate::bind::display(&text)
    });
    method!("set_attr", |node, name: &str, value: rhai::Dynamic| {
        DocOp::SetAttr {
            node,
            name: name.to_string(),
            value: crate::bind::display(&value),
        }
    });
    method!("set_style", |node, prop: &str, value: rhai::Dynamic| {
        DocOp::SetStyle {
            node,
            prop: prop.to_string(),
            value: crate::bind::display(&value),
        }
    });
    method!("show", |node| DocOp::SetAttr {
        node,
        name: "visible".to_string(),
        value: "true".to_string()
    });
    method!("hide", |node| DocOp::SetAttr {
        node,
        name: "visible".to_string(),
        value: "false".to_string()
    });
    method!("focus", |node| DocOp::Focus { node });

    let s = shared.clone();
    engine.register_fn("store", move |key: &str| -> rhai::Dynamic {
        s.borrow()
            .store
            .get(key)
            .map_or(rhai::Dynamic::UNIT, Value::to_dynamic)
    });
    let s = shared.clone();
    engine.register_fn(
        "store_or",
        move |key: &str, default: rhai::Dynamic| -> rhai::Dynamic {
            s.borrow().store.get(key).map_or(default, Value::to_dynamic)
        },
    );
    let s = shared.clone();
    engine.register_fn("set_store", move |key: &str, value: rhai::Dynamic| {
        let value = Value::from_dynamic(&value);
        let mut sh = s.borrow_mut();
        // Visible to later `store()` calls in the same run.
        sh.store.insert(key.to_string(), value.clone());
        if sh.store_writes.len() < MAX_OPS {
            sh.store_writes.push((key.to_string(), value));
        }
    });

    fn action(shared: &Rc<RefCell<Shared>>, action: UiAction) {
        let mut sh = shared.borrow_mut();
        if sh.actions.len() < MAX_OPS {
            sh.actions.push(action);
        }
    }
    let s = shared.clone();
    engine.register_fn("emit", move |name: &str, data: rhai::Dynamic| {
        action(
            &s,
            UiAction::Emit {
                name: name.to_string(),
                data: crate::bind::display(&data),
                source: String::new(),
            },
        );
    });
    let s = shared.clone();
    engine.register_fn("emit", move |name: &str| {
        action(
            &s,
            UiAction::Emit {
                name: name.to_string(),
                data: String::new(),
                source: String::new(),
            },
        );
    });
    let s = shared.clone();
    engine.register_fn("command", move |text: &str| {
        action(&s, UiAction::Command(text.to_string()))
    });
    let s = shared.clone();
    engine.register_fn("set_cvar", move |name: &str, value: rhai::Dynamic| {
        action(
            &s,
            UiAction::SetCvar {
                name: name.to_string(),
                value: crate::bind::display(&value),
            },
        );
    });
    let s = shared.clone();
    engine.register_fn("play_sound", move |name: &str| {
        action(&s, UiAction::PlaySound(name.to_string()))
    });
    let s = shared.clone();
    engine.register_fn("show_layer", move |layer: &str, file: &str| {
        action(
            &s,
            UiAction::ShowLayer {
                layer: layer.to_string(),
                path: file.to_string(),
            },
        );
    });
    let s = shared.clone();
    engine.register_fn("hide_layer", move |layer: &str| {
        action(&s, UiAction::HideLayer(layer.to_string()))
    });
    let s = shared.clone();
    engine.register_fn("schedule", move |seconds: rhai::FLOAT, function: &str| {
        op(
            &s,
            DocOp::Schedule {
                seconds: seconds as f32,
                function: function.to_string(),
            },
        );
    });
    let s = shared.clone();
    engine.register_fn("schedule", move |seconds: rhai::INT, function: &str| {
        op(
            &s,
            DocOp::Schedule {
                seconds: seconds as f32,
                function: function.to_string(),
            },
        );
    });
    let s = shared.clone();
    engine.register_fn("time", move || s.borrow().time);

    let s = shared.clone();
    engine.on_print(move |text| action(&s, UiAction::Log(crate::LogLevel::Info, text.to_string())));
    let s = shared.clone();
    engine.register_fn("warn", move |x: rhai::Dynamic| {
        action(
            &s,
            UiAction::Log(crate::LogLevel::Warn, crate::bind::display(&x)),
        );
    });
    let s = shared.clone();
    engine.register_fn("error", move |x: rhai::Dynamic| {
        action(
            &s,
            UiAction::Log(crate::LogLevel::Error, crate::bind::display(&x)),
        );
    });
}

/// One document's script VM: the engine, what the files defined, and the
/// top-level variables they left behind.
pub(crate) struct Script {
    pub engine: rhai::Engine,
    pub module: rhai::AST,
    pub scope: rhai::Scope<'static>,
    pub shared: Rc<RefCell<Shared>>,
    /// Handler snippets compiled against `module`, by source.
    handlers: HashMap<String, Result<rhai::AST, String>>,
}

impl Script {
    pub fn new() -> Script {
        let shared = Rc::new(RefCell::new(Shared::default()));
        let mut engine = kerosene_script::sandboxed_engine();
        register(&mut engine, &shared);
        Script {
            engine,
            module: rhai::AST::empty(),
            scope: rhai::Scope::new(),
            shared,
            handlers: HashMap::new(),
        }
    }

    /// Run a script file, keeping its functions and variables.
    pub fn load(&mut self, name: &str, source: &str) -> Result<(), String> {
        let ast = self
            .engine
            .compile(source)
            .map_err(|e| format!("{name}: {e}"))?;
        self.engine
            .run_ast_with_scope(&mut self.scope, &ast)
            .map_err(|e| format!("{name}: {e}"))?;
        self.module = self.module.merge(&ast.clone_functions_only());
        self.handlers.clear();
        Ok(())
    }

    fn arity(&self, name: &str) -> Option<usize> {
        self.module
            .iter_functions()
            .find(|f| f.name == name)
            .map(|f| f.params.len())
    }

    /// Call a script function if it exists, passing as many of `args` as it
    /// takes -- `fn on_event()` and `fn on_event(name, data)` both work.
    pub fn call(&mut self, name: &str, args: &[rhai::Dynamic]) -> Result<bool, String> {
        let Some(arity) = self.arity(name) else {
            return Ok(false);
        };
        let args: Vec<rhai::Dynamic> = args.iter().take(arity).cloned().collect();
        if args.len() < arity {
            return Err(format!("{name} takes {arity} arguments"));
        }
        let options = rhai::CallFnOptions::new().eval_ast(false);
        self.engine
            .call_fn_with_options::<rhai::Dynamic>(
                options,
                &mut self.scope,
                &self.module,
                name,
                args,
            )
            .map(|_| true)
            .map_err(|e| format!("{name}: {e}"))
    }

    /// Run a snippet from an attribute (`onactivate="resume()"`) with extra
    /// variables in scope for its duration.
    pub fn run_handler(
        &mut self,
        source: &str,
        vars: Vec<(&'static str, rhai::Dynamic)>,
    ) -> Result<(), String> {
        if !self.handlers.contains_key(source) {
            let compiled = self
                .engine
                .compile_with_scope(&self.scope, crate::bind::single_quotes_to_double(source))
                .map(|ast| self.module.merge(&ast))
                .map_err(|e| format!("{source:?}: {e}"));
            self.handlers.insert(source.to_string(), compiled);
        }
        let ast = match &self.handlers[source] {
            Ok(ast) => ast.clone(),
            Err(e) => return Err(e.clone()),
        };
        let mark = self.scope.len();
        for (name, value) in vars {
            self.scope.push_dynamic(name, value);
        }
        let result = self.engine.run_ast_with_scope(&mut self.scope, &ast);
        self.scope.rewind(mark);
        result.map_err(|e| format!("{source:?}: {e}"))
    }
}
