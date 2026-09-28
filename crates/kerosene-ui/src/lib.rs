// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The game UI: HUDs, menus, overlays and world panels, written as content.
//!
//! Source 2 draws its interface with Panorama -- XML layouts, CSS styles and
//! script, loaded from the game's content like any other asset -- and that is
//! the shape this takes, with the pieces Kerosene already has standing in for
//! a browser's:
//!
//! | | |
//! |---|---|
//! | Layout | `.kui`, XML ([`markup`]) |
//! | Style | `.kcss`, a subset of CSS ([`css`], [`style`]) laid out with flexbox by [taffy] |
//! | Behaviour | `.kscr`, Rhai, sandboxed like a level's scripts ([`script`]) |
//! | State | a [`UiStore`] of dotted keys the game publishes |
//! | Wiring | `{expressions}` in attributes that re-run when their keys change ([`bind`]) |
//!
//! The last two are the part that is not Panorama. A HUD element *declares*
//! what it shows -- `<Label text="{weapon.ammo}"/>`,
//! `class="crosshair xh-{weapon.active}"` -- the way an HTMX page declares what
//! it fetches, rather than a script listening for an event and poking the
//! element. Most of a HUD needs no script at all.
//!
//! # What this crate is not
//!
//! It does not draw. Everything ends in a [`DisplayList`] of quads, which
//! `kerosene-render` turns into a draw call or two; it does not know about
//! winit, so input arrives as [`UiInput`]; and it does not read files except
//! through a [`Loader`], which the engine points at its VFS. That keeps the
//! whole of it -- cascade, layout, text, bindings, scripts, input -- testable
//! without a window or a GPU, which is how it is tested.
//!
//! ```
//! use kerosene_ui::{UiStore, UiSystem};
//! use std::collections::BTreeMap;
//!
//! let mut files = BTreeMap::new();
//! files.insert("ui/hud.kui".to_string(), r#"
//!     <root>
//!         <Label id="ammo" text="{weapon.ammo} / {weapon.reserve}"/>
//!     </root>"#.to_string());
//!
//! let mut ui = UiSystem::new();
//! ui.show("hud", "ui/hud.kui", &files).unwrap();
//!
//! let mut store = UiStore::new();
//! store.set("weapon.ammo", 12);
//! store.set("weapon.reserve", 36);
//! ui.update(0.016, (1920, 1080), &mut store, &files);
//!
//! let hud = ui.document("hud").unwrap();
//! assert_eq!(hud.attr(hud.find("ammo").unwrap(), "text"), Some("12 / 36"));
//! ```

pub mod bind;
pub mod css;
pub mod document;
pub mod markup;
pub mod script;
pub mod store;
pub mod style;
pub mod system;
pub mod text;

pub use document::{Document, NodeId, REFERENCE_HEIGHT};
pub use kerosene_scene::{ClipRect, DisplayList, DrawItem, Images, Quad, TextureRef, draw};
pub use store::{Event, UiStore, Value};
pub use system::UiSystem;
pub use text::{ATLAS_SIZE, Fonts, GlyphAtlas};

use std::collections::BTreeMap;

/// Extension of a layout file.
pub const LAYOUT_EXTENSION: &str = kerosene_vfs::ext::UI_LAYOUT;
/// Extension of a stylesheet.
pub const STYLE_EXTENSION: &str = kerosene_vfs::ext::UI_STYLE;

/// Where documents read their files from.
pub trait Loader {
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

impl Loader for kerosene_vfs::Vfs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        kerosene_vfs::Vfs::read(self, path).ok()
    }
}

/// In-memory files, for tests and for layouts built in code.
impl Loader for BTreeMap<String, String> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.get(path).map(|s| s.as_bytes().to_vec())
    }
}

/// Severity of a message a document produced.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Something a document asked the engine to do.
///
/// The UI's whole reach outside itself, listed. A script cannot touch the
/// world; it can ask for one of these.
#[derive(Clone, PartialEq, Debug)]
pub enum UiAction {
    /// Run console text, as typed.
    Command(String),
    /// Set a convar (through the console, so cheat flags hold).
    SetCvar {
        name: String,
        value: String,
    },
    /// Play a UI sound, heard flat.
    PlaySound(String),
    /// An event for the game: a keypad code entered, a menu choice. `source`
    /// is the layer (`hud`) or world panel (`panel:<name>`) that sent it.
    Emit {
        name: String,
        data: String,
        source: String,
    },
    /// Put a layout on a layer.
    ShowLayer {
        layer: String,
        path: String,
    },
    HideLayer(String),
    Log(LogLevel, String),
    /// Something for the store -- an achievement, a stat, a score -- from
    /// the `platform` (or `steam`) object.
    Platform(kerosene_platform::PlatformAction),
}

/// Keys the UI does something with. Everything else is left to the game.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiKey {
    Tab,
    BackTab,
    Enter,
    Space,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Backspace,
}

/// Input, already translated from the window system.
#[derive(Clone, PartialEq, Debug)]
pub enum UiInput {
    /// The pointer is at `(x, y)` physical pixels.
    PointerMove {
        x: f32,
        y: f32,
    },
    /// The primary button went down or up.
    PointerButton {
        down: bool,
    },
    Key(UiKey),
    /// Typed text.
    Text(String),
}
