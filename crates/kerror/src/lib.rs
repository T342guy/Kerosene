// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! This crate, the `kerror` crate, is the custom error handling and values tool used in kerosene.
//! Custom errors are used to better discribe failures (obviously), and make things easier in terms of debugging.
//! All kerosene errors are appended by the `kengine` name, as the error is assosiated with the engine and not an external problem.
//! Any error outside of kerosene, like game-facing code, uses the appendant `kgameErr_`, followed by its name.
//!
//! While all might seem like bool errors, others are functions to trigger graceful aborts, resets, amongst other things like warnings and failure counters.
//!
//! `kerror` also handles health checking, so the error tracking value `kengineCountRenderIssueRecovery` would be counting up from how many render errors were,
//! and then recovered from successfully.

mod abort;
mod error;
mod health;

pub use abort::{
    Shutdown, ShutdownReason, abort, install_signal_handlers, on_shutdown, poll_shutdown,
    request_shutdown, run_shutdown_hooks, shutdown, shutdown_reason, shutdown_requested,
};
pub use error::{EngineError, GameError, KError, Result, Severity};
pub use health::{Health, health, recovery_id};
