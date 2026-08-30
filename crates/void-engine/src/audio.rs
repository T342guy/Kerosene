// SPDX-License-Identifier: LGPL-3.0-or-later
//! Sound, from the engine's side.
//!
//! `void-audio` decodes and mixes; this decides *when*. It owns the bank of
//! loaded sounds, keeps the listener on the player, loads sound scripts out of
//! the content tree, and answers the console.
//!
//! The mixer exists whether or not a sound card does. That is deliberate: if
//! audio only ran when a device opened, then everything about a game's
//! behaviour that touches sound -- how many voices a trigger starts, whether a
//! looping ambience was stopped -- would differ between a machine with sound
//! and one without, and only one of those would ever be tested. Here, a
//! missing device costs the last hop to the speakers and nothing else.

use std::sync::{Arc, Mutex};
use void_audio::{Mixer, Sound, SoundBank, SoundHandle, SoundParams, SoundScript};
use void_math::{Basis, Vec3};
use void_vfs::Vfs;

/// The sample rate used when there is no device to ask.
const HEADLESS_RATE: u32 = 48_000;

pub struct AudioSystem {
    pub bank: SoundBank,
    mixer: Arc<Mutex<Mixer>>,
    #[cfg(feature = "audio")]
    device: Option<void_audio::device::AudioDevice>,
    /// What went wrong opening a device, said once.
    pub status: String,
}

impl Default for AudioSystem {
    fn default() -> Self { AudioSystem::silent() }
}

impl AudioSystem {
    /// A mixer with no device behind it.
    pub fn silent() -> AudioSystem {
        AudioSystem {
            bank: SoundBank::new(),
            mixer: Arc::new(Mutex::new(Mixer::new(HEADLESS_RATE))),
            #[cfg(feature = "audio")]
            device: None,
            status: "no audio device".to_string(),
        }
    }

    /// Try to open the default output device, falling back to silence.
    pub fn open() -> AudioSystem {
        #[cfg(feature = "audio")]
        {
            match void_audio::device::AudioDevice::open() {
                Ok(device) => {
                    let mixer = Arc::clone(device.mixer());
                    let status = format!("{} at {} Hz", device.name(), device.sample_rate());
                    return AudioSystem {
                        bank: SoundBank::new(),
                        mixer,
                        device: Some(device),
                        status,
                    };
                }
                Err(e) => {
                    // Once, at info: a machine without a sound card is a
                    // normal machine, not a broken one.
                    log::info!("audio: {e}; running silent");
                    let mut silent = AudioSystem::silent();
                    silent.status = format!("{e}");
                    return silent;
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

    pub fn mixer(&self) -> &Arc<Mutex<Mixer>> { &self.mixer }

    /// Do something with the mixer.
    ///
    /// A poisoned lock is recovered from rather than propagated: the audio
    /// thread panicking should cost the sound, not the game.
    pub fn with_mixer<R>(&self, f: impl FnOnce(&mut Mixer) -> R) -> R {
        let mut mixer = self.mixer.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut mixer)
    }

    /// Point the ears at the player.
    pub fn set_listener(&self, position: Vec3, basis: Basis) {
        self.with_mixer(|mixer| {
            mixer.listener.position = position;
            mixer.listener.basis = basis;
        });
    }

    pub fn set_volume(&self, volume: f32) {
        self.with_mixer(|mixer| mixer.volume = volume.clamp(0.0, 1.0));
    }

    pub fn stop_all(&self) {
        self.with_mixer(|mixer| mixer.stop_all());
    }

    pub fn stop(&self, handle: SoundHandle) {
        self.with_mixer(|mixer| mixer.stop(handle));
    }

    /// Load every `.voidsnd` in the content tree.
    pub fn load_scripts(&mut self, vfs: &Vfs) {
        for path in vfs.list("scripts", Some(void_audio::SCRIPT_EXTENSION)) {
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
        if let Some(sound) = self.bank.get(name) { return Some(sound) }
        if self.bank.already_missing(name) { return None }

        let (script_path, _) = self.bank.resolve(name);
        // The compiled form first, then the source. A shipped game has only
        // the first; a checkout mid-edit may have only the second, and a
        // designer who has just dropped a `.wav` in should hear it without
        // running a build to find out whether it is the right one.
        let compiled_path = swap_extension(&script_path, void_audio::compiled::EXTENSION);
        let (path, bytes) = match vfs.read(&compiled_path) {
            Ok(bytes) => (compiled_path, bytes),
            Err(_) => match vfs.read(&script_path) {
                Ok(bytes) => (script_path, bytes),
                Err(e) => {
                    // Once per name: a trigger firing every tick would
                    // otherwise fill the console until nothing else in it is
                    // readable.
                    log::warn!("sound `{name}`: {e}");
                    self.bank.mark_missing(name);
                    return None;
                }
            },
        };

        let decoded = if path.ends_with(void_audio::compiled::EXTENSION) {
            void_audio::compiled::decode(&bytes).map(|(sound, _)| sound)
        } else {
            void_audio::wav::decode(&bytes)
        };
        match decoded {
            Ok(sound) => {
                let sound = Arc::new(sound);
                self.bank.insert(name, Arc::clone(&sound));
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
        Some(self.with_mixer(|mixer| mixer.play(sound, params)))
    }

    /// Play with parameters worked out by the caller.
    pub fn play_with(&mut self, vfs: &Vfs, name: &str, params: SoundParams) -> Option<SoundHandle> {
        let sound = self.sound(vfs, name)?;
        Some(self.with_mixer(|mixer| mixer.play(sound, params)))
    }

    /// Forget every decoded sound, keeping the scripts.
    pub fn forget_sounds(&mut self) {
        self.stop_all();
        self.bank.forget_all();
    }
}

/// The same path wearing a different extension.
///
/// String work rather than `Path::with_extension`, because a VFS path is a
/// forward-slashed name inside an archive and not something the platform's
/// path rules have any business normalising.
fn swap_extension(path: &str, extension: &str) -> String {
    match path.rsplit_once('.') {
        // Only the last component's extension: `sound/v1.2/click` has a dot in
        // a directory name and no extension at all.
        Some((stem, tail)) if !tail.contains('/') => format!("{stem}.{extension}"),
        _ => format!("{path}.{extension}"),
    }
}
