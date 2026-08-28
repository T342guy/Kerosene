// SPDX-License-Identifier: LGPL-3.0-or-later
//! What a script can call.
//!
//! The whole API, in one file, on purpose. A scripting surface that is spread
//! across the codebase is one nobody can audit, and the question "what can a
//! map's script actually do" has to have an answer you can read in one
//! sitting.
//!
//! Reads come from the snapshot; writes become [`ScriptAction`]s. Nothing
//! here touches the engine.

use crate::{EntityView, ScriptAction, ScriptLevel, Shared, MAX_ACTIONS};
use rhai::{Dynamic, Engine};
use std::cell::RefCell;
use std::rc::Rc;
use void_math::Vec3;

/// A script's view of one entity.
///
/// Carries the id so actions can name it, and a copy of the fields so reads
/// do not have to go back through the shared state.
#[derive(Clone, Debug)]
pub struct Ent {
    view: EntityView,
    shared: Rc<RefCell<Shared>>,
}

impl Ent {
    fn queue(&mut self, action: ScriptAction) {
        push(&self.shared, action);
    }
}

fn push(shared: &Rc<RefCell<Shared>>, action: ScriptAction) {
    let mut shared = shared.borrow_mut();
    if shared.actions.len() >= MAX_ACTIONS { return }
    shared.actions.push(action);
}

pub fn register(engine: &mut Engine, shared: &Rc<RefCell<Shared>>) {
    register_vector(engine);
    register_output(engine, shared);
    register_console(engine, shared);
    register_entities(engine, shared);
    register_sound(engine, shared);
    register_world(engine, shared);
}

// ---- vectors --------------------------------------------------------------

fn register_vector(engine: &mut Engine) {
    engine
        .register_type_with_name::<Vec3>("Vector")
        .register_fn("Vector", |x: f64, y: f64, z: f64| Vec3::new(x as f32, y as f32, z as f32))
        .register_fn("Vector", || Vec3::ZERO)
        .register_get_set(
            "x",
            |v: &mut Vec3| v.x as f64,
            |v: &mut Vec3, n: f64| v.x = n as f32,
        )
        .register_get_set(
            "y",
            |v: &mut Vec3| v.y as f64,
            |v: &mut Vec3, n: f64| v.y = n as f32,
        )
        .register_get_set(
            "z",
            |v: &mut Vec3| v.z as f64,
            |v: &mut Vec3, n: f64| v.z = n as f32,
        )
        .register_fn("+", |a: Vec3, b: Vec3| a + b)
        .register_fn("-", |a: Vec3, b: Vec3| a - b)
        .register_fn("*", |a: Vec3, s: f64| a * s as f32)
        .register_fn("*", |s: f64, a: Vec3| a * s as f32)
        .register_fn("length", |v: Vec3| v.length() as f64)
        .register_fn("distance", |a: Vec3, b: Vec3| (a - b).length() as f64)
        .register_fn("dot", |a: Vec3, b: Vec3| a.dot(b) as f64)
        .register_fn("normalize", |v: Vec3| v.normalize_or_zero())
        .register_fn("to_string", |v: Vec3| {
            format!(
                "{} {} {}",
                void_math::format_float(v.x),
                void_math::format_float(v.y),
                void_math::format_float(v.z)
            )
        })
        .register_fn("==", |a: Vec3, b: Vec3| a == b);
}

// ---- printing -------------------------------------------------------------

fn register_output(engine: &mut Engine, shared: &Rc<RefCell<Shared>>) {
    // rhai's own `print` and `debug` are routed to the console too, so a
    // script written the obvious way says something visible.
    let sink = Rc::clone(shared);
    engine.on_print(move |text| push(&sink, ScriptAction::Log(ScriptLevel::Print, text.to_string())));

    let sink = Rc::clone(shared);
    engine.on_debug(move |text, source, pos| {
        let where_ = source.map(|s| format!("{s}:{pos}")).unwrap_or_else(|| pos.to_string());
        push(&sink, ScriptAction::Log(ScriptLevel::Print, format!("[{where_}] {text}")));
    });

    let sink = Rc::clone(shared);
    engine.register_fn("warn", move |text: &str| {
        push(&sink, ScriptAction::Log(ScriptLevel::Warn, text.to_string()))
    });

    let sink = Rc::clone(shared);
    engine.register_fn("error", move |text: &str| {
        push(&sink, ScriptAction::Log(ScriptLevel::Error, text.to_string()))
    });
}

// ---- console --------------------------------------------------------------

fn register_console(engine: &mut Engine, shared: &Rc<RefCell<Shared>>) {
    let sink = Rc::clone(shared);
    engine.register_fn("command", move |text: &str| {
        push(&sink, ScriptAction::Command(text.to_string()))
    });

    let source = Rc::clone(shared);
    engine.register_fn("cvar", move |name: &str| -> String {
        source.borrow().view.cvars.get(name).cloned().unwrap_or_default()
    });

    let source = Rc::clone(shared);
    engine.register_fn("cvar_float", move |name: &str| -> f64 {
        source
            .borrow()
            .view
            .cvars
            .get(name)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
    });

    // Setting a convar is a console command, not a second path into the
    // convar table: one way in means one place that enforces cheat flags.
    let sink = Rc::clone(shared);
    engine.register_fn("set_cvar", move |name: &str, value: &str| {
        push(&sink, ScriptAction::Command(format!("{name} \"{value}\"")))
    });
}

// ---- entities -------------------------------------------------------------

fn register_entities(engine: &mut Engine, shared: &Rc<RefCell<Shared>>) {
    engine
        .register_type_with_name::<Ent>("Entity")
        .register_get("id", |e: &mut Ent| e.view.id as i64)
        .register_get("classname", |e: &mut Ent| e.view.classname.clone())
        .register_get("targetname", |e: &mut Ent| e.view.targetname.clone())
        .register_get("origin", |e: &mut Ent| e.view.origin)
        .register_fn("get", |e: &mut Ent, key: &str| {
            e.view.field(key).unwrap_or_default().to_string()
        })
        .register_fn("get_float", |e: &mut Ent, key: &str| {
            e.view.field(key).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
        })
        .register_fn("has", |e: &mut Ent, key: &str| e.view.field(key).is_some())
        .register_fn("to_string", |e: &mut Ent| {
            if e.view.targetname.is_empty() {
                format!("{} #{}", e.view.classname, e.view.id)
            } else {
                format!("{} `{}`", e.view.classname, e.view.targetname)
            }
        })
        .register_fn("set", |e: &mut Ent, key: &str, value: &str| {
            let action = ScriptAction::SetField {
                entity: e.view.id,
                key: key.to_string(),
                value: value.to_string(),
            };
            e.queue(action);
        })
        .register_fn("set", |e: &mut Ent, key: &str, value: f64| {
            let action = ScriptAction::SetField {
                entity: e.view.id,
                key: key.to_string(),
                value: void_math::format_float(value as f32),
            };
            e.queue(action);
        })
        .register_fn("set_origin", |e: &mut Ent, origin: Vec3| {
            let action = ScriptAction::SetOrigin { entity: e.view.id, origin };
            e.queue(action);
        })
        .register_fn("kill", |e: &mut Ent| {
            let action = ScriptAction::Kill { entity: e.view.id };
            e.queue(action);
        })
        .register_fn("fire", |e: &mut Ent, input: &str| {
            let action = ScriptAction::FireInput {
                target: entity_target(&e.view),
                input: input.to_string(),
                parameter: String::new(),
                delay: 0.0,
            };
            e.queue(action);
        });

    let source = Rc::clone(shared);
    let sink = Rc::clone(shared);
    engine.register_fn("find_by_name", move |name: &str| -> Dynamic {
        let found = source.borrow().view.by_name(name).next().cloned();
        match found {
            Some(view) => Dynamic::from(Ent { view, shared: Rc::clone(&sink) }),
            None => Dynamic::UNIT,
        }
    });

    let source = Rc::clone(shared);
    let sink = Rc::clone(shared);
    engine.register_fn("find_all_by_name", move |name: &str| -> rhai::Array {
        source
            .borrow()
            .view
            .by_name(name)
            .cloned()
            .map(|view| Dynamic::from(Ent { view, shared: Rc::clone(&sink) }))
            .collect()
    });

    let source = Rc::clone(shared);
    let sink = Rc::clone(shared);
    engine.register_fn("find_by_class", move |class: &str| -> rhai::Array {
        source
            .borrow()
            .view
            .by_class(class)
            .cloned()
            .map(|view| Dynamic::from(Ent { view, shared: Rc::clone(&sink) }))
            .collect()
    });

    let source = Rc::clone(shared);
    let sink = Rc::clone(shared);
    engine.register_fn("player", move || -> Dynamic {
        let found = source.borrow().view.player.clone();
        match found {
            Some(view) => Dynamic::from(Ent { view, shared: Rc::clone(&sink) }),
            None => Dynamic::UNIT,
        }
    });

    // The workhorse. Every arity, because the common call is the short one and
    // making people write `ent_fire(t, i, "", 0.0)` is how an API gets a
    // reputation.
    let sink = Rc::clone(shared);
    engine.register_fn("ent_fire", move |target: &str, input: &str| {
        push(
            &sink,
            ScriptAction::FireInput {
                target: target.to_string(),
                input: input.to_string(),
                parameter: String::new(),
                delay: 0.0,
            },
        )
    });

    let sink = Rc::clone(shared);
    engine.register_fn("ent_fire", move |target: &str, input: &str, parameter: &str| {
        push(
            &sink,
            ScriptAction::FireInput {
                target: target.to_string(),
                input: input.to_string(),
                parameter: parameter.to_string(),
                delay: 0.0,
            },
        )
    });

    let sink = Rc::clone(shared);
    engine.register_fn(
        "ent_fire",
        move |target: &str, input: &str, parameter: &str, delay: f64| {
            push(
                &sink,
                ScriptAction::FireInput {
                    target: target.to_string(),
                    input: input.to_string(),
                    parameter: parameter.to_string(),
                    delay: delay.max(0.0) as f32,
                },
            )
        },
    );
}

/// How an action names an entity that may not have a `targetname`.
///
/// Anonymous entities get `!id:N`, which the engine resolves back to the
/// handle. Without it, `find_by_class("light")[0].fire("Kill")` would silently
/// address every unnamed entity in the map, or none.
fn entity_target(view: &EntityView) -> String {
    if view.targetname.is_empty() {
        format!("!id:{}", view.id)
    } else {
        view.targetname.clone()
    }
}

/// The prefix an anonymous entity target carries.
pub const ID_TARGET_PREFIX: &str = "!id:";

/// Read back an `!id:N` target.
pub fn parse_id_target(target: &str) -> Option<u64> {
    target.strip_prefix(ID_TARGET_PREFIX)?.parse().ok()
}

// ---- sound ----------------------------------------------------------------

fn register_sound(engine: &mut Engine, shared: &Rc<RefCell<Shared>>) {
    let sink = Rc::clone(shared);
    engine.register_fn("play_sound", move |name: &str| {
        push(
            &sink,
            ScriptAction::PlaySound { name: name.to_string(), position: None, volume: 1.0 },
        )
    });

    let sink = Rc::clone(shared);
    engine.register_fn("play_sound", move |name: &str, position: Vec3| {
        push(
            &sink,
            ScriptAction::PlaySound {
                name: name.to_string(),
                position: Some(position),
                volume: 1.0,
            },
        )
    });

    let sink = Rc::clone(shared);
    engine.register_fn("play_sound", move |name: &str, position: Vec3, volume: f64| {
        push(
            &sink,
            ScriptAction::PlaySound {
                name: name.to_string(),
                position: Some(position),
                volume: volume.max(0.0) as f32,
            },
        )
    });

    let sink = Rc::clone(shared);
    engine.register_fn("stop_sounds", move || push(&sink, ScriptAction::StopAllSounds));
}

// ---- the world ------------------------------------------------------------

fn register_world(engine: &mut Engine, shared: &Rc<RefCell<Shared>>) {
    let source = Rc::clone(shared);
    engine.register_fn("time", move || source.borrow().view.time as f64);

    let source = Rc::clone(shared);
    engine.register_fn("tick", move || source.borrow().view.tick as i64);

    let source = Rc::clone(shared);
    engine.register_fn("map_name", move || source.borrow().view.map.clone());

    let source = Rc::clone(shared);
    engine.register_fn("entity_count", move || source.borrow().view.entities.len() as i64);
}
