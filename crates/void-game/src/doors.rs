// SPDX-License-Identifier: LGPL-3.0-or-later
//! Moving brushes: doors, buttons, rotating brushes and switchable ones.
//!
//! A door and a button are the same machine. Both travel along an axis by
//! their own size less a lip, both take a `speed` to do it, both come back
//! after a `wait`, and both can be locked. What differs is only which outputs
//! they fire on the way -- so the state machine is written once and the names
//! it fires are chosen by class. Two copies of this would drift, and the one
//! that drifted would be the one nobody was testing.

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
/// A rotating brush that is already turning when the map starts.
pub const SF_START_ON: u32 = 1;
/// Turn about the forward axis rather than up.
pub const SF_ROTATE_X: u32 = 2;
/// Turn about the left axis rather than up.
pub const SF_ROTATE_Y: u32 = 4;

/// The outputs one class of mover fires, and the input that sends it back.
///
/// A door opens and closes; a button presses in and pops out. Same movement,
/// different vocabulary, and a designer wiring one should see the words that
/// belong to the thing in front of them.
struct MoverOutputs {
    /// Fired when it starts travelling away from its resting position.
    start_forward: &'static str,
    /// Fired when it starts travelling back. Buttons announce nothing here:
    /// popping back out is not an event anyone wires to.
    start_back: Option<&'static str>,
    /// Fired on arrival at the far end.
    fully_forward: &'static str,
    /// Fired on arrival back home.
    fully_back: &'static str,
    /// The input `wait` fires at itself to come back.
    ret: &'static str,
}

const DOOR_OUTPUTS: MoverOutputs = MoverOutputs {
    start_forward: "OnOpen",
    start_back: Some("OnClose"),
    fully_forward: "OnFullyOpen",
    fully_back: "OnFullyClosed",
    ret: "Close",
};

const BUTTON_OUTPUTS: MoverOutputs = MoverOutputs {
    // Source fires OnPressed the moment the button is pressed rather than when
    // it finishes moving, and that is the right choice: a designer wiring a
    // button wants the door to start opening as the button goes in, not a
    // quarter second later.
    start_forward: "OnPressed",
    start_back: None,
    fully_forward: "OnIn",
    fully_back: "OnOut",
    ret: "Unpress",
};

fn outputs_for(classname: &str) -> &'static MoverOutputs {
    if classname.eq_ignore_ascii_case("func_button") { &BUTTON_OUTPUTS } else { &DOOR_OUTPUTS }
}

fn outputs_of(world: &EntityWorld, id: EntityId) -> &'static MoverOutputs {
    world.get(id).map_or(&DOOR_OUTPUTS, |e| outputs_for(&e.classname))
}

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
            .on_spawn(spawn_mover)
            .on_think(think_mover)
            .input("Open", |w, id, e| start(w, id, e, true))
            .input("Close", |w, id, e| start(w, id, e, false))
            .input("Toggle", input_toggle)
            // Pressing a door is a toggle, which is what makes the use key
            // work on it without the map wiring anything at all.
            .input("Use", input_toggle)
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
        ClassDef::new("func_button")
            .on_spawn(spawn_mover)
            .on_think(think_mover)
            // Press and Use are the same act from two directions: a player
            // looking at it, or a map firing at it.
            .input("Press", |w, id, e| start(w, id, e, true))
            .input("Use", |w, id, e| start(w, id, e, true))
            .input("Unpress", |w, id, e| start(w, id, e, false))
            .input("Lock", |w, id, _| { set_field(w, id, "locked", Value::Bool(true)); true })
            .input("Unlock", |w, id, _| { set_field(w, id, "locked", Value::Bool(false)); true })
            .output("OnPressed")
            .output("OnIn")
            .output("OnOut")
            .output("OnUseLocked"),
    );

    registry.register(
        ClassDef::new("func_rotating")
            .on_spawn(spawn_rotating)
            .on_think(think_rotating)
            .input("Start", |w, id, _| { set_spinning(w, id, true); true })
            .input("Stop", |w, id, _| { set_spinning(w, id, false); true })
            .input("Toggle", |w, id, _| {
                let on = w.get(id).is_some_and(|e| e.fields.bool("spinning", false));
                set_spinning(w, id, !on);
                true
            })
            .input("Reverse", |w, id, _| {
                let speed = field_f32(w, id, "maxspeed", 100.0);
                set_field(w, id, "maxspeed", Value::Float(-speed));
                true
            })
            .input("SetSpeed", |w, id, e| {
                if let Some(v) = e.parameter_f32() { set_field(w, id, "maxspeed", Value::Float(v)); }
                true
            }),
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

fn spawn_rotating(world: &mut EntityWorld, id: EntityId) {
    let on = world.get(id).is_some_and(|e| e.has_spawnflag(SF_START_ON));
    set_field(world, id, "last_move", Value::Float(world.time));
    set_spinning(world, id, on);
}

/// Start or stop a rotating brush.
///
/// Stopping clears the think rather than leaving one scheduled that does
/// nothing: a map with fifty stopped fans should cost nothing to run.
fn set_spinning(world: &mut EntityWorld, id: EntityId, on: bool) {
    set_field(world, id, "spinning", Value::Bool(on));
    if on {
        // The clock restarts, or a fan switched on after a minute would jump
        // through a minute's worth of rotation on its first think.
        set_field(world, id, "last_move", Value::Float(world.time));
        world.set_think_delay(id, 0.0);
    } else {
        world.clear_think(id);
    }
}

fn think_rotating(world: &mut EntityWorld, id: EntityId) {
    let Some(entity) = world.get(id) else { return };
    if !entity.fields.bool("spinning", false) { return }

    let speed = entity.fields.f32("maxspeed", 100.0);
    let elapsed = (world.time - entity.fields.f32("last_move", world.time)).max(0.0);
    // Source's numbering: 2 turns about forward, 4 about left, otherwise up.
    let turned = speed * elapsed;
    let mut angles = entity.angles;
    if entity.has_spawnflag(SF_ROTATE_X) {
        angles.roll += turned;
    } else if entity.has_spawnflag(SF_ROTATE_Y) {
        angles.pitch += turned;
    } else {
        angles.yaw += turned;
    }
    // Wrapped every think, so a fan left running for an hour does not lose
    // precision to a number that only ever grows.
    let angles = angles.normalized();

    if let Some(e) = world.get_mut(id) { e.angles = angles }
    set_field(world, id, "last_move", Value::Float(world.time));
    world.set_think_delay(id, MOVE_INTERVAL);
}

fn spawn_brush(world: &mut EntityWorld, id: EntityId) {
    let start_disabled = world.get(id).map(|e| e.fields.bool("startdisabled", false)).unwrap_or(false);
    set_field(world, id, "disabled", Value::Bool(start_disabled));
}

/// How far a door travels, and which way.
///
/// The distance comes from the geometry, not from a keyvalue: a door moves by
/// its own size along its movement axis, less the `lip` that stays visible.
/// That is what lets a designer resize a door and have it still work.
///
/// Public, and used by the editor as well as by the door itself, because the
/// editor draws where a door will end up. Two copies of this formula would
/// mean the picture and the behaviour agreeing only by luck.
pub fn travel(size: Vec3, movedir: Vec3, lip: f32) -> (Vec3, f32) {
    let dir = movedir.normalize_or_zero();
    let dir = if dir.length_squared() < 1e-6 { Vec3::Z } else { dir };
    // Extent along the movement axis, whatever axis that is.
    let extent = (size.x * dir.x).abs() + (size.y * dir.y).abs() + (size.z * dir.z).abs();
    (dir, (extent - lip).max(1.0))
}

/// Work out how far the door travels and which way.
///
/// The distance comes from the geometry, not from a keyvalue: a door moves by
/// its own size along its movement axis, less the `lip` that stays visible.
/// That is what lets a designer resize a door and have it still work.
fn spawn_mover(world: &mut EntityWorld, id: EntityId) {
    let Some(entity) = world.get(id) else { return };

    let mins = entity.fields.vec3("model_mins", Vec3::ZERO);
    let maxs = entity.fields.vec3("model_maxs", Vec3::ZERO);
    let (dir, travel) = travel(
        maxs - mins,
        entity.fields.vec3("movedir", Vec3::Z),
        entity.fields.f32("lip", 8.0),
    );

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

    if start_open
        && let Some(e) = world.get_mut(id)
    {
        e.origin = dir * travel;
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

    let outputs = outputs_of(world, id);
    set_field(world, id, "door_state", Value::Int(if opening { state::OPENING } else { state::CLOSING }));
    set_field(world, id, "last_move", Value::Float(world.time));
    let announce = if opening { Some(outputs.start_forward) } else { outputs.start_back };
    if let Some(name) = announce {
        world.fire_output(id, name, event.activator, None);
    }
    world.set_think_delay(id, 0.0);
    true
}

fn input_toggle(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let current = world.get(id).map(|e| e.fields.i32("door_state", state::CLOSED)).unwrap_or(0);
    let opening = current == state::CLOSED || current == state::CLOSING;
    start(world, id, event, opening)
}

fn think_mover(world: &mut EntityWorld, id: EntityId) {
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
        let outputs = outputs_of(world, id);
        set_field(world, id, "door_state", Value::Int(next_state));
        if next_state == state::OPEN {
            world.fire_output(id, outputs.fully_forward, None, None);
            // A positive `wait` sends it back by itself; -1 leaves it where it
            // is until something tells it otherwise.
            let wait = field_f32(world, id, "wait", 4.0);
            if wait > 0.0 {
                world.queue_input(
                    void_entity::Target::Myself,
                    outputs.ret,
                    "",
                    wait,
                    None,
                    Some(id),
                );
            }
        } else {
            world.fire_output(id, outputs.fully_back, None, None);
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
