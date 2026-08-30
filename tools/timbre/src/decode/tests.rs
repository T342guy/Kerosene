// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;

/// The fixtures: one tone, in three containers. See their README.
fn fixture(name: &str) -> (PathBuf, Vec<u8>) {
    let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("the {name} fixture must be readable: {e}"));
    (path, bytes)
}

use std::path::PathBuf;

// ---- knowing what a file is -----------------------------------------------

#[test]
fn every_extension_maps_to_the_format_it_names() {
    assert_eq!(Format::of(Path::new("a.wav")), Some(Format::Wav));
    assert_eq!(Format::of(Path::new("a.WAV")), Some(Format::Wav));
    assert_eq!(Format::of(Path::new("a.wave")), Some(Format::Wav));
    assert_eq!(Format::of(Path::new("a.flac")), Some(Format::Flac));
    assert_eq!(Format::of(Path::new("a.MP3")), Some(Format::Mp3));
}

#[test]
fn a_format_timbre_does_not_read_is_not_guessed_at() {
    assert_eq!(Format::of(Path::new("a.ogg")), None);
    assert_eq!(Format::of(Path::new("a.aiff")), None);
    assert_eq!(Format::of(Path::new("a")), None);
}

#[test]
fn only_mp3_is_flagged_as_already_lossy() {
    assert!(Format::Mp3.is_lossy());
    assert!(!Format::Wav.is_lossy());
    assert!(!Format::Flac.is_lossy(), "FLAC is lossless, whatever its size suggests");
}

#[test]
fn every_listed_extension_actually_maps_to_a_format() {
    // The list drives the file scan; an entry that maps to nothing would find
    // files the decoder then refuses.
    for extension in EXTENSIONS {
        let path = PathBuf::from(format!("a.{extension}"));
        assert!(Format::of(&path).is_some(), "{extension} is listed but not read");
    }
}

#[test]
fn an_unreadable_extension_says_what_it_does_read() {
    let err = any(Path::new("a.ogg"), b"").unwrap_err().to_string();
    assert!(err.contains("wav"), "{err}");
    assert!(err.contains("flac"), "{err}");
    assert!(err.contains("mp3"), "{err}");
}

// ---- decoding the real thing ----------------------------------------------

#[test]
fn a_wav_decodes_through_the_engines_own_decoder() {
    let (path, bytes) = fixture("tone.wav");
    let decoded = any(&path, &bytes).unwrap();
    assert_eq!(decoded.format, Format::Wav);
    assert_eq!(decoded.sound.channels, 1);
    assert_eq!(decoded.sound.sample_rate, 44100);
    assert_eq!(decoded.sound.frames(), 8820);
}

#[test]
fn a_flac_decodes_to_the_same_sound_the_wav_holds() {
    // FLAC is lossless, so this is not an approximation: the samples must be
    // the samples. If they are not, something in the conversion is wrong.
    let (wav_path, wav_bytes) = fixture("tone.wav");
    let (flac_path, flac_bytes) = fixture("tone.flac");
    let wav = any(&wav_path, &wav_bytes).unwrap().sound;
    let flac = any(&flac_path, &flac_bytes).unwrap();

    assert_eq!(flac.format, Format::Flac);
    assert_eq!(flac.sound.channels, wav.channels);
    assert_eq!(flac.sound.sample_rate, wav.sample_rate);
    assert_eq!(flac.sound.frames(), wav.frames());

    let worst = wav
        .samples
        .iter()
        .zip(&flac.sound.samples)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 1e-4, "lossless should mean identical, worst difference {worst}");
}

#[test]
fn a_flac_declaring_a_loop_in_its_tags_has_it_read() {
    // A WAV says this in its `smpl` chunk. A FLAC says it in a Vorbis comment,
    // and nothing standard says it must -- but LOOPSTART/LOOPLENGTH is what
    // game audio has settled on, and not reading it loses what a WAV keeps.
    let (path, bytes) = fixture("tone.flac");
    let region = any(&path, &bytes).unwrap().looping.expect("the fixture has loop tags");
    assert_eq!(region.start, 1000);
    assert_eq!(region.end, 4000, "start plus length");
}

#[test]
fn an_mp3_decodes_to_roughly_the_sound_it_was_made_from() {
    // Roughly, because it is lossy and 64 kbit at that. Close enough that a
    // wildly wrong decode -- silence, noise, the wrong rate -- would fail.
    let (wav_path, wav_bytes) = fixture("tone.wav");
    let (mp3_path, mp3_bytes) = fixture("tone.mp3");
    let wav = any(&wav_path, &wav_bytes).unwrap().sound;
    let mp3 = any(&mp3_path, &mp3_bytes).unwrap();

    assert_eq!(mp3.format, Format::Mp3);
    assert_eq!(mp3.sound.channels, 1);
    assert_eq!(mp3.sound.sample_rate, 44100);
    // An MP3 carries encoder delay and padding, so the frame count is close
    // rather than equal.
    let difference = mp3.sound.frames().abs_diff(wav.frames());
    assert!(difference < 3000, "decoded {} frames against {}", mp3.sound.frames(), wav.frames());

    let peak = mp3.sound.samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!((0.4..0.8).contains(&peak), "a 0.6 tone should decode near 0.6, got {peak}");
}

#[test]
fn an_mp3_declares_no_loop_of_its_own() {
    let (path, bytes) = fixture("tone.mp3");
    assert_eq!(any(&path, &bytes).unwrap().looping, None);
}

// ---- files that are not what they claim -----------------------------------

#[test]
fn a_flac_that_is_not_one_fails_with_its_name_in_the_message() {
    let err = any(Path::new("broken.flac"), b"not a flac at all, not even close")
        .unwrap_err();
    assert!(format!("{err:#}").contains("broken.flac"), "{err:#}");
}

#[test]
fn an_mp3_that_is_not_one_fails_rather_than_decoding_noise() {
    let result = any(Path::new("broken.mp3"), &vec![0x5au8; 4096]);
    assert!(result.is_err(), "a buffer of junk should not decode to a sound");
}

#[test]
fn a_truncated_flac_is_an_error_rather_than_a_panic() {
    let (path, bytes) = fixture("tone.flac");
    assert!(any(&path, &bytes[..bytes.len() / 3]).is_err());
}

// ---- a name that lies about the bytes --------------------------------------

#[test]
fn a_file_named_wav_that_holds_an_mp3_says_which_it_is() {
    // Not hypothetical: files downloaded and renamed arrive like this, and
    // "not a RIFF/WAVE file" sends someone looking for a corrupt file rather
    // than a mislabelled one.
    let (_, mp3) = fixture("tone.mp3");
    let err = any(Path::new("music/track.wav"), &mp3).unwrap_err().to_string();
    assert!(err.contains("named .wav"), "{err}");
    assert!(err.contains("are mp3"), "{err}");
}

#[test]
fn a_bare_mp3_with_no_tag_is_still_recognised() {
    // An MP3 that starts on a frame sync rather than an ID3 header is a shape
    // that really turns up -- it is what a file downloaded and renamed to
    // `.wav` looked like -- and the one that would otherwise slip through.
    // 0xFFFB is eleven sync bits, MPEG-1, layer III.
    assert_eq!(sniff(&[0xff, 0xfb, 0x90, 0x64]), Some("mp3"));
    assert_eq!(sniff(&[0xff, 0xf3, 0x48, 0xc4]), Some("mp3"), "MPEG-2 is the same shape");
}

#[test]
fn eleven_set_bits_alone_are_not_enough_to_call_it_an_mp3() {
    // The reserved version and layer values say it is something else, and a
    // sniffer that shouted "mp3" at every 0xFF 0xFF would misdiagnose more
    // than it diagnosed.
    assert_eq!(sniff(&[0xff, 0xe8, 0x00, 0x00]), None, "reserved version");
    assert_eq!(sniff(&[0xff, 0xe0, 0x00, 0x00]), None, "reserved layer");
}

#[test]
fn a_flac_named_wav_is_caught_too() {
    let (_, flac) = fixture("tone.flac");
    let err = any(Path::new("a.wav"), &flac).unwrap_err().to_string();
    assert!(err.contains("are flac"), "{err}");
}

#[test]
fn a_container_timbre_cannot_read_is_named_as_such() {
    // Better than "not a RIFF file": it says what the file is and that this
    // tool does not read it, which is two different pieces of the answer.
    let mut ogg = b"OggS".to_vec();
    ogg.resize(64, 0);
    let err = any(Path::new("a.wav"), &ogg).unwrap_err().to_string();
    assert!(err.contains("ogg"), "{err}");
    assert!(err.contains("does not read"), "{err}");
}

#[test]
fn a_file_that_matches_its_extension_passes_the_check() {
    for name in ["tone.wav", "tone.flac", "tone.mp3"] {
        let (path, bytes) = fixture(name);
        assert!(any(&path, &bytes).is_ok(), "{name} should decode");
    }
}

#[test]
fn bytes_that_look_like_nothing_are_left_to_the_decoder() {
    // Sniffing is a diagnostic, not a gate: something unrecognised should
    // reach the real decoder and get the real error rather than a guess.
    assert_eq!(sniff(b"\x00\x01\x02\x03 nothing in particular"), None);
}
