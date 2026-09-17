// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

#[test]
fn next_prime_finds_the_next_prime() {
    assert_eq!(next_prime(0), 2);
    assert_eq!(next_prime(2), 2);
    assert_eq!(next_prime(3), 3);
    assert_eq!(next_prime(4), 5);
    assert_eq!(next_prime(1000), 1009);
    assert_eq!(next_prime(1109), 1109);
}

#[test]
fn a_delay_line_gives_back_what_went_in_that_many_writes_ago() {
    let mut line = DelayLine::new(10);
    for i in 0..20 {
        line.write(i as f32);
    }
    // Last written was 19; one back is 19, three back is 17.
    assert_eq!(line.read(1), 19.0);
    assert_eq!(line.read(3), 17.0);
    assert_eq!(line.read_fractional(2.5), 17.5);
}

#[test]
fn a_one_pole_at_the_top_of_the_band_passes_a_step_almost_at_once() {
    let mut open = OnePole::open();
    assert_eq!(open.process(1.0), 1.0);
    let mut lp = OnePole::new(20.0, 48_000.0);
    let y = lp.process(1.0);
    assert!(y < 0.01, "a 20 Hz filter jumped to {y} in one sample");
}

#[test]
fn a_one_pole_settles_to_its_input() {
    let mut lp = OnePole::new(1000.0, 48_000.0);
    let mut y = 0.0;
    for _ in 0..48_000 {
        y = lp.process(0.5);
    }
    assert!((y - 0.5).abs() < 1e-5, "{y}");
}

#[test]
fn a_bad_cutoff_does_not_make_a_nan() {
    for c in [f32::NAN, f32::INFINITY, -1.0, 0.0, 1e9] {
        let a = coefficient(c, 48_000.0);
        assert!(a.is_finite() && (0.0..1.0).contains(&a), "{c} -> {a}");
    }
}

#[test]
fn an_allpass_passes_energy_through_unchanged() {
    let mut ap = Allpass::new(97, 0.6);
    let mut energy_in = 0.0;
    let mut energy_out = 0.0;
    let mut seed = 1u32;
    for i in 0..200_000 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = if i < 100_000 {
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        } else {
            0.0
        };
        energy_in += x * x;
        let y = ap.process(x);
        energy_out += y * y;
    }
    let ratio = energy_out / energy_in;
    assert!((ratio - 1.0).abs() < 0.02, "energy ratio {ratio}");
}
