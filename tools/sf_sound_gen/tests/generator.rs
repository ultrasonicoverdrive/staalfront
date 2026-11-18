//! The generator is deterministic and the committed files are its output (WP11.3b). Integer maths only, so
//! these numbers hold on every platform; a recipe change moves them on purpose and nothing else should.

use std::path::{Path, PathBuf};

use sf_sound_gen::{FILE_PEAK, SAMPLE_RATE, SOUNDS, files};

/// (file, byte length, FNV-1a 64 of the bytes), pinned as literals, sorted by file name. WP11.3c moved every
/// file (more low end, saturation, echoes) and added 12 (three variants of six events).
const PINNED: [(&str, usize, u64); 30] = [
    ("explosion_large.wav", 105884, 0xddc4330c68cabdc3),
    ("explosion_medium.wav", 66194, 0x41d509cb1f4f0a9f),
    ("explosion_small.wav", 44144, 0x7696ec08fbea6bf6),
    ("impact_bullet.wav", 11068, 0xcc6977c3f0463abd),
    ("impact_bullet_metal.wav", 11068, 0x9ee4622c9a4d0ed2),
    ("impact_bullet_ricochet.wav", 11068, 0x01ddbf4f60ac8107),
    ("impact_cannon.wav", 39734, 0x271fd30d3b0f5632),
    ("impact_demolition.wav", 66194, 0xaee503cf9273f674),
    ("impact_miss.wav", 17684, 0x00eaf28cb3ceaaee),
    ("impact_miss_far.wav", 19888, 0x3f4f2c86bcfa77bc),
    ("impact_miss_skip.wav", 19888, 0x30c0e78774fddce9),
    ("impact_precision.wav", 17684, 0xe196cc4713319717),
    ("impact_precision_deep.wav", 22094, 0x411aa90272db483e),
    ("impact_precision_slap.wav", 22094, 0xf9f4eef9bb603b4c),
    ("impact_rocket.wav", 48554, 0x304460a4e95a7057),
    ("impact_shell.wav", 33118, 0x6ff79aedda0a68b4),
    ("unit_destroyed_infantry.wav", 30914, 0xe28f1ecd60702ecf),
    (
        "unit_destroyed_infantry_grunt.wav",
        30914,
        0x6c92d2c0191cc950,
    ),
    (
        "unit_destroyed_infantry_shout.wav",
        26504,
        0x1738a7fdcf45af13,
    ),
    ("weapon_fire_bullet.wav", 19888, 0xb8b4a1e30c96d541),
    ("weapon_fire_bullet_heavy.wav", 22094, 0x0ec08f0d7cd4155a),
    ("weapon_fire_bullet_mg.wav", 22094, 0x39af21864906ec9b),
    ("weapon_fire_cannon.wav", 35324, 0x273b13ab43708403),
    ("weapon_fire_demolition.wav", 35324, 0x6a65e3cd34bd1f95),
    ("weapon_fire_demolition_drop.wav", 35324, 0x3b93b04dadcdc30c),
    ("weapon_fire_demolition_slow.wav", 44144, 0x689ff03e4efc40d0),
    ("weapon_fire_howitzer.wav", 57374, 0x76367a02cd62d965),
    ("weapon_fire_precision.wav", 26504, 0xf5d5268e466de992),
    ("weapon_fire_rocket.wav", 44144, 0x7d8a0467ab1d2583),
    ("weapon_fire_shell.wav", 28708, 0xb22851144e7e378f),
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

/// (event, [the variants after its first file]) of the six sounds WP11.3c redesigned, as literals: the bank entry
/// of each lists exactly these as `variants` (the first file of the event is its `file`).
const VARIANTS: [(&str, [&str; 2]); 6] = [
    (
        "unit.destroyed.infantry",
        [
            "unit_destroyed_infantry_shout.wav",
            "unit_destroyed_infantry_grunt.wav",
        ],
    ),
    (
        "weapon.fire.demolition",
        [
            "weapon_fire_demolition_drop.wav",
            "weapon_fire_demolition_slow.wav",
        ],
    ),
    (
        "impact.bullet",
        ["impact_bullet_metal.wav", "impact_bullet_ricochet.wav"],
    ),
    (
        "impact.precision",
        ["impact_precision_deep.wav", "impact_precision_slap.wav"],
    ),
    (
        "impact.miss",
        ["impact_miss_far.wav", "impact_miss_skip.wav"],
    ),
    (
        "weapon.fire.bullet",
        ["weapon_fire_bullet_mg.wav", "weapon_fire_bullet_heavy.wav"],
    ),
];

fn bank_text() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio/bank.ron"),
    )
    .unwrap()
}

/// The text of the bank entry of `event`, from its head to the closing `),` at the entry's own indent.
fn bank_entry(bank: &str, event: &str) -> String {
    let head = format!("\"{event}\": (");
    let at = bank
        .find(&head)
        .unwrap_or_else(|| panic!("no bank entry for {event}"));
    let rest = &bank[at..];
    let end = rest.find("\n        ),").expect("entry end");
    rest[..end].to_string()
}

#[test]
fn every_sound_has_its_bank_entry_with_its_file_and_its_variants() {
    let bank = bank_text();
    // Every spec of an event is in its entry: the first as `file`, the rest as `variants`, in SOUNDS order.
    let mut events: Vec<&str> = SOUNDS.iter().map(|s| s.event).collect();
    events.dedup();
    assert_eq!(events.len(), 18, "18 events (the variants share theirs)");
    for event in events {
        let files: Vec<&str> = SOUNDS
            .iter()
            .filter(|s| s.event == event)
            .map(|s| s.file)
            .collect();
        let entry = bank_entry(&bank, event);
        assert!(
            entry.contains(&format!("file: \"audio/sfx/{}\"", files[0])),
            "{event} does not point at {}",
            files[0]
        );
        let listed: Vec<String> = match entry.find("variants: [") {
            None => Vec::new(),
            Some(at) => {
                let list = &entry[at + "variants: [".len()..];
                list[..list.find(']').unwrap()]
                    .split(',')
                    .map(|v| {
                        v.trim()
                            .trim_matches('"')
                            .trim_start_matches("audio/sfx/")
                            .to_string()
                    })
                    .filter(|v| !v.is_empty())
                    .collect()
            }
        };
        assert_eq!(listed, files[1..], "{event}: variants of the bank entry");
    }
}

#[test]
fn the_six_redesigned_sounds_have_exactly_these_variants_and_nothing_else_has_any() {
    for (event, variants) in VARIANTS {
        let from_specs: Vec<&str> = SOUNDS
            .iter()
            .filter(|s| s.event == event)
            .map(|s| s.file)
            .collect();
        assert_eq!(from_specs.len(), 3, "{event}: three files");
        assert_eq!(from_specs[1..], variants, "{event}");
    }
    let with_variants = SOUNDS
        .iter()
        .filter(|s| SOUNDS.iter().filter(|o| o.event == s.event).count() > 1)
        .map(|s| s.event)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(with_variants.len(), 6, "{with_variants:?}");
    // The three files of an event differ (a variant that is a copy would be no variant).
    for (event, _) in VARIANTS {
        let hashes: std::collections::BTreeSet<u64> = SOUNDS
            .iter()
            .filter(|s| s.event == event)
            .map(|s| fnv1a(&s.bytes()))
            .collect();
        assert_eq!(hashes.len(), 3, "{event}");
    }
}

fn samples_of(bytes: &[u8]) -> Vec<i32> {
    bytes[44..]
        .chunks(2)
        .map(|c| i32::from(i16::from_le_bytes([c[0], c[1]])))
        .collect()
}

fn rms(x: &[i32]) -> f64 {
    (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// RMS of the part of `x` below about 250 Hz (a one-pole low pass, k = 20 of 256 at 22.05 kHz).
fn low_rms(x: &[i32]) -> f64 {
    let mut y = 0.0f64;
    let low: Vec<i32> = x
        .iter()
        .map(|v| {
            y += (f64::from(*v) - y) * 20.0 / 256.0;
            y as i32
        })
        .collect();
    rms(&low)
}

/// WP11.3c "more power": the files have the same peak, so the weight is in the RMS and the low end. Before the
/// WP the RMS of the 18 files was 2,644 to 5,737 (11 of them under 4,300) and the low-end RMS of a rifle,
/// a bullet impact and a miss 1,615, 346 and 778; now every file is at 6,000 or more and has 4,000 or more
/// below 250 Hz (measured: lowest 6,569 and 4,380). A recipe without its sub thump or its soft saturation
/// falls under these (mutations in the PR).
#[test]
fn every_file_has_power_rms_and_low_end() {
    for s in &SOUNDS {
        let x = samples_of(&s.bytes());
        let (r, l) = (rms(&x), low_rms(&x));
        assert!(r >= 6_000.0, "{}: rms {r:.0}", s.file);
        assert!(l >= 4_000.0, "{}: low-end rms {l:.0}", s.file);
    }
}

/// Peak-to-mean of the power spectrum over 800 Hz to 4 kHz in 25 Hz steps, in the first 120 ms (a Hann window):
/// a ping (a lone sine) is far above a noise-like crack. Measured with the pre-WP11.3c `impact_precision`
/// (ping of 1.1 kHz falling): 29.2; the three new files 11.5, 6.9 and 6.3.
fn spectral_peakiness(x: &[i32]) -> f64 {
    let n = x.len().min(ms_to_samples(120));
    let w: Vec<f64> = (0..n)
        .map(|i| {
            f64::from(x[i])
                * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos())
        })
        .collect();
    let mut power = Vec::new();
    let mut f = 800.0;
    while f <= 4_000.0 {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, v) in w.iter().enumerate() {
            let a = 2.0 * std::f64::consts::PI * f * i as f64 / f64::from(SAMPLE_RATE);
            re += v * a.cos();
            im -= v * a.sin();
        }
        power.push(re * re + im * im);
        f += 25.0;
    }
    let mean = power.iter().sum::<f64>() / power.len() as f64;
    power.iter().cloned().fold(0.0, f64::max) / mean
}

fn ms_to_samples(ms: u32) -> usize {
    sf_sound_gen::ms(ms)
}

/// Tijmen, 16-11-2026: "impact precision moet anders". The new one is a crack and a thud with no ping: no narrow
/// peak in the mid and high spectrum.
#[test]
fn impact_precision_has_no_ping_in_any_variant() {
    for s in SOUNDS.iter().filter(|s| s.event == "impact.precision") {
        let peaky = spectral_peakiness(&samples_of(&s.bytes()));
        assert!(peaky < 16.0, "{}: peak-to-mean {peaky:.1}", s.file);
    }
}

/// The cry of `unit.destroyed.infantry` is a voice-like sound: it has energy between 300 Hz and 2 kHz in its
/// first 150 ms (before the fall), which the old dull thud and rattle had far less of. Measured band RMS (first
/// 150 ms, 300 Hz to 2 kHz): old file 3,251; new 9,942, 7,536 and 6,492; the limit is 5,000.
#[test]
fn the_infantry_death_starts_with_a_cry() {
    for s in SOUNDS
        .iter()
        .filter(|s| s.event == "unit.destroyed.infantry")
    {
        let x = samples_of(&s.bytes());
        let mut head = x[..ms_to_samples(150)].to_vec();
        // Band 300 Hz to 2 kHz: a one-pole high pass at ~300 Hz then a low pass at ~2 kHz.
        let (mut lo, mut y) = (0.0f64, 0.0f64);
        for v in head.iter_mut() {
            lo += (f64::from(*v) - lo) * 20.0 / 256.0;
            let hp = f64::from(*v) - lo;
            y += (hp - y) * 150.0 / 256.0;
            *v = y as i32;
        }
        let band = rms(&head);
        assert!(band >= 5_000.0, "{}: band rms {band:.0}", s.file);
    }
}
