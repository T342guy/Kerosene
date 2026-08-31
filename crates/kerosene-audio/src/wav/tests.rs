// SPDX-License-Identifier: MPL-2.0
use super::*;

/// Build a WAV in memory, so the tests do not need files on disk.
fn wav(format: u16, bits: u16, channels: u16, rate: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let block_align = channels * bits / 8;
    let byte_rate = rate * block_align as u32;

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());

    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn pcm16(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[test]
fn sixteen_bit_pcm_is_the_common_case() {
    let bytes = wav(FORMAT_PCM, 16, 1, 44100, &pcm16(&[0, 16384, -16384, 32767]));
    let sound = decode(&bytes).unwrap();
    assert_eq!(sound.channels, 1);
    assert_eq!(sound.sample_rate, 44100);
    assert_eq!(sound.frames(), 4);
    assert!((sound.samples[0] - 0.0).abs() < 1e-6);
    assert!((sound.samples[1] - 0.5).abs() < 1e-3);
    assert!((sound.samples[2] + 0.5).abs() < 1e-3);
    assert!((sound.samples[3] - 1.0).abs() < 1e-3);
}

#[test]
fn eight_bit_pcm_is_unsigned_and_centred_on_128() {
    // The one PCM depth that is not two's complement. Getting it wrong gives
    // a sound that is entirely positive and clicks on every loop.
    let bytes = wav(FORMAT_PCM, 8, 1, 22050, &[128, 255, 0, 192]);
    let sound = decode(&bytes).unwrap();
    assert!((sound.samples[0]).abs() < 1e-6, "silence is 128, not 0");
    assert!(sound.samples[1] > 0.9);
    assert!(sound.samples[2] < -0.9);
    assert!(sound.samples[3] > 0.4 && sound.samples[3] < 0.6);
}

#[test]
fn twenty_four_bit_pcm_sign_extends() {
    // Three bytes, little-endian, signed. Without the sign extension every
    // negative sample comes out as a very large positive one.
    let quiet_negative: [u8; 3] = [0x00, 0x00, 0xFF]; // -65536 in 24-bit
    let bytes = wav(FORMAT_PCM, 24, 1, 48000, &quiet_negative);
    let sound = decode(&bytes).unwrap();
    assert!(sound.samples[0] < 0.0, "got {}", sound.samples[0]);
    assert!(sound.samples[0] > -0.01, "the magnitude is wrong: {}", sound.samples[0]);
}

#[test]
fn thirty_two_bit_float_passes_through() {
    let data: Vec<u8> = [0.25f32, -0.5, 1.0].iter().flat_map(|f| f.to_le_bytes()).collect();
    let bytes = wav(FORMAT_FLOAT, 32, 1, 48000, &data);
    let sound = decode(&bytes).unwrap();
    assert_eq!(sound.samples, vec![0.25, -0.5, 1.0]);
}

#[test]
fn stereo_stays_interleaved() {
    let bytes = wav(FORMAT_PCM, 16, 2, 44100, &pcm16(&[32767, -32768, 0, 0]));
    let sound = decode(&bytes).unwrap();
    assert_eq!(sound.channels, 2);
    assert_eq!(sound.frames(), 2);
    assert!(sound.sample(0, 0) > 0.9, "left");
    assert!(sound.sample(0, 1) < -0.9, "right");
}

#[test]
fn a_mono_sound_reads_the_same_in_both_ears() {
    // The mixer asks for channel 1 of everything; a mono sound has to answer.
    let bytes = wav(FORMAT_PCM, 16, 1, 44100, &pcm16(&[16384]));
    let sound = decode(&bytes).unwrap();
    assert_eq!(sound.sample(0, 0), sound.sample(0, 1));
}

#[test]
fn reading_past_the_end_is_silence_not_a_panic() {
    let bytes = wav(FORMAT_PCM, 16, 1, 44100, &pcm16(&[16384]));
    let sound = decode(&bytes).unwrap();
    assert_eq!(sound.sample(9999, 0), 0.0);
}

#[test]
fn duration_comes_out_in_seconds() {
    let bytes = wav(FORMAT_PCM, 16, 1, 1000, &pcm16(&[0; 500]));
    let sound = decode(&bytes).unwrap();
    assert!((sound.duration() - 0.5).abs() < 1e-6);
}

#[test]
fn chunks_it_does_not_understand_are_skipped() {
    // Real files are full of LIST, fact, and whatever an editor felt like
    // writing. Refusing them would refuse most of the WAVs in the world.
    let mut bytes = wav(FORMAT_PCM, 16, 1, 44100, &pcm16(&[16384, -16384]));
    let mut extra = Vec::new();
    extra.extend_from_slice(b"LIST");
    extra.extend_from_slice(&8u32.to_le_bytes());
    extra.extend_from_slice(b"INFOtest");
    // Insert after the header, before fmt.
    bytes.splice(12..12, extra);
    let sound = decode(&bytes).unwrap();
    assert_eq!(sound.frames(), 2);
}

#[test]
fn an_odd_length_chunk_is_padded_and_the_next_one_still_parses() {
    let mut bytes = wav(FORMAT_PCM, 16, 1, 44100, &pcm16(&[16384]));
    let mut extra = Vec::new();
    extra.extend_from_slice(b"junk");
    extra.extend_from_slice(&3u32.to_le_bytes());
    extra.extend_from_slice(&[1, 2, 3, 0]); // three bytes plus a pad
    bytes.splice(12..12, extra);
    assert_eq!(decode(&bytes).unwrap().frames(), 1);
}

#[test]
fn an_extensible_header_is_read_through_to_the_real_format() {
    // What a modern recorder writes. The real format tag is buried in a GUID.
    let mut out = Vec::new();
    let data = pcm16(&[16384]);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&FORMAT_EXTENSIBLE.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // channels
    out.extend_from_slice(&44100u32.to_le_bytes());
    out.extend_from_slice(&88200u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(&22u16.to_le_bytes()); // cbSize
    out.extend_from_slice(&16u16.to_le_bytes()); // valid bits
    out.extend_from_slice(&0u32.to_le_bytes()); // channel mask
    out.extend_from_slice(&FORMAT_PCM.to_le_bytes()); // the GUID's first two bytes
    out.extend_from_slice(&[0; 14]);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);

    let sound = decode(&out).unwrap();
    assert_eq!(sound.frames(), 1);
    assert!((sound.samples[0] - 0.5).abs() < 1e-3);
}

// ---- files that are wrong -------------------------------------------------

#[test]
fn something_that_is_not_a_wav_is_refused() {
    assert!(decode(b"").is_err());
    assert!(decode(b"not audio at all").is_err());
    assert!(decode(b"RIFF\0\0\0\0AVI ").is_err());
}

#[test]
fn a_file_with_no_data_chunk_is_refused() {
    let mut bytes = wav(FORMAT_PCM, 16, 1, 44100, &[]);
    // Rename the data chunk so it is not found.
    let at = bytes.windows(4).position(|w| w == b"data").unwrap();
    bytes[at..at + 4].copy_from_slice(b"nope");
    assert!(decode(&bytes).is_err());
}

#[test]
fn a_chunk_claiming_more_bytes_than_the_file_holds_does_not_read_off_the_end() {
    // The one that matters: a length field is a number in a file, and a
    // truncated recording is normal.
    let mut bytes = wav(FORMAT_PCM, 16, 1, 44100, &pcm16(&[16384, -16384]));
    let at = bytes.windows(4).position(|w| w == b"data").unwrap();
    bytes[at + 4..at + 8].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    let sound = decode(&bytes).expect("takes what is there");
    assert_eq!(sound.frames(), 2);
}

#[test]
fn a_sample_rate_of_zero_is_refused_rather_than_dividing_by_it() {
    let bytes = wav(FORMAT_PCM, 16, 1, 0, &pcm16(&[0]));
    assert!(decode(&bytes).is_err());
}

#[test]
fn more_than_stereo_is_refused_with_a_reason() {
    let bytes = wav(FORMAT_PCM, 16, 6, 48000, &pcm16(&[0; 12]));
    match decode(&bytes) {
        Err(AudioError::Unsupported(text)) => assert!(text.contains("channels"), "{text}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unknown_encoding_says_which_one() {
    let bytes = wav(0x11, 4, 1, 22050, &[0, 1, 2, 3]); // IMA ADPCM
    match decode(&bytes) {
        Err(AudioError::Unsupported(text)) => assert!(text.contains("17"), "{text}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_truncated_fmt_chunk_is_refused() {
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&[1, 0, 1, 0]);
    assert!(decode(&out).is_err());
}
