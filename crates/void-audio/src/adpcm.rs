// SPDX-License-Identifier: LGPL-3.0-or-later
//! IMA ADPCM: four bits a sample.
//!
//! Audio is the largest thing in most games' downloads and WAV is the least
//! compressed way to store it, so something had to give. The choice was
//! between pulling in a Vorbis or Opus decoder and writing this, and this won
//! for the same reason [`crate::wav`] is written out rather than pulled in:
//! it is small, it is understandable in one sitting, and it adds no C library
//! to a toolchain whose whole shipping story is `cargo build`.
//!
//! Four bits a sample against sixteen is a flat 4:1, which is worse than
//! Vorbis and enormously simpler. It is what Source used, for the same
//! trade.
//!
//! ## What it costs
//!
//! Quality: ADPCM is lossy, and audibly so on quiet material with a lot of
//! high frequency in it -- a cymbal, a hiss, a soft room tone. It is close to
//! transparent on the things games are mostly made of: speech, impacts,
//! machinery, footsteps. That is why the compiler does not apply it to
//! everything.
//!
//! There is also an **attack transient**, and it is worth knowing about
//! rather than discovering. The quantiser starts at its smallest step and
//! climbs, so a stream that opens loud takes a few hundred samples -- some
//! milliseconds -- to be tracked accurately. On a sound with a sharp onset
//! that softens the very front of it. A block-based encoder hides this by
//! restating the predictor every few hundred samples, at the cost of a header
//! per block and a format nobody here needs to seek into; the honest answer
//! for material where it matters is PCM16, which is why the encoding is a
//! choice per sound rather than a setting for the project.
//!
//! Memory: nothing. The engine decodes a whole sound at load and mixes from
//! floats, so this saves download and disk, not RAM. Worth being plain about,
//! because "compressed audio" usually implies both.
//!
//! ## Why there are no blocks
//!
//! A WAV ADPCM file is cut into blocks, each restating the predictor, so a
//! player can seek without decoding from the start. Nothing here needs that:
//! a sound is decoded once, whole, when it loads. So this is a plain stream
//! with one predictor per channel, which is both smaller and simpler -- and
//! the one thing it gives up is the one thing nobody asks of it.

/// How the step index moves after each code. Mirrored across the sign bit.
const INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// The quantiser's step sizes, growing by about 11% each entry.
const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408,
    449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066,
    2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630,
    9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794,
    32767,
];

/// One channel's running estimate of where the signal is.
#[derive(Clone, Copy, Debug, Default)]
struct State {
    predictor: i32,
    index: i32,
}

impl State {
    /// Advance by one code, returning the sample it reconstructs.
    ///
    /// The encoder calls this too, on the code it just chose, so that both
    /// sides walk the same predictor. An encoder that reconstructed
    /// differently would drift further out of step with every sample.
    fn step(&mut self, code: u8) -> i16 {
        let step = STEP_TABLE[self.index as usize];
        let mut diff = step >> 3;
        if code & 4 != 0 { diff += step }
        if code & 2 != 0 { diff += step >> 1 }
        if code & 1 != 0 { diff += step >> 2 }

        if code & 8 != 0 { self.predictor -= diff } else { self.predictor += diff }
        self.predictor = self.predictor.clamp(-32768, 32767);
        self.index = (self.index + INDEX_TABLE[code as usize]).clamp(0, 88);
        self.predictor as i16
    }

    /// Choose the code that best reaches `sample` from here, and take it.
    fn encode(&mut self, sample: i16) -> u8 {
        let step = STEP_TABLE[self.index as usize];
        let delta = sample as i32 - self.predictor;
        let sign = if delta < 0 { 8u8 } else { 0 };
        let mut magnitude = delta.abs();

        // Three bits of magnitude, greedily: step, half, quarter.
        let mut code = 0u8;
        if magnitude >= step {
            code |= 4;
            magnitude -= step;
        }
        if magnitude >= step >> 1 {
            code |= 2;
            magnitude -= step >> 1;
        }
        if magnitude >= step >> 2 {
            code |= 1;
        }
        code |= sign;

        self.step(code);
        code
    }
}

/// Compress interleaved 16-bit samples.
///
/// Two codes to a byte, low nibble first, in the order the samples arrive --
/// so a stereo stream alternates left and right within a byte, which is what
/// keeps the two channels' predictors advancing together.
pub fn encode(samples: &[i16], channels: u16) -> Vec<u8> {
    let channels = channels.max(1) as usize;
    let mut states = vec![State::default(); channels];
    let mut out = Vec::with_capacity(samples.len().div_ceil(2));
    let mut pending: Option<u8> = None;

    for (i, &sample) in samples.iter().enumerate() {
        let code = states[i % channels].encode(sample);
        match pending.take() {
            Some(low) => out.push(low | (code << 4)),
            None => pending = Some(code),
        }
    }
    // An odd sample count leaves half a byte; the high nibble is padding and
    // the frame count in the header is what says to ignore it.
    if let Some(low) = pending {
        out.push(low);
    }
    out
}

/// Expand back to interleaved 16-bit samples.
///
/// `count` is how many samples to produce, from the header rather than from
/// the byte length, because the last byte may be half padding.
pub fn decode(bytes: &[u8], channels: u16, count: usize) -> Vec<i16> {
    let channels = channels.max(1) as usize;
    let mut states = vec![State::default(); channels];
    let mut out = Vec::with_capacity(count);

    for i in 0..count {
        let Some(&byte) = bytes.get(i / 2) else { break };
        let code = if i % 2 == 0 { byte & 0x0f } else { byte >> 4 };
        out.push(states[i % channels].step(code));
    }
    out
}

/// How many bytes `samples` compresses to.
pub fn encoded_len(samples: usize) -> usize {
    samples.div_ceil(2)
}

#[cfg(test)]
mod tests;
