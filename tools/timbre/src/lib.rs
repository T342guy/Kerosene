// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Timbre -- the Kerosene sound compiler.
//!
//! Turns `.wav` into `.keroaud`, the studiomdl of audio. It exists for the
//! same reason Alchemy does: the engine should load sounds, not decode and
//! decide about them.
//!
//! What it decides:
//!
//! * **Encoding.** ADPCM at a quarter the size, or 16-bit PCM. Per sound,
//!   because the compression is close to transparent on an impact and audible
//!   on a quiet room tone.
//! * **Gain.** Applied at build time rather than at play time, so a sound
//!   that was recorded too hot is fixed once instead of in every entity that
//!   uses it.
//! * **Loop points.** Lifted from the WAV's `smpl` chunk if it has one, or
//!   set by hand.
//! * **Channels.** A sound meant to be placed in the world has to be mono, and
//!   this is the only place that can be checked before someone hears it being
//!   wrong.
//!
//! Settings live in a [`build`] script beside the sounds, so the window and
//! the command line produce identical output -- the same discipline that makes
//! the texture build a library call rather than a second implementation.

pub mod build;
pub mod decode;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use kerosene_audio::compiled::{self, Encoding, Loop};
use kerosene_audio::wav::Sound;

/// Every source extension Timbre reads. See [`decode`].
pub use decode::EXTENSIONS as SOURCE_EXTENSIONS;

/// What one sound was compiled with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub encoding: Encoding,
    /// Multiplied into every sample before encoding.
    pub gain: f32,
    /// Fold stereo down to one channel, so the sound can be placed in the
    /// world.
    pub mono: bool,
    /// Loop region in frames; empty means take whatever the source says.
    pub looping: Option<Loop>,
}

impl Default for Options {
    fn default() -> Self {
        Options { encoding: Encoding::Adpcm, gain: 1.0, mono: false, looping: None }
    }
}

/// What compiling one sound produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Compiled {
    pub source: PathBuf,
    pub output: PathBuf,
    pub source_bytes: usize,
    pub output_bytes: usize,
    pub info: compiled::Info,
    /// Things worth saying that are not failures.
    pub warnings: Vec<String>,
}

impl Compiled {
    /// How much smaller the compiled form is, as a fraction saved.
    ///
    /// Negative when it grew, which is a real outcome rather than a bug: an
    /// MP3 at 64 kbit is already smaller than four bits a sample, so
    /// compiling one costs size as well as quality.
    pub fn saved(&self) -> f32 {
        if self.source_bytes == 0 { return 0.0 }
        1.0 - (self.output_bytes as f32 / self.source_bytes as f32)
    }

    pub fn grew(&self) -> bool { self.output_bytes > self.source_bytes }

    /// How the size change reads, in the direction it actually went.
    pub fn size_change(&self) -> String {
        if self.grew() {
            format!("{:.0}% larger", -self.saved() * 100.0)
        } else {
            format!("{:.0}% smaller", self.saved() * 100.0)
        }
    }
}

impl std::fmt::Display for Compiled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -- {:.2}s, {} {}, {} ({})",
            self.output.display(),
            self.info.duration(),
            self.info.channels,
            if self.info.channels == 1 { "channel" } else { "channels" },
            self.info.encoding.name(),
            self.size_change(),
        )
    }
}

/// Apply the options to decoded samples.
///
/// Separated from the file handling so the window can show exactly what will
/// be written without writing it -- the waveform on screen is the samples that
/// go into the file, not an approximation of them.
pub fn prepare(sound: &Sound, options: &Options) -> Sound {
    let mut out = if options.mono && sound.channels == 2 {
        Sound {
            channels: 1,
            sample_rate: sound.sample_rate,
            // Averaged, not summed: summing two correlated channels is a
            // 6 dB boost and clips anything that was already loud.
            samples: sound.samples.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect(),
        }
    } else {
        sound.clone()
    };

    if options.gain != 1.0 {
        for s in &mut out.samples {
            *s = (*s * options.gain).clamp(-1.0, 1.0);
        }
    }
    out
}

/// The loudest sample, after the options are applied.
pub fn peak_of(sound: &Sound) -> f32 {
    sound.samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

/// Compile one `.wav` into a `.keroaud`.
pub fn compile(source: &Path, output: &Path, options: &Options) -> Result<Compiled> {
    let bytes = std::fs::read(source)
        .with_context(|| format!("reading {}", source.display()))?;
    let read = decode::any(source, &bytes)?;
    let decoded = read.sound;

    let mut warnings = Vec::new();
    if read.format.is_lossy() {
        warnings.push(format!(
            "{} is already lossy; compiling it further compounds the artifacts \
             rather than cancelling them. A lossless source makes a better build.",
            read.format.name()
        ));
    }
    let prepared = prepare(&decoded, options);
    let peak = peak_of(&prepared);

    if peak >= 0.999 && peak_of(&decoded) < 0.999 {
        warnings.push(format!(
            "gain of {:.2} clips this sound; it peaked at {:.2} before it",
            options.gain,
            peak_of(&decoded)
        ));
    }
    if peak < 0.05 && peak > 0.0 {
        warnings.push(format!("very quiet: peaks at {peak:.3} of full scale"));
    }
    if prepared.channels == 2 {
        warnings.push(
            "stereo, so it cannot be placed in the world -- compile it mono to position it"
                .to_string(),
        );
    }

    let looping = options.looping.or(read.looping).unwrap_or_default();

    let encoded = compiled::encode(&prepared, options.encoding, looping);
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(output, &encoded)
        .with_context(|| format!("writing {}", output.display()))?;

    // Checked after encoding, because it is the encoded size that decides it.
    // A source that is already compressed gains nothing from four bits a
    // sample and can cost more than it saves.
    if encoded.len() > bytes.len() {
        warnings.push(String::from(
            "the compiled file is larger than its source. An already-compressed \
             source gains nothing from ADPCM, and pcm16 at least keeps the quality.",
        ));
    }

    let info = compiled::read_info(&encoded)?;
    Ok(Compiled {
        source: source.to_path_buf(),
        output: output.to_path_buf(),
        source_bytes: bytes.len(),
        output_bytes: encoded.len(),
        info,
        warnings,
    })
}

/// The loop region a WAV declares in its `smpl` chunk, if it declares one.
///
/// Audio editors write this and nothing in the engine read it, which is why
/// looping was previously all-or-nothing: a room tone with a proper loop
/// region had it thrown away and was repeated end to end instead, click and
/// all.
pub fn loop_from_wav(bytes: &[u8], frames: u32) -> Option<Loop> {
    let chunk = find_chunk(bytes, b"smpl")?;
    // The sample loop count sits at offset 28, and the first loop's start and
    // end at 44 and 48 within the chunk body.
    if chunk.len() < 60 { return None }
    let at = |o: usize| u32::from_le_bytes([chunk[o], chunk[o + 1], chunk[o + 2], chunk[o + 3]]);
    if at(28) == 0 { return None }

    let start = at(44);
    // The `smpl` end is the last frame *played*, inclusive; ours is one past.
    let end = at(48).saturating_add(1).min(frames);
    (start < end).then_some(Loop { start, end })
}

/// Find a RIFF chunk's body by its four-character id.
fn find_chunk<'a>(bytes: &'a [u8], id: &[u8; 4]) -> Option<&'a [u8]> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let body = at + 8;
        let end = body.checked_add(size)?;
        if end > bytes.len() { return None }
        if &bytes[at..at + 4] == id {
            return Some(&bytes[body..end]);
        }
        // Chunks are word aligned, and a file that forgets the pad byte is
        // common enough that walking past it is not worth failing over.
        at = body + size + (size & 1);
    }
    None
}

/// Where a source sound's compiled form goes.
///
/// `sound/door/move.wav` becomes `sound/door/move.keroaud`: beside it, so the
/// path a script names is the path either form is found at.
pub fn output_for(source: &Path) -> PathBuf {
    source.with_extension(compiled::EXTENSION)
}

/// What a whole-tree build did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Batch {
    pub compiled: Vec<Compiled>,
    /// Sources that were already up to date.
    pub skipped: usize,
    pub failed: Vec<(PathBuf, String)>,
}

impl Batch {
    pub fn source_bytes(&self) -> usize { self.compiled.iter().map(|c| c.source_bytes).sum() }
    pub fn output_bytes(&self) -> usize { self.compiled.iter().map(|c| c.output_bytes).sum() }
    pub fn warnings(&self) -> usize { self.compiled.iter().map(|c| c.warnings.len()).sum() }
}

impl std::fmt::Display for Batch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let saved = self.source_bytes().saturating_sub(self.output_bytes());
        write!(
            f,
            "{} sound(s) compiled, {} already up to date, {} KiB saved",
            self.compiled.len(),
            self.skipped,
            saved / 1024
        )?;
        if !self.failed.is_empty() {
            write!(f, ", {} failed", self.failed.len())?;
        }
        Ok(())
    }
}

/// Compile every source sound under a content tree's `sound/` directory.
pub fn build_sounds(content: &Path, force: bool) -> Result<Batch> {
    let root = content.join("sound");
    let script = build::Script::load_beside(&root)?;
    let mut batch = Batch::default();
    let found = sources(&root);

    // Two sources with the same name compile to the same file, so one of them
    // silently wins and which one depends on the sort order. Named rather than
    // resolved: only the person who put both there knows which they meant.
    for (a, b) in colliding(&found) {
        batch.failed.push((
            b.clone(),
            format!(
                "{} would overwrite the compiled form of {}. Two sources cannot share a name.",
                b.display(),
                a.display()
            ),
        ));
    }

    for source in found {
        let output = output_for(&source);
        if !force && is_up_to_date(&source, &output, &script.path) {
            batch.skipped += 1;
            continue;
        }
        let options = script.options_for(&source, &root);
        match compile(&source, &output, &options) {
            Ok(done) => batch.compiled.push(done),
            Err(e) => batch.failed.push((source, format!("{e:#}"))),
        }
    }
    Ok(batch)
}

/// Pairs of sources that compile to the same output.
fn colliding(sources: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
    let mut seen: std::collections::BTreeMap<PathBuf, PathBuf> = std::collections::BTreeMap::new();
    let mut clashes = Vec::new();
    for source in sources {
        let output = output_for(source);
        match seen.get(&output) {
            Some(first) => clashes.push((first.clone(), source.clone())),
            None => {
                seen.insert(output, source.clone());
            }
        }
    }
    clashes
}

/// Whether a compiled sound is newer than its source and its settings.
///
/// The settings matter as much as the source: changing a gain in the window
/// and rebuilding has to actually rebuild, or the change silently does
/// nothing and the next person spends an hour on it.
fn is_up_to_date(source: &Path, output: &Path, script: &Path) -> bool {
    let Ok(out) = output.metadata().and_then(|m| m.modified()) else { return false };
    let newer_than_out = |p: &Path| {
        p.metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t > out)
    };
    !newer_than_out(source) && !newer_than_out(script)
}

/// Every source sound under a directory, in a stable order.
pub fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(dir, &mut found);
    found.sort();
    found
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if decode::Format::of(&path).is_some() {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests;
