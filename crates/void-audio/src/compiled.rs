// SPDX-License-Identifier: LGPL-3.0-or-later
//! `.voidaud` -- compiled audio, the format Timbre writes and the engine reads.
//!
//! Every other kind of content in this engine has a source and a compiled
//! form: `.png` becomes `.voidtex`, `.obj` becomes `.voidmdl`. Sound was the
//! exception. WAV shipped raw, which cost a factor of four in download size,
//! carried no loop points, and let a stereo file be placed in the world where
//! it cannot meaningfully be panned -- silently, and forever.
//!
//! So sound has a compiled form too. It holds four things a WAV does not
//! usefully carry:
//!
//! * **Encoding.** 16-bit PCM, or IMA ADPCM at a quarter the size. Chosen per
//!   sound rather than globally, because the compression is close to
//!   transparent on an impact and audible on a quiet room tone.
//! * **Loop points.** A sample-accurate region to repeat, rather than "loop
//!   the whole file", which is the only thing a bare `.wav` and a `"loop" "1"`
//!   between them can say.
//! * **Peak.** What the loudest sample is, computed once at build time, so
//!   nothing has to scan a buffer to know whether a gain will clip it.
//! * **Whether it may be positioned.** A stereo sound cannot be panned --
//!   there is one pan and two channels already carrying their own image -- so
//!   the compiler records what it is and the engine can say so rather than
//!   playing something subtly wrong.
//!
//! The header is 64 bytes with room at the end, so a later field costs no
//! version bump.

use crate::wav::Sound;
use crate::{AudioError, adpcm};

/// The extension compiled audio uses.
pub const EXTENSION: &str = "voidaud";

const MAGIC: [u8; 4] = *b"VOAU";
const VERSION: u32 = 1;
const HEADER_SIZE: usize = 64;

/// A sound this size or larger is implausible and is refused before anything
/// is allocated for it. Ten minutes of 48 kHz stereo, near enough.
const MAX_BYTES: usize = 256 * 1024 * 1024;

/// How the samples are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Signed 16-bit, little endian. Half a float WAV, and lossless enough
    /// that nothing can be blamed on it.
    Pcm16,
    /// Four bits a sample. See [`crate::adpcm`].
    Adpcm,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Pcm16 => "pcm16",
            Encoding::Adpcm => "adpcm",
        }
    }

    pub fn parse(name: &str) -> Option<Encoding> {
        match name.trim().to_ascii_lowercase().as_str() {
            "pcm16" | "pcm" | "raw" => Some(Encoding::Pcm16),
            "adpcm" | "ima" => Some(Encoding::Adpcm),
            _ => None,
        }
    }

    fn tag(self) -> u16 {
        match self {
            Encoding::Pcm16 => 0,
            Encoding::Adpcm => 1,
        }
    }

    fn from_tag(tag: u16) -> Option<Encoding> {
        match tag {
            0 => Some(Encoding::Pcm16),
            1 => Some(Encoding::Adpcm),
            _ => None,
        }
    }
}

/// A region to repeat, in frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Loop {
    pub start: u32,
    /// One past the last frame played before jumping back to `start`.
    pub end: u32,
}

impl Loop {
    pub fn is_empty(&self) -> bool { self.end <= self.start }
}

/// Everything a compiled sound knows about itself besides its samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Info {
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: u32,
    pub encoding: Encoding,
    /// The loudest absolute sample, in `[0, 1]`.
    pub peak: f32,
    /// The region to repeat, empty if it does not loop.
    pub looping: Loop,
}

impl Info {
    pub fn duration(&self) -> f32 {
        if self.sample_rate == 0 { return 0.0 }
        self.frames as f32 / self.sample_rate as f32
    }

    /// Whether this sound can be placed in the world.
    ///
    /// Mono only. Panning a stereo source means applying one pan to a signal
    /// that already carries its own left and right, which is not a position
    /// and does not sound like one.
    pub fn can_be_positioned(&self) -> bool { self.channels == 1 }
}

/// Compile samples into the bytes of a `.voidaud`.
pub fn encode(sound: &Sound, encoding: Encoding, looping: Loop) -> Vec<u8> {
    let samples: Vec<i16> = sound
        .samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect();
    let peak = sound.samples.iter().fold(0.0f32, |a, s| a.max(s.abs())).min(1.0);

    let mut out = Vec::with_capacity(HEADER_SIZE + samples.len() * 2);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&sound.channels.to_le_bytes());
    out.extend_from_slice(&encoding.tag().to_le_bytes());
    out.extend_from_slice(&sound.sample_rate.to_le_bytes());
    out.extend_from_slice(&(sound.frames() as u32).to_le_bytes());
    out.extend_from_slice(&looping.start.to_le_bytes());
    out.extend_from_slice(&looping.end.to_le_bytes());
    out.extend_from_slice(&peak.to_le_bytes());
    out.resize(HEADER_SIZE, 0);

    match encoding {
        Encoding::Pcm16 => {
            for s in &samples {
                out.extend_from_slice(&s.to_le_bytes());
            }
        }
        Encoding::Adpcm => out.extend_from_slice(&adpcm::encode(&samples, sound.channels)),
    }
    out
}

/// Read a `.voidaud` back into samples the mixer can use.
pub fn decode(bytes: &[u8]) -> Result<(Sound, Info), AudioError> {
    let info = read_info(bytes)?;
    let body = &bytes[HEADER_SIZE..];
    let count = info.frames as usize * info.channels as usize;

    let samples: Vec<f32> = match info.encoding {
        Encoding::Pcm16 => {
            let available = body.len() / 2;
            if available < count {
                return Err(AudioError::Malformed(format!(
                    "truncated: header says {count} samples, the file holds {available}"
                )));
            }
            body[..count * 2]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                .collect()
        }
        Encoding::Adpcm => {
            let decoded = adpcm::decode(body, info.channels, count);
            if decoded.len() < count {
                return Err(AudioError::Malformed(format!(
                    "truncated: header says {count} samples, the file holds {}",
                    decoded.len()
                )));
            }
            decoded.iter().map(|&s| s as f32 / 32768.0).collect()
        }
    };

    Ok((
        Sound { channels: info.channels, sample_rate: info.sample_rate, samples },
        info,
    ))
}

/// Read only the header, for a listing that does not want the samples.
pub fn read_info(bytes: &[u8]) -> Result<Info, AudioError> {
    if bytes.len() > MAX_BYTES {
        return Err(AudioError::Malformed("file is implausibly large".into()));
    }
    if bytes.len() < HEADER_SIZE {
        return Err(AudioError::Malformed(format!(
            "truncated: a header is {HEADER_SIZE} bytes and the file is {}",
            bytes.len()
        )));
    }
    if bytes[..4] != MAGIC {
        return Err(AudioError::Malformed(format!("not a .{EXTENSION} file (bad magic)")));
    }
    let u32_at = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);

    let version = u32_at(4);
    if version != VERSION {
        return Err(AudioError::Unsupported(format!(
            "version {version}; this build reads version {VERSION}"
        )));
    }

    let channels = u16_at(8);
    if channels == 0 || channels > 2 {
        return Err(AudioError::Unsupported(format!("{channels} channels; mono and stereo only")));
    }
    let encoding = Encoding::from_tag(u16_at(10))
        .ok_or_else(|| AudioError::Unsupported(format!("encoding {}", u16_at(10))))?;
    let sample_rate = u32_at(12);
    if sample_rate == 0 {
        return Err(AudioError::Malformed("sample rate is zero".into()));
    }
    let frames = u32_at(16);
    let start = u32_at(20);
    let end = u32_at(24);
    let peak = f32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);

    // A loop past the end of the sound would send the mixer's cursor somewhere
    // there are no samples, so it is refused here rather than survived there.
    if end > frames || (end > 0 && start >= end) {
        return Err(AudioError::Malformed(format!(
            "loop {start}..{end} does not fit in {frames} frames"
        )));
    }

    Ok(Info {
        channels,
        sample_rate,
        frames,
        encoding,
        peak: if peak.is_finite() { peak.clamp(0.0, 1.0) } else { 0.0 },
        looping: Loop { start, end },
    })
}

#[cfg(test)]
mod tests;
