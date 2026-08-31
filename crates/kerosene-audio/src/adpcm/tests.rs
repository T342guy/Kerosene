// SPDX-License-Identifier: MPL-2.0
use super::*;

/// A sine at `hz`, the shape most like real audio that is easy to reason about.
fn sine(frames: usize, hz: f32, rate: f32, amplitude: f32) -> Vec<i16> {
    (0..frames)
        .map(|i| {
            let t = i as f32 / rate;
            (amplitude * 32767.0 * (std::f32::consts::TAU * hz * t).sin()) as i16
        })
        .collect()
}

/// Root-mean-square error between two signals, in 16-bit units.
fn rms_error(a: &[i16], b: &[i16]) -> f32 {
    assert_eq!(a.len(), b.len());
    let sum: f64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| {
            let d = x as f64 - y as f64;
            d * d
        })
        .sum();
    (sum / a.len() as f64).sqrt() as f32
}

#[test]
fn compression_is_four_to_one() {
    // The entire reason this exists. Sixteen bits in, four bits out.
    let samples = sine(1000, 440.0, 44100.0, 0.8);
    let encoded = encode(&samples, 1);
    assert_eq!(encoded.len(), 500);
    assert_eq!(encoded.len(), encoded_len(samples.len()));
}

#[test]
fn a_round_trip_stays_close_to_the_original() {
    let samples = sine(4410, 440.0, 44100.0, 0.8);
    let decoded = decode(&encode(&samples, 1), 1, samples.len());

    assert_eq!(decoded.len(), samples.len());
    // Under 1% of full scale. ADPCM is lossy and this is the size of the loss
    // -- a number worth pinning, because a bug in the predictor shows up as
    // this growing rather than as anything failing.
    let error = rms_error(&samples, &decoded);
    assert!(error < 327.0, "rms error {error} is more than 1% of full scale");
}

#[test]
fn silence_stays_silent() {
    // The failure this catches is audible and unmistakable: a predictor that
    // does not settle turns a silent passage into a buzz.
    let samples = vec![0i16; 2000];
    let decoded = decode(&encode(&samples, 1), 1, samples.len());
    let loudest = decoded.iter().map(|s| s.abs()).max().unwrap();
    assert!(loudest < 40, "silence decoded with a peak of {loudest}");
}

#[test]
fn the_encoder_and_decoder_walk_the_same_predictor() {
    // They must, or they drift further apart with every sample and the end of
    // a long sound is noise. Tested by comparing the two halves of a long
    // signal: if they were drifting, the second half would be much worse.
    let samples = sine(44100, 220.0, 44100.0, 0.7);
    let decoded = decode(&encode(&samples, 1), 1, samples.len());

    let half = samples.len() / 2;
    let early = rms_error(&samples[..half], &decoded[..half]);
    let late = rms_error(&samples[half..], &decoded[half..]);
    assert!(late < early * 1.5, "error grew from {early} to {late} over one second");
}

#[test]
fn stereo_channels_do_not_bleed_into_each_other() {
    // Each channel gets its own predictor. Sharing one would make a loud left
    // channel drag the right one around with it.
    let frames = 2000;
    let mut interleaved = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = i as f32 / 44100.0;
        interleaved.push((0.9 * 32767.0 * (std::f32::consts::TAU * 200.0 * t).sin()) as i16);
        interleaved.push(0); // right channel is silent throughout
    }

    let decoded = decode(&encode(&interleaved, 2), 2, interleaved.len());
    let right_peak = decoded.iter().skip(1).step_by(2).map(|s| s.abs()).max().unwrap();
    assert!(right_peak < 60, "a silent right channel decoded with a peak of {right_peak}");
}

#[test]
fn an_odd_sample_count_survives_the_half_byte_at_the_end() {
    // Two codes to a byte, so an odd count leaves the high nibble as padding.
    // The frame count says to ignore it; getting that wrong appends a click.
    let samples = sine(101, 300.0, 8000.0, 0.5);
    let encoded = encode(&samples, 1);
    assert_eq!(encoded.len(), 51);

    let decoded = decode(&encoded, 1, samples.len());
    assert_eq!(decoded.len(), 101, "the padding nibble must not become a sample");
}

#[test]
fn a_truncated_stream_stops_rather_than_reading_past_the_end() {
    // Content is edited by people and written by tools; a truncated file is a
    // normal Tuesday, and it should decode short rather than panic.
    let samples = sine(1000, 440.0, 44100.0, 0.5);
    let encoded = encode(&samples, 1);
    let decoded = decode(&encoded[..100], 1, samples.len());
    assert!(decoded.len() < samples.len());
    assert_eq!(decoded.len(), 200, "two samples a byte, and then it stops");
}

#[test]
fn a_full_scale_signal_does_not_wrap_around() {
    // The predictor is clamped, not wrapped. Without that, a loud passage
    // turns inside out and the result is a very loud noise rather than a
    // slightly wrong sound.
    let samples: Vec<i16> = (0..1000)
        .map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN })
        .collect();
    let decoded = decode(&encode(&samples, 1), 1, samples.len());
    // Every decoded sample must stay on the side of zero its source was on
    // once the predictor has caught up.
    for (i, (&want, &got)) in samples.iter().zip(&decoded).enumerate().skip(50) {
        assert_eq!(
            want.signum(), got.signum(),
            "sample {i} flipped sign: wanted {want}, got {got}"
        );
    }
}
