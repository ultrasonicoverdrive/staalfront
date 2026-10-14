//! The committed CC0 test sound `assets/audio/sfx/test_click.wav` is generated
//! by this file (integer maths only, so the bytes are identical on every
//! platform). The test fails if the committed file drifts from the generator.
//!
//! Regenerate with `SF_REGEN_TEST_SOUND=1 cargo test -p sf_audio --test test_sound`.

use std::path::{Path, PathBuf};

const SAMPLE_RATE: u32 = 22_050;
const SAMPLES: u32 = 3_307; // 0.15 s
const FREQ_HZ: u32 = 880;
const PEAK: i64 = 18_000;

/// A short decaying 880 Hz triangle beep, 16-bit mono PCM WAV.
fn click_wav() -> Vec<u8> {
    let mut pcm = Vec::with_capacity(SAMPLES as usize * 2);
    let mut phase: u32 = 0;
    // phase step per sample in 1/2^32 of a period
    let step = ((u64::from(FREQ_HZ) << 32) / u64::from(SAMPLE_RATE)) as u32;
    for i in 0..SAMPLES {
        // triangle in -32768..32767 from the top bits of the phase
        let p = (phase >> 16) as i64; // 0..65535
        let tri = if p < 32_768 {
            p * 2 - 32_768
        } else {
            (65_535 - p) * 2 - 32_768
        };
        // squared linear decay, with a 2 ms attack to avoid a hard start
        let left = i64::from(SAMPLES - i);
        let decay = left * left / i64::from(SAMPLES); // 0..SAMPLES
        let attack = i64::from(i.min(44)) * 1000 / 44; // 0..1000
        let s = tri * PEAK / 32_768 * decay / i64::from(SAMPLES) * attack / 1000;
        pcm.extend_from_slice(&(s as i16).to_le_bytes());
        phase = phase.wrapping_add(step);
    }
    let data_len = pcm.len() as u32;
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&pcm);
    wav
}

fn committed_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio/sfx/test_click.wav")
}

#[test]
fn committed_test_sound_matches_the_generator() {
    let generated = click_wav();
    if std::env::var_os("SF_REGEN_TEST_SOUND").is_some() {
        std::fs::write(committed_path(), &generated).unwrap();
    }
    let committed = std::fs::read(committed_path()).expect("assets/audio/sfx/test_click.wav");
    assert!(committed.len() <= 20 * 1024, "{} bytes", committed.len());
    assert_eq!(
        committed, generated,
        "regenerate with SF_REGEN_TEST_SOUND=1"
    );
}

#[test]
fn the_test_sound_decodes_and_is_not_silent() {
    let data = kira::sound::static_sound::StaticSoundData::from_file(committed_path()).unwrap();
    assert_eq!(data.num_frames(), SAMPLES as usize);
    let loudest = (0..data.num_frames())
        .filter_map(|i| data.frame_at_index(i))
        .map(|f| f.left.abs())
        .fold(0.0_f32, f32::max);
    assert!(loudest > 0.2 && loudest < 0.6, "{loudest}");
}
