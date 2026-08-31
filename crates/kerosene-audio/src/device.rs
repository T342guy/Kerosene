// SPDX-License-Identifier: MPL-2.0
//! The last hop: a mixer feeding a sound card.
//!
//! Behind a feature flag because it is the only part of this crate that needs
//! a C library (ALSA, on Linux) and the only part that cannot run in a test.
//! Everything that decides how a sound *sounds* is in [`crate::mixer`], which
//! builds and is tested without any of this.
//!
//! The callback runs on the audio thread, which has a hard deadline and must
//! never block for long or allocate. It takes the mixer's lock, fills the
//! buffer, and lets go. The game holds the same lock to start and stop
//! sounds, for the length of a push onto a vector.
//!
//! **A game with no audio device is a game.** Every failure here is reported
//! once and then silence, never a refusal to start.

use crate::{AudioError, Mixer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

/// An open output stream and the mixer feeding it.
pub struct AudioDevice {
    mixer: Arc<Mutex<Mixer>>,
    /// Kept alive: dropping the stream stops the sound.
    _stream: cpal::Stream,
    rate: u32,
    name: String,
}

impl AudioDevice {
    /// Open the default output device with its own mixer.
    pub fn open() -> Result<AudioDevice, AudioError> {
        AudioDevice::open_with(|rate| Arc::new(Mutex::new(Mixer::new(rate))))
    }

    /// Open the default output device, building the mixer once the device's
    /// sample rate is known.
    ///
    /// The rate is the device's to choose, and a mixer built at the wrong one
    /// plays everything at the wrong speed -- so the caller is handed the
    /// answer rather than having to guess it first.
    pub fn open_with(
        make_mixer: impl FnOnce(u32) -> Arc<Mutex<Mixer>>,
    ) -> Result<AudioDevice, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::NoDevice("no default output device".into()))?;
        let name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "unknown".into());

        let config = device
            .default_output_config()
            .map_err(|e| AudioError::NoDevice(format!("no output config: {e}")))?;
        let rate = config.sample_rate();
        let channels = config.channels() as usize;

        let mixer = make_mixer(rate);
        let stream = build_stream(&device, &config, Arc::clone(&mixer), channels)?;
        stream
            .play()
            .map_err(|e| AudioError::NoDevice(format!("could not start the stream: {e}")))?;

        Ok(AudioDevice { mixer, _stream: stream, rate, name })
    }

    pub fn mixer(&self) -> &Arc<Mutex<Mixer>> { &self.mixer }
    pub fn sample_rate(&self) -> u32 { self.rate }
    pub fn name(&self) -> &str { &self.name }

    /// Do something with the mixer.
    ///
    /// A poisoned lock is recovered from rather than propagated: the audio
    /// thread panicking should cost the sound, not the game.
    pub fn with_mixer<R>(&self, f: impl FnOnce(&mut Mixer) -> R) -> R {
        let mut mixer = self.mixer.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut mixer)
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    mixer: Arc<Mutex<Mixer>>,
    channels: usize,
) -> Result<cpal::Stream, AudioError> {
    let stream_config: cpal::StreamConfig = config.config();
    // Moved into exactly one arm below.
    let on_error = |e| log::error!("audio stream: {e}");

    // One scratch buffer, grown on the audio thread only when the host asks
    // for a bigger block than before. Allocating every callback is the
    // classic way to make audio stutter.
    let mut scratch: Vec<f32> = Vec::new();

    let fill = move |data: &mut [f32]| {
        let frames = data.len() / channels.max(1);
        if scratch.len() < frames * 2 { scratch.resize(frames * 2, 0.0); }
        let block = &mut scratch[..frames * 2];

        match mixer.try_lock() {
            Ok(mut mixer) => mixer.mix(block),
            // Rather than wait for the game thread and miss the deadline:
            // one block of silence is a click, a late buffer is a dropout.
            Err(_) => block.fill(0.0),
        }

        for (frame, out) in data.chunks_mut(channels).enumerate() {
            let (l, r) = (block[frame * 2], block[frame * 2 + 1]);
            match out.len() {
                0 => {}
                1 => out[0] = (l + r) * 0.5,
                _ => {
                    out[0] = l;
                    out[1] = r;
                    // More than stereo: the extra channels get silence rather
                    // than a copy, which would put the same sound in the
                    // surrounds.
                    for extra in &mut out[2..] { *extra = 0.0; }
                }
            }
        }
    };

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let mut fill = fill;
            device.build_output_stream(
                stream_config,
                move |data: &mut [f32], _| fill(data),
                on_error,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let mut fill = fill;
            let mut block: Vec<f32> = Vec::new();
            device.build_output_stream(
                stream_config,
                move |data: &mut [i16], _| {
                    if block.len() < data.len() { block.resize(data.len(), 0.0); }
                    let slice = &mut block[..data.len()];
                    fill(slice);
                    for (out, v) in data.iter_mut().zip(slice.iter()) {
                        *out = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                },
                on_error,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let mut fill = fill;
            let mut block: Vec<f32> = Vec::new();
            device.build_output_stream(
                stream_config,
                move |data: &mut [u16], _| {
                    if block.len() < data.len() { block.resize(data.len(), 0.0); }
                    let slice = &mut block[..data.len()];
                    fill(slice);
                    for (out, v) in data.iter_mut().zip(slice.iter()) {
                        *out = ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 65535.0) as u16;
                    }
                },
                on_error,
                None,
            )
        }
        other => {
            return Err(AudioError::Unsupported(format!("sample format {other:?}")));
        }
    };

    stream.map_err(|e| AudioError::NoDevice(format!("could not open a stream: {e}")))
}
