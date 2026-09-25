// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Entities that reach the game UI, and the decal a mapper places.
//!
//! | Class | What it does |
//! |---|---|
//! | `logic_ui` | Wiring into the UI: send it an event, publish a value, show or hide a layer |
//! | `point_worldpanel` | A UI layout on a surface in the level: a screen, a keypad, a sign |
//! | `infodecal` | A decal projected onto the nearest surface when the map starts |
//!
//! None of them touches the UI: like `logic_script`, each leaves a request
//! for the engine, which owns the UI, and this crate stays free of it. A world
//! panel's events come back the other way as outputs -- the panel's script
//! calls `emit("OnUnlock", code)` and whatever the mapper wired to `OnUnlock`
//! fires.

use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("logic_ui")
            .input("Emit", emit)
            .input("SetValue", set_value)
            .input("ShowLayer", show_layer)
            .input("HideLayer", hide_layer),
    );
    registry.register(
        ClassDef::new("point_worldpanel")
            .input("Enable", |w, id, _| set_enabled(w, id, true))
            .input("Disable", |w, id, _| set_enabled(w, id, false))
            .input("Emit", emit)
            .output("OnPanelEvent"),
    );
    registry.register(ClassDef::new("infodecal").on_spawn(place_decal));
}

/// Whether a world panel is showing: `startdisabled` at spawn, then the
/// `Enable`/`Disable` inputs.
pub fn panel_enabled(world: &EntityWorld, id: EntityId) -> bool {
    world.get(id).is_some_and(|e| {
        !e.fields
            .bool("disabled", e.fields.bool("startdisabled", false))
    })
}

fn set_enabled(world: &mut EntityWorld, id: EntityId, on: bool) -> bool {
    match world.get_mut(id) {
        Some(e) => {
            e.fields
                .set("disabled", Value::from_keyvalue(if on { "0" } else { "1" }));
            true
        }
        None => false,
    }
}

fn require(event: &InputEvent, what: &str) -> Option<String> {
    let p = event.parameter.trim();
    if p.is_empty() {
        log::warn!("{what}: needs a parameter");
        return None;
    }
    Some(p.to_string())
}

fn emit(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let Some(p) = require(event, "Emit") else {
        return false;
    };
    world.request(host_requests::UI_EMIT, p, id, event.activator);
    true
}

fn set_value(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let Some(p) = require(event, "SetValue") else {
        return false;
    };
    world.request(host_requests::UI_SET, p, id, event.activator);
    true
}

fn show_layer(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let Some(p) = require(event, "ShowLayer") else {
        return false;
    };
    world.request(host_requests::UI_SHOW, p, id, event.activator);
    true
}

fn hide_layer(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let Some(p) = require(event, "HideLayer") else {
        return false;
    };
    world.request(host_requests::UI_HIDE, p, id, event.activator);
    true
}

fn place_decal(world: &mut EntityWorld, id: EntityId) {
    let Some(e) = world.get(id) else { return };
    let texture = e
        .fields
        .text("texture")
        .map(|t| t.into_owned())
        .unwrap_or_default();
    if texture.trim().is_empty() {
        log::warn!("infodecal with no texture");
        return;
    }
    let size = e.fields.f32("size", 32.0);
    world.request(
        host_requests::PLACE_DECAL,
        format!("{texture} {size}"),
        id,
        None,
    );
}
