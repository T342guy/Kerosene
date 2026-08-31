// SPDX-License-Identifier: MPL-2.0
use super::*;

fn sound() -> Arc<Sound> {
    Arc::new(Sound { channels: 1, sample_rate: 44_100, samples: vec![0.5; 100] })
}

const SCRIPT: &str = r#"
sound
{
    "name"        "door/open"
    "file"        "sound/door/open.wav"
    "volume"      "0.8"
    "pitch"       "1.2"
    "attenuation" "0.5"
    "distance"    "96"
    "max"         "2048"
}

sound
{
    "name"  "ambient/hum"
    "loop"  "1"
}
"#;

#[test]
fn a_script_says_what_a_name_means() {
    let script = SoundScript::parse(SCRIPT).unwrap();
    let def = script.get("door/open").expect("defined");
    assert_eq!(def.file, "sound/door/open.wav");
    assert_eq!(def.volume, 0.8);
    assert_eq!(def.pitch, 1.2);
    assert_eq!(def.attenuation, 0.5);
    assert_eq!(def.reference_distance, 96.0);
    assert_eq!(def.max_distance, 2048.0);
    assert!(!def.looping);
}

#[test]
fn a_definition_falls_back_to_sensible_values() {
    let script = SoundScript::parse(SCRIPT).unwrap();
    let def = script.get("ambient/hum").expect("defined");
    assert!(def.looping);
    assert_eq!(def.volume, 1.0);
    // With no file named, the name is the path -- and the compiled form,
    // which is what a shipped game holds. The source is still found, but by
    // `candidates` rather than by guessing one extension.
    assert_eq!(def.file, "sound/ambient/hum.keroaud");
}

#[test]
fn names_are_matched_without_regard_to_case() {
    let script = SoundScript::parse(SCRIPT).unwrap();
    assert!(script.get("DOOR/OPEN").is_some());
}

#[test]
fn a_block_with_no_name_is_skipped_rather_than_failing_the_file() {
    // One bad entry should not silence a whole game.
    let script = SoundScript::parse(r#"
        sound { "file" "x.wav" }
        sound { "name" "good" }
    "#)
    .unwrap();
    assert_eq!(script.len(), 1);
    assert!(script.get("good").is_some());
}

#[test]
fn a_later_script_overrides_an_earlier_one() {
    let mut script = SoundScript::parse(SCRIPT).unwrap();
    script.merge(SoundScript::parse(r#" sound { "name" "door/open" "volume" "0.1" } "#).unwrap());
    assert_eq!(script.get("door/open").unwrap().volume, 0.1);
    assert_eq!(script.len(), 2, "overriding one added a second");
}

#[test]
fn a_definition_becomes_the_parameters_it_describes() {
    let script = SoundScript::parse(SCRIPT).unwrap();
    let params = script.get("door/open").unwrap().params();
    assert_eq!(params.volume, 0.8);
    assert_eq!(params.reference_distance, 96.0);
    // Where a sound is comes from whatever plays it, not from the script.
    assert_eq!(params.position, None);
}

// ---- the bank -------------------------------------------------------------

#[test]
fn a_named_sound_resolves_through_the_script() {
    let mut bank = SoundBank::new();
    bank.add_script(SoundScript::parse(SCRIPT).unwrap());
    let (path, params) = bank.resolve("door/open");
    assert_eq!(path, "sound/door/open.wav");
    assert_eq!(params.volume, 0.8);
}

#[test]
fn a_name_nobody_defined_is_taken_as_a_path() {
    // So `play sound/test.wav` works without anyone writing a script first.
    // A bare name gets the compiled extension; the rest are reached through
    // `candidates`, which is what stops one guess standing in for the answer.
    let bank = SoundBank::new();
    assert_eq!(bank.resolve("test").0, "sound/test.keroaud");
    assert_eq!(bank.resolve("weapons/fire.wav").0, "sound/weapons/fire.wav");
    assert_eq!(bank.resolve("sound/weapons/fire.wav").0, "sound/weapons/fire.wav");
    assert_eq!(bank.resolve("/leading.wav").0, "sound/leading.wav");
}

#[test]
fn loaded_sounds_come_back_by_name_whatever_the_case() {
    let mut bank = SoundBank::new();
    bank.insert("Door/Open", sound());
    assert!(bank.is_loaded("door/OPEN"));
    assert!(bank.get("DOOR/open").is_some());
    assert_eq!(bank.len(), 1);
}

#[test]
fn a_missing_sound_is_complained_about_once() {
    // Otherwise a trigger firing every tick fills the console with the same
    // line until nothing else is readable.
    let mut bank = SoundBank::new();
    assert!(!bank.already_missing("nope"));
    bank.mark_missing("nope");
    assert!(bank.already_missing("NOPE"));

    // ...and loading it later clears the mark.
    bank.insert("nope", sound());
    assert!(!bank.already_missing("nope"));
}

#[test]
fn forgetting_everything_empties_the_bank() {
    let mut bank = SoundBank::new();
    bank.insert("a", sound());
    bank.mark_missing("b");
    bank.forget_all();
    assert!(bank.is_empty());
    assert!(!bank.already_missing("b"));
}

// ---- resolving a name to a file -------------------------------------------

#[test]
fn a_bare_name_looks_for_the_compiled_form_first() {
    // What a shipped game holds, and what a build produces.
    let bank = SoundBank::new();
    let candidates = bank.candidates("ui/click");
    assert_eq!(candidates[0], "sound/ui/click.keroaud");
    assert!(candidates.contains(&"sound/ui/click.wav".to_string()));
}

#[test]
fn a_named_source_is_still_tried_after_its_compiled_sibling() {
    // Mid-edit, a designer who has just dropped a `.wav` in should hear it
    // without running a build to find out whether it is the right one.
    let bank = SoundBank::new();
    let candidates = bank.candidates("ui/click.wav");
    assert_eq!(candidates[0], "sound/ui/click.keroaud");
    assert!(candidates.contains(&"sound/ui/click.wav".to_string()));
}

#[test]
fn a_name_the_script_defines_resolves_through_the_file_it_names() {
    let mut bank = SoundBank::new();
    bank.add_script(
        SoundScript::parse(r#"sound { "name" "door/move" "file" "sound/door/move.wav" }"#)
            .unwrap(),
    );
    let candidates = bank.candidates("door/move");
    assert_eq!(candidates[0], "sound/door/move.keroaud");
    assert!(candidates.contains(&"sound/door/move.wav".to_string()));
}

#[test]
fn a_source_the_engine_cannot_read_is_still_offered_as_a_candidate_path() {
    // So that a `.flac` named outright reaches the decoder and gets a real
    // error, rather than being quietly dropped from the list.
    let bank = SoundBank::new();
    assert!(
        bank.candidates("music/theme.flac").contains(&"sound/music/theme.flac".to_string()),
        "{:?}",
        bank.candidates("music/theme.flac")
    );
}

#[test]
fn a_flac_beside_the_name_is_found_for_the_message() {
    // The bug this exists for: `play ambient/track` reported
    // `sound/ambient/track.wav` missing when the file on disk was a `.flac` --
    // a path nobody had written, about a file that was right there.
    let there = |p: &str| p == "sound/ambient/track.flac";
    assert_eq!(
        uncompiled_source("sound/ambient/track.keroaud", there).as_deref(),
        Some("sound/ambient/track.flac")
    );
}

#[test]
fn nothing_beside_the_name_means_nothing_to_say_about_it() {
    assert_eq!(uncompiled_source("sound/ambient/track.keroaud", |_| false), None);
}

#[test]
fn siblings_keep_a_dot_in_a_directory_name_out_of_it() {
    // `sound/v1.2/click` has a dot in a directory and no extension at all.
    let out = siblings("sound/v1.2/click");
    assert!(out.iter().all(|p| p.starts_with("sound/v1.2/click")), "{out:?}");
}
