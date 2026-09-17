// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;
use crate::dsp::OnePole;

const RATE: u32 = 48_000;

fn params(rt60: [f32; 4]) -> ReverbParams {
    ReverbParams {
        rt60,
        predelay: 0.0,
        diffusion: 0.5,
        wet: 1.0,
        enabled: true,
    }
}

/// The network's response to a single click, `seconds` long, left ear.
fn impulse_response(p: ReverbParams, seconds: f32) -> Vec<f32> {
    let mut fdn = Fdn::new(RATE);
    fdn.set_params(p);
    let frames = (seconds * RATE as f32) as usize;
    let mut send = vec![0.0; frames];
    send[0] = 1.0;
    let mut out = vec![0.0; frames * 2];
    // In blocks, the way the mixer drives it.
    for start in (0..frames).step_by(512) {
        let end = (start + 512).min(frames);
        fdn.process(&send[start..end], &mut out[start * 2..end * 2]);
    }
    out.iter().step_by(2).copied().collect()
}

/// Reverberation time from a response, the way it is measured in a real
/// room: integrate the energy backwards, find where it has fallen 5 dB and
/// 25 dB, and scale that 20 dB span to 60.
fn measure_rt60(h: &[f32]) -> f32 {
    let mut energy = vec![0.0f64; h.len()];
    let mut acc = 0.0f64;
    for i in (0..h.len()).rev() {
        acc += (h[i] as f64) * (h[i] as f64);
        energy[i] = acc;
    }
    let total = energy[0];
    let db = |e: f64| 10.0 * (e / total).log10();
    let at = |level: f64| energy.iter().position(|&e| db(e) <= level).unwrap();
    let t5 = at(-5.0);
    let t25 = at(-25.0);
    (t25 - t5) as f32 / RATE as f32 * 3.0
}

/// Two poles of low-pass, for pulling one band out of a response.
fn lowpassed(h: &[f32], cutoff: f32) -> Vec<f32> {
    let mut a = OnePole::new(cutoff, RATE as f32);
    let mut b = OnePole::new(cutoff, RATE as f32);
    h.iter().map(|&x| b.process(a.process(x))).collect()
}

fn highpassed(h: &[f32], cutoff: f32) -> Vec<f32> {
    let mut a = OnePole::new(cutoff, RATE as f32);
    let mut b = OnePole::new(cutoff, RATE as f32);
    h.iter()
        .map(|&x| {
            let y = x - a.process(x);
            y - b.process(y)
        })
        .collect()
}

fn energy(h: &[f32]) -> f32 {
    h.iter().map(|x| x * x).sum()
}

fn noise(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
}

#[test]
fn fdn_impulse_decays_to_rt60() {
    let h = impulse_response(params([1.0; 4]), 3.0);
    let measured = measure_rt60(&h);
    assert!(
        (measured - 1.0).abs() < 0.15,
        "asked for 1.0 s, measured {measured}"
    );
}

#[test]
fn fdn_band_rt60_matches() {
    let h = impulse_response(params([2.0, 1.0, 1.0, 0.5]), 5.0);
    let low = measure_rt60(&lowpassed(&h, 120.0));
    let high = measure_rt60(&highpassed(&h, 10_000.0));
    assert!(
        (low - 2.0).abs() / 2.0 < 0.25,
        "low band measured {low}, wanted 2.0"
    );
    assert!(
        (high - 0.5).abs() / 0.5 < 0.25,
        "high band measured {high}, wanted 0.5"
    );
    assert!(
        low > high * 2.5,
        "the bands did not separate: {low} vs {high}"
    );
}

#[test]
fn carpet_decays_faster_than_concrete() {
    let concrete = impulse_response(params([2.5, 2.3, 1.9, 1.4]), 2.0);
    let carpet = impulse_response(params([1.2, 0.6, 0.25, 0.2]), 2.0);
    let late = (RATE as usize)..(RATE as usize * 3 / 2);
    let e_concrete = energy(&concrete[late.clone()]);
    let e_carpet = energy(&carpet[late]);
    assert!(
        e_carpet < e_concrete * 0.01,
        "carpet {e_carpet} should be far below concrete {e_concrete} a second in"
    );
}

#[test]
fn fdn_is_stable() {
    let mut fdn = Fdn::new(RATE);
    fdn.set_params(params([MAX_RT60; 4]));
    let mut seed = 7;
    let mut peak = 0.0f32;
    let mut send = vec![0.0; 1024];
    let mut out = vec![0.0; 2048];
    for _ in 0..(RATE as usize * 10 / 1024) {
        for s in &mut send {
            *s = noise(&mut seed);
        }
        out.fill(0.0);
        fdn.process(&send, &mut out);
        for &s in &out {
            assert!(s.is_finite(), "the network produced {s}");
            peak = peak.max(s.abs());
        }
    }
    assert!(peak < 50.0, "the network ran away to {peak}");
}

#[test]
fn fdn_delays_are_mutually_prime() {
    let lengths = Fdn::new(RATE).line_lengths();
    assert_eq!(lengths.len(), LINES);
    for (i, &a) in lengths.iter().enumerate() {
        assert_eq!(crate::dsp::next_prime(a), a, "{a} is not prime");
        for &b in &lengths[i + 1..] {
            assert_ne!(a, b);
        }
    }
    // And a different rate gives different, still prime, lengths.
    for l in Fdn::new(44_100).line_lengths() {
        assert_eq!(crate::dsp::next_prime(l), l);
    }
}

#[test]
fn predelay_shifts_onset() {
    let onset = |predelay: f32| {
        let h = impulse_response(
            ReverbParams {
                predelay,
                ..params([0.5; 4])
            },
            0.5,
        );
        h.iter().position(|s| s.abs() > 1e-6).unwrap()
    };
    let short = onset(0.0);
    let long = onset(0.05);
    let expected = (0.05 * RATE as f32) as usize;
    let shift = long - short;
    assert!(
        (shift as i64 - expected as i64).abs() <= 2,
        "50 ms of pre-delay moved the onset by {shift} samples, not {expected}"
    );
}

#[test]
fn room_change_has_no_discontinuity() {
    let mut fdn = Fdn::new(RATE);
    fdn.set_params(ReverbParams::preset("hall").unwrap());
    let mut send = vec![0.0; 256];
    let mut out = vec![0.0; 512];
    let mut phase = 0.0f32;
    let mut last = 0.0f32;
    let mut max_step = [0.0f32; 2];
    let blocks = RATE as usize / 256;
    for block in 0..blocks * 2 {
        if block == blocks {
            fdn.set_params(ReverbParams::preset("room").unwrap());
        }
        for s in &mut send {
            *s = phase.sin() * 0.5;
            phase += std::f32::consts::TAU * 200.0 / RATE as f32;
        }
        out.fill(0.0);
        fdn.process(&send, &mut out);
        let which = usize::from(block >= blocks);
        for s in out.iter().step_by(2) {
            max_step[which] = max_step[which].max((s - last).abs());
            last = *s;
        }
    }
    assert!(max_step[0] > 0.0, "the hall made no sound");
    assert!(
        max_step[1] < max_step[0] * 2.0,
        "changing room stepped by {} where the steady tone stepped at most {}",
        max_step[1],
        max_step[0]
    );
}

#[test]
fn a_disabled_network_leaves_the_buffer_alone() {
    let mut fdn = Fdn::new(RATE);
    let send = vec![1.0; 64];
    let mut out: Vec<f32> = (0..128).map(|i| i as f32 * 0.001).collect();
    let before = out.clone();
    fdn.process(&send, &mut out);
    assert_eq!(out, before);
    assert!(!fdn.is_active());
}

#[test]
fn switching_off_fades_out_and_then_costs_nothing() {
    let mut fdn = Fdn::new(RATE);
    fdn.set_params(ReverbParams::preset("hall").unwrap());
    let send = vec![1.0; 256];
    let mut out = vec![0.0; 512];
    // Long enough for the input to come round the longest line.
    for _ in 0..20 {
        fdn.process(&send, &mut out);
    }
    assert!(fdn.is_active());
    fdn.set_params(ReverbParams {
        enabled: false,
        ..ReverbParams::preset("hall").unwrap()
    });
    // The tail is still there for a while...
    out.fill(0.0);
    fdn.process(&send, &mut out);
    assert!(out.iter().any(|s| *s != 0.0), "the tail was cut off");
    // ...and gone within a couple of seconds.
    for _ in 0..(RATE as usize * 2 / 256) {
        out.fill(0.0);
        fdn.process(&send, &mut out);
    }
    assert!(!fdn.is_active());
    out.fill(0.0);
    fdn.process(&send, &mut out);
    assert!(out.iter().all(|s| *s == 0.0));
}

#[test]
fn presets_are_all_sane() {
    for name in ReverbParams::PRESETS {
        let p = ReverbParams::preset(name).expect(name);
        assert!(p.enabled);
        assert_eq!(p, p.clamped(), "{name} is out of range");
        assert!(p.wet > 0.0);
    }
    assert!(ReverbParams::preset("bathroom").is_none());
    assert_eq!(ReverbParams::preset(" Hall "), ReverbParams::preset("hall"));
}

#[test]
fn clamping_tames_nonsense() {
    let p = ReverbParams {
        rt60: [f32::NAN, -1.0, 1e9, 1.0],
        predelay: 4.0,
        diffusion: -3.0,
        wet: f32::INFINITY,
        enabled: true,
    }
    .clamped();
    assert_eq!(p.rt60, [MIN_RT60, MIN_RT60, MAX_RT60, 1.0]);
    assert_eq!(p.predelay, MAX_PREDELAY);
    assert_eq!(p.diffusion, 0.0);
    assert_eq!(p.wet, 0.0);
}
