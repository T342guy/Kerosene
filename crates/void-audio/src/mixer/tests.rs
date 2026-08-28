// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;
use void_math::Angles;

const RATE: u32 = 48_000;

/// A sound that is a constant 1.0, so gain is readable straight off the
/// output rather than having to be inferred from a waveform.
fn steady(frames: usize, channels: u16) -> Arc<Sound> {
    Arc::new(Sound {
        channels,
        sample_rate: RATE,
        samples: vec![1.0; frames * channels as usize],
    })
}

fn mixer() -> Mixer { Mixer::new(RATE) }

/// Mix one block and report the peak in each ear.
fn peaks(mixer: &mut Mixer, frames: usize) -> (f32, f32) {
    let mut out = vec![0.0; frames * 2];
    mixer.mix(&mut out);
    let left = out.iter().step_by(2).fold(0.0f32, |a, s| a.max(s.abs()));
    let right = out.iter().skip(1).step_by(2).fold(0.0f32, |a, s| a.max(s.abs()));
    (left, right)
}

/// A listener at the origin facing +X, which is yaw 0.
fn facing_x() -> Listener {
    Listener { position: Vec3::ZERO, basis: Angles::new(0.0, 0.0, 0.0).vectors() }
}

// ---- playing and stopping -------------------------------------------------

#[test]
fn a_played_sound_is_audible() {
    let mut mixer = mixer();
    mixer.play(steady(1000, 1), SoundParams::default());
    let (l, r) = peaks(&mut mixer, 128);
    assert!(l > 0.9 && r > 0.9, "{l} {r}");
}

#[test]
fn silence_is_what_comes_out_when_nothing_is_playing() {
    let mut mixer = mixer();
    let mut out = vec![9.0; 64];
    mixer.mix(&mut out);
    assert!(out.iter().all(|s| *s == 0.0), "the buffer was not cleared");
}

#[test]
fn a_sound_ends_and_its_voice_goes_away() {
    let mut mixer = mixer();
    mixer.play(steady(64, 1), SoundParams::default());
    assert_eq!(mixer.voice_count(), 1);
    let mut out = vec![0.0; 256 * 2];
    mixer.mix(&mut out);
    assert_eq!(mixer.voice_count(), 0, "the voice outlived the sound");
}

#[test]
fn a_looping_sound_does_not_end() {
    let mut mixer = mixer();
    let handle = mixer.play(steady(64, 1), SoundParams::default().looping());
    for _ in 0..10 {
        let mut out = vec![0.0; 256 * 2];
        mixer.mix(&mut out);
    }
    assert!(mixer.is_playing(handle));
    let (l, _) = peaks(&mut mixer, 128);
    assert!(l > 0.9, "a looping sound went quiet");
}

#[test]
fn stopping_a_sound_stops_it() {
    let mut mixer = mixer();
    let handle = mixer.play(steady(10_000, 1), SoundParams::default().looping());
    mixer.stop(handle);
    assert!(!mixer.is_playing(handle));
    let (l, r) = peaks(&mut mixer, 128);
    assert_eq!((l, r), (0.0, 0.0));
}

#[test]
fn stop_all_clears_everything() {
    let mut mixer = mixer();
    for _ in 0..5 { mixer.play(steady(10_000, 1), SoundParams::default()); }
    mixer.stop_all();
    assert_eq!(mixer.voice_count(), 0);
}

#[test]
fn a_handle_comes_back_even_for_a_sound_nobody_will_hear() {
    // So a caller can stop what it started without checking first.
    let mut mixer = mixer();
    mixer.listener = facing_x();
    let far = SoundParams { position: Some(Vec3::new(1e6, 0.0, 0.0)), ..Default::default() };
    let handle = mixer.play(steady(100, 1), far);
    assert!(mixer.is_playing(handle));
}

// ---- volume ---------------------------------------------------------------

#[test]
fn volume_scales_what_comes_out() {
    let mut mixer = mixer();
    mixer.play(steady(1000, 1), SoundParams::default().with_volume(0.25));
    let (l, _) = peaks(&mut mixer, 512);
    assert!((l - 0.25).abs() < 0.02, "{l}");
}

#[test]
fn the_master_volume_applies_on_top() {
    let mut mixer = mixer();
    mixer.volume = 0.5;
    mixer.play(steady(1000, 1), SoundParams::default().with_volume(0.5));
    let (l, _) = peaks(&mut mixer, 512);
    assert!((l - 0.25).abs() < 0.02, "{l}");
}

#[test]
fn the_output_is_clipped_rather_than_wrapped() {
    // Eight sounds at full volume sum to 8. Wrapping would turn a loud moment
    // into a completely different waveform -- a buzz, not a bang.
    let mut mixer = mixer();
    for _ in 0..8 { mixer.play(steady(1000, 1), SoundParams::default()); }
    let mut out = vec![0.0; 256 * 2];
    mixer.mix(&mut out);
    assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)), "something escaped the clip");
    assert!(out.iter().any(|s| *s > 0.99), "and it should be loud");
}

// ---- distance -------------------------------------------------------------

#[test]
fn a_sound_at_the_listener_is_at_full_volume() {
    let listener = facing_x();
    let params = SoundParams::at(listener.position);
    let [l, r] = gains_for(&params, &listener);
    assert!((l * l + r * r - 1.0).abs() < 0.01, "constant power: {l} {r}");
}

#[test]
fn getting_further_away_gets_quieter() {
    let listener = facing_x();
    let mut last = f32::MAX;
    for distance in [0.0f32, 64.0, 128.0, 512.0, 1024.0, 2048.0] {
        let params = SoundParams::at(Vec3::new(distance, 0.0, 0.0));
        let [l, r] = gains_for(&params, &listener);
        let total = l + r;
        assert!(total <= last + 1e-4, "louder at {distance} than at the step before");
        last = total;
    }
}

#[test]
fn inside_the_reference_distance_nothing_changes() {
    // Without this a footstep two paces away is noticeably quieter than one
    // underfoot, which is not how hearing works.
    let listener = facing_x();
    let near = gains_for(&SoundParams::at(Vec3::new(10.0, 0.0, 0.0)), &listener);
    let edge = gains_for(&SoundParams::at(Vec3::new(120.0, 0.0, 0.0)), &listener);
    assert!(
        ((near[0] + near[1]) - (edge[0] + edge[1])).abs() < 0.05,
        "{near:?} vs {edge:?}"
    );
}

#[test]
fn past_the_maximum_distance_a_sound_is_not_heard() {
    let listener = facing_x();
    let params = SoundParams {
        position: Some(Vec3::new(5000.0, 0.0, 0.0)),
        max_distance: 4096.0,
        ..Default::default()
    };
    assert_eq!(gains_for(&params, &listener), [0.0, 0.0]);
}

#[test]
fn a_sound_fades_out_rather_than_switching_off_at_its_limit() {
    // A sound that vanishes the instant you step past its range is audible as
    // a click, which is worse than not hearing it at all.
    let listener = facing_x();
    let params = |d: f32| SoundParams {
        position: Some(Vec3::new(d, 0.0, 0.0)),
        max_distance: 1000.0,
        ..Default::default()
    };
    let near_limit = gains_for(&params(990.0), &listener);
    assert!(near_limit[0] + near_limit[1] < 0.02, "not faded: {near_limit:?}");
    assert!(near_limit[0] + near_limit[1] > 0.0, "already gone: {near_limit:?}");
}

#[test]
fn zero_attenuation_carries_forever() {
    // What music and a level-wide ambience want.
    let listener = facing_x();
    let params = SoundParams {
        position: Some(Vec3::new(2000.0, 0.0, 0.0)),
        attenuation: 0.0,
        max_distance: 100_000.0,
        ..Default::default()
    };
    let [l, r] = gains_for(&params, &listener);
    assert!(l + r > 0.9, "{l} {r}");
}

#[test]
fn a_sound_with_no_position_is_heard_flat() {
    let listener = Listener {
        position: Vec3::new(1000.0, 2000.0, 3000.0),
        basis: Angles::new(30.0, 120.0, 0.0).vectors(),
    };
    assert_eq!(gains_for(&SoundParams::default(), &listener), [1.0, 1.0]);
}

// ---- panning --------------------------------------------------------------

#[test]
fn a_sound_on_the_right_is_louder_in_the_right_ear() {
    let listener = facing_x();
    // Facing +X, "right" is -Y in a Z-up, right-handed world.
    let right = listener.basis.right * 200.0;
    let [l, r] = gains_for(&SoundParams::at(right), &listener);
    assert!(r > l * 2.0, "left {l}, right {r}");
}

#[test]
fn a_sound_on_the_left_is_louder_in_the_left_ear() {
    let listener = facing_x();
    let left = listener.basis.right * -200.0;
    let [l, r] = gains_for(&SoundParams::at(left), &listener);
    assert!(l > r * 2.0, "left {l}, right {r}");
}

#[test]
fn a_sound_straight_ahead_is_centred() {
    let listener = facing_x();
    let ahead = listener.basis.forward * 200.0;
    let [l, r] = gains_for(&SoundParams::at(ahead), &listener);
    assert!((l - r).abs() < 1e-3, "left {l}, right {r}");
}

#[test]
fn turning_around_swaps_the_ears() {
    let mut listener = facing_x();
    let sound = listener.basis.right * 200.0;
    let before = gains_for(&SoundParams::at(sound), &listener);

    listener.basis = Angles::new(0.0, 180.0, 0.0).vectors();
    let after = gains_for(&SoundParams::at(sound), &listener);
    assert!(before[1] > before[0], "started on the right");
    assert!(after[0] > after[1], "did not move to the left: {after:?}");
}

#[test]
fn panning_keeps_the_loudness_constant_across_the_front() {
    // A sound crossing in front must not dip in the middle, which is what a
    // linear pan does.
    let listener = facing_x();
    let mut powers = Vec::new();
    for angle in [-90.0f32, -45.0, 0.0, 45.0, 90.0] {
        let radians = angle.to_radians();
        let at = listener.basis.forward * radians.cos() * 100.0
            + listener.basis.right * radians.sin() * 100.0;
        let [l, r] = gains_for(&SoundParams::at(at), &listener);
        powers.push(l * l + r * r);
    }
    let min = powers.iter().cloned().fold(f32::MAX, f32::min);
    let max = powers.iter().cloned().fold(0.0f32, f32::max);
    assert!(max - min < 0.05, "power varies across the front: {powers:?}");
}

// ---- pitch and resampling -------------------------------------------------

#[test]
fn a_higher_pitch_finishes_sooner() {
    let frames = 4800; // 100 ms at 48 kHz
    let mut slow = mixer();
    slow.play(steady(frames, 1), SoundParams::default());
    let mut fast = mixer();
    fast.play(steady(frames, 1), SoundParams::default().with_pitch(4.0));

    let block = vec![0.0; 2048 * 2];
    let mut buffer = block.clone();
    fast.mix(&mut buffer);
    fast.mix(&mut buffer);
    fast.mix(&mut buffer);
    assert_eq!(fast.voice_count(), 0, "the fast one should have finished");

    let mut buffer = block;
    slow.mix(&mut buffer);
    assert_eq!(slow.voice_count(), 1, "the slow one should still be going");
}

#[test]
fn a_sound_recorded_at_another_rate_is_resampled_rather_than_played_wrong() {
    // A 22 kHz file on a 48 kHz device must take twice as long, not half.
    let mut mixer = Mixer::new(48_000);
    let sound = Arc::new(Sound { channels: 1, sample_rate: 24_000, samples: vec![1.0; 2400] });
    mixer.play(sound, SoundParams::default());

    // 2400 frames at 24 kHz is 100 ms, which is 4800 frames at 48 kHz.
    let mut out = vec![0.0; 4000 * 2];
    mixer.mix(&mut out);
    assert_eq!(mixer.voice_count(), 1, "it finished too early");
    let mut out = vec![0.0; 1000 * 2];
    mixer.mix(&mut out);
    assert_eq!(mixer.voice_count(), 0, "it did not finish");
}

#[test]
fn a_pitch_of_zero_does_not_hang_the_mixer() {
    let mut mixer = mixer();
    mixer.play(steady(100, 1), SoundParams::default().with_pitch(0.0));
    let mut out = vec![0.0; 128 * 2];
    mixer.mix(&mut out); // must return
    assert!(mixer.voice_count() <= 1);
}

// ---- limits ---------------------------------------------------------------

#[test]
fn there_is_a_ceiling_on_how_many_sounds_play_at_once() {
    // A trigger firing every tick would otherwise stack thousands of copies,
    // which is both deafening and slow.
    let mut mixer = mixer();
    for _ in 0..MAX_VOICES * 3 {
        mixer.play(steady(100_000, 1), SoundParams::default().looping());
    }
    assert_eq!(mixer.voice_count(), MAX_VOICES);
}

#[test]
fn the_voice_that_gives_way_is_the_quietest_one() {
    let mut mixer = mixer();
    mixer.listener = facing_x();
    // Fill up with distant sounds, then add a near one.
    for _ in 0..MAX_VOICES {
        let far = SoundParams { position: Some(Vec3::new(3000.0, 0.0, 0.0)), ..Default::default() };
        mixer.play(steady(100_000, 1), far.looping());
    }
    let near = mixer.play(steady(100_000, 1), SoundParams::at(Vec3::new(10.0, 0.0, 0.0)).looping());
    assert!(mixer.is_playing(near), "the loudest sound was the one dropped");
    assert_eq!(mixer.voice_count(), MAX_VOICES);
}

#[test]
fn a_moving_sound_can_be_moved() {
    let mut mixer = mixer();
    mixer.listener = facing_x();
    let handle = mixer.play(steady(100_000, 1), SoundParams::at(Vec3::new(0.0, 0.0, 0.0)).looping());
    mixer.set_position(handle, Vec3::new(0.0, 0.0, 5000.0));
    let (l, r) = peaks(&mut mixer, 2048);
    assert!(l + r < 0.3, "it did not move away: {l} {r}");
}

#[test]
fn an_empty_sound_does_not_divide_by_anything() {
    let mut mixer = mixer();
    let empty = Arc::new(Sound { channels: 1, sample_rate: RATE, samples: Vec::new() });
    mixer.play(empty, SoundParams::default());
    let mut out = vec![0.0; 64 * 2];
    mixer.mix(&mut out);
    assert!(out.iter().all(|s| *s == 0.0));
}

#[test]
fn mixing_into_an_empty_buffer_is_not_an_error() {
    let mut mixer = mixer();
    mixer.play(steady(100, 1), SoundParams::default());
    mixer.mix(&mut []);
}
