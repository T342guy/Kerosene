// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! The start/stop manager: who starts what, in which order, who owns each
//! thread, and how it all comes down again.
//!
//! Every part of the engine that wants to run work of its own is a
//! [`Module`]. A [`Manager`] starts them in three [`Phase`]s -- the engine's
//! own, then module crates, then the game's -- and stops them in exactly the
//! reverse order. Starting is two steps: every module *preloads* (loads
//! what it needs while nothing is running yet) before any module *starts*
//! (brings its threads up with all that data in place).
//!
//! A module never calls `std::thread::spawn`. It asks its [`Ctx`] for a
//! named thread or a bounded [`Pool`]; the manager keeps the handles, so a
//! stop can signal every thread, wait for it, and give up on one that
//! ignores the signal rather than hang the exit. A panic on a managed
//! thread is reported to [`kerror`] as fatal instead of vanishing.
//!
//! The [`mem`] module is the other half: a counting global allocator, with
//! each managed thread's allocations counted under its module's name.
//!
//! ```
//! use kerosene_lifecycle::{Ctx, Manager, Module, Phase};
//!
//! struct Hello;
//! impl Module for Hello {
//!     fn name(&self) -> &'static str { "hello" }
//!     fn start(&mut self, ctx: &mut Ctx) -> kerror::Result<()> {
//!         ctx.spawn("worker", |token| {
//!             while !token.is_stopped() {
//!                 token.sleep(std::time::Duration::from_millis(5));
//!             }
//!         })
//!     }
//! }
//!
//! let mut manager = Manager::new();
//! manager.register(Phase::Modules, Hello);
//! manager.start().unwrap();
//! assert_eq!(manager.thread_names(), ["kerosene-hello-worker"]);
//! manager.stop();
//! assert!(manager.thread_names().is_empty());
//! ```
#![warn(missing_docs)]

mod manager;
pub mod mem;
mod pool;
mod thread;

pub use manager::{Ctx, Manager, Module, Phase, default_workers};
pub use mem::TrackingAllocator;
pub use pool::Pool;
pub use thread::StopToken;

#[cfg(test)]
mod tests;
