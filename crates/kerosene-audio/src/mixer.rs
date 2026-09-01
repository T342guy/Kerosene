// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The mixer: voices in, stereo out.
//!
//! Everything here is arithmetic on buffers, with no device in it, which is
//! what makes it testable -- a sound that pans the wrong way or never ends is
//! a numeric fact, not something to notice by ear on the third playthrough.
//!
//! The model is Source's, because it is the one level designers already know
//! how to reason about: a sound is placed in the world, gets quieter with
//! distance according to its own attenuation, and is panned by where it is
//! relative to the way you are facing. Sounds with no position are heard flat
//! -- interface clicks, music, the player's own footsteps.

use std::sync::Arc;
use kerosene_math::{Basis, Vec3};

use crate::Sound;

/// A playing sound, so it can be stopped or moved later.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SoundHandle(pub u64);

/// Where the ears are.
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    pub position: Vec3,
    pub basis: Basis,
}

impl Default for Listener {
    fn default() -> Self {
        Listener { position: Vec3::ZERO, basis: kerosene_math::Angles::ZERO.vectors() }
    }
}

/// How a sound is heard.
#[derive(Clone, Copy, Debug)]
pub struct SoundParams {
    pub volume: f32,
    /// Playback rate. 2.0 is an octave up and half the length.
    pub pitch: f32,
    pub looping: bool,
    /// Where it is, or `None` to be heard flat.
    pub position: Option<Vec3>,
    /// Distance at which it is at full volume, in kerosene units.
    pub reference_distance: f32,
    /// How quickly it falls off past that. Zero never gets quieter.
    pub attenuation: f32,
    /// Distance past which it is not heard at all.
    pub max_distance: f32,
}

impl Default for SoundParams {
    fn default() -> Self {
        SoundParams {
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            position: None,
            // Roughly a room's width: inside it a sound is at full volume,
            // which is what stops a footstep two paces away from being
            // noticeably quieter than one underfoot.
            reference_distance: 128.0,
            attenuation: 1.0,
            max_distance: 4096.0,
        }
    }
}

impl SoundParams {
    pub fn at(position: Vec3) -> SoundParams {
        SoundParams { position: Some(position), ..Default::default() }
    }

    pub fn looping(mut self) -> SoundParams {
        self.looping = true;
        self
    }

    pub fn with_volume(mut self, volume: f32) -> SoundParams {
        self.volume = volume;
        self
    }

    pub fn with_pitch(mut self, pitch: f32) -> SoundParams {
        self.pitch = pitch;
        self
    }
}

struct Voice {
    handle: SoundHandle,
    sound: Arc<Sound>,
    /// Position in the source, in frames. Fractional because pitch and
    /// resampling both mean the read head lands between samples.
    cursor: f64,
    params: SoundParams,
    /// Gains applied last block, ramped towards rather than jumped to.
    gain: [f32; 2],
    started: bool,
}

/// How many voices may sound at once.
///
/// A cap rather than none: a trigger firing every tick would otherwise stack
/// thousands of copies of the same sound, which is both deafening and slow.
/// The quietest voice gives way, which is the one nobody will miss.
pub const MAX_VOICES: usize = 64;

/// Per-block gain ramping, as a fraction of the way to the target.
///
/// Jumping straight to a new gain clicks -- a discontinuity in a waveform is
/// a click, and a sound moving past the listener changes gain every block.
const RAMP: f32 = 0.35;

/// The mixer.
pub struct Mixer {
    output_rate: u32,
    voices: Vec<Voice>,
    next_handle: u64,
    pub listener: Listener,
    /// Master volume, 0 to 1.
    pub volume: f32,
}

impl Mixer {
    pub fn new(output_rate: u32) -> Mixer {
        Mixer {
            output_rate: output_rate.max(1),
            voices: Vec::new(),
            next_handle: 1,
            listener: Listener::default(),
            volume: 1.0,
        }
    }

    pub fn output_rate(&self) -> u32 { self.output_rate }
    pub fn voice_count(&self) -> usize { self.voices.len() }
    pub fn is_playing(&self, handle: SoundHandle) -> bool {
        self.voices.iter().any(|v| v.handle == handle)
    }

    /// Start a sound. Returns a handle even if it is immediately inaudible,
    /// so a caller can stop something it started without checking first.
    pub fn play(&mut self, sound: Arc<Sound>, params: SoundParams) -> SoundHandle {
        let handle = SoundHandle(self.next_handle);
        self.next_handle += 1;

        if self.voices.len() >= MAX_VOICES {
            self.drop_quietest();
        }

        let gain = self.gains(&params);
        self.voices.push(Voice {
            handle,
            sound,
            cursor: 0.0,
            params,
            gain,
            started: false,
        });
        handle
    }

    pub fn stop(&mut self, handle: SoundHandle) {
        self.voices.retain(|v| v.handle != handle);
    }

    pub fn stop_all(&mut self) { self.voices.clear(); }

    /// Move a playing sound, for something that is going somewhere.
    pub fn set_position(&mut self, handle: SoundHandle, position: Vec3) {
        if let Some(voice) = self.voices.iter_mut().find(|v| v.handle == handle) {
            voice.params.position = Some(position);
        }
    }

    /// Mix the next block into an interleaved stereo buffer.
    ///
    /// The buffer is *replaced*, not added to: the caller owns the timeline
    /// and a mixer that accumulated would depend on what was there before.
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        if frames == 0 { return }

        let master = self.volume.clamp(0.0, 1.0);
        let listener = self.listener;
        let rate = self.output_rate as f64;

        for voice in &mut self.voices {
            let target = gains_for(&voice.params, &listener);
            // Ramp from wherever the last block ended, except on the very
            // first block of a sound, which starts where it belongs.
            if !voice.started {
                voice.gain = target;
                voice.started = true;
            }

            let source_rate = voice.sound.sample_rate.max(1) as f64;
            let step = (source_rate / rate) * voice.params.pitch.max(0.01) as f64;
            let total = voice.sound.frames();
            if total == 0 { continue }

            for frame in 0..frames {
                let position = voice.cursor;
                if position >= total as f64 {
                    if !voice.params.looping { break }
                    voice.cursor = position % total as f64;
                }

                // Ramp once per frame rather than per block, so a fast-moving
                // sound does not step.
                for channel in 0..2 {
                    voice.gain[channel] += (target[channel] - voice.gain[channel]) * RAMP / frames as f32;
                }

                let index = voice.cursor as usize;
                let fraction = (voice.cursor - index as f64) as f32;
                for channel in 0..2u16 {
                    let a = voice.sound.sample(index, channel);
                    let b = if index + 1 < total {
                        voice.sound.sample(index + 1, channel)
                    } else if voice.params.looping {
                        voice.sound.sample(0, channel)
                    } else {
                        0.0
                    };
                    let sample = a + (b - a) * fraction;
                    out[frame * 2 + channel as usize] +=
                        sample * voice.gain[channel as usize] * master;
                }

                voice.cursor += step;
            }
        }

        // Retire anything that ran off the end.
        let voices = &mut self.voices;
        voices.retain(|v| {
            v.params.looping || v.cursor < v.sound.frames() as f64
        });

        // Clipping rather than wrapping: a sum over 1.0 has to become loud,
        // not become a different waveform.
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }

    fn gains(&self, params: &SoundParams) -> [f32; 2] {
        gains_for(params, &self.listener)
    }

    fn drop_quietest(&mut self) {
        let listener = self.listener;
        let quietest = self
            .voices
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let ga = gains_for(&a.params, &listener);
                let gb = gains_for(&b.params, &listener);
                (ga[0] + ga[1]).total_cmp(&(gb[0] + gb[1]))
            })
            .map(|(i, _)| i);
        if let Some(index) = quietest { self.voices.remove(index); }
    }
}

/// Left and right gain for a sound, given where the listener is.
///
/// Split out so the model can be argued with on its own: how loud a thing is
/// at a distance is a design decision, not an implementation detail.
pub fn gains_for(params: &SoundParams, listener: &Listener) -> [f32; 2] {
    let volume = params.volume.max(0.0);
    let Some(position) = params.position else {
        // Unpositioned: heard flat in both ears.
        return [volume, volume];
    };

    let to_sound = position - listener.position;
    let distance = to_sound.length();
    if distance >= params.max_distance { return [0.0, 0.0] }

    // Inverse-distance falloff past a reference radius, inside which the
    // sound is at full volume. Without the radius, a sound at the listener's
    // own position divides by zero, and one a step away is much quieter than
    // one underfoot -- neither of which is how hearing works.
    let reference = params.reference_distance.max(1.0);
    let beyond = (distance - reference).max(0.0);
    let mut gain = reference / (reference + params.attenuation.max(0.0) * beyond);

    // Fade the last quarter of the range to nothing, so a sound does not
    // audibly switch off at its maximum distance.
    let fade_from = params.max_distance * 0.75;
    if distance > fade_from {
        let span = (params.max_distance - fade_from).max(1e-3);
        gain *= 1.0 - (distance - fade_from) / span;
    }
    gain *= volume;

    // Constant-power panning: the two gains square-sum to one, so a sound
    // crossing in front keeps the same loudness rather than dipping in the
    // middle.
    let direction = if distance > 1e-4 { to_sound / distance } else { Vec3::ZERO };
    let side = direction.dot(listener.basis.right).clamp(-1.0, 1.0);
    let angle = (side + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
    [gain * angle.cos(), gain * angle.sin()]
}

#[cfg(test)]
mod tests;
