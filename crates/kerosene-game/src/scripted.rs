// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! `logic_script` -- the entity that gives a level a script.
//!
//! Entity I/O is a graph, and a graph is the right shape for most of what a
//! level does. It is the wrong shape for arithmetic, for conditions, and for
//! anything that has to remember more than a counter. `logic_script` is the
//! seam: an output fires one of its inputs, and a function in the map's script
//! runs.
//!
//! The class holds no VM of its own. It leaves a request on the entity world
//! and the engine, which owns the VM, picks it up at the end of the tick.
//! That keeps the game code free of the script engine entirely -- the same
//! split the rest of this crate keeps from the engine.

use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, host_requests};

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("logic_script")
            .on_spawn(spawn)
            .input("RunScriptCode", run_code)
            .input("CallScriptFunction", call_function)
            .input("RunScriptFile", run_file)
            .output("OnScriptRun"),
    );
}

/// Load the file named by `scriptfile`, if there is one.
///
/// Deferred to the engine rather than done here: this crate has no filesystem
/// and no VM, and giving it either would be the beginning of the game DLL
/// becoming the engine.
fn spawn(world: &mut EntityWorld, id: EntityId) {
    let file = world
        .get(id)
        .and_then(|e| e.fields.text("scriptfile").map(str::to_string))
        .unwrap_or_default();
    if !file.trim().is_empty() {
        world.request(host_requests::SCRIPT_FILE, file, id, None);
    }
}

fn run_code(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    // The parameter if there is one, otherwise whatever the entity was given
    // in the editor -- so a `logic_script` can be a one-liner with no wiring.
    let source = if event.parameter.trim().is_empty() {
        world.get(id).and_then(|e| e.fields.text("code").map(str::to_string)).unwrap_or_default()
    } else {
        event.parameter.clone()
    };
    if source.trim().is_empty() {
        log::warn!("logic_script: RunScriptCode with nothing to run");
        return false;
    }
    world.request(host_requests::SCRIPT, source, id, event.activator);
    world.fire_output(id, "OnScriptRun", event.activator, None);
    true
}

fn call_function(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let name = if event.parameter.trim().is_empty() {
        world.get(id).and_then(|e| e.fields.text("function").map(str::to_string)).unwrap_or_default()
    } else {
        event.parameter.clone()
    };
    if name.trim().is_empty() {
        log::warn!("logic_script: CallScriptFunction with no function named");
        return false;
    }
    world.request(host_requests::SCRIPT_CALL, name, id, event.activator);
    world.fire_output(id, "OnScriptRun", event.activator, None);
    true
}

fn run_file(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let file = if event.parameter.trim().is_empty() {
        world.get(id).and_then(|e| e.fields.text("scriptfile").map(str::to_string)).unwrap_or_default()
    } else {
        event.parameter.clone()
    };
    if file.trim().is_empty() {
        log::warn!("logic_script: RunScriptFile with no file named");
        return false;
    }
    world.request(host_requests::SCRIPT_FILE, file, id, event.activator);
    true
}
