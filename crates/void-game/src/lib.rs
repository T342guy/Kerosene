//! The sample game: entity classes and the rules that use them.
//!
//! This is the analogue of Source's game DLL. `void-entity` knows how to route
//! an input and run a think; everything here decides what those inputs *mean*.
//! The split is worth keeping strictly: nothing in this crate is required by
//! the engine, and a different game would replace it wholesale.
//!
//! The classes implemented are the ones a level actually needs to be a level:
//!
//! | Class | What it does |
//! |---|---|
//! | `worldspawn` | Holds map-wide settings: sky, fog, level name |
//! | `info_player_start` | Where the player appears |
//! | `func_door` | A brush that slides open and shut |
//! | `func_brush` | A brush that can be turned on and off |
//! | `func_detail` | Decoration; baked into the world at compile time |
//! | `trigger_multiple` | Fires when something enters its volume |
//! | `trigger_once` | The same, once |
//! | `logic_relay` | Passes a signal on, with a delay |
//! | `logic_auto` | Fires when the map starts |
//! | `math_counter` | Counts, and fires when it hits a limit |
//! | `point_message` | Prints to the console |
//!
//! Lighting entities (`light`, `light_spot`, `light_environment`) are read by
//! Radiance at compile time and are inert here, which is why a lit map needs
//! no lights at runtime at all.

pub mod doors;
pub mod logic;
pub mod triggers;

use std::sync::Arc;
use void_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, InputHandler, Value};

/// Register every class this game provides.
pub fn register(registry: &mut ClassRegistry) {
    // Inert classes still get registered, so that loading a map does not warn
    // about every light in it.
    for inert in [
        "worldspawn",
        "info_player_start",
        "info_target",
        "light",
        "light_spot",
        "light_environment",
        "func_detail",
        "prop_static",
    ] {
        registry.register(ClassDef::new(inert));
    }

    doors::register(registry);
    triggers::register(registry);
    logic::register(registry);

    // Inputs every entity understands, as Source makes them.
    registry.register_common_input("Kill", input_kill);
    registry.register_common_input("AddOutput", input_add_output);
    registry.register_common_input("FireUser1", |w, id, _| fire_user(w, id, 1));
    registry.register_common_input("FireUser2", |w, id, _| fire_user(w, id, 2));
}

/// A registry with this game's classes already in it.
pub fn registry() -> Arc<ClassRegistry> {
    let mut r = ClassRegistry::new();
    register(&mut r);
    Arc::new(r)
}

fn input_kill(world: &mut EntityWorld, id: EntityId, _e: &void_entity::io::InputEvent) -> bool {
    world.remove(id);
    true
}

/// `AddOutput` rewires an entity at runtime: `"OnTrigger target,Input,,0,-1"`.
///
/// Source uses this constantly for effects a designer could not wire up in
/// advance, and it is cheap to support because a connection is just data.
fn input_add_output(
    world: &mut EntityWorld,
    id: EntityId,
    event: &void_entity::io::InputEvent,
) -> bool {
    let Some((output, rest)) = event.parameter.trim().split_once(char::is_whitespace) else {
        log::warn!("AddOutput: expected '<output> <target>,<input>,<param>,<delay>,<times>'");
        return false;
    };
    match void_map::Connection::parse(output, rest.trim()) {
        Ok(c) => {
            if let Some(e) = world.get_mut(id) { e.connections.push(c.into()); }
            true
        }
        Err(err) => {
            log::warn!("AddOutput: {err}");
            false
        }
    }
}

/// `FireUser1`/`FireUser2` fire `OnUser1`/`OnUser2`.
///
/// A general-purpose signal with no meaning of its own, which is exactly why
/// it is useful: a designer wires whatever they like to it.
fn fire_user(world: &mut EntityWorld, id: EntityId, n: u8) -> bool {
    world.fire_output(id, &format!("OnUser{n}"), None, None);
    true
}

/// Shorthand for reading a numeric field with a default.
pub(crate) fn field_f32(world: &EntityWorld, id: EntityId, key: &str, default: f32) -> f32 {
    world.get(id).map_or(default, |e| e.fields.f32(key, default))
}

pub(crate) fn set_field(world: &mut EntityWorld, id: EntityId, key: &str, value: Value) {
    if let Some(e) = world.get_mut(id) { e.fields.set(key, value); }
}

/// Register a class built from a name and a list of inputs.
pub(crate) fn simple_class(
    registry: &mut ClassRegistry,
    name: &'static str,
    inputs: &[(&'static str, InputHandler)],
) {
    let mut def = ClassDef::new(name);
    for (input, handler) in inputs {
        def = def.input(input, *handler);
    }
    registry.register(def);
}
