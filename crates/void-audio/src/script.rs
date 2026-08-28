// SPDX-License-Identifier: LGPL-3.0-or-later
//! Sound scripts: what a name means.
//!
//! A level fires `door/open`, and what that *is* -- which file, how loud, how
//! far it carries, at what pitch -- lives here rather than on the entity. The
//! same indirection materials have, for the same reason: making every door in
//! a game quieter should be one edit, not a hunt through a map.
//!
//! ```text
//! sound
//! {
//!     "name"        "door/open"
//!     "file"        "sound/door/open.wav"
//!     "volume"      "0.8"
//!     "pitch"       "1.0"
//!     "attenuation" "1.0"
//!     "distance"    "128"
//!     "max"         "2048"
//! }
//! ```

use crate::SoundParams;
use std::collections::BTreeMap;
use void_kv::KeyValues;

/// One named sound.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundDef {
    pub name: String,
    pub file: String,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    /// Distance at which it plays at full volume, in void units.
    pub reference_distance: f32,
    pub attenuation: f32,
    pub max_distance: f32,
}

impl Default for SoundDef {
    fn default() -> Self {
        let params = SoundParams::default();
        SoundDef {
            name: String::new(),
            file: String::new(),
            volume: params.volume,
            pitch: params.pitch,
            looping: params.looping,
            reference_distance: params.reference_distance,
            attenuation: params.attenuation,
            max_distance: params.max_distance,
        }
    }
}

impl SoundDef {
    /// The mixer parameters this definition describes.
    ///
    /// Position is left unset: where a sound is comes from whatever plays it,
    /// not from the script.
    pub fn params(&self) -> SoundParams {
        SoundParams {
            volume: self.volume,
            pitch: self.pitch,
            looping: self.looping,
            position: None,
            reference_distance: self.reference_distance,
            attenuation: self.attenuation,
            max_distance: self.max_distance,
        }
    }
}

/// A parsed set of sound definitions.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct SoundScript {
    defs: BTreeMap<String, SoundDef>,
}

impl SoundScript {
    /// Parse a `.voidsnd`.
    ///
    /// A block missing a name is skipped with a warning rather than failing
    /// the file: one bad entry should not silence a whole game.
    pub fn parse(text: &str) -> Result<SoundScript, void_kv::ParseError> {
        let kv = KeyValues::parse(text)?;
        let mut script = SoundScript::default();

        for block in kv.blocks("sound") {
            let Some(name) = block.get("name").filter(|n| !n.trim().is_empty()) else {
                log::warn!("sound script: a block with no name");
                continue;
            };
            let file = block
                .get("file")
                .map(str::to_string)
                .unwrap_or_else(|| crate::default_path(name));

            let def = SoundDef {
                name: name.to_string(),
                file,
                volume: block.get_or("volume", 1.0f32),
                pitch: block.get_or("pitch", 1.0f32),
                looping: block.get_or("loop", 0i32) != 0,
                reference_distance: block.get_or("distance", 128.0f32),
                attenuation: block.get_or("attenuation", 1.0f32),
                max_distance: block.get_or("max", 4096.0f32),
            };
            script.defs.insert(name.to_ascii_lowercase(), def);
        }
        Ok(script)
    }

    pub fn get(&self, name: &str) -> Option<&SoundDef> {
        self.defs.get(&name.to_ascii_lowercase())
    }

    pub fn insert(&mut self, def: SoundDef) {
        self.defs.insert(def.name.to_ascii_lowercase(), def);
    }

    /// Fold another script in, later definitions winning, so a mod can
    /// override one sound without copying the file.
    pub fn merge(&mut self, other: SoundScript) {
        self.defs.extend(other.defs);
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.defs.values().map(|d| d.name.as_str())
    }

    pub fn len(&self) -> usize { self.defs.len() }
    pub fn is_empty(&self) -> bool { self.defs.is_empty() }
}
