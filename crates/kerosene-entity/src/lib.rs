// SPDX-License-Identifier: LGPL-3.0-or-later
//! The entity system and its I/O graph.
//!
//! Alongside brush geometry, entity I/O is what makes a Source level a level
//! rather than a model. A button's `OnPressed` fires a door's `Open` after a
//! delay; the door's `OnFullyOpen` fires a relay; the relay fires three more
//! things. No scripting language, no code -- just outputs wired to inputs in
//! the editor, and it composes far further than it has any right to.
//!
//! ```text
//! func_button                 func_door               logic_relay
//! ------------                ---------               -----------
//! OnPressed  --0.0s-->  Open                       
//!                        OnFullyOpen  --0.5s-->  Trigger  --> ...
//! ```
//!
//! Three pieces make it work:
//!
//! * **Entities are a bag of named fields** ([`Fields`]), not typed structs,
//!   because the meaningful fields belong to the game rather than the engine.
//! * **Classes are registered handlers** ([`ClassDef`]), the same split Source
//!   draws between its engine and its game DLL. `kerosene-entity` knows how to
//!   route an input; `kerosene-game` knows what `Open` means.
//! * **Fired outputs become queued events**, not immediate calls. Delays need
//!   a queue anyway, and routing everything through it means an entity firing
//!   an output at itself cannot recurse into the stack.

pub mod io;
mod registry;
pub mod schema;
mod value;
mod world;

pub use io::{Connection, InputEvent, PendingEvent, Target};
pub use registry::{ClassDef, ClassRegistry, InputHandler, SpawnHandler, ThinkHandler};
pub use schema::{ClassKind, ClassSpec, IoSpec, KeyKind, KeySpec, Schema, SchemaError};
pub use value::{Fields, Value};
pub use world::{Entity, EntityId, EntityWorld, HostRequest, SpawnError, host_requests};

/// How many events may be dispatched in one tick before the engine assumes a
/// loop and stops.
///
/// Two relays firing each other with zero delay would otherwise spin forever.
/// Source has the same guard for the same reason.
pub const MAX_EVENTS_PER_TICK: usize = 4096;

/// Special target names an output may address instead of a `targetname`.
pub mod targets {
    /// Whatever set this chain off -- usually the player who touched a trigger.
    pub const ACTIVATOR: &str = "!activator";
    /// The entity that fired the output.
    pub const CALLER: &str = "!caller";
    /// The entity receiving the input, addressing itself.
    pub const SELF: &str = "!self";
    /// The local player.
    pub const PLAYER: &str = "!player";
}
