// SPDX-License-Identifier: LGPL-3.0-or-later
//! Moving brushes: doors and toggleable brush entities.

use crate::{field_f32, set_field};
use void_entity::io::InputEvent;
use void_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value};
use void_math::Vec3;

/// How often a moving door updates, in seconds.
///
/// Movers run on their own cadence rather than every frame: a door takes a
/// second or two to open, and stepping it 20 times a second is
/// indistinguishable from stepping it 200 times while costing a tenth as much.
///
/// The *step size* is not derived from this interval, though. Think times are
/// quantised to the tick rate, so a requested 0.05s gap is really 0.0625s at
/// 64 tick -- and a door that assumed otherwise would run a quarter slower
/// than its `speed` says, differently on every tick rate. Movement integrates
/// the time that actually elapsed instead.
const MOVE_INTERVAL: f32 = 0.05;

/// Spawnflag bits, matching the names Source gives them.
pub const SF_START_OPEN: u32 = 1;

/// Which way a door is going.
mod state {
    pub const CLOSED: i32 = 0;
    pub const OPENING: i32 = 1;
    pub const OPEN: i32 = 2;
    pub const CLOSING: i32 = 3;
}

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("func_door")
            .on_spawn(spawn_door)
            .on_think(think_door)
            .input("Open", |w, id, e| start(w, id, e, true))
            .input("Close", |w, id, e| start(w, id, e, false))
            .input("Toggle", input_toggle)
            .input("Lock", |w, id, _| { set_field(w, id, "locked", Value::Bool(true)); true })
            .input("Unlock", |w, id, _| { set_field(w, id, "locked", Value::Bool(false)); true })
            .input("SetSpeed", |w, id, e| {
                if let Some(v) = e.parameter_f32() { set_field(w, id, "speed", Value::Float(v)); }
                true
            })
            .output("OnOpen")
            .output("OnClose")
            .output("OnFullyOpen")
            .output("OnFullyClosed")
            .output("OnLockedUse"),
    );

    registry.register(
        ClassDef::new("func_brush")
            .on_spawn(spawn_brush)
            .input("Enable", |w, id, _| { set_field(w, id, "disabled", Value::Bool(false)); true })
            .input("Disable", |w, id, _| { set_field(w, id, "disabled", Value::Bool(true)); true })
            .input("Toggle", |w, id, _| {
                let off = w.get(id).map(|e| e.fields.bool("disabled", false)).unwrap_or(false);
                set_field(w, id, "disabled", Value::Bool(!off));
                true
            }),
    );
}

fn spawn_brush(world: &mut EntityWorld, id: EntityId) {
    let start_disabled = world.get(id).map(|e| e.fields.bool("startdisabled", false)).unwrap_or(false);
    set_field(world, id, "disabled", Value::Bool(start_disabled));
}

/// Work out how far the door travels and which way.
///
/// The distance comes from the geometry, not from a keyvalue: a door moves by
/// its own size along its movement axis, less the `lip` that stays visible.
/// That is what lets a designer resize a door and have it still work.
fn spawn_door(world: &mut EntityWorld, id: EntityId) {
    let Some(entity) = world.get(id) else { return };

    let dir = entity.fields.vec3("movedir", Vec3::Z).normalize_or_zero();
    let dir = if dir.length_squared() < 1e-6 { Vec3::Z } else { dir };

    let mins = entity.fields.vec3("model_mins", Vec3::ZERO);
    let maxs = entity.fields.vec3("model_maxs", Vec3::ZERO);
    let size = maxs - mins;
    // Extent along the movement axis, whatever axis that is.
    let extent = (size.x * dir.x).abs() + (size.y * dir.y).abs() + (size.z * dir.z).abs();
    let lip = entity.fields.f32("lip", 8.0);
    let travel = (extent - lip).max(1.0);

    let speed = entity.fields.f32("speed", 100.0).max(1.0);
    // Spawnflag 1 is "starts open", as Source numbers it. A named bit rather
    // than a key of its own, so it agrees with every other class here.
    let start_open = entity.has_spawnflag(SF_START_OPEN);

    set_field(world, id, "movedir", Value::Vector(dir));
    set_field(world, id, "travel", Value::Float(travel));
    set_field(world, id, "speed", Value::Float(speed));
    set_field(world, id, "progress", Value::Float(if start_open { 1.0 } else { 0.0 }));
    set_field(world, id, "door_state", Value::Int(if start_open { state::OPEN } else { state::CLOSED }));
    set_field(world, id, "locked", Value::Bool(false));

    if start_open {
        if let Some(e) = world.get_mut(id) { e.origin = dir * travel; }
    }
}

fn start(world: &mut EntityWorld, id: EntityId, event: &InputEvent, opening: bool) -> bool {
    if world.get(id).map(|e| e.fields.bool("locked", false)).unwrap_or(false) {
        world.fire_output(id, "OnLockedUse", event.activator, None);
        return true;
    }

    let current = world.get(id).map(|e| e.fields.i32("door_state", state::CLOSED)).unwrap_or(0);
    let already = if opening {
        current == state::OPEN || current == state::OPENING
    } else {
        current == state::CLOSED || current == state::CLOSING
    };
    if already { return true; }

    set_field(world, id, "door_state", Value::Int(if opening { state::OPENING } else { state::CLOSING }));
    set_field(world, id, "last_move", Value::Float(world.time));
    world.fire_output(id, if opening { "OnOpen" } else { "OnClose" }, event.activator, None);
    world.set_think_delay(id, 0.0);
    true
}

fn input_toggle(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let current = world.get(id).map(|e| e.fields.i32("door_state", state::CLOSED)).unwrap_or(0);
    let opening = current == state::CLOSED || current == state::CLOSING;
    start(world, id, event, opening)
}

fn think_door(world: &mut EntityWorld, id: EntityId) {
    let Some(entity) = world.get(id) else { return };
    let door_state = entity.fields.i32("door_state", state::CLOSED);
    let travel = entity.fields.f32("travel", 64.0);
    let speed = entity.fields.f32("speed", 100.0);
    let dir = entity.fields.vec3("movedir", Vec3::Z);
    let mut progress = entity.fields.f32("progress", 0.0);
    let last_move = entity.fields.f32("last_move", world.time);

    // Integrate the elapsed time rather than assuming the interval was met.
    let elapsed = (world.time - last_move).max(0.0);
    let step = (speed * elapsed) / travel.max(1.0);
    let mut next_state = door_state;

    match door_state {
        state::OPENING => {
            progress += step;
            if progress >= 1.0 {
                progress = 1.0;
                next_state = state::OPEN;
            }
        }
        state::CLOSING => {
            progress -= step;
            if progress <= 0.0 {
                progress = 0.0;
                next_state = state::CLOSED;
            }
        }
        _ => return,
    }

    set_field(world, id, "progress", Value::Float(progress));
    set_field(world, id, "last_move", Value::Float(world.time));
    if let Some(e) = world.get_mut(id) { e.origin = dir * (travel * progress); }

    if next_state != door_state {
        set_field(world, id, "door_state", Value::Int(next_state));
        if next_state == state::OPEN {
            world.fire_output(id, "OnFullyOpen", None, None);
            // A positive `wait` closes the door again by itself; -1 leaves it
            // open until something tells it otherwise.
            let wait = field_f32(world, id, "wait", 4.0);
            if wait > 0.0 {
                world.queue_input(
                    void_entity::Target::Myself,
                    "Close",
                    "",
                    wait,
                    None,
                    Some(id),
                );
            }
        } else {
            world.fire_output(id, "OnFullyClosed", None, None);
        }
        return;
    }

    world.set_think_delay(id, MOVE_INTERVAL);
}

/// How far along its travel a door is, in `0..1`.
pub fn door_progress(world: &EntityWorld, id: EntityId) -> f32 {
    world.get(id).map_or(0.0, |e| e.fields.f32("progress", 0.0))
}

/// Whether a `func_brush` is currently solid and drawn.
pub fn brush_enabled(world: &EntityWorld, id: EntityId) -> bool {
    world.get(id).is_none_or(|e| !e.fields.bool("disabled", false))
}
