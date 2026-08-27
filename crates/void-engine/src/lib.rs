// SPDX-License-Identifier: LGPL-3.0-or-later
//! The engine: the host that ties every other crate together.
//!
//! ```text
//!   void-vfs  ->  content            void-console -> convars and commands
//!   void-bsp  ->  the compiled map   void-entity  -> entities and their I/O
//!   void-physics -> how you move     void-render  -> what you see
//!   void-game    -> what things do
//! ```
//!
//! The split that matters most is between [`engine::Engine`] and [`host`].
//! `Engine` is the whole simulation and needs no display: a dedicated server
//! runs exactly it. `host` adds a window, a GPU and an input device on top.
//! Being unable to start a server without a GPU would be a serious mistake in
//! an engine meant to host multiplayer games, so the boundary is enforced by
//! `Engine` simply not knowing what a surface is.

pub mod collision;
pub mod engine;
pub mod host;
pub mod input;

pub use collision::{LevelCollision, Mover};
pub use engine::{Engine, EngineConfig, Level, PlayerState};
pub use input::{HeldActions, InputState, InputSystem};
