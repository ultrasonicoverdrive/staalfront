//! The generator is deterministic and the committed files are its output (WP11.3b). Integer maths only, so
//! these numbers hold on every platform; a recipe change moves them on purpose and nothing else should.

use std::path::{Path, PathBuf};

use sf_sound_gen::{FILE_PEAK, SAMPLE_RATE, SOUNDS, files};

/// (file, byte length, FNV-1a 64 of the bytes), pinned as literals, sorted by file name.
const PINNED: [(&str, usize, u64); 18] = [
    ("explosion_large.wav", 88244, 0xa076a984215b0b18),
    ("explosion_medium.wav", 52964, 0x34b10eb362ce178f),
    ("explosion_small.wav", 35324, 0x477b2c2073229269),
    ("impact_bullet.wav", 6658, 0x1c68b963b01abd0c),
    ("impact_cannon.wav", 30914, 0xdfc2664220a75686),
    ("impact_demolition.wav", 52964, 0x43b68980051c7a73),
    ("impact_miss.wav", 13274, 0x3b70621c9f72c8bf),
    ("impact_precision.wav", 11068, 0x46a57510910b65ee),
    ("impact_rocket.wav", 39734, 0x410329ac07ebe021),
    ("impact_shell.wav", 26504, 0xd092ac1a545a04d9),
    ("unit_destroyed_infantry.wav", 22094, 0x1210efbb7ff8e4fb),
    ("weapon_fire_bullet.wav", 11068, 0x8e8b82450d275324),
    ("weapon_fire_cannon.wav", 26504, 0xc5c2cb6c7b4cdafe),
    ("weapon_fire_demolition.wav", 19888, 0xa229930a926b2345),
    ("weapon_fire_howitzer.wav", 44144, 0x767c8173897ffeb3),
    ("weapon_fire_precision.wav", 22094, 0xd82d83a3c2bbf445),
    ("weapon_fire_rocket.wav", 39734, 0x97c73ed0b596b921),
    ("weapon_fire_shell.wav", 22094, 0x81a74f7049066ac2),
];

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn sfx_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio/sfx")
}

#[test]
fn the_same_seed_gives_the_same_bytes_and_every_file_is_pinned() {
    let first = files();
    let second = files();
    assert_eq!(first, second, "two runs differ");
    assert_eq!(first.len(), PINNED.len());
    let mut sorted: Vec<_> = first.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for ((name, bytes), (pin_name, len, hash)) in sorted.into_iter().zip(PINNED) {
        assert_eq!(name, pin_name);
        assert_eq!(bytes.len(), len, "{name} length");
        assert_eq!(fnv1a(bytes), hash, "{name} hash (a recipe changed?)");
    }
}

#[test]
fn the_committed_files_are_what_a_fresh_run_writes() {
    for (name, bytes) in files() {
        let on_disk = std::fs::read(sfx_dir().join(&name))
            .unwrap_or_else(|e| panic!("{name} is not committed: {e}"));
        assert!(
            on_disk == bytes,
            "{name} is out of date: run `cargo run -p sf_sound_gen`"
        );
    }
}

#[test]
fn every_file_is_a_loud_unclipped_mono_wav_of_its_length_and_all_stay_in_budget() {
    let mut total = 0usize;
    for s in &SOUNDS {
        let bytes = s.bytes();
        total += bytes.len();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            SAMPLE_RATE
        );
        let samples: Vec<i16> = bytes[44..]
            .chunks(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(
            samples.len() as u64,
            u64::from(SAMPLE_RATE) * u64::from(s.ms) / 1000,
            "{} length",
            s.file
        );
        let peak = samples.iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert_eq!(peak, FILE_PEAK, "{} is normalised to the file peak", s.file);
        let loud = samples.iter().filter(|v| v.abs() > 500).count();
        assert!(loud > samples.len() / 20, "{} is mostly silent", s.file);
        assert!(
            samples.last().unwrap().abs() < 50,
            "{} ends with a click",
            s.file
        );
    }
    assert!(
        total <= 2_000_000,
        "all fight sounds: {total} bytes, budget 2 MB"
    );
}

#[test]
fn every_sound_has_its_bank_entry_with_its_file() {
    let bank = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio/bank.ron"),
    )
    .unwrap();
    for s in &SOUNDS {
        let head = format!("\"{}\": (", s.event);
        let at = bank
            .find(&head)
            .unwrap_or_else(|| panic!("no bank entry for {}", s.event));
        let rest = bank[at + head.len()..].trim_start();
        assert!(
            rest.starts_with(&format!("file: \"audio/sfx/{}\"", s.file)),
            "{} does not point at {}",
            s.event,
            s.file
        );
    }
}
