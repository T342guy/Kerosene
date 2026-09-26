// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The base content: what every game has before it has anything of its own.
//!
//! A game made with `kerosene-tools new` starts with an empty content tree,
//! and a game crate with no content at all still has to start. Both work
//! because the engine carries a small archive of its own -- the developer
//! and tool textures, the stock props, sounds and sound table, the default
//! HUD and menus, and the demo map `kero_start` -- compiled into the binary
//! and mounted beneath everything else.
//!
//! Beneath, so it is only ever a fallback: a game that ships
//! `ui/hud.keroui` sees its own HUD, and a file the game has not replaced is
//! found here. `path` in the console lists it last, as `BASE`.
//!
//! The archive is `base/base.vault`, packed from the repository's content by
//! `scripts/build-content.sh` and listed by `base/MANIFEST`. A test in this
//! module fails when it no longer matches the files it was packed from.

use kerosene_vfs::Vfs;

/// The packed base content.
pub static BASE_VAULT: &[u8] = include_bytes!("../base/base.vault");

/// The demo map in the base content: what the engine opens when nothing
/// names a map to start on.
pub const DEMO_MAP: &str = "kero_start";

/// The search-path id the base content is mounted under.
pub const BASE_ID: &str = "BASE";

/// Mount the base content at the end of `vfs`'s search order.
pub fn mount(vfs: &mut Vfs) {
    // Compiled in and checked by a test, so failing here is a broken build
    // rather than a missing file; said, and carried on without.
    if let Err(e) = vfs.mount_static(BASE_VAULT, "base content", BASE_ID) {
        log::error!("the engine's base content would not mount: {e}");
    }
}

#[cfg(test)]
mod tests;
