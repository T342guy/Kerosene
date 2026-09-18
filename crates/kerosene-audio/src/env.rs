// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! What the world does to one sound on its way to the ears.
//!
//! The mixer knows about distance and direction; it does not know about
//! walls, air, or rooms, and must not, because it has no map. Everything the
//! map contributes is boiled down by the engine into a [`VoiceEnv`] per voice
//! -- how muffled, how much quieter, how much of it the room hears -- and the
//! mixer only has to apply it. The functions beside it are the model: how
//! much air takes out of a sound over a distance is a design decision, and
//! it lives here where it can be argued with and tested on its own.

/// The cutoff a voice has when nothing is in the way: fully open.
pub const OPEN_CUTOFF: f32 = 20_000.0;

/// The cutoff of a fully occluded voice: what a closed door leaves.
pub const OCCLUDED_CUTOFF: f32 = 800.0;

/// How much quieter a fully occluded voice is, in decibels.
pub const OCCLUDED_DB: f32 = -12.0;

/// The distance over which air takes the cutoff down one decade, in units.
///
/// At this range a voice is low-passed at 2 kHz; at twice it, 200 Hz. Air
/// at 8 kHz really does lose more than a decibel every hundred metres, and
/// a shout across a field should not arrive with every consonant intact.
pub const AIR_DECADE: f32 = 4096.0;

/// How one voice is shaped by its surroundings, for the next block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceEnv {
    /// Low-pass cutoff in hertz. [`OPEN_CUTOFF`] passes everything.
    pub cutoff_hz: f32,
    /// A gain on top of falloff and volume, 0 to 1.
    pub gain: f32,
    /// How much of the voice feeds the room's reverb, 0 to 1.
    pub send: f32,
}

impl VoiceEnv {
    /// Nothing in the way, no air worth mentioning, no room. The mixer
    /// skips the filter entirely for a voice at identity, so a game that
    /// never sets an env sounds exactly as it did before there was one.
    pub const IDENTITY: VoiceEnv = VoiceEnv {
        cutoff_hz: OPEN_CUTOFF,
        gain: 1.0,
        send: 0.0,
    };

    pub fn is_identity(&self) -> bool {
        self.cutoff_hz >= OPEN_CUTOFF && self.gain >= 1.0 && self.send <= 0.0
    }

    /// Whether this is near enough to identity that ramping the rest of the
    /// way would be inaudible.
    pub fn is_nearly_identity(&self) -> bool {
        self.cutoff_hz >= OPEN_CUTOFF * 0.999 && self.gain >= 0.9999 && self.send <= 1e-4
    }
}

impl Default for VoiceEnv {
    fn default() -> Self {
        VoiceEnv::IDENTITY
    }
}

/// The cutoff air leaves a sound with after `distance` units.
pub fn air_cutoff(distance: f32) -> f32 {
    let distance = distance.max(0.0);
    (OPEN_CUTOFF * 10.0f32.powf(-distance / AIR_DECADE)).max(100.0)
}

/// The cutoff a voice has behind `occlusion` worth of wall, 0 to 1.
pub fn occlusion_cutoff(occlusion: f32) -> f32 {
    let occlusion = occlusion.clamp(0.0, 1.0);
    OPEN_CUTOFF * (OCCLUDED_CUTOFF / OPEN_CUTOFF).powf(occlusion)
}

/// The gain a voice has behind `occlusion` worth of wall, 0 to 1.
pub fn occlusion_gain(occlusion: f32) -> f32 {
    crate::dsp::db(OCCLUDED_DB * occlusion.clamp(0.0, 1.0))
}

/// How much of a voice at `distance` the room should hear, given the room's
/// own wetness.
///
/// More with distance rather than less: the direct sound falls off and the
/// room does not, which is why a far-off door sounds like the hall it is in
/// and a near one sounds like a door.
pub fn send_for(room_wet: f32, distance: f32, reference_distance: f32) -> f32 {
    let reference = reference_distance.max(1.0);
    room_wet.clamp(0.0, 1.0) * (distance / reference).clamp(0.25, 1.0)
}

#[cfg(test)]
mod tests;
