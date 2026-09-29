// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Moving brushes: doors, buttons, rotating brushes and switchable ones.
//!
//! A door and a button are the same machine. Both travel along an axis by
//! their own size less a lip, both take a `speed` to do it, both come back
//! after a `wait`, and both can be locked. What differs is only which outputs
//! they fire on the way -- so the state machine is written once and the names
//! it fires are chosen by class. Two copies of this would drift, and the one
//! that drifted would be the one nobody was testing.

use crate::components::Switchable;
use kerosene_ecs::prelude::*;
use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};
use kerosene_math::Vec3;

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
    /// Fired instead of moving when it is locked.
    locked: &'static str,
}

const DOOR_OUTPUTS: MoverOutputs = MoverOutputs {
    start_forward: "OnOpen",
    start_back: Some("OnClose"),
    fully_forward: "OnFullyOpen",
    fully_back: "OnFullyClosed",
    ret: "Close",
    locked: "OnLockedUse",
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
    // Source's name for it on a button, and the one the schema declares.
    locked: "OnUseLocked",
};

fn outputs_for(classname: &str) -> &'static MoverOutputs {
    if classname.eq_ignore_ascii_case("button") {
        &BUTTON_OUTPUTS
    } else {
        &DOOR_OUTPUTS
    }
}

fn outputs_of(world: &EntityWorld, id: EntityId) -> &'static MoverOutputs {
    world
        .get(id)
        .map_or(&DOOR_OUTPUTS, |e| outputs_for(&e.classname))
}

/// A door or a button: what the map says about it, and where it has got to.
///
/// The keys' labels and help live in the schema, because a door and a
/// button word them differently; their types and defaults live here, and
/// the schema's check holds it to them.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Mover {
    /// Which way it travels. Normalised at spawn.
    #[reflect(@Key("movedir"))]
    pub movedir: Vec3,
    /// Units per second.
    #[reflect(@Key("speed"))]
    pub speed: f32,
    /// How much of it stays showing at the far end.
    #[reflect(@Key("lip"))]
    pub lip: f32,
    /// Fires its locked output instead of moving.
    #[reflect(@Key("locked"))]
    pub locked: bool,
    /// Seconds at the far end before coming back; -1 to stay.
    #[reflect(@Key("wait"))]
    pub wait: f32,
    /// Played as it starts to move. Empty for silence.
    #[reflect(@Key("noise_move"))]
    pub noise_move: String,
    /// Played when it reaches either end.
    #[reflect(@Key("noise_stop"))]
    pub noise_stop: String,
    /// Played when something tries it while it is locked.
    #[reflect(@Key("noise_locked"))]
    pub noise_locked: String,
    /// How far it travels, from its geometry. Set at spawn.
    pub travel: f32,
    /// How far along it is: 0 at rest, 1 at the far end.
    pub progress: f32,
    /// One of the `state` constants.
    pub door_state: i32,
    /// Bumped by every move, so a return queued by an earlier one can tell
    /// it is stale.
    pub move_serial: i32,
    /// Game time of the last step, to integrate from.
    pub last_move: f32,
}

impl Default for Mover {
    /// A door's.
    fn default() -> Self {
        Mover {
            movedir: Vec3::Z,
            speed: 100.0,
            lip: 8.0,
            locked: false,
            wait: 4.0,
            noise_move: "door/move".into(),
            noise_stop: String::new(),
            noise_locked: String::new(),
            travel: 64.0,
            progress: 0.0,
            door_state: state::CLOSED,
            move_serial: 0,
            last_move: 0.0,
        }
    }
}

impl Mover {
    /// A button's: it goes into the wall, slower and not as far, pops back
    /// out after a second, and makes no noise unless told to.
    pub fn button() -> Self {
        Mover {
            movedir: -Vec3::Z,
            speed: 40.0,
            lip: 4.0,
            wait: 1.0,
            noise_move: String::new(),
            ..Mover::default()
        }
    }

    /// The mover a class starts with, for an editor drawing where one will
    /// end up before any key is set.
    pub fn of_class(classname: &str) -> Mover {
        if classname.eq_ignore_ascii_case("button") {
            Mover::button()
        } else {
            Mover::default()
        }
    }
}

/// A brush that spins in place.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Rotating {
    /// Degrees per second; negative turns the other way.
    #[reflect(@Key("maxspeed"))]
    pub maxspeed: f32,
    /// Whether it is turning.
    pub spinning: bool,
    /// Game time of the last step, to integrate from.
    pub last_move: f32,
}

impl Default for Rotating {
    fn default() -> Self {
        Rotating {
            maxspeed: 100.0,
            spinning: false,
            last_move: 0.0,
        }
    }
}

/// Which way a door is going.
mod state {
    pub const CLOSED: i32 = 0;
    pub const OPENING: i32 = 1;
    pub const OPEN: i32 = 2;
    pub const CLOSING: i32 = 3;
}

/// Register the movers: `door`, `button`, `rotating` and
/// `brush`.
pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("door")
            .component::<Mover>()
            .on_spawn(spawn_mover)
            .on_think(think_mover)
            .input("Open", |w, id, e| start(w, id, e, true))
            .input("Close", |w, id, e| start(w, id, e, false))
            .input("Toggle", input_toggle)
            // Pressing a door is a toggle, which is what makes the use key
            // work on it without the map wiring anything at all.
            .input("Use", input_toggle)
            .input("Lock", |w, id, _| set_locked(w, id, true))
            .input("Unlock", |w, id, _| set_locked(w, id, false))
            .input("SetSpeed", |w, id, e| {
                if let Some(v) = e.parameter_f32()
                    && let Some(m) = w.component_mut::<Mover>(id)
                {
                    m.speed = v;
                }
                true
            })
            .output("OnOpen")
            .output("OnClose")
            .output("OnFullyOpen")
            .output("OnFullyClosed")
            .output("OnLockedUse"),
    );

    registry.register(
        ClassDef::new("button")
            .component_with(Mover::button)
            .on_spawn(spawn_mover)
            .on_think(think_mover)
            // Press and Use are the same act from two directions: a player
            // looking at it, or a map firing at it.
            .input("Press", |w, id, e| start(w, id, e, true))
            .input("Use", |w, id, e| start(w, id, e, true))
            .input("Unpress", |w, id, e| start(w, id, e, false))
            .input("Lock", |w, id, _| set_locked(w, id, true))
            .input("Unlock", |w, id, _| set_locked(w, id, false))
            .output("OnPressed")
            .output("OnIn")
            .output("OnOut")
            .output("OnUseLocked"),
    );

    registry.register(
        ClassDef::new("rotating")
            .component::<Rotating>()
            .on_spawn(spawn_rotating)
            .on_think(think_rotating)
            .input("Start", |w, id, _| {
                set_spinning(w, id, true);
                true
            })
            .input("Stop", |w, id, _| {
                set_spinning(w, id, false);
                true
            })
            .input("Toggle", |w, id, _| {
                let on = w.component::<Rotating>(id).is_some_and(|r| r.spinning);
                set_spinning(w, id, !on);
                true
            })
            .input("Reverse", |w, id, _| {
                if let Some(r) = w.component_mut::<Rotating>(id) {
                    r.maxspeed = -r.maxspeed;
                }
                true
            })
            .input("SetSpeed", |w, id, e| {
                if let Some(v) = e.parameter_f32()
                    && let Some(r) = w.component_mut::<Rotating>(id)
                {
                    r.maxspeed = v;
                }
                true
            }),
    );

    registry.register(
        ClassDef::new("brush")
            .component::<Switchable>()
            .input("Enable", |w, id, _| {
                crate::components::set_disabled(w, id, false)
            })
            .input("Disable", |w, id, _| {
                crate::components::set_disabled(w, id, true)
            })
            .input("Toggle", |w, id, _| {
                crate::components::toggle_disabled(w, id)
            }),
    );
}

fn spawn_rotating(world: &mut EntityWorld, id: EntityId) {
    let on = world.get(id).is_some_and(|e| e.has_spawnflag(SF_START_ON));
    set_spinning(world, id, on);
}

/// Start or stop a rotating brush.
///
/// Stopping clears the think rather than leaving one scheduled that does
/// nothing: a map with fifty stopped fans should cost nothing to run.
fn set_spinning(world: &mut EntityWorld, id: EntityId, on: bool) {
    let now = world.time;
    let Some(r) = world.component_mut::<Rotating>(id) else {
        return;
    };
    r.spinning = on;
    if on {
        // The clock restarts, or a fan switched on after a minute would jump
        // through a minute's worth of rotation on its first think.
        r.last_move = now;
        world.set_think_delay(id, 0.0);
    } else {
        world.clear_think(id);
    }
}

fn think_rotating(world: &mut EntityWorld, id: EntityId) {
    let Some(rotating) = world.component::<Rotating>(id).cloned() else {
        return;
    };
    let Some(entity) = world.get(id) else { return };
    if !rotating.spinning {
        return;
    }

    let speed = rotating.maxspeed;
    let elapsed = (world.time - rotating.last_move).max(0.0);
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

    if let Some(e) = world.get_mut(id) {
        e.angles = angles
    }
    let now = world.time;
    if let Some(r) = world.component_mut::<Rotating>(id) {
        r.last_move = now;
    }
    world.set_think_delay(id, MOVE_INTERVAL);
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
    let dir = if dir.length_squared() < 1e-6 {
        Vec3::Z
    } else {
        dir
    };
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
    // The brush's bounds, which the loader puts on every brush entity.
    let mins = entity.fields.vec3("model_mins", Vec3::ZERO);
    let maxs = entity.fields.vec3("model_maxs", Vec3::ZERO);
    // Spawnflag 1 is "starts open", as Source numbers it. A named bit rather
    // than a key of its own, so it agrees with every other class here.
    let start_open = entity.has_spawnflag(SF_START_OPEN);
    let Some(mover) = world.component_mut::<Mover>(id) else {
        return;
    };

    let (dir, travel) = travel(maxs - mins, mover.movedir, mover.lip);
    mover.movedir = dir;
    mover.travel = travel;
    mover.speed = mover.speed.max(1.0);
    mover.progress = if start_open { 1.0 } else { 0.0 };
    mover.door_state = if start_open {
        state::OPEN
    } else {
        state::CLOSED
    };
    // A mover's noises play once each; `looping` is what the engine's
    // entity sound reads, and it defaults to on for the ambient classes.
    world.set_keyvalue(id, "looping", Value::Bool(false));

    if start_open && let Some(e) = world.get_mut(id) {
        e.origin = dir * travel;
    }
}

/// The parameter a mover's own `wait` return carries: which move it belongs
/// to. A return queued by a move that has since been undone -- closed by
/// hand and opened again -- must not cut the new one's wait short.
const AUTO_RETURN: &str = "auto_return:";

fn start(world: &mut EntityWorld, id: EntityId, event: &InputEvent, opening: bool) -> bool {
    let Some(mover) = world.component::<Mover>(id).cloned() else {
        return false;
    };
    if let Some(serial) = event.parameter.strip_prefix(AUTO_RETURN)
        && serial.parse::<i32>().ok() != Some(mover.move_serial)
    {
        return true;
    }
    if mover.locked {
        let locked = outputs_of(world, id).locked;
        noise(world, id, &mover.noise_locked, event.activator);
        world.fire_output(id, locked, event.activator, None);
        return true;
    }

    let already = if opening {
        mover.door_state == state::OPEN || mover.door_state == state::OPENING
    } else {
        mover.door_state == state::CLOSED || mover.door_state == state::CLOSING
    };
    if already {
        return true;
    }

    let outputs = outputs_of(world, id);
    let now = world.time;
    if let Some(m) = world.component_mut::<Mover>(id) {
        m.move_serial = m.move_serial.wrapping_add(1);
        m.door_state = if opening {
            state::OPENING
        } else {
            state::CLOSING
        };
        m.last_move = now;
    }
    noise(world, id, &mover.noise_move, event.activator);
    let announce = if opening {
        Some(outputs.start_forward)
    } else {
        outputs.start_back
    };
    if let Some(name) = announce {
        world.fire_output(id, name, event.activator, None);
    }
    world.set_think_delay(id, 0.0);
    true
}

fn set_locked(world: &mut EntityWorld, id: EntityId, locked: bool) -> bool {
    if let Some(m) = world.component_mut::<Mover>(id) {
        m.locked = locked;
    }
    true
}

fn input_toggle(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let current = world
        .component::<Mover>(id)
        .map_or(state::CLOSED, |m| m.door_state);
    let opening = current == state::CLOSED || current == state::CLOSING;
    start(world, id, event, opening)
}

fn think_mover(world: &mut EntityWorld, id: EntityId) {
    let Some(mover) = world.component::<Mover>(id).cloned() else {
        return;
    };
    let mut progress = mover.progress;

    // Integrate the elapsed time rather than assuming the interval was met.
    let elapsed = (world.time - mover.last_move).max(0.0);
    let step = (mover.speed * elapsed) / mover.travel.max(1.0);
    let mut next_state = mover.door_state;

    match mover.door_state {
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

    let now = world.time;
    if let Some(m) = world.component_mut::<Mover>(id) {
        m.progress = progress;
        m.last_move = now;
        m.door_state = next_state;
    }
    if let Some(e) = world.get_mut(id) {
        e.origin = mover.movedir * (mover.travel * progress);
    }

    if next_state != mover.door_state {
        let outputs = outputs_of(world, id);
        noise(world, id, &mover.noise_stop, None);
        if next_state == state::OPEN {
            world.fire_output(id, outputs.fully_forward, None, None);
            // A positive `wait` sends it back by itself; -1 leaves it where it
            // is until something tells it otherwise.
            if mover.wait > 0.0 {
                world.queue_input(
                    kerosene_entity::Target::Myself,
                    outputs.ret,
                    &format!("{AUTO_RETURN}{}", mover.move_serial),
                    mover.wait,
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

/// Play one of a mover's noises, unless it is empty.
fn noise(world: &mut EntityWorld, id: EntityId, name: &str, activator: Option<EntityId>) {
    if !name.trim().is_empty() {
        world.request(host_requests::PLAY_SOUND, name.trim(), id, activator);
    }
}

/// How far along its travel a door is, in `0..1`.
pub fn door_progress(world: &EntityWorld, id: EntityId) -> f32 {
    world.component::<Mover>(id).map_or(0.0, |m| m.progress)
}

/// Whether a `brush` is currently solid and drawn.
pub fn brush_enabled(world: &EntityWorld, id: EntityId) -> bool {
    world
        .component::<Switchable>(id)
        .is_none_or(|s| !s.disabled)
}
