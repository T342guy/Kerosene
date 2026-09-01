// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Reading source audio, whatever it arrived as.
//!
//! WAV is decoded by [`kerosene_audio::wav`] -- the engine's own decoder, written
//! out rather than pulled in, and the one that also reads the `smpl` chunk
//! loop points. FLAC and MP3 go through Symphonia.
//!
//! ## Why a dependency is allowed here and not in the engine
//!
//! `kerosene-audio` decodes what a *player's machine* loads, which is why it is a
//! few hundred lines of hand-written RIFF parsing: it is a place an unexpected
//! file should produce an error rather than a panic, and every byte of it
//! ships in the game.
//!
//! Timbre is a build tool. It runs on the machine that makes the content,
//! never on the machine that plays it, and `kiln --ship` refuses to put a tool
//! in a distribution at all. So a decoder here costs a game nothing, and
//! writing a FLAC decoder by hand -- let alone an MP3 one -- would be a
//! thousand lines of someone else's solved problem.
//!
//! Alchemy already made this call: it pulls in `image` for PNG and JPEG, and
//! the engine reads only `.kerotex`. This is the same split.
//!
//! ## On lossy sources
//!
//! MP3 is supported because people have MP3s, not because it is a good thing
//! to build from. Compiling one to ADPCM is lossy-to-lossy: the two sets of
//! artifacts do not cancel, they compound, and the second encoder spends its
//! bits describing the first one's mistakes. Timbre says so on every one.

use anyhow::{Context, Result, bail};
use std::path::Path;
use kerosene_audio::compiled::Loop;
use kerosene_audio::wav::Sound;

/// The formats Timbre will read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Wav,
    Flac,
    Mp3,
}

/// Every source extension, for finding files and reporting.
pub const EXTENSIONS: &[&str] = &["wav", "flac", "mp3"];

impl Format {
    pub fn of(path: &Path) -> Option<Format> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        match extension.as_str() {
            "wav" | "wave" => Some(Format::Wav),
            "flac" => Some(Format::Flac),
            "mp3" => Some(Format::Mp3),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Mp3 => "mp3",
        }
    }

    /// Whether the source has already thrown information away.
    ///
    /// Only MP3, and it matters: compiling it further is lossy-to-lossy, and
    /// the artifacts compound rather than cancelling.
    pub fn is_lossy(self) -> bool {
        matches!(self, Format::Mp3)
    }
}

/// A decoded source and what it said about itself.
#[derive(Debug)]
pub struct Decoded {
    pub sound: Sound,
    pub format: Format,
    /// A loop the file declares, if it declares one.
    pub looping: Option<Loop>,
}

/// Decode whatever this file is.
pub fn any(path: &Path, bytes: &[u8]) -> Result<Decoded> {
    let format = Format::of(path).with_context(|| {
        format!(
            "{}: not a sound Timbre reads. It reads {}.",
            path.display(),
            EXTENSIONS.join(", ")
        )
    })?;

    // What the bytes say, before what the name says. A file downloaded and
    // renamed is the common case here, and "not a RIFF/WAVE file" sends
    // someone looking for a corrupt file rather than a mislabelled one.
    if let Some(actual) = sniff(bytes)
        && actual != format.name()
    {
        bail!(
            "{}: named .{} but the bytes are {}. Rename it, or convert it with an \
             audio editor -- the extension is what Timbre picks a decoder by.",
            path.display(),
            format.name(),
            actual
        );
    }

    match format {
        Format::Wav => {
            let sound = kerosene_audio::wav::decode(bytes)
                .with_context(|| format!("decoding {}", path.display()))?;
            let looping = crate::loop_from_wav(bytes, sound.frames() as u32);
            Ok(Decoded { sound, format, looping })
        }
        Format::Flac | Format::Mp3 => symphonia(path, bytes, format),
    }
}

/// What a file actually is, from its first few bytes.
///
/// Only enough containers to recognise the ones people arrive with. `None`
/// means "nothing recognised", which is not the same as "wrong" -- an MP3 with
/// no ID3 tag starts with a frame sync that is easy to confuse with data.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.len() >= magic.len() && &bytes[..magic.len()] == magic;
    if starts(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WAVE" {
        return Some("wav");
    }
    if starts(b"fLaC") {
        return Some("flac");
    }
    if starts(b"ID3") {
        return Some("mp3");
    }
    // A bare MP3 with no tag starts on a frame sync: eleven set bits, then a
    // version and layer that are not the reserved values. Enough certainty for
    // a diagnostic, which is all this is used for.
    if bytes.len() >= 2
        && bytes[0] == 0xff
        && bytes[1] & 0xe0 == 0xe0
        && bytes[1] & 0x18 != 0x08
        && bytes[1] & 0x06 != 0x00
    {
        return Some("mp3");
    }
    if starts(b"OggS") {
        return Some("ogg, which Timbre does not read");
    }
    if starts(b"\x1a\x45\xdf\xa3") {
        return Some("matroska or webm, which Timbre does not read");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("mp4 or m4a, which Timbre does not read");
    }
    if starts(b"FORM") && bytes.len() >= 12 && &bytes[8..12] == b"AIFF" {
        return Some("aiff, which Timbre does not read");
    }
    None
}

fn symphonia(path: &Path, bytes: &[u8], format: Format) -> Result<Decoded> {
    use symphonia::core::codecs::CodecParameters;
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let source = Box::new(std::io::Cursor::new(bytes.to_vec()));
    let stream = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    hint.with_extension(format.name());

    let mut reader = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .with_context(|| format!("reading {}", path.display()))?;

    let track = reader
        .default_track(TrackType::Audio)
        .with_context(|| format!("{}: no audio track", path.display()))?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        bail!("{}: the audio track has no codec parameters", path.display());
    };

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .with_context(|| format!("decoding {}", path.display()))?;

    let mut samples: Vec<f32> = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    let mut channels = 0u16;
    let mut rate = 0u32;

    while let Some(packet) = reader
        .next_packet()
        .with_context(|| format!("reading {}", path.display()))?
    {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .with_context(|| format!("decoding {}", path.display()))?;
        let spec = decoded.spec();
        channels = spec.channels().count() as u16;
        rate = spec.rate();

        chunk.clear();
        decoded.copy_to_vec_interleaved(&mut chunk);
        samples.extend_from_slice(&chunk);
    }

    if channels == 0 || rate == 0 || samples.is_empty() {
        bail!("{}: decoded to nothing", path.display());
    }
    if channels > 2 {
        bail!(
            "{}: {channels} channels. A game sound is mono or stereo; \
             fold it down in an audio editor first.",
            path.display()
        );
    }

    let sound = Sound { channels, sample_rate: rate, samples };
    let frames = sound.frames() as u32;
    Ok(Decoded { sound, format, looping: loop_from_tags(&mut reader, frames) })
}

/// The loop a container declares in its tags.
///
/// `LOOPSTART` with `LOOPLENGTH` or `LOOPEND` is the convention game audio has
/// settled on in Vorbis comments, which is what a FLAC carries. It is not a
/// standard and nothing enforces it; reading it costs nothing and not reading
/// it means a looping ambience compiled from FLAC loses what a WAV would have
/// kept.
fn loop_from_tags(
    reader: &mut Box<dyn symphonia::core::formats::FormatReader + '_>,
    frames: u32,
) -> Option<Loop> {
    let binding = reader.metadata();
    let revision = binding.current()?;

    let mut start: Option<u32> = None;
    let mut length: Option<u32> = None;
    let mut end: Option<u32> = None;
    for tag in &revision.media.tags {
        let number = tag.raw.value.to_string().trim().parse::<u32>().ok();
        match tag.raw.key.to_ascii_uppercase().replace('_', "").as_str() {
            "LOOPSTART" => start = number,
            "LOOPLENGTH" => length = number,
            "LOOPEND" => end = number,
            _ => {}
        }
    }

    let start = start?;
    let end = end.or_else(|| length.map(|l| start.saturating_add(l)))?.min(frames);
    (start < end).then_some(Loop { start, end })
}

#[cfg(test)]
mod tests;
