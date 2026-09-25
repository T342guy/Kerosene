// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The sample game: entity classes and the rules that use them.
//!
//! This is the analogue of Source's game DLL. `kerosene-entity` knows how to route
//! an input and run a think; everything here decides what those inputs *mean*.
//! The split is kept strictly: the engine does not depend on this crate
//! (only its tests do), and a game replaces or extends it by implementing
//! `kerosene_engine::Game` -- `kerosene::game::Stock` is this crate as one.
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
//! | `func_ladder` | A volume the player climbs |
//! | `func_button` | A brush the player presses |
//! | `trigger_multiple` | Fires when something enters its volume |
//! | `trigger_once` | The same, once |
//! | `logic_relay` | Passes a signal on, with a delay |
//! | `logic_auto` | Fires when the map starts |
//! | `math_counter` | Counts, and fires when it hits a limit |
//! | `point_message` | Prints to the console |
//! | `logic_script` | Runs a script function |
//! | `ambient_generic` | A sound placed in the world |
//! | `logic_ui` | Sends the game UI events and values |
//! | `point_worldpanel` | A UI layout on a surface in the level |
//! | `infodecal` | A decal placed by the mapper |
//!
//! Lighting entities (`light`, `light_spot`, `light_environment`) and
//! reflection probes (`env_cubemap`) are read by Radiance at compile time and
//! are inert here, which is why a lit map needs
//! no lights at runtime at all.

pub mod animated;
pub mod doors;
pub mod lights;
pub mod logic;
pub mod platform;
pub mod props;
pub mod schema;
pub mod scripted;
pub mod sound;
pub mod triggers;
pub mod ui;
pub mod weapons;

use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value};
use std::sync::Arc;

/// Register every class this game provides.
pub fn register(registry: &mut ClassRegistry) {
    // Inert classes still get registered, so that loading a map does not warn
    // about every light in it.
    for inert in [
        "worldspawn",
        "info_player_start",
        "info_target",
        // A named point two maps share, which a level change places the
        // player relative to. Read by the engine; nothing to do here.
        "info_landmark",
        "light",
        "light_spot",
        "light_environment",
        // Read by Radiance at compile time, like the lights.
        "env_cubemap",
        // Read by Resonance at compile time, like the lights by Radiance.
        "env_acoustic_override",
        "func_detail",
        // A ladder is geometry, not behaviour: the compiler gives its brushes
        // ladder contents and the movement solver does the rest, so there is
        // nothing here for it to do but be a class a map may legally contain.
        "func_ladder",
        "prop_static",
    ] {
        registry.register(ClassDef::new(inert));
    }

    doors::register(registry);
    triggers::register(registry);
    logic::register(registry);
    scripted::register(registry);
    sound::register(registry);
    props::register(registry);
    lights::register(registry);
    animated::register(registry);
    ui::register(registry);
    platform::register(registry);

    // Inputs every entity understands, as Source makes them.
    registry.register_common_input("Kill", input_kill);
    registry.register_common_input("AddOutput", input_add_output);
    registry.register_common_input("FireUser1", |w, id, _| fire_user(w, id, 1));
    registry.register_common_input("FireUser2", |w, id, _| fire_user(w, id, 2));
    registry.register_common_output("OnUser1");
    registry.register_common_output("OnUser2");
}

/// A registry with this game's classes already in it.
pub fn registry() -> Arc<ClassRegistry> {
    let mut r = ClassRegistry::new();
    register(&mut r);
    Arc::new(r)
}

fn input_kill(world: &mut EntityWorld, id: EntityId, _e: &kerosene_entity::io::InputEvent) -> bool {
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
    event: &kerosene_entity::io::InputEvent,
) -> bool {
    let Some((output, rest)) = event.parameter.trim().split_once(char::is_whitespace) else {
        log::warn!("AddOutput: expected '<output> <target>,<input>,<param>,<delay>,<times>'");
        return false;
    };
    match kerosene_map::Connection::parse(output, rest.trim()) {
        Ok(c) => {
            if let Some(e) = world.get_mut(id) {
                e.connections.push(c.into());
            }
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
    world
        .get(id)
        .map_or(default, |e| e.fields.f32(key, default))
}

pub(crate) fn set_field(world: &mut EntityWorld, id: EntityId, key: &str, value: Value) {
    if let Some(e) = world.get_mut(id) {
        e.fields.set(key, value);
    }
}
