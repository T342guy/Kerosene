// SPDX-License-Identifier: LGPL-3.0-or-later
//! Per-sound compile settings, in a file beside the sounds.
//!
//! The window and the command line have to produce identical output. The only
//! way to guarantee that is for the settings to live somewhere both read
//! rather than in one of them -- the same reasoning that makes the texture
//! build a library call instead of a second implementation.
//!
//! So a gain slider dragged in Timbre writes here, and `timbre build` run from
//! a script picks it up. Neither is the authority; this file is.
//!
//! ```text
//! defaults
//! {
//!     "encoding" "adpcm"
//! }
//!
//! sound
//! {
//!     "file"     "ambient/room_tone.wav"
//!     "encoding" "pcm16"
//!     "gain"     "0.8"
//!     "mono"     "1"
//! }
//! ```
//!
//! A sound with no block gets the defaults, which is most of them: the point
//! of the file is the exceptions.

use crate::Options;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use kerosene_audio::compiled::{Encoding, Loop};
use kerosene_kv::KeyValues;

/// What the settings file is called, inside the sound directory.
pub const FILE_NAME: &str = "timbre.kerobuild";

/// Settings for a sound tree.
#[derive(Clone, Debug, Default)]
pub struct Script {
    /// Where it lives, whether or not it exists yet.
    pub path: PathBuf,
    pub defaults: Options,
    /// Keyed by path relative to the sound root, with forward slashes.
    entries: BTreeMap<String, Options>,
}

impl Script {
    /// Read the settings that belong to a sound root, or start an empty set.
    ///
    /// A missing file is the normal case and not an error: a project that has
    /// never opened Timbre still builds, with the defaults.
    pub fn load_beside(root: &Path) -> Result<Script> {
        let path = root.join(FILE_NAME);
        if !path.is_file() {
            return Ok(Script { path, ..Default::default() });
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut script = Script::parse(&text).with_context(|| format!("{}", path.display()))?;
        script.path = path;
        Ok(script)
    }

    pub fn parse(text: &str) -> Result<Script> {
        let kv = KeyValues::parse(text).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut script = Script::default();

        if let Some(block) = kv.block("defaults") {
            script.defaults = options_from(block, &Options::default());
        }
        for block in kv.blocks("sound") {
            let Some(file) = block.get("file").map(str::trim).filter(|f| !f.is_empty()) else {
                continue;
            };
            script
                .entries
                .insert(normalise(file), options_from(block, &script.defaults));
        }
        Ok(script)
    }

    /// The options for one source file.
    pub fn options_for(&self, source: &Path, root: &Path) -> Options {
        let key = source
            .strip_prefix(root)
            .map(|r| normalise(&r.to_string_lossy()))
            .unwrap_or_else(|_| normalise(&source.to_string_lossy()));
        self.entries.get(&key).copied().unwrap_or(self.defaults)
    }

    /// Whether a file has settings of its own rather than taking the defaults.
    pub fn has_entry(&self, relative: &str) -> bool {
        self.entries.contains_key(&normalise(relative))
    }

    /// Give one file its own settings.
    pub fn set(&mut self, relative: &str, options: Options) {
        self.entries.insert(normalise(relative), options);
    }

    /// Put a file back on the defaults.
    pub fn clear(&mut self, relative: &str) {
        self.entries.remove(&normalise(relative));
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &Options)> {
        self.entries.iter()
    }

    /// Write the file, creating the directory if it is not there.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, self.to_text())
            .with_context(|| format!("writing {}", self.path.display()))?;
        Ok(())
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "// How each sound is compiled. Written by Timbre's window and read by\n\
             // `timbre build`, so both produce the same files.\n\
             //\n\
             // A sound with no block here takes the defaults, which is most of them:\n\
             // the point of this file is the exceptions.\n\n",
        );
        out.push_str(&block_text("defaults", None, &self.defaults, &Options::default()));
        for (file, options) in &self.entries {
            out.push('\n');
            out.push_str(&block_text("sound", Some(file), options, &self.defaults));
        }
        out
    }
}

fn options_from(block: &KeyValues, fallback: &Options) -> Options {
    let encoding = block
        .get("encoding")
        .and_then(Encoding::parse)
        .unwrap_or(fallback.encoding);
    let gain = block
        .get("gain")
        .and_then(|g| g.trim().parse::<f32>().ok())
        // A gain of zero is silence, which nobody means to compile; a negative
        // one is a phase flip nobody asked for either.
        .filter(|g| *g > 0.0 && g.is_finite())
        .unwrap_or(fallback.gain);
    let mono = block
        .get("mono")
        .map(|m| matches!(m.trim(), "1" | "true" | "yes"))
        .unwrap_or(fallback.mono);

    let start = block.get("loopstart").and_then(|s| s.trim().parse::<u32>().ok());
    let end = block.get("loopend").and_then(|s| s.trim().parse::<u32>().ok());
    let looping = match (start, end) {
        (Some(start), Some(end)) if end > start => Some(Loop { start, end }),
        // Both zero is how "no loop, and I mean it" is written, which has to
        // be distinguishable from "nothing said, take the WAV's own".
        (Some(0), Some(0)) => Some(Loop::default()),
        _ => fallback.looping,
    };

    Options { encoding, gain, mono, looping }
}

fn block_text(name: &str, file: Option<&str>, options: &Options, against: &Options) -> String {
    let mut out = format!("{name}\n{{\n");
    if let Some(file) = file {
        out.push_str(&format!("\t\"file\"      \"{file}\"\n"));
    }
    // Only what differs, so the file stays readable and a default that changes
    // later reaches everything that never overrode it.
    if file.is_none() || options.encoding != against.encoding {
        out.push_str(&format!("\t\"encoding\"  \"{}\"\n", options.encoding.name()));
    }
    if options.gain != against.gain {
        out.push_str(&format!("\t\"gain\"      \"{:.3}\"\n", options.gain));
    }
    if options.mono != against.mono {
        out.push_str(&format!("\t\"mono\"      \"{}\"\n", u8::from(options.mono)));
    }
    if let Some(region) = options.looping
        && options.looping != against.looping
    {
        out.push_str(&format!("\t\"loopstart\" \"{}\"\n", region.start));
        out.push_str(&format!("\t\"loopend\"   \"{}\"\n", region.end));
    }
    out.push_str("}\n");
    out
}

/// One spelling of a path, so a lookup written either way finds the entry.
fn normalise(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_ascii_lowercase()
}

#[cfg(test)]
mod tests;
