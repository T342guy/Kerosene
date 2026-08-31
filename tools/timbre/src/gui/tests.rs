// SPDX-License-Identifier: MPL-2.0
use super::*;

fn tone(frames: usize, channels: u16, amplitude: f32) -> Sound {
    let samples = (0..frames * channels as usize)
        .map(|i| {
            let frame = i / channels as usize;
            amplitude * (std::f32::consts::TAU * 440.0 * frame as f32 / 44100.0).sin()
        })
        .collect();
    Sound { channels, sample_rate: 44100, samples }
}

#[test]
fn the_envelope_keeps_the_extremes_rather_than_the_average() {
    // Averaging a symmetrical waveform gives zero, and the picture that draws
    // is a flat line through the middle of a loud sound.
    let envelope = envelope_of(&tone(44100, 1, 0.8), 100);
    assert_eq!(envelope.len(), 100);
    for (low, high) in &envelope {
        assert!(*high > 0.7, "a loud sound should reach up: {high}");
        assert!(*low < -0.7, "and down: {low}");
    }
}

#[test]
fn a_short_sound_gets_fewer_columns_rather_than_repeated_ones() {
    let envelope = envelope_of(&tone(50, 1, 0.5), 900);
    assert_eq!(envelope.len(), 50, "one column a frame, and no padding");
}

#[test]
fn an_empty_sound_draws_nothing_instead_of_dividing_by_zero() {
    let empty = Sound { channels: 1, sample_rate: 44100, samples: Vec::new() };
    assert!(envelope_of(&empty, 900).is_empty());
}

#[test]
fn the_envelope_covers_both_channels_of_a_stereo_sound() {
    // Drawing only the left channel would hide clipping on the right.
    let mut sound = tone(1000, 2, 0.2);
    for frame in 0..1000 {
        sound.samples[frame * 2 + 1] = 0.95;
    }
    let envelope = envelope_of(&sound, 10);
    assert!(envelope.iter().all(|(_, high)| *high > 0.9), "the loud channel must show");
}

#[test]
fn decibels_read_the_way_a_sound_engineer_expects() {
    assert_eq!(decibels(1.0), "0.0 dB");
    assert!(decibels(0.5).starts_with("-6.0"), "{}", decibels(0.5));
    assert_eq!(decibels(0.0), "-inf dB");
}

#[test]
fn the_gain_slider_round_trips_through_decibels() {
    // The slider works in dB and the file stores a multiplier; a conversion
    // that did not round-trip would creep every time the window was opened.
    for gain in [0.25f32, 0.5, 1.0, 2.0, 3.98] {
        let back = 10f32.powf(decibels_value(gain) / 20.0);
        assert!((back - gain).abs() < 1e-4, "{gain} became {back}");
    }
}
