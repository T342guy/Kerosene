// SPDX-License-Identifier: LGPL-3.0-or-later
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
    // With no file named, the name is the path.
    assert_eq!(def.file, "sound/ambient/hum.wav");
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
    let bank = SoundBank::new();
    assert_eq!(bank.resolve("test").0, "sound/test.wav");
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
