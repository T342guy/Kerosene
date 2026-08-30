// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;
use void_audio::wav::Sound;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "timbre-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tone(frames: usize, channels: u16, amplitude: f32) -> Sound {
    let samples = (0..frames * channels as usize)
        .map(|i| {
            let frame = i / channels as usize;
            amplitude * (std::f32::consts::TAU * 440.0 * frame as f32 / 44100.0).sin()
        })
        .collect();
    Sound { channels, sample_rate: 44100, samples }
}

/// A minimal 16-bit PCM WAV, optionally with a `smpl` chunk declaring a loop.
fn wav_bytes(sound: &Sound, loop_region: Option<(u32, u32)>) -> Vec<u8> {
    let data: Vec<u8> = sound
        .samples
        .iter()
        .flat_map(|&s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();

    let mut smpl = Vec::new();
    if let Some((start, end)) = loop_region {
        smpl.resize(36, 0);
        smpl[28..32].copy_from_slice(&1u32.to_le_bytes()); // one loop
        smpl.resize(60, 0);
        smpl[44..48].copy_from_slice(&start.to_le_bytes());
        smpl[48..52].copy_from_slice(&end.to_le_bytes());
    }

    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes()); // PCM
    body.extend_from_slice(&sound.channels.to_le_bytes());
    body.extend_from_slice(&sound.sample_rate.to_le_bytes());
    let block = sound.channels * 2;
    body.extend_from_slice(&(sound.sample_rate * block as u32).to_le_bytes());
    body.extend_from_slice(&block.to_le_bytes());
    body.extend_from_slice(&16u16.to_le_bytes());
    if !smpl.is_empty() {
        body.extend_from_slice(b"smpl");
        body.extend_from_slice(&(smpl.len() as u32).to_le_bytes());
        body.extend_from_slice(&smpl);
    }
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(&data);

    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn write_wav(dir: &Path, name: &str, sound: &Sound) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, wav_bytes(sound, None)).unwrap();
    path
}

// ---- preparing samples ----------------------------------------------------

#[test]
fn gain_is_applied_before_encoding_rather_than_at_play_time() {
    // A sound recorded too hot should be fixed once, not in every entity that
    // plays it.
    let quiet = prepare(&tone(100, 1, 0.4), &Options { gain: 2.0, ..Options::default() });
    assert!((peak_of(&quiet) - 0.8).abs() < 0.01, "peaked at {}", peak_of(&quiet));
}

#[test]
fn gain_clamps_instead_of_wrapping_around() {
    // Wrapping turns a loud passage inside out, which is very loud noise
    // rather than a slightly wrong sound.
    let loud = prepare(&tone(100, 1, 0.9), &Options { gain: 4.0, ..Options::default() });
    assert!(loud.samples.iter().all(|s| (-1.0..=1.0).contains(s)));
    assert!((peak_of(&loud) - 1.0).abs() < 1e-6);
}

#[test]
fn folding_to_mono_averages_rather_than_sums() {
    // Summing two correlated channels is a 6 dB boost, which clips anything
    // that was already loud.
    let stereo = tone(100, 2, 0.8);
    let mono = prepare(&stereo, &Options { mono: true, ..Options::default() });
    assert_eq!(mono.channels, 1);
    assert_eq!(mono.frames(), 100);
    assert!(peak_of(&mono) <= 0.81, "averaging should not raise the peak: {}", peak_of(&mono));
}

#[test]
fn a_mono_sound_is_left_alone_by_the_mono_option() {
    let sound = tone(50, 1, 0.5);
    let out = prepare(&sound, &Options { mono: true, ..Options::default() });
    assert_eq!(out.samples, sound.samples);
}

// ---- loop points ----------------------------------------------------------

#[test]
fn a_loop_region_is_read_out_of_the_wav_that_declares_one() {
    // Audio editors write this and nothing read it, so a room tone with a
    // proper loop had it thrown away and was repeated end to end, click and
    // all.
    let sound = tone(1000, 1, 0.5);
    let bytes = wav_bytes(&sound, Some((100, 899)));
    let region = loop_from_wav(&bytes, 1000).expect("the smpl chunk should be found");
    assert_eq!(region.start, 100);
    // `smpl` names the last frame played; ours is one past it.
    assert_eq!(region.end, 900);
}

#[test]
fn a_wav_with_no_loop_chunk_reports_none() {
    let bytes = wav_bytes(&tone(100, 1, 0.5), None);
    assert_eq!(loop_from_wav(&bytes, 100), None);
}

#[test]
fn a_loop_that_runs_past_the_end_is_clamped_to_it() {
    let bytes = wav_bytes(&tone(100, 1, 0.5), Some((10, 9999)));
    let region = loop_from_wav(&bytes, 100).unwrap();
    assert_eq!(region.end, 100);
}

#[test]
fn a_file_that_is_not_a_wav_yields_no_loop_rather_than_reading_rubbish() {
    assert_eq!(loop_from_wav(b"not a riff file at all", 100), None);
}

// ---- compiling ------------------------------------------------------------

#[test]
fn compiling_writes_a_readable_voidaud_beside_the_source() {
    let dir = scratch("compile");
    let source = write_wav(&dir, "door/move.wav", &tone(4410, 1, 0.7));
    let output = output_for(&source);
    assert_eq!(output.extension().unwrap(), "voidaud");

    let done = compile(&source, &output, &Options::default()).unwrap();
    assert!(output.is_file());
    assert_eq!(done.info.frames, 4410);
    assert_eq!(done.info.channels, 1);

    let (back, _) = void_audio::compiled::decode(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(back.frames(), 4410);
}

#[test]
fn adpcm_makes_the_file_much_smaller_than_its_source() {
    let dir = scratch("smaller");
    let source = write_wav(&dir, "long.wav", &tone(44100, 1, 0.7));
    let done = compile(&source, &output_for(&source), &Options::default()).unwrap();

    assert!(done.saved() > 0.7, "only saved {:.0}%", done.saved() * 100.0);
}

#[test]
fn a_gain_that_clips_says_so_rather_than_doing_it_quietly() {
    let dir = scratch("clip-warn");
    let source = write_wav(&dir, "hot.wav", &tone(100, 1, 0.9));
    let options = Options { gain: 4.0, ..Options::default() };
    let done = compile(&source, &output_for(&source), &options).unwrap();

    assert!(
        done.warnings.iter().any(|w| w.contains("clips")),
        "expected a clipping warning, got {:?}",
        done.warnings
    );
}

#[test]
fn a_stereo_sound_is_told_it_cannot_be_placed_in_the_world() {
    // The bug this prevents is silent: a stereo sound given a position gets
    // one pan applied to two channels that already carry their own image, and
    // it just sounds slightly wrong forever.
    let dir = scratch("stereo-warn");
    let source = write_wav(&dir, "wide.wav", &tone(100, 2, 0.5));
    let done = compile(&source, &output_for(&source), &Options::default()).unwrap();

    assert!(!done.info.can_be_positioned());
    assert!(
        done.warnings.iter().any(|w| w.contains("cannot be placed")),
        "{:?}",
        done.warnings
    );
}

#[test]
fn folding_a_stereo_sound_to_mono_clears_the_warning() {
    let dir = scratch("stereo-fixed");
    let source = write_wav(&dir, "wide.wav", &tone(100, 2, 0.5));
    let options = Options { mono: true, ..Options::default() };
    let done = compile(&source, &output_for(&source), &options).unwrap();

    assert!(done.info.can_be_positioned());
    assert!(done.warnings.is_empty(), "{:?}", done.warnings);
}

#[test]
fn a_loop_declared_in_the_source_reaches_the_compiled_file() {
    let dir = scratch("loop-through");
    let path = dir.join("tone.wav");
    std::fs::write(&path, wav_bytes(&tone(1000, 1, 0.5), Some((100, 899)))).unwrap();

    let done = compile(&path, &output_for(&path), &Options::default()).unwrap();
    assert_eq!(done.info.looping.start, 100);
    assert_eq!(done.info.looping.end, 900);
}

#[test]
fn a_loop_set_by_hand_beats_the_one_in_the_file() {
    let dir = scratch("loop-override");
    let path = dir.join("tone.wav");
    std::fs::write(&path, wav_bytes(&tone(1000, 1, 0.5), Some((100, 899)))).unwrap();

    let options = Options {
        looping: Some(void_audio::compiled::Loop { start: 400, end: 600 }),
        ..Options::default()
    };
    let done = compile(&path, &output_for(&path), &options).unwrap();
    assert_eq!(done.info.looping.start, 400);
    assert_eq!(done.info.looping.end, 600);
}

#[test]
fn a_source_that_is_not_audio_fails_with_its_name_in_the_message() {
    let dir = scratch("bad-source");
    let path = dir.join("broken.wav");
    std::fs::write(&path, b"this is not a wav").unwrap();

    let err = compile(&path, &output_for(&path), &Options::default()).unwrap_err();
    let text = format!("{err:#}");
    assert!(text.contains("broken.wav"), "{text}");
}

// ---- building a tree ------------------------------------------------------

#[test]
fn a_build_compiles_every_sound_under_the_tree() {
    let dir = scratch("build");
    let sound = dir.join("sound");
    write_wav(&sound, "ui/click.wav", &tone(500, 1, 0.5));
    write_wav(&sound, "door/move.wav", &tone(900, 1, 0.5));
    write_wav(&sound, "ambient/room.wav", &tone(2000, 1, 0.3));

    let batch = build_sounds(&dir, false).unwrap();
    assert_eq!(batch.compiled.len(), 3);
    assert!(batch.failed.is_empty());
    assert!(sound.join("ui/click.voidaud").is_file());
    assert!(sound.join("door/move.voidaud").is_file());
}

#[test]
fn a_second_build_skips_what_has_not_changed() {
    let dir = scratch("incremental");
    write_wav(&dir.join("sound"), "click.wav", &tone(500, 1, 0.5));

    assert_eq!(build_sounds(&dir, false).unwrap().compiled.len(), 1);
    let again = build_sounds(&dir, false).unwrap();
    assert_eq!(again.compiled.len(), 0);
    assert_eq!(again.skipped, 1);
}

#[test]
fn forcing_a_build_recompiles_regardless() {
    let dir = scratch("forced");
    write_wav(&dir.join("sound"), "click.wav", &tone(500, 1, 0.5));

    build_sounds(&dir, false).unwrap();
    assert_eq!(build_sounds(&dir, true).unwrap().compiled.len(), 1);
}

#[test]
fn changing_the_settings_rebuilds_even_though_the_source_is_untouched() {
    // The failure this catches wastes an afternoon: a gain changed in the
    // window, a build that says "up to date", and a file that still sounds
    // exactly as wrong as it did before.
    let dir = scratch("settings-rebuild");
    let sound_root = dir.join("sound");
    write_wav(&sound_root, "click.wav", &tone(500, 1, 0.5));
    build_sounds(&dir, false).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut script = build::Script::load_beside(&sound_root).unwrap();
    script.set("click.wav", Options { gain: 0.5, ..Options::default() });
    script.save().unwrap();

    let batch = build_sounds(&dir, false).unwrap();
    assert_eq!(batch.compiled.len(), 1, "a settings change has to rebuild");
}

#[test]
fn one_broken_file_does_not_stop_the_others() {
    let dir = scratch("partial");
    let sound = dir.join("sound");
    write_wav(&sound, "good.wav", &tone(500, 1, 0.5));
    std::fs::write(sound.join("bad.wav"), b"not audio").unwrap();

    let batch = build_sounds(&dir, false).unwrap();
    assert_eq!(batch.compiled.len(), 1);
    assert_eq!(batch.failed.len(), 1);
    assert!(batch.failed[0].0.ends_with("bad.wav"));
}

#[test]
fn a_tree_with_no_sounds_is_not_an_error() {
    let dir = scratch("empty");
    std::fs::create_dir_all(dir.join("sound")).unwrap();
    let batch = build_sounds(&dir, false).unwrap();
    assert!(batch.compiled.is_empty());
    assert!(batch.failed.is_empty());
}

#[test]
fn a_project_with_no_sound_directory_at_all_is_not_an_error() {
    let dir = scratch("no-sound-dir");
    assert!(build_sounds(&dir, false).unwrap().compiled.is_empty());
}

// ---- sources that would tread on each other -------------------------------

#[test]
fn two_sources_with_the_same_name_are_refused_rather_than_racing() {
    // `click.wav` and `click.flac` compile to the same `click.voidaud`, so one
    // silently wins and which one depends on the sort order. Only the person
    // who put both there knows which they meant.
    let dir = scratch("collision");
    let sound = dir.join("sound");
    write_wav(&sound, "click.wav", &tone(100, 1, 0.5));
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone.flac"),
        sound.join("click.flac"),
    )
    .unwrap();

    let batch = build_sounds(&dir, false).unwrap();
    assert_eq!(batch.failed.len(), 1, "the clash should be reported");
    assert!(batch.failed[0].1.contains("cannot share a name"), "{:?}", batch.failed[0]);
}

#[test]
fn a_flac_source_compiles_like_any_other() {
    let dir = scratch("flac-build");
    let sound = dir.join("sound");
    std::fs::create_dir_all(&sound).unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone.flac"),
        sound.join("chime.flac"),
    )
    .unwrap();

    let batch = build_sounds(&dir, false).unwrap();
    assert_eq!(batch.compiled.len(), 1);
    assert!(sound.join("chime.voidaud").is_file());
    assert!(batch.compiled[0].warnings.is_empty(), "{:?}", batch.compiled[0].warnings);
}

#[test]
fn an_mp3_source_compiles_and_says_it_was_already_lossy() {
    let dir = scratch("mp3-build");
    let sound = dir.join("sound");
    std::fs::create_dir_all(&sound).unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone.mp3"),
        sound.join("hum.mp3"),
    )
    .unwrap();

    let batch = build_sounds(&dir, false).unwrap();
    assert_eq!(batch.compiled.len(), 1);
    assert!(
        batch.compiled[0].warnings.iter().any(|w| w.contains("already lossy")),
        "{:?}",
        batch.compiled[0].warnings
    );
}

#[test]
fn a_compiled_file_larger_than_its_source_says_so() {
    // Real, and the opposite of the point: a 64 kbit MP3 is already smaller
    // than four bits a sample.
    let dir = scratch("grew");
    let sound = dir.join("sound");
    std::fs::create_dir_all(&sound).unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone.mp3"),
        sound.join("hum.mp3"),
    )
    .unwrap();

    let batch = build_sounds(&dir, false).unwrap();
    let done = &batch.compiled[0];
    assert!(done.grew(), "the fixture should compile larger than it started");
    assert!(done.size_change().contains("larger"), "{}", done.size_change());
    assert!(done.warnings.iter().any(|w| w.contains("larger than its source")));
}
