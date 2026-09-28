// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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

use crate::components::{Switchable, is_disabled, set_disabled};
use kerosene_ecs::prelude::*;
use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, host_requests};

/// A `point_worldpanel`: which layout, how big, and how it looks.
///
/// The engine reads these by key when it draws the panel (see
/// `kerosene_engine::ui`).
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct WorldPanel {
    /// The `.kui` file to show.
    #[reflect(@Key("layout"), @Label("Layout"), @Help("The .kui file to show."))]
    pub layout: String,
    /// In units.
    #[reflect(@Key("width"), @Label("Width (units)"))]
    pub width: f32,
    /// In units.
    #[reflect(@Key("height"), @Label("Height (units)"))]
    pub height: f32,
    /// The texture's height in pixels; its width follows the panel's shape.
    #[reflect(@Key("resolution"), @Label("Pixels tall"))]
    pub resolution: i32,
    /// How brightly the screen glows.
    #[reflect(@Key("brightness"), @Label("Brightness"))]
    pub brightness: f32,
    /// Whether the player can point at it and press use to click.
    #[reflect(@Key("interactive"), @Label("Interactive"))]
    pub interactive: bool,
}

impl Default for WorldPanel {
    fn default() -> Self {
        WorldPanel {
            layout: "ui/panels/status.kui".into(),
            width: 32.0,
            height: 32.0,
            resolution: 512,
            brightness: 1.0,
            interactive: false,
        }
    }
}

/// An `infodecal`: what to project, and how big.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Decal {
    /// The material.
    #[reflect(@Key("texture"), @Label("Material"), @Widget::Material)]
    pub texture: String,
    /// In units.
    #[reflect(@Key("size"), @Label("Size (units)"))]
    pub size: f32,
}

impl Default for Decal {
    fn default() -> Self {
        Decal {
            texture: "decals/crack".into(),
            size: 32.0,
        }
    }
}

/// Register the UI classes: `logic_ui`, `point_worldpanel` and `infodecal`.
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
            .component::<Switchable>()
            .component::<WorldPanel>()
            .input("Enable", |w, id, _| set_enabled(w, id, true))
            .input("Disable", |w, id, _| set_enabled(w, id, false))
            .input("Emit", emit)
            .output("OnPanelEvent"),
    );
    registry.register(
        ClassDef::new("infodecal")
            .component::<Decal>()
            .on_spawn(place_decal),
    );
}

/// Whether a world panel is showing: not switched off by `startdisabled`
/// or the `Disable` input.
pub fn panel_enabled(world: &EntityWorld, id: EntityId) -> bool {
    world.exists(id) && !is_disabled(world, id)
}

fn set_enabled(world: &mut EntityWorld, id: EntityId, on: bool) -> bool {
    set_disabled(world, id, !on)
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
    let Some(decal) = world.component::<Decal>(id).cloned() else {
        return;
    };
    if decal.texture.trim().is_empty() {
        log::warn!("infodecal with no texture");
        return;
    }
    world.request(
        host_requests::PLACE_DECAL,
        format!("{} {}", decal.texture, decal.size),
        id,
        None,
    );
}
