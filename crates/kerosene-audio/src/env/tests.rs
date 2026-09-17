// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

#[test]
fn identity_is_identity() {
    assert!(VoiceEnv::IDENTITY.is_identity());
    assert!(VoiceEnv::default().is_identity());
    assert!(
        !VoiceEnv {
            cutoff_hz: 5000.0,
            ..VoiceEnv::IDENTITY
        }
        .is_identity()
    );
    assert!(
        !VoiceEnv {
            send: 0.1,
            ..VoiceEnv::IDENTITY
        }
        .is_identity()
    );
}

#[test]
fn air_absorption_is_monotonic() {
    let mut last = air_cutoff(0.0);
    assert_eq!(last, OPEN_CUTOFF);
    for d in (0..=20_000).step_by(64) {
        let c = air_cutoff(d as f32);
        assert!(c <= last, "cutoff rose from {last} to {c} at {d}");
        assert!(c >= 100.0);
        last = c;
    }
    // A decade per AIR_DECADE.
    assert!((air_cutoff(AIR_DECADE) - 2000.0).abs() < 1.0);
}

#[test]
fn occlusion_runs_from_open_to_a_closed_door() {
    assert_eq!(occlusion_cutoff(0.0), OPEN_CUTOFF);
    assert!((occlusion_cutoff(1.0) - OCCLUDED_CUTOFF).abs() < 0.5);
    assert!(occlusion_cutoff(0.5) < OPEN_CUTOFF && occlusion_cutoff(0.5) > OCCLUDED_CUTOFF);
    assert_eq!(occlusion_gain(0.0), 1.0);
    assert!((occlusion_gain(1.0) - 0.251).abs() < 0.002);
    // Out of range is clamped, not extrapolated.
    assert_eq!(occlusion_cutoff(7.0), occlusion_cutoff(1.0));
}

#[test]
fn the_send_grows_with_distance_and_never_vanishes_up_close() {
    assert_eq!(send_for(1.0, 0.0, 128.0), 0.25);
    assert_eq!(send_for(1.0, 64.0, 128.0), 0.5);
    assert_eq!(send_for(1.0, 128.0, 128.0), 1.0);
    assert_eq!(send_for(1.0, 10_000.0, 128.0), 1.0);
    assert_eq!(send_for(0.5, 10_000.0, 128.0), 0.5);
    assert_eq!(send_for(0.0, 10_000.0, 128.0), 0.0);
}
