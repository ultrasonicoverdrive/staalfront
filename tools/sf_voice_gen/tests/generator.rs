//! The voice filter is deterministic, the committed files are its output from the committed Piper sources,
//! every line is a usable file, and the walkie-talkie band is where the constants say (WP11.2b).

use std::path::{Path, PathBuf};

use sf_sound_gen::{FILE_PEAK, SAMPLE_RATE};
use sf_voice_gen::{
    ANNOUNCEMENTS, BAND_HIGH_HZ, BAND_LOW_HZ, LINES, OPEN_MS, PRESENCE_HZ, TAIL_MS, cos_q30,
    exp_neg_q30, files, radio_band, read_wav, source_dir, trim,
};

fn voice_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio/voice")
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// (file, byte length, FNV-1a 64 of the bytes), pinned as literals, in the order of `LINES`.
const PINNED: [(&str, usize, u64); 9] = [
    ("unit_select_ready.wav", 22680, 0x667080df4a07ce5c),
    ("unit_select_online.wav", 29604, 0x88a6d64e2a579f22),
    ("unit_select_standing_by.wav", 44944, 0x1aa50e952e5e07c0),
    ("unit_move_moving.wav", 22736, 0x03f1c3213fb0a3a2),
    ("unit_move_route_set.wav", 29966, 0xe94daf926d5a5b48),
    ("unit_move_on_it.wav", 22784, 0x9f6d5ecdfece8489),
    ("unit_attack_engaging.wav", 30412, 0xa1177ff11fd3504c),
    ("unit_attack_target_locked.wav", 37670, 0xb635936c1741064e),
    ("unit_attack_firing.wav", 24538, 0x7820f7aada0cc3a0),
];

/// The committed Piper sources, (file, byte length, FNV-1a 64), as literals: the pinned input.
const SOURCES: [(&str, usize, u64); 9] = [
    ("unit_select_ready.wav", 39468, 0x7094b61d137dd5ba),
    ("unit_select_online.wav", 46124, 0xa819d8ce49400d7e),
    ("unit_select_standing_by.wav", 61484, 0xcef9439e25bbef25),
    ("unit_move_moving.wav", 38956, 0xc63828363d10e057),
    ("unit_move_route_set.wav", 49708, 0x046c686d2f46915a),
    ("unit_move_on_it.wav", 38444, 0x5186785939fcbe03),
    ("unit_attack_engaging.wav", 48172, 0xc81cfcb20c2e877d),
    ("unit_attack_target_locked.wav", 52780, 0x50c329fe4aa34cd9),
    ("unit_attack_firing.wav", 42028, 0x0e76e32133752cb7),
];

#[test]
fn every_line_has_its_committed_piper_source_and_it_is_the_pinned_one() {
    for (l, (file, len, hash)) in LINES.iter().zip(SOURCES) {
        assert_eq!(l.file, file);
        let b = std::fs::read(source_dir().join(file))
            .unwrap_or_else(|e| panic!("{file}: no source ({e})"));
        assert_eq!(b.len(), len, "{file} length");
        assert_eq!(fnv1a(&b), hash, "{file}: the Piper source changed");
        let s = read_wav(&b).unwrap();
        assert!(s.iter().any(|v| v.abs() > 5_000), "{file} is silent");
    }
    assert!(source_dir().join("MODEL_CARD.md").is_file());
}

#[test]
fn the_same_source_gives_the_same_bytes_and_every_file_is_pinned() {
    let first = files();
    assert_eq!(first, files(), "two runs differ");
    for ((name, bytes), (pin, len, hash)) in first.iter().zip(PINNED) {
        assert_eq!(name, pin);
        assert_eq!(bytes.len(), len, "{name} length");
        assert_eq!(
            fnv1a(bytes),
            hash,
            "{name} hash (the filter or a source changed?)"
        );
    }
}

#[test]
fn the_committed_files_are_what_a_fresh_run_writes() {
    for (name, bytes) in files() {
        let on_disk = std::fs::read(voice_dir().join(&name))
            .unwrap_or_else(|e| panic!("{name} is not committed: {e}"));
        assert!(
            on_disk == bytes,
            "{name} is out of date: run `cargo run -p sf_voice_gen`"
        );
    }
}

#[test]
fn every_line_is_an_unclipped_mono_wav_of_at_most_1_5_s_near_its_trimmed_source_with_the_same_peak()
{
    let mut total = 0usize;
    for l in &LINES {
        let bytes = l.bytes();
        total += bytes.len();
        assert_eq!(&bytes[0..4], b"RIFF");
        let s = read_wav(&bytes).unwrap();
        let ms = s.len() as u64 * 1000 / u64::from(SAMPLE_RATE);
        assert!((400..=1500).contains(&ms), "{}: {ms} ms", l.file);
        // The words are the trimmed source plus the squelch on opening and the tail.
        let src: Vec<i64> = l.source().iter().map(|v| i64::from(*v)).collect();
        let words = trim(&src).len();
        assert!(words < src.len(), "{}: nothing trimmed", l.file);
        let extra = u64::from(SAMPLE_RATE) * u64::from(OPEN_MS + TAIL_MS) / 1000;
        assert_eq!(s.len() as u64, words as u64 + extra, "{}", l.file);
        let peak = s.iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert_eq!(peak, FILE_PEAK, "{} peak", l.file);
        assert!(
            s.iter().all(|v| *v != i16::MAX && *v != i16::MIN),
            "{} clips",
            l.file
        );
        let loud = s.iter().filter(|v| v.abs() > 2_000).count();
        assert!(loud > s.len() / 5, "{}: only {loud} loud samples", l.file);
        assert!(s.last().unwrap().abs() < 50, "{} ends with a click", l.file);
    }
    assert!(total <= 1_000_000, "all voices: {total} bytes");
}

#[test]
fn three_lines_per_event_with_the_agreed_words() {
    for event in ["unit.select", "unit.move", "unit.attack"] {
        assert_eq!(
            LINES.iter().filter(|l| l.event == event).count(),
            3,
            "{event}"
        );
    }
    let texts: Vec<&str> = LINES.iter().map(|l| l.text).collect();
    assert_eq!(
        texts,
        [
            "Ready",
            "Online",
            "Standing by",
            "Moving",
            "Route set",
            "On it",
            "Engaging",
            "Target locked",
            "Firing"
        ]
    );
    // The render script has the same file names and words (with a full stop).
    let script =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("piper_lines.py"))
            .unwrap();
    for l in &LINES {
        let key = l.file.trim_end_matches(".wav");
        assert!(
            script.contains(&format!("\"{key}\": \"{}.\"", l.text)),
            "{key} not in piper_lines.py"
        );
    }
}

#[test]
fn a_short_or_broken_wav_is_an_error_not_a_panic() {
    let mut head = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    head.extend_from_slice(&4u32.to_le_bytes());
    head.extend_from_slice(&[1, 0, 1, 0]);
    assert!(read_wav(&head).is_err(), "a 4-byte fmt chunk");
    assert!(read_wav(b"RIFF").is_err());
    assert!(read_wav(b"nope nope nope").is_err());
}

#[test]
fn the_integer_cosine_and_exponential_are_exact_enough() {
    const PI_Q60: i128 = 3_622_009_729_038_561_421;
    let one = 1i64 << 30;
    assert_eq!(cos_q30(0), one);
    assert!((cos_q30(PI_Q60) + one).abs() <= 2);
    assert!(cos_q30(PI_Q60 / 2).abs() <= 2);
    assert!((cos_q30(PI_Q60 / 3) - one / 2).abs() <= 2);
    let e = exp_neg_q30(1i128 << 59);
    assert!((e - 651_257_336).abs() <= 2, "{e}");
}

/// Gain of the walkie-talkie band at `f` Hz: the peak of a steady sine through [`radio_band`] over the input's.
fn band_gain(f: u32) -> f64 {
    let n = 22_050usize;
    let input: Vec<i64> = (0..n)
        .map(|i| {
            let w = 2.0 * std::f64::consts::PI * f64::from(f) * i as f64 / f64::from(SAMPLE_RATE);
            (w.sin() * 1_000_000.0) as i64
        })
        .collect();
    let mut x = input.clone();
    radio_band(&mut x);
    let settled = &x[n / 2..];
    settled.iter().map(|v| v.abs()).max().unwrap() as f64 / 1_000_000.0
}

/// The band limits of the filter, measured with tones: flat-ish in the band with the presence bump on top,
/// at least 20 dB down an octave outside each edge.
#[test]
fn the_walkie_talkie_band_passes_300_to_3000_hz_with_a_presence_bump() {
    assert_eq!((BAND_LOW_HZ, BAND_HIGH_HZ, PRESENCE_HZ), (300, 3000, 1800));
    let g = |f| {
        let v = band_gain(f);
        println!("band gain at {f} Hz: {v:.3}");
        v
    };
    let (g100, g150, g700, g1800, g6000, g8000) =
        (g(100), g(150), g(700), g(1800), g(6000), g(8000));
    assert!(g1800 > 1.5 * g700, "presence bump: {g1800} vs {g700}");
    assert!((0.7..=1.3).contains(&g700), "band: {g700}");
    assert!(g150 < g700 / 10.0, "an octave under 300 Hz: {g150}");
    assert!(g100 < g700 / 30.0, "100 Hz: {g100}");
    assert!(g6000 < g700 / 10.0, "an octave over 3000 Hz: {g6000}");
    assert!(g8000 < g700 / 30.0, "8 kHz: {g8000}");
}

/// The announcements (WP11.2c): (file, byte length, FNV-1a 64 of the bytes), in the order of `ANNOUNCEMENTS`.
const ANNOUNCE_PINNED: [(&str, usize, u64); 19] = [
    ("announce_structure_ready.wav", 44722, 0xe47629af4ff0f62c),
    ("announce_unit_ready.wav", 35064, 0x88fb174725f6365f),
    ("announce_building.wav", 22602, 0x8e1991570589c255),
    ("announce_training.wav", 24020, 0xab07d54c0e01a58a),
    ("announce_on_hold.wav", 30048, 0x8018f1151378775c),
    ("announce_cancelled.wav", 29342, 0x4784e33dda69ac83),
    ("announce_base_under_attack.wav", 48642, 0xd0e7170d55bb0750),
    (
        "announce_harvester_under_attack.wav",
        57842,
        0x71603a18246d7711,
    ),
    ("announce_low_power.wav", 29158, 0xd4f9f92575c737c5),
    ("announce_insufficient_funds.wav", 54096, 0xb58f53e704002439),
    ("announce_storage_full.wav", 42296, 0xe1cfc514a2031567),
    ("announce_unit_lost.wav", 38858, 0x2040ff4d24990faf),
    ("announce_structure_lost.wav", 51076, 0xf85f00124b525dd1),
    ("announce_new_options.wav", 54208, 0xf4bb599c94dd3d44),
    ("announce_cannot_deploy.wav", 44940, 0x9b1a45c7758da813),
    ("announce_victory.wav", 27812, 0x6ccd4f4183ec3ad5),
    ("announce_defeat.wav", 24146, 0x88fac8d41b82c7f4),
    ("announce_structure_sold.wav", 45024, 0x5344e9687367501b),
    ("announce_repairing.wav", 28958, 0x0eb5bc70f92ae248),
];

/// The committed Piper sources of the announcements, (file, byte length, FNV-1a 64), as literals.
const ANNOUNCE_SOURCES: [(&str, usize, u64); 19] = [
    ("announce_structure_ready.wav", 61484, 0x2edf9e3056c8b1a5),
    ("announce_unit_ready.wav", 52780, 0xfc7d4edff0966f50),
    ("announce_building.wav", 38956, 0xe9eb2f6a0d258f91),
    ("announce_training.wav", 38956, 0x79c63024a4c71b5c),
    ("announce_on_hold.wav", 49196, 0xef0d931e8f8d9245),
    ("announce_cancelled.wav", 45612, 0x08637d374d18df8c),
    ("announce_base_under_attack.wav", 69164, 0xb3cb3639ebf3989e),
    (
        "announce_harvester_under_attack.wav",
        76332,
        0xf182e9dbe41ba968,
    ),
    ("announce_low_power.wav", 46124, 0xff0d57d38e73731e),
    ("announce_insufficient_funds.wav", 73260, 0xb73d4a69ad21b960),
    ("announce_storage_full.wav", 58924, 0xe9a46c3919a5566a),
    ("announce_unit_lost.wav", 54828, 0xa9dd7c617a850073),
    ("announce_structure_lost.wav", 67116, 0x8f3f444ee696fdc4),
    ("announce_new_options.wav", 72748, 0xd6ccad345206ab8b),
    ("announce_cannot_deploy.wav", 62508, 0xf5ac3e7482fc889b),
    ("announce_victory.wav", 45100, 0xbde76710d982a6f9),
    ("announce_defeat.wav", 41004, 0x328bef6f01fb8b06),
    ("announce_structure_sold.wav", 61996, 0x9078242223f0d472),
    ("announce_repairing.wav", 45100, 0x5eb52142d6645793),
];

#[test]
fn every_announcement_has_its_committed_piper_source_and_it_is_the_pinned_one() {
    assert_eq!(ANNOUNCEMENTS.len(), 19);
    for (l, (file, len, hash)) in ANNOUNCEMENTS.iter().zip(ANNOUNCE_SOURCES) {
        assert_eq!(l.file, file);
        let b = std::fs::read(source_dir().join(file))
            .unwrap_or_else(|e| panic!("{file}: no source ({e})"));
        assert_eq!(b.len(), len, "{file} length");
        assert_eq!(fnv1a(&b), hash, "{file}: the Piper source changed");
        let s = read_wav(&b).unwrap();
        assert!(s.iter().any(|v| v.abs() > 5_000), "{file} is silent");
    }
}

/// The outputs are pinned and equal a fresh run and the committed files; the filter is the one of the unit
/// voices, unchanged, which the nine pins of `the_same_source_gives_the_same_bytes_and_every_file_is_pinned`
/// prove (they did not move).
#[test]
fn the_announcements_are_pinned_equal_a_fresh_run_and_the_unit_voices_are_still_first() {
    let first = files();
    assert_eq!(first, files(), "two runs differ");
    assert_eq!(first.len(), LINES.len() + ANNOUNCEMENTS.len());
    for ((name, bytes), (pin, len, hash)) in first[LINES.len()..].iter().zip(ANNOUNCE_PINNED) {
        assert_eq!(name, pin);
        assert_eq!(bytes.len(), len, "{name} length");
        assert_eq!(
            fnv1a(bytes),
            hash,
            "{name} hash (the filter or a source changed?)"
        );
        let on_disk = std::fs::read(voice_dir().join(name))
            .unwrap_or_else(|e| panic!("{name} is not committed: {e}"));
        assert!(
            on_disk == *bytes,
            "{name} is out of date: run `cargo run -p sf_voice_gen`"
        );
    }
    // The nine unit voices come first and keep their names.
    let names: Vec<&str> = first.iter().map(|(n, _)| n.as_str()).take(9).collect();
    let want: Vec<&str> = LINES.iter().map(|l| l.file).collect();
    assert_eq!(names, want);
}

/// Every announcement is a usable file: mono, unclipped, the same peak, no longer than the unit voices' 1.5 s
/// (the longest is 1.31 s), no click at the end.
#[test]
fn every_announcement_is_an_unclipped_mono_wav_of_at_most_1_5_s_with_the_same_peak() {
    let mut longest = 0;
    for l in &ANNOUNCEMENTS {
        let s = read_wav(&l.bytes()).unwrap();
        let ms = s.len() as u64 * 1000 / u64::from(SAMPLE_RATE);
        longest = longest.max(ms);
        assert!((400..=1500).contains(&ms), "{}: {ms} ms", l.file);
        let src: Vec<i64> = l.source().iter().map(|v| i64::from(*v)).collect();
        let extra = u64::from(SAMPLE_RATE) * u64::from(OPEN_MS + TAIL_MS) / 1000;
        assert_eq!(
            s.len() as u64,
            trim(&src).len() as u64 + extra,
            "{}",
            l.file
        );
        let peak = s.iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert_eq!(peak, FILE_PEAK, "{} peak", l.file);
        assert!(
            s.iter().all(|v| *v != i16::MAX && *v != i16::MIN),
            "{} clips",
            l.file
        );
        assert!(s.last().unwrap().abs() < 50, "{} ends with a click", l.file);
    }
    assert_eq!(longest, 1310, "the longest announcement");
}

/// The event names and the words of the 19 announcements, as literals, and the render script has the same
/// file names and words (with a full stop).
#[test]
fn the_announcements_have_the_agreed_events_and_words() {
    let want: [(&str, &str); 19] = [
        ("announce.structure_ready", "Structure ready"),
        ("announce.unit_ready", "Unit ready"),
        ("announce.building", "Building"),
        ("announce.training", "Training"),
        ("announce.on_hold", "On hold"),
        ("announce.cancelled", "Cancelled"),
        ("announce.base_under_attack", "Base under attack"),
        ("announce.harvester_under_attack", "Harvester under attack"),
        ("announce.low_power", "Power low"),
        ("announce.insufficient_funds", "Not enough credits"),
        ("announce.storage_full", "Storage full"),
        ("announce.unit_lost", "Unit lost"),
        ("announce.structure_lost", "Structure lost"),
        ("announce.new_options", "New options available"),
        ("announce.cannot_deploy", "Cannot build there"),
        ("announce.victory", "Victory"),
        ("announce.defeat", "Defeat"),
        ("announce.structure_sold", "Structure sold"),
        ("announce.repairing", "Repairing"),
    ];
    let got: Vec<(&str, &str)> = ANNOUNCEMENTS.iter().map(|l| (l.event, l.text)).collect();
    assert_eq!(got, want);
    for l in &ANNOUNCEMENTS {
        assert_eq!(
            l.file,
            format!("{}.wav", l.event.replace("announce.", "announce_"))
        );
    }
    let script =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("piper_lines.py"))
            .unwrap();
    for l in &ANNOUNCEMENTS {
        let key = l.file.trim_end_matches(".wav");
        assert!(
            script.contains(&format!("\"{key}\": \"{}.\"", l.text)),
            "{key} not in piper_lines.py"
        );
    }
    // The hiss seeds are all different, those of the unit voices included.
    let mut seeds: Vec<u32> = LINES
        .iter()
        .chain(ANNOUNCEMENTS.iter())
        .map(|l| l.seed)
        .collect();
    seeds.sort_unstable();
    seeds.dedup();
    assert_eq!(seeds.len(), 28);
}
