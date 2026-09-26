// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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

use kerosene_math::{Basis, Vec3};
use std::sync::Arc;

use crate::Sound;
use crate::dsp::OnePole;
use crate::env::VoiceEnv;
use crate::reverb::{Fdn, ReverbParams};

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
        Listener {
            position: Vec3::ZERO,
            basis: kerosene_math::Angles::ZERO.vectors(),
        }
    }
}

/// How a sound is heard.
#[derive(Clone, Copy, Debug)]
pub struct SoundParams {
    pub volume: f32,
    /// Playback rate. 2.0 is an octave up and half the length.
    pub pitch: f32,
    pub looping: bool,
    /// The frames to repeat when looping, `start..end`, or `None` for the
    /// whole sound. A compiled `.keroaud` carries this so an ambience can
    /// have an attack that plays once and a body that loops.
    pub loop_region: Option<(usize, usize)>,
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
            loop_region: None,
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
        SoundParams {
            position: Some(position),
            ..Default::default()
        }
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
    /// What the world is doing to this voice, and what it will be doing:
    /// muffling, quietening, and how much the room hears. Ramped like the
    /// gains. At identity the whole path is skipped.
    env: VoiceEnv,
    env_target: VoiceEnv,
    /// The muffling itself, one per ear.
    lowpass: [OnePole; 2],
    /// Whether the filters were running last block, so they can be primed
    /// with the signal when they switch in rather than starting from zero.
    shaped: bool,
}

impl Voice {
    fn is_shaped(&self) -> bool {
        !(self.env.is_identity() && self.env_target.is_identity())
    }
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

/// How many finished voices the mixer will remember for the game thread
/// before it starts forgetting the oldest. Well over what a tick can end.
const MAX_ENDED: usize = 256;

/// The mixer.
pub struct Mixer {
    output_rate: u32,
    voices: Vec<Voice>,
    next_handle: u64,
    pub listener: Listener,
    /// Master volume, 0 to 1.
    pub volume: f32,
    /// Where the game thread leaves a new listener and volume for the next
    /// block to pick up. See [`MixerControl`].
    control: Arc<MixerControl>,
    /// The room, and the mono bus the voices feed it through.
    reverb: Fdn,
    send: Vec<f32>,
    /// Voices that ran off the end since the game thread last asked.
    ended: Vec<SoundHandle>,
}

/// The per-tick knobs, kept outside the mixer's own lock.
///
/// The audio callback `try_lock`s the mixer and plays silence if it loses --
/// a late buffer being worse than a quiet one. The game thread used to take
/// that same lock every tick just to move the listener, and a callback that
/// landed in the middle of it was a dropped block. These live in their own
/// tiny lock instead, held for a copy and no longer; the mixer reads them at
/// the top of each block.
#[derive(Default)]
pub struct MixerControl {
    listener: std::sync::Mutex<Listener>,
    /// Volume as its bit pattern, so it needs no lock at all.
    volume: std::sync::atomic::AtomicU32,
    /// The room the listener is in, and whether it changed since the mixer
    /// last looked.
    reverb: std::sync::Mutex<(ReverbParams, bool)>,
    /// Per-voice shaping for the next block. Swapped in and out whole, so
    /// neither thread allocates once both buffers have grown to size.
    voice_env: std::sync::Mutex<Vec<(SoundHandle, VoiceEnv)>>,
    /// Voices that finished, for the game thread to stop tracking.
    ended: std::sync::Mutex<Vec<SoundHandle>>,
}

impl MixerControl {
    fn new() -> MixerControl {
        MixerControl {
            listener: std::sync::Mutex::new(Listener::default()),
            volume: std::sync::atomic::AtomicU32::new(1.0f32.to_bits()),
            reverb: std::sync::Mutex::new((ReverbParams::default(), false)),
            voice_env: std::sync::Mutex::new(Vec::with_capacity(MAX_VOICES)),
            ended: std::sync::Mutex::new(Vec::with_capacity(MAX_ENDED)),
        }
    }

    /// The room to be in, for the next block.
    pub fn set_reverb(&self, params: ReverbParams) {
        *self.reverb.lock().unwrap_or_else(|e| e.into_inner()) = (params, true);
    }

    /// Hand over every voice's shaping for the next block.
    ///
    /// Swaps the caller's list with the one held here and gives back the old
    /// one, which the caller clears and refills next tick -- the two buffers
    /// trade places forever and nobody allocates. A list the mixer has not
    /// consumed yet is simply replaced: the newest is the one that matters.
    pub fn set_voice_envs(&self, envs: &mut Vec<(SoundHandle, VoiceEnv)>) {
        let mut held = self.voice_env.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::swap(&mut *held, envs);
    }

    /// Collect the voices that finished since last time, into the caller's
    /// list -- swapped, like `set_voice_envs`, so the caller should clear
    /// what it gets back once it has read it.
    pub fn take_ended(&self, into: &mut Vec<SoundHandle>) {
        let mut held = self.ended.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::swap(&mut *held, into);
        held.clear();
    }

    /// Point the ears somewhere, for the next block.
    pub fn set_listener(&self, listener: Listener) {
        *self.listener.lock().unwrap_or_else(|e| e.into_inner()) = listener;
    }

    /// Master volume, 0 to 1, for the next block.
    pub fn set_volume(&self, volume: f32) {
        self.volume.store(
            volume.clamp(0.0, 1.0).to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

impl Mixer {
    pub fn new(output_rate: u32) -> Mixer {
        Mixer {
            output_rate: output_rate.max(1),
            voices: Vec::new(),
            next_handle: 1,
            listener: Listener::default(),
            volume: 1.0,
            control: Arc::new(MixerControl::new()),
            reverb: Fdn::new(output_rate.max(1)),
            send: vec![0.0; 4096],
            ended: Vec::with_capacity(MAX_ENDED),
        }
    }

    /// The room, directly. `MixerControl::set_reverb` is the same from the
    /// game thread.
    pub fn set_reverb(&mut self, params: ReverbParams) {
        self.reverb.set_params(params);
    }

    pub fn reverb(&self) -> &ReverbParams {
        self.reverb.params()
    }

    /// Whether the room is doing anything at all this block.
    pub fn reverb_active(&self) -> bool {
        self.reverb.is_active()
    }

    /// Shape one voice, directly. From the game thread, hand a whole list to
    /// `MixerControl::set_voice_envs` instead.
    pub fn set_voice_env(&mut self, handle: SoundHandle, env: VoiceEnv) {
        if let Some(voice) = self.voices.iter_mut().find(|v| v.handle == handle) {
            voice.env_target = env;
        }
    }

    /// The handle the game thread steers the listener and volume through.
    pub fn control(&self) -> Arc<MixerControl> {
        Arc::clone(&self.control)
    }

    /// Take whatever the game thread has set since the last block. Never
    /// waits: if the control lock is momentarily held, the previous listener
    /// serves one more block. `mix` does this itself; it is public for a
    /// test that wants to read the result without mixing.
    pub fn apply_control(&mut self) {
        if let Ok(listener) = self.control.listener.try_lock() {
            self.listener = *listener;
        }
        self.volume = f32::from_bits(
            self.control
                .volume
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        if let Ok(mut reverb) = self.control.reverb.try_lock()
            && reverb.1
        {
            reverb.1 = false;
            self.reverb.set_params(reverb.0);
        }
        if let Ok(mut envs) = self.control.voice_env.try_lock() {
            for (handle, env) in envs.drain(..) {
                if let Some(voice) = self.voices.iter_mut().find(|v| v.handle == handle) {
                    voice.env_target = env;
                }
            }
        }
        if !self.ended.is_empty()
            && let Ok(mut ended) = self.control.ended.try_lock()
        {
            let room = MAX_ENDED.saturating_sub(ended.len());
            ended.extend(self.ended.drain(..).take(room));
            self.ended.clear();
        }
    }

    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }
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
            env: VoiceEnv::IDENTITY,
            env_target: VoiceEnv::IDENTITY,
            lowpass: [OnePole::open(); 2],
            shaped: false,
        });
        handle
    }

    pub fn stop(&mut self, handle: SoundHandle) {
        self.voices.retain(|v| v.handle != handle);
    }

    pub fn stop_all(&mut self) {
        self.voices.clear();
    }

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
        if frames == 0 {
            return;
        }
        self.apply_control();

        let master = self.volume.clamp(0.0, 1.0);
        let listener = self.listener;
        let rate = self.output_rate as f64;

        // The room's input. Only voices with a send touch it, so when
        // nothing does it stays zero and costs nothing.
        if self.send.len() < frames {
            self.send.resize(frames, 0.0);
        }
        let send = &mut self.send[..frames];
        send.fill(0.0);

        for voice in &mut self.voices {
            let target = gains_for(&voice.params, &listener);
            // Ramp from wherever the last block ended, except on the very
            // first block of a sound, which starts where it belongs.
            if !voice.started {
                voice.gain = target;
                voice.env = voice.env_target;
                voice.started = true;
            }

            // Shaping by the world, if any. The cutoff moves once a block:
            // the filter's output stays continuous across a coefficient
            // change, so it need not move every frame the way gain does.
            let shaped = voice.is_shaped();
            let prime = shaped && !voice.shaped;
            voice.shaped = shaped;
            if shaped {
                let from = voice.env.cutoff_hz.max(1.0).ln();
                let to = voice.env_target.cutoff_hz.max(1.0).ln();
                voice.env.cutoff_hz = (from + (to - from) * RAMP).exp();
                for lp in &mut voice.lowpass {
                    lp.set_cutoff(voice.env.cutoff_hz, rate as f32);
                }
            }

            let source_rate = voice.sound.sample_rate.max(1) as f64;
            let step = (source_rate / rate) * voice.params.pitch.max(0.01) as f64;
            let total = voice.sound.frames();
            if total == 0 {
                continue;
            }
            // Where a loop jumps back to, and from: the region the file
            // declares when it is usable, else the whole sound.
            let (loop_start, loop_end) = match voice.params.loop_region {
                Some((start, end)) if start < end && end <= total => (start, end),
                _ => (0, total),
            };

            for frame in 0..frames {
                let position = voice.cursor;
                if position >= loop_end as f64 {
                    if !voice.params.looping {
                        if position >= total as f64 {
                            break;
                        }
                    } else {
                        let span = (loop_end - loop_start) as f64;
                        voice.cursor = loop_start as f64 + (position - loop_start as f64) % span;
                    }
                }

                // Ramp once per frame rather than per block, so a fast-moving
                // sound does not step.
                for (gain, target) in voice.gain.iter_mut().zip(target) {
                    *gain += (target - *gain) * RAMP / frames as f32;
                }
                if shaped {
                    let k = RAMP / frames as f32;
                    voice.env.gain += (voice.env_target.gain - voice.env.gain) * k;
                    voice.env.send += (voice.env_target.send - voice.env.send) * k;
                }

                let index = voice.cursor as usize;
                let fraction = (voice.cursor - index as f64) as f32;
                let mut mono = 0.0;
                for channel in 0..2u16 {
                    let a = voice.sound.sample(index, channel);
                    let b = if voice.params.looping && index + 1 >= loop_end {
                        voice.sound.sample(loop_start, channel)
                    } else if index + 1 < total {
                        voice.sound.sample(index + 1, channel)
                    } else {
                        0.0
                    };
                    let mut sample = a + (b - a) * fraction;
                    if shaped {
                        let lowpass = &mut voice.lowpass[channel as usize];
                        if prime && frame == 0 {
                            lowpass.prime(sample);
                        }
                        sample = lowpass.process(sample) * voice.env.gain;
                        mono += sample * 0.5;
                    }
                    out[frame * 2 + channel as usize] +=
                        sample * voice.gain[channel as usize] * master;
                }
                if shaped && voice.env.send > 0.0 {
                    send[frame] += mono * voice.env.send * voice.params.volume.max(0.0) * master;
                }

                voice.cursor += step;
            }

            // Once a shaped voice has ramped back to nothing, drop it onto
            // the plain path again so it is bit for bit what it was.
            if shaped && voice.env_target.is_identity() && voice.env.is_nearly_identity() {
                voice.env = VoiceEnv::IDENTITY;
                for lp in &mut voice.lowpass {
                    *lp = OnePole::open();
                }
            }
        }

        // The room on top of the dry mix.
        self.reverb.process(&self.send[..frames], out);

        // Retire anything that ran off the end, remembering who for the
        // game thread.
        let ended = &mut self.ended;
        self.voices.retain(|v| {
            let alive = v.params.looping || v.cursor < v.sound.frames() as f64;
            if !alive && ended.len() < MAX_ENDED {
                ended.push(v.handle);
            }
            alive
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
        if let Some(index) = quietest {
            let voice = self.voices.remove(index);
            // Gone as surely as if it had ended, and the game thread tracking
            // it needs to hear so.
            if self.ended.len() < MAX_ENDED {
                self.ended.push(voice.handle);
            }
        }
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
    if distance >= params.max_distance {
        return [0.0, 0.0];
    }

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
    let direction = if distance > 1e-4 {
        to_sound / distance
    } else {
        Vec3::ZERO
    };
    let side = direction.dot(listener.basis.right).clamp(-1.0, 1.0);
    let angle = (side + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
    [gain * angle.cos(), gain * angle.sin()]
}

#[cfg(test)]
mod tests;
