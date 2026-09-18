// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Sound, from the engine's side.
//!
//! `kerosene-audio` decodes and mixes; this decides *when*. It owns the bank of
//! loaded sounds, keeps the listener on the player, loads sound scripts out of
//! the content tree, and answers the console.
//!
//! The mixer exists whether or not a sound card does. That is deliberate: if
//! audio only ran when a device opened, then everything about a game's
//! behaviour that touches sound -- how many voices a trigger starts, whether a
//! looping ambience was stopped -- would differ between a machine with sound
//! and one without, and only one of those would ever be tested. Here, a
//! missing device costs the last hop to the speakers and nothing else.

use kerosene_audio::{
    Mixer, ReverbParams, Sound, SoundBank, SoundHandle, SoundParams, SoundScript, VoiceEnv, env,
};
use kerosene_math::{Basis, Vec3};
use kerosene_vfs::Vfs;
use std::sync::{Arc, Mutex};

/// Spawnflag 2 on a sound entity: heard flat, wherever the listener is,
/// rather than placed at the entity. Read by [`Engine::play_entity_sound`]
/// (crate::Engine::play_entity_sound), so it is an engine convention that any
/// game's sound class can use; the stock `ambient_generic` does.
pub const SF_EVERYWHERE: u32 = 2;

/// The sample rate used when there is no device to ask.
const HEADLESS_RATE: u32 = 48_000;

/// How many voices get their occlusion re-traced in one tick. The rest keep
/// last tick's answer and take their turn next time; walls do not move
/// much in a sixty-fourth of a second.
const OCCLUSION_BUDGET: usize = 24;

/// A positioned voice the engine steers through the world every tick.
#[derive(Clone, Copy, Debug)]
struct TrackedVoice {
    handle: SoundHandle,
    position: Vec3,
    reference_distance: f32,
    max_distance: f32,
    /// How much wall was between it and the ear when last asked: `None`
    /// means no path at all.
    occlusion: Option<f32>,
}

pub struct AudioSystem {
    pub bank: SoundBank,
    mixer: Arc<Mutex<Mixer>>,
    #[cfg(feature = "audio")]
    device: Option<kerosene_audio::device::AudioDevice>,
    /// The listener and volume, settable without the mixer's lock.
    control: Arc<kerosene_audio::MixerControl>,
    /// What went wrong opening a device, said once.
    pub status: String,
    /// Warnings already given, so a bad entity is one line and not one per
    /// trigger.
    warned: std::collections::HashSet<String>,
    /// The room last handed to the mixer, so an unchanged one costs nothing
    /// a tick.
    reverb: Option<ReverbParams>,
    /// Every positioned voice still playing, and the buffers that carry its
    /// shaping to the mixer and its ending back. The buffers trade places
    /// with the mixer's rather than being rebuilt, so steady state allocates
    /// nothing.
    tracked: Vec<TrackedVoice>,
    envs: Vec<(SoundHandle, VoiceEnv)>,
    ended: Vec<SoundHandle>,
    /// Where the round-robin over occlusion traces got to.
    occlusion_cursor: usize,
    /// The acoustic room the listener was last in, for the debug readout.
    pub room: Option<u16>,
}

impl Default for AudioSystem {
    fn default() -> Self {
        AudioSystem::silent()
    }
}

impl AudioSystem {
    /// A mixer with no device behind it.
    pub fn silent() -> AudioSystem {
        let mixer = Mixer::new(HEADLESS_RATE);
        let control = mixer.control();
        AudioSystem {
            bank: SoundBank::new(),
            mixer: Arc::new(Mutex::new(mixer)),
            control,
            #[cfg(feature = "audio")]
            device: None,
            status: "no audio device".to_string(),
            warned: std::collections::HashSet::new(),
            reverb: None,
            tracked: Vec::new(),
            envs: Vec::new(),
            ended: Vec::new(),
            occlusion_cursor: 0,
            room: None,
        }
    }

    /// Try to open the default output device, falling back to silence.
    pub fn open() -> AudioSystem {
        #[cfg(feature = "audio")]
        {
            match kerosene_audio::device::AudioDevice::open() {
                Ok(device) => {
                    let mixer = Arc::clone(device.mixer());
                    let control = mixer.lock().unwrap_or_else(|e| e.into_inner()).control();
                    let status = format!("{} at {} Hz", device.name(), device.sample_rate());
                    AudioSystem {
                        bank: SoundBank::new(),
                        mixer,
                        control,
                        device: Some(device),
                        status,
                        warned: std::collections::HashSet::new(),
                        reverb: None,
                        tracked: Vec::new(),
                        envs: Vec::new(),
                        ended: Vec::new(),
                        occlusion_cursor: 0,
                        room: None,
                    }
                }
                Err(e) => {
                    // Once, at info: a machine without a sound card is a
                    // normal machine, not a broken one.
                    log::info!("audio: {e}; running silent");
                    let mut silent = AudioSystem::silent();
                    silent.status = format!("{e}");
                    silent
                }
            }
        }
        #[cfg(not(feature = "audio"))]
        {
            let mut silent = AudioSystem::silent();
            silent.status = "built without audio".to_string();
            silent
        }
    }

    /// Whether sound is actually reaching a device.
    pub fn is_audible(&self) -> bool {
        #[cfg(feature = "audio")]
        {
            self.device.is_some()
        }
        #[cfg(not(feature = "audio"))]
        {
            false
        }
    }

    pub fn mixer(&self) -> &Arc<Mutex<Mixer>> {
        &self.mixer
    }

    /// Log a warning the first time it is given, and not again.
    pub fn warn_once(&mut self, message: String) {
        if self.warned.insert(message.clone()) {
            log::warn!("{message}");
        }
    }

    /// Do something with the mixer.
    ///
    /// A poisoned lock is recovered from rather than propagated: the audio
    /// thread panicking should cost the sound, not the game.
    pub fn with_mixer<R>(&self, f: impl FnOnce(&mut Mixer) -> R) -> R {
        let mut mixer = self.mixer.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut mixer)
    }

    /// Point the ears at the player.
    ///
    /// Through the mixer's control handle, not its lock: this runs every
    /// tick, and taking the lock the audio callback needs was a dropped
    /// block whenever the two coincided.
    pub fn set_listener(&self, position: Vec3, basis: Basis) {
        self.control
            .set_listener(kerosene_audio::Listener { position, basis });
    }

    pub fn set_volume(&self, volume: f32) {
        self.control.set_volume(volume);
    }

    /// The room the listener is in, for the next block. Handed over only
    /// when it differs from last time; the mixer slides to it from wherever
    /// it is.
    pub fn set_reverb(&mut self, params: ReverbParams) {
        if self.reverb != Some(params) {
            self.reverb = Some(params);
            self.control.set_reverb(params);
        }
    }

    /// The room the mixer was last told about.
    pub fn reverb(&self) -> Option<ReverbParams> {
        self.reverb
    }

    pub fn stop_all(&mut self) {
        self.with_mixer(|mixer| mixer.stop_all());
        self.tracked.clear();
    }

    pub fn stop(&mut self, handle: SoundHandle) {
        self.with_mixer(|mixer| mixer.stop(handle));
        self.tracked.retain(|v| v.handle != handle);
    }

    /// Start a decoded sound, and follow it through the world if it has a
    /// place in it. Every voice the engine starts comes through here, so
    /// none escapes the shaping.
    pub fn start(&mut self, sound: Arc<Sound>, params: SoundParams) -> SoundHandle {
        let handle = self.with_mixer(|mixer| mixer.play(sound, params));
        if let Some(position) = params.position {
            self.tracked.push(TrackedVoice {
                handle,
                position,
                reference_distance: params.reference_distance,
                max_distance: params.max_distance,
                occlusion: Some(0.0),
            });
        }
        handle
    }

    /// Move a positioned voice.
    pub fn move_voice(&mut self, handle: SoundHandle, position: Vec3) {
        self.with_mixer(|mixer| mixer.set_position(handle, position));
        if let Some(v) = self.tracked.iter_mut().find(|v| v.handle == handle) {
            v.position = position;
        }
    }

    /// How many positioned voices are being followed.
    pub fn tracked_count(&self) -> usize {
        self.tracked.len()
    }

    /// Where every followed voice is and how much wall was last found in
    /// its way (`None`: no way through), for the debug overlay.
    pub fn tracked_voices(&self) -> impl Iterator<Item = (Vec3, Option<f32>)> + '_ {
        self.tracked.iter().map(|v| (v.position, v.occlusion))
    }

    /// Shape every tracked voice for the next block: air over its distance,
    /// walls in its way, and how much of it the listener's room should hear.
    ///
    /// `reach` answers, for a source position, how much wall lies between it
    /// and the ear -- `None` for no path at all -- and is asked for at most
    /// [`OCCLUSION_BUDGET`] voices a tick, the others keeping their last
    /// answer. Traces are the one expensive thing here and the map is the
    /// engine's, which is why the question is a callback.
    pub fn update_voices(
        &mut self,
        eye: Vec3,
        room_wet: f32,
        air: bool,
        mut reach: impl FnMut(Vec3) -> Option<f32>,
    ) {
        // Forget what has finished.
        self.control.take_ended(&mut self.ended);
        if !self.ended.is_empty() {
            let ended = &self.ended;
            self.tracked.retain(|v| !ended.contains(&v.handle));
            self.ended.clear();
        }
        if self.tracked.is_empty() {
            return;
        }

        // Ask about walls, a budget's worth at a time, round robin.
        let count = self.tracked.len();
        let asked = count.min(OCCLUSION_BUDGET);
        for i in 0..asked {
            let index = (self.occlusion_cursor + i) % count;
            let position = self.tracked[index].position;
            self.tracked[index].occlusion = reach(position);
        }
        self.occlusion_cursor = (self.occlusion_cursor + asked) % count;

        self.envs.clear();
        for voice in &self.tracked {
            let distance = voice.position.distance(eye);
            let mut shaped = VoiceEnv::IDENTITY;
            if air {
                shaped.cutoff_hz = env::air_cutoff(distance);
            }
            match voice.occlusion {
                None => {
                    shaped.gain = 0.0;
                }
                Some(occlusion) if occlusion > 0.0 => {
                    shaped.cutoff_hz = shaped.cutoff_hz.min(env::occlusion_cutoff(occlusion));
                    shaped.gain = env::occlusion_gain(occlusion);
                }
                Some(_) => {}
            }
            if shaped.gain > 0.0 && room_wet > 0.0 {
                // The room hears it as far as the ear does, fading out over
                // the same last quarter the dry sound does.
                let fade_from = voice.max_distance * 0.75;
                let fade = if distance >= voice.max_distance {
                    0.0
                } else if distance > fade_from {
                    1.0 - (distance - fade_from) / (voice.max_distance - fade_from).max(1e-3)
                } else {
                    1.0
                };
                shaped.send = env::send_for(room_wet, distance, voice.reference_distance) * fade;
            }
            self.envs.push((voice.handle, shaped));
        }
        self.control.set_voice_envs(&mut self.envs);
    }

    /// Load every `.kerosnd` in the content tree.
    pub fn load_scripts(&mut self, vfs: &Vfs) {
        for path in vfs.list("scripts", Some(kerosene_audio::SCRIPT_EXTENSION)) {
            match vfs.read_string(&path) {
                Ok(text) => match SoundScript::parse(&text) {
                    Ok(script) => {
                        log::info!("{} sounds from {path}", script.len());
                        self.bank.add_script(script);
                    }
                    Err(e) => log::error!("{path}: {e}"),
                },
                Err(e) => log::error!("could not read {path}: {e}"),
            }
        }
    }

    /// Get a sound, loading it the first time it is asked for.
    ///
    /// On demand rather than up front: a level references a handful of the
    /// sounds a game ships, and decoding all of them to play three is work
    /// nobody asked for.
    pub fn sound(&mut self, vfs: &Vfs, name: &str) -> Option<Arc<Sound>> {
        if let Some(sound) = self.bank.get(name) {
            return Some(sound);
        }
        if self.bank.already_missing(name) {
            return None;
        }

        // Every form the name might be, not one guessed path. Guessing was
        // what reported `sound/ambient/track.wav` missing when the file on
        // disk was a `.flac` -- a path nobody had written, about a file that
        // was right there.
        let candidates = self.bank.candidates(name);
        let found = candidates
            .iter()
            .find_map(|path| vfs.read(path).ok().map(|bytes| (path.clone(), bytes)));

        let Some((path, bytes)) = found else {
            // Once per name: a trigger firing every tick would otherwise fill
            // the console until nothing else in it is readable.
            self.bank.mark_missing(name);
            log::warn!("sound `{name}`: {}", explain_missing(vfs, &candidates));
            return None;
        };

        // A compiled file says where its loop is; that is half of why the
        // format exists, and it is kept rather than dropped on the floor.
        let decoded = if path.ends_with(kerosene_audio::compiled::EXTENSION) {
            kerosene_audio::compiled::decode(&bytes).map(|(sound, info)| {
                let region = (!info.looping.is_empty())
                    .then_some((info.looping.start as usize, info.looping.end as usize));
                (sound, region)
            })
        } else {
            kerosene_audio::wav::decode(&bytes).map(|sound| (sound, None))
        };
        match decoded {
            Ok((sound, region)) => {
                let sound = Arc::new(sound);
                self.bank.insert(name, Arc::clone(&sound));
                self.bank.set_loop_region(name, region);
                Some(sound)
            }
            Err(e) => {
                log::warn!("sound `{name}` ({path}): {e}");
                self.bank.mark_missing(name);
                None
            }
        }
    }

    /// Play a sound by name, with whatever the script says about it.
    ///
    /// `position` overrides the script: where a sound is comes from what
    /// plays it.
    pub fn play(
        &mut self,
        vfs: &Vfs,
        name: &str,
        position: Option<Vec3>,
        volume_scale: f32,
    ) -> Option<SoundHandle> {
        let sound = self.sound(vfs, name)?;
        let (_, mut params) = self.bank.resolve(name);
        params.position = position;
        params.volume *= volume_scale.max(0.0);
        Some(self.start(sound, params))
    }

    /// Play with parameters worked out by the caller.
    pub fn play_with(&mut self, vfs: &Vfs, name: &str, params: SoundParams) -> Option<SoundHandle> {
        let sound = self.sound(vfs, name)?;
        Some(self.start(sound, params))
    }

    /// Forget every decoded sound, keeping the scripts.
    pub fn forget_sounds(&mut self) {
        self.stop_all();
        self.bank.forget_all();
    }
}

/// Why a sound could not be found, in terms of what is actually on disk.
///
/// Three different situations wear the same "not found" in most engines, and
/// only one of them means the file is absent:
///
/// * A source is there but the engine does not read it. Then the answer is a
///   build, and saying so is the difference between two minutes and an
///   afternoon.
/// * Nothing is there under any name, and the useful thing is the list of what
///   was tried.
/// * The name is a typo, which the list also reveals.
pub fn explain_missing(vfs: &kerosene_vfs::Vfs, candidates: &[String]) -> String {
    let first = candidates.first().map(String::as_str).unwrap_or("");
    if let Some(source) = kerosene_audio::uncompiled_source(first, |p| vfs.exists(p)) {
        return format!(
            "{source} is there, but the engine reads compiled sound and .wav. \
             Run `timbre build` to compile it."
        );
    }
    format!(
        "none of {} was found in any search path",
        candidates.join(", ")
    )
}
