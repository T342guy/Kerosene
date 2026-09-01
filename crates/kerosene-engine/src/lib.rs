// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The engine: the host that ties every other crate together.
//!
//! ```text
//!   kerosene-vfs  ->  content            kerosene-console -> convars and commands
//!   kerosene-bsp  ->  the compiled map   kerosene-entity  -> entities and their I/O
//!   kerosene-physics -> how you move     kerosene-render  -> what you see
//!   kerosene-game    -> what things do
//! ```
//!
//! The split that matters most is between [`engine::Engine`] and [`host`].
//! `Engine` is the whole simulation and needs no display: a dedicated server
//! runs exactly it. `host` adds a window, a GPU and an input device on top.
//! Being unable to start a server without a GPU would be a serious mistake in
//! an engine meant to host multiplayer games, so the boundary is enforced by
//! `Engine` simply not knowing what a surface is.

pub mod audio;
pub mod console_ui;
pub mod collision;
pub mod engine;
pub mod host;
pub mod input;
pub mod scripting;

pub use collision::{LevelCollision, Mover};
pub use engine::{Engine, EngineConfig, Level, PlayerState};
pub use input::{HeldActions, InputState, InputSystem};
