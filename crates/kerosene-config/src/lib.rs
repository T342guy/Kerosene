// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Engine configuration: the file every program reads to decide how to start.
//!
//! A game has settings that are nobody's map and nobody's project: which
//! renderer to use, how big the window is, whether it syncs to the display.
//! They live in one file, `engineconf.keroconfig`, at the top of the content
//! tree, and they always exist: the first program to look for the file and
//! not find it writes one with the defaults in it.
//!
//! The file is KeyValues, like everything else a person might edit by hand:
//!
//! ```text
//! engineconf
//! {
//!     "renderer" "vulkan"
//!     "width"    "1280"
//!     "height"   "720"
//!     "vsync"    "1"
//! }
//! ```
//!
//! Every key is optional and every key falls back to a default, so a file
//! with nothing in it is a valid answer, and one with a mistake in it still
//! starts -- it just logs the mistake and uses the default for that key.

use std::path::Path;

mod renderer;
pub mod gpu;
#[cfg(test)]
mod tests;

pub use renderer::Renderer;

/// The file's name, wherever the content root is.
pub const FILENAME: &str = "engineconf.keroconfig";

/// The window size a config defaults to, in pixels.
pub const DEFAULT_WIDTH: u32 = 1280;
pub const DEFAULT_HEIGHT: u32 = 720;

/// What `engineconf.keroconfig` says, with every field defaulted.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineConf {
    pub renderer: Renderer,
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
}

impl Default for EngineConf {
    fn default() -> Self {
        EngineConf {
            renderer: Renderer::default(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            vsync: true,
        }
    }
}

impl EngineConf {
    /// Read the config beside a content root, writing the defaults when there
    /// is no file there.
    ///
    /// Never fails and never leaves the caller without an answer: an unreadable
    /// or unwritable file is a warning, not a reason to refuse to start. That
    /// is the whole point of "always exists" -- the engine works on a fresh
    /// clone and in a read-only install alike.
    pub fn load_or_create(root: &Path) -> EngineConf {
        let path = root.join(FILENAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => EngineConf::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let conf = EngineConf::default();
                let text = conf.to_document();
                match std::fs::write(&path, text) {
                    Ok(()) => log::info!("wrote default engine config {}", path.display()),
                    Err(e) => log::warn!("could not write {} ({e}); using defaults", path.display()),
                }
                conf
            }
            Err(e) => {
                log::warn!("could not read {} ({e}); using defaults", path.display());
                EngineConf::default()
            }
        }
    }

    /// Parse a config file's text, defaulting anything absent or wrong.
    pub fn parse(text: &str) -> EngineConf {
        let kv = match kerosene_kv::KeyValues::parse(text) {
            Ok(kv) => kv,
            Err(e) => {
                log::warn!("engine config did not parse ({e}); using defaults");
                return EngineConf::default();
            }
        };
        // Accept the block whether it is the document root or nested inside
        // one, for the same reason the project file reader does.
        let block = kv.block("engineconf").unwrap_or(&kv);

        let renderer = match block.get("renderer").map(str::trim).filter(|s| !s.is_empty()) {
            None => Renderer::default(),
            Some(name) => match Renderer::from_str(name) {
                Some(renderer) => renderer,
                None => {
                    log::warn!("unknown renderer {name:?}; using {}", Renderer::default().label());
                    Renderer::default()
                }
            },
        };

        EngineConf {
            renderer,
            width: number(block, "width", DEFAULT_WIDTH),
            height: number(block, "height", DEFAULT_HEIGHT),
            vsync: boolean(block, "vsync", true),
        }
    }

    /// Serialise as a config file, with the comments a person wants.
    pub fn to_document(&self) -> String {
        let mut kv = kerosene_kv::KeyValues::new("engineconf");
        kv.push("renderer", self.renderer.label());
        kv.push_value("width", self.width);
        kv.push_value("height", self.height);
        kv.push_value("vsync", self.vsync);

        format!(
            "// Kerosene engine configuration.\n\
             // Written the first time a program needed it; every key below is\n\
             // optional and falls back to its default when absent or wrong.\n\
             {}\n",
            kv.to_text()
        )
    }
}

/// A numeric key, defaulted -- and warned about -- when absent or malformed.
fn number(block: &kerosene_kv::KeyValues, key: &str, default: u32) -> u32 {
    match block.optional::<u32>(key) {
        Ok(Some(value)) => value.max(1),
        Ok(None) => default,
        Err(_) => {
            log::warn!("engine config key {key:?} is not a number; using {default}");
            default
        }
    }
}

/// A boolean key, defaulted -- and warned about -- when absent or malformed.
fn boolean(block: &kerosene_kv::KeyValues, key: &str, default: bool) -> bool {
    match block.optional::<bool>(key) {
        Ok(Some(value)) => value,
        Ok(None) => default,
        Err(_) => {
            log::warn!("engine config key {key:?} is not a boolean; using {default}");
            default
        }
    }
}
