// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "timbre-build-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_missing_file_is_an_empty_set_rather_than_an_error() {
    // A project that has never opened Timbre still has to build.
    let dir = scratch("missing");
    let script = Script::load_beside(&dir).unwrap();
    assert_eq!(script.options_for(Path::new("anything.wav"), &dir), Options::default());
}

#[test]
fn a_sound_with_no_block_takes_the_defaults() {
    let script = Script::parse(r#"defaults { "encoding" "pcm16" "gain" "0.5" }"#).unwrap();
    let options = script.options_for(Path::new("sound/click.wav"), Path::new("sound"));
    assert_eq!(options.encoding, Encoding::Pcm16);
    assert_eq!(options.gain, 0.5);
}

#[test]
fn a_block_overrides_only_what_it_names() {
    let script = Script::parse(
        r#"
        defaults { "encoding" "adpcm" "gain" "0.9" }
        sound { "file" "ambient/room.wav" "encoding" "pcm16" }
        "#,
    )
    .unwrap();
    let options = script.options_for(Path::new("sound/ambient/room.wav"), Path::new("sound"));
    assert_eq!(options.encoding, Encoding::Pcm16, "the override");
    assert_eq!(options.gain, 0.9, "and the default it did not mention");
}

#[test]
fn paths_match_however_they_are_spelt() {
    let script = Script::parse(r#"sound { "file" "UI/Click.wav" "gain" "2.0" }"#).unwrap();
    let options = script.options_for(Path::new("sound/ui/click.wav"), Path::new("sound"));
    assert_eq!(options.gain, 2.0);
    assert!(script.has_entry("ui/click.wav"));
    assert!(script.has_entry("UI\\Click.wav"), "a Windows path names the same sound");
}

#[test]
fn a_nonsense_gain_falls_back_instead_of_compiling_silence() {
    // Zero is silence and negative is a phase flip; neither is what anyone
    // typed on purpose, and both are hard to diagnose by ear.
    for bad in ["0", "-1.5", "banana", ""] {
        let script = Script::parse(&format!(r#"sound {{ "file" "a.wav" "gain" "{bad}" }}"#)).unwrap();
        let options = script.options_for(Path::new("a.wav"), Path::new("."));
        assert_eq!(options.gain, 1.0, "gain {bad:?} should have been ignored");
    }
}

#[test]
fn an_unknown_encoding_falls_back_rather_than_failing_the_build() {
    let script = Script::parse(r#"sound { "file" "a.wav" "encoding" "vorbis" }"#).unwrap();
    assert_eq!(script.options_for(Path::new("a.wav"), Path::new(".")).encoding, Encoding::Adpcm);
}

#[test]
fn an_explicit_zero_loop_means_do_not_loop_rather_than_say_nothing() {
    // Distinguishable from an absent one, which means "take whatever the WAV
    // itself declares".
    let script = Script::parse(r#"sound { "file" "a.wav" "loopstart" "0" "loopend" "0" }"#).unwrap();
    let options = script.options_for(Path::new("a.wav"), Path::new("."));
    assert_eq!(options.looping, Some(Loop::default()));

    let silent = Script::parse(r#"sound { "file" "a.wav" }"#).unwrap();
    assert_eq!(silent.options_for(Path::new("a.wav"), Path::new(".")).looping, None);
}

#[test]
fn a_saved_script_reads_back_as_what_was_saved() {
    // The property the whole file exists for: the window writes it and the
    // command line reads it, so the two produce the same build.
    let dir = scratch("round-trip");
    let mut script = Script::load_beside(&dir).unwrap();
    script.defaults = Options { encoding: Encoding::Pcm16, ..Options::default() };
    script.set(
        "door/move.wav",
        Options {
            encoding: Encoding::Adpcm,
            gain: 0.75,
            mono: true,
            looping: Some(Loop { start: 10, end: 900 }),
        },
    );
    script.save().unwrap();

    let read = Script::load_beside(&dir).unwrap();
    assert_eq!(read.defaults.encoding, Encoding::Pcm16);
    let options = read.options_for(&dir.join("door/move.wav"), &dir);
    assert_eq!(options.encoding, Encoding::Adpcm);
    assert!((options.gain - 0.75).abs() < 1e-3);
    assert!(options.mono);
    assert_eq!(options.looping, Some(Loop { start: 10, end: 900 }));
}

#[test]
fn a_written_script_only_states_what_differs_from_the_defaults() {
    // So the file stays readable, and so a default changed later reaches
    // everything that never overrode it.
    let dir = scratch("terse");
    let mut script = Script::load_beside(&dir).unwrap();
    script.set("plain.wav", Options::default());
    script.set("loud.wav", Options { gain: 2.0, ..Options::default() });
    let text = script.to_text();

    assert!(text.contains("\"file\"      \"plain.wav\""));
    assert!(text.contains("\"gain\"      \"2.000\""), "{text}");
    // The plain entry named its file and nothing else.
    let plain = text.split("\"plain.wav\"").nth(1).unwrap().split('}').next().unwrap();
    assert!(!plain.contains("gain"), "plain entry should be bare: {plain}");
}

#[test]
fn clearing_an_entry_puts_it_back_on_the_defaults() {
    let mut script = Script {
        defaults: Options { gain: 0.5, ..Options::default() },
        ..Script::default()
    };
    script.set("a.wav", Options { gain: 2.0, ..Options::default() });
    assert_eq!(script.options_for(Path::new("a.wav"), Path::new(".")).gain, 2.0);

    script.clear("a.wav");
    assert!(!script.has_entry("a.wav"));
    assert_eq!(script.options_for(Path::new("a.wav"), Path::new(".")).gain, 0.5);
}

#[test]
fn a_block_with_no_file_is_skipped_rather_than_keyed_on_nothing() {
    let script = Script::parse(r#"sound { "gain" "2.0" }"#).unwrap();
    assert_eq!(script.entries().count(), 0);
}
