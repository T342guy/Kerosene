// SPDX-License-Identifier: LGPL-3.0-or-later
//! Sound: decoding, mixing, and getting it to the speakers.
//!
//! Three layers, separable on purpose:
//!
//! * [`wav`] turns a file into samples. Written out rather than pulled in,
//!   because a decoder is somewhere an unexpected file should produce an
//!   error rather than a panic.
//! * [`mixer`] turns voices into a stereo buffer. Pure arithmetic, no device,
//!   which is what makes panning and falloff testable rather than something
//!   you notice by ear on the third playthrough.
//! * [`device`] hands that buffer to the sound card, behind a feature flag,
//!   because it is the only part that needs a C library on Linux and the only
//!   part that cannot run in a test.
//!
//! Between them sits [`SoundBank`]: the thing that turns `"door/open"` into
//! samples, through a script that says which files that name might mean.
//!
//! A game with no working audio device is a game, not an error. Every entry
//! point here degrades to silence and says so once.

use std::collections::HashMap;
use std::sync::Arc;

pub mod adpcm;
pub mod compiled;
pub mod mixer;
pub mod script;
pub mod wav;

#[cfg(feature = "device")]
pub mod device;

pub use mixer::{Listener, Mixer, SoundHandle, SoundParams, gains_for};
pub use script::{SoundDef, SoundScript};
pub use wav::Sound;

/// The extension a sound script uses.
pub const SCRIPT_EXTENSION: &str = "voidsnd";

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("{0}")]
    Malformed(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("no sound named `{0}`")]
    NoSuchSound(String),
    #[error("no audio device: {0}")]
    NoDevice(String),
}

/// Loaded sounds, by the name content refers to them by.
///
/// The indirection matters for the same reason materials do: a level says
/// `door/open`, and which file that is -- and how loud, and how far it
/// carries -- is one edit in a script rather than a hunt through entities.
#[derive(Default)]
pub struct SoundBank {
    script: SoundScript,
    loaded: HashMap<String, Arc<Sound>>,
    /// Names that failed to load, so a missing file is complained about once
    /// rather than every time a trigger fires.
    missing: HashMap<String, ()>,
}

impl SoundBank {
    pub fn new() -> SoundBank { SoundBank::default() }

    /// Add definitions from a `.voidsnd` script.
    pub fn add_script(&mut self, script: SoundScript) {
        self.script.merge(script);
    }

    pub fn script(&self) -> &SoundScript { &self.script }

    /// Put decoded samples in under a name, bypassing the script.
    ///
    /// The path a raw `play sound/foo.wav` takes, and what tests use.
    pub fn insert(&mut self, name: &str, sound: Arc<Sound>) {
        self.loaded.insert(name.to_ascii_lowercase(), sound);
        self.missing.remove(&name.to_ascii_lowercase());
    }

    pub fn get(&self, name: &str) -> Option<Arc<Sound>> {
        self.loaded.get(&name.to_ascii_lowercase()).cloned()
    }

    pub fn is_loaded(&self, name: &str) -> bool {
        self.loaded.contains_key(&name.to_ascii_lowercase())
    }

    pub fn len(&self) -> usize { self.loaded.len() }
    pub fn is_empty(&self) -> bool { self.loaded.is_empty() }

    /// Whether this name has already been reported as missing.
    pub fn already_missing(&self, name: &str) -> bool {
        self.missing.contains_key(&name.to_ascii_lowercase())
    }

    pub fn mark_missing(&mut self, name: &str) {
        self.missing.insert(name.to_ascii_lowercase(), ());
    }

    /// Resolve a name to the file to load and the parameters to play it with.
    ///
    /// A name the script does not define is taken as a path, so
    /// `play sound/test.wav` works without anyone having to write a script
    /// first.
    pub fn resolve(&self, name: &str) -> (String, SoundParams) {
        match self.script.get(name) {
            Some(def) => (def.file.clone(), def.params()),
            None => (default_path(name), SoundParams::default()),
        }
    }

    pub fn forget_all(&mut self) {
        self.loaded.clear();
        self.missing.clear();
    }
}

/// Where a sound file lives, given a bare name.
pub fn default_path(name: &str) -> String {
    let name = name.trim_start_matches('/');
    if name.contains('.') {
        if name.starts_with("sound/") { name.to_string() } else { format!("sound/{name}") }
    } else {
        format!("sound/{name}.wav")
    }
}

#[cfg(test)]
mod tests;
