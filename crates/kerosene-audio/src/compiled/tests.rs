// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
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
fn a_pcm_round_trip_is_faithful() {
    let sound = tone(1000, 1, 0.75);
    let bytes = encode(&sound, Encoding::Pcm16, Loop::default());
    let (back, info) = decode(&bytes).unwrap();

    assert_eq!(info.channels, 1);
    assert_eq!(info.sample_rate, 44100);
    assert_eq!(info.frames, 1000);
    assert_eq!(info.encoding, Encoding::Pcm16);
    assert_eq!(back.samples.len(), sound.samples.len());
    for (a, b) in sound.samples.iter().zip(&back.samples) {
        assert!((a - b).abs() < 1e-3, "{a} became {b}");
    }
}

#[test]
fn adpcm_is_a_quarter_the_size_of_pcm() {
    // The reason the encoding is a choice at all.
    let sound = tone(8000, 1, 0.75);
    let pcm = encode(&sound, Encoding::Pcm16, Loop::default());
    let small = encode(&sound, Encoding::Adpcm, Loop::default());

    let pcm_body = pcm.len() - 64;
    let adpcm_body = small.len() - 64;
    assert_eq!(pcm_body, 16_000);
    assert_eq!(adpcm_body, 4_000);
}

#[test]
fn an_adpcm_round_trip_is_close_enough_to_hear_as_the_same_sound() {
    let sound = tone(4000, 1, 0.75);
    let (back, info) = decode(&encode(&sound, Encoding::Adpcm, Loop::default())).unwrap();

    assert_eq!(info.encoding, Encoding::Adpcm);
    assert_eq!(back.samples.len(), sound.samples.len());

    // Past the attack. The quantiser starts at its smallest step and has to
    // climb to reach a loud signal, so the first few hundred samples of any
    // ADPCM stream are the worst it ever gets -- see the codec's own docs.
    let worst = sound.samples[ATTACK..]
        .iter()
        .zip(&back.samples[ATTACK..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 0.05, "worst sample error {worst} once the quantiser has caught up");
}

/// How many samples the quantiser needs to reach a loud signal from rest.
const ATTACK: usize = 256;

#[test]
fn the_attack_transient_settles_quickly_rather_than_lasting() {
    // It is a real artifact and the reason PCM16 exists as a choice, but it
    // has to be over in milliseconds. A predictor that never caught up would
    // look the same at the start and be a different bug entirely.
    let sound = tone(4000, 1, 0.75);
    let (back, _) = decode(&encode(&sound, Encoding::Adpcm, Loop::default())).unwrap();

    let worst_in = |range: std::ops::Range<usize>| {
        sound.samples[range.clone()]
            .iter()
            .zip(&back.samples[range])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max)
    };

    assert!(worst_in(0..ATTACK) > worst_in(ATTACK..1000), "the start should be the worst of it");
    assert!(
        worst_in(ATTACK..1000) < 0.05,
        "settled to {} after {ATTACK} samples, which is under 6ms at 44.1 kHz",
        worst_in(ATTACK..1000)
    );
}

#[test]
fn stereo_survives_both_encodings() {
    for encoding in [Encoding::Pcm16, Encoding::Adpcm] {
        let sound = tone(500, 2, 0.6);
        let (back, info) = decode(&encode(&sound, encoding, Loop::default())).unwrap();
        assert_eq!(info.channels, 2, "{encoding:?}");
        assert_eq!(info.frames, 500, "{encoding:?}");
        assert_eq!(back.frames(), 500, "{encoding:?}");
    }
}

#[test]
fn the_peak_is_computed_at_build_time() {
    // So nothing has to scan a buffer at runtime to know whether a gain will
    // clip it.
    let mut sound = tone(100, 1, 0.5);
    sound.samples[10] = -0.9;
    let info = read_info(&encode(&sound, Encoding::Pcm16, Loop::default())).unwrap();
    assert!((info.peak - 0.9).abs() < 1e-3, "peak {}", info.peak);
}

#[test]
fn loop_points_survive_the_round_trip() {
    let sound = tone(1000, 1, 0.5);
    let region = Loop { start: 100, end: 900 };
    let info = read_info(&encode(&sound, Encoding::Pcm16, region)).unwrap();
    assert_eq!(info.looping, region);
    assert!(!info.looping.is_empty());
}

#[test]
fn a_sound_with_no_loop_says_so_rather_than_looping_the_whole_file() {
    let sound = tone(1000, 1, 0.5);
    let info = read_info(&encode(&sound, Encoding::Pcm16, Loop::default())).unwrap();
    assert!(info.looping.is_empty());
}

#[test]
fn only_a_mono_sound_may_be_placed_in_the_world() {
    // A stereo source has one pan and two channels already carrying their own
    // image. Applying a position to it is not a position and does not sound
    // like one, so the format records what it is and lets the engine say so.
    let mono = read_info(&encode(&tone(10, 1, 0.5), Encoding::Pcm16, Loop::default())).unwrap();
    let stereo = read_info(&encode(&tone(10, 2, 0.5), Encoding::Pcm16, Loop::default())).unwrap();
    assert!(mono.can_be_positioned());
    assert!(!stereo.can_be_positioned());
}

// ---- files that are not what they claim -----------------------------------

#[test]
fn a_file_that_is_not_one_is_refused_by_its_magic() {
    let err = read_info(&[0u8; 64]).unwrap_err().to_string();
    assert!(err.contains("bad magic"), "{err}");
}

#[test]
fn a_truncated_header_is_reported_as_truncated() {
    let bytes = encode(&tone(10, 1, 0.5), Encoding::Pcm16, Loop::default());
    let err = read_info(&bytes[..32]).unwrap_err().to_string();
    assert!(err.contains("truncated"), "{err}");
}

#[test]
fn a_truncated_body_is_caught_rather_than_decoded_short() {
    // Silently returning half a sound would produce a click and no diagnosis.
    let bytes = encode(&tone(1000, 1, 0.5), Encoding::Pcm16, Loop::default());
    let err = decode(&bytes[..200]).unwrap_err().to_string();
    assert!(err.contains("truncated"), "{err}");
}

#[test]
fn a_loop_past_the_end_is_refused_at_load() {
    // It would send the mixer's cursor somewhere there are no samples.
    let mut bytes = encode(&tone(100, 1, 0.5), Encoding::Pcm16, Loop::default());
    bytes[20..24].copy_from_slice(&10u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&5000u32.to_le_bytes());
    let err = read_info(&bytes).unwrap_err().to_string();
    assert!(err.contains("does not fit"), "{err}");
}

#[test]
fn a_newer_version_says_so_instead_of_reading_it_wrong() {
    let mut bytes = encode(&tone(10, 1, 0.5), Encoding::Pcm16, Loop::default());
    bytes[4..8].copy_from_slice(&99u32.to_le_bytes());
    let err = read_info(&bytes).unwrap_err().to_string();
    assert!(err.contains("version 99"), "{err}");
}

#[test]
fn encoding_names_round_trip_and_forgive_spelling() {
    for encoding in [Encoding::Pcm16, Encoding::Adpcm] {
        assert_eq!(Encoding::parse(encoding.name()), Some(encoding));
    }
    assert_eq!(Encoding::parse("  ADPCM "), Some(Encoding::Adpcm));
    assert_eq!(Encoding::parse("vorbis"), None);
}
