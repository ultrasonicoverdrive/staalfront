//! The generator is deterministic and the committed files are its output (WP11.4b). Integer maths only, so
//! these numbers hold on every platform; a recipe change moves them on purpose and nothing else should.

use std::path::{Path, PathBuf};

use kira::sound::static_sound::StaticSoundData;
use sf_music_gen::{BUDGET_BYTES, FILE_PEAK, SAMPLE_RATE, TRACKS, files, render, samples_of};

/// (file, samples, byte length, FNV-1a 64 of the bytes), pinned as literals, in `TRACKS` order.
const PINNED: [(&str, usize, usize, u64); 2] = [
    ("calm.flac", 1_512_000, 1_509_638, 0xe98d_0e9a_91e7_347a),
    ("combat.flac", 1_488_375, 2_050_089, 0x4814_8f0a_0cf7_bb62),
];

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn music_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/audio/music")
}

#[test]
fn the_same_run_gives_the_same_bytes_and_every_file_is_pinned() {
    let first = files();
    let second = files();
    assert_eq!(first, second, "two runs differ");
    assert_eq!(first.len(), PINNED.len());
    for ((name, bytes), (pin_name, samples, len, hash)) in first.iter().zip(PINNED) {
        assert_eq!(name, pin_name);
        let spec = TRACKS.iter().find(|t| t.file == pin_name).unwrap();
        assert_eq!(samples_of(spec), samples, "{name} samples");
        assert_eq!(bytes.len(), len, "{name} length");
        assert_eq!(fnv1a(bytes), hash, "{name} hash (a recipe changed?)");
    }
}

#[test]
fn the_committed_files_are_what_a_fresh_run_writes() {
    for (name, bytes) in files() {
        let on_disk = std::fs::read(music_dir().join(&name))
            .unwrap_or_else(|e| panic!("{name} is missing ({e}); run `cargo run -p sf_music_gen`"));
        assert!(
            on_disk == bytes,
            "{name} is out of date; run `cargo run -p sf_music_gen`"
        );
    }
    // No stray file next to them.
    let mut names: Vec<String> = std::fs::read_dir(music_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut want: Vec<String> = TRACKS.iter().map(|t| t.file.to_string()).collect();
    want.sort();
    assert_eq!(names, want);
}

#[test]
fn the_tracks_have_the_tempo_length_and_size_of_the_work_package() {
    let all = files();
    let total: usize = all.iter().map(|(_, b)| b.len()).sum();
    assert!(
        total <= BUDGET_BYTES,
        "{total} bytes is over the budget of {BUDGET_BYTES}"
    );
    assert_eq!(BUDGET_BYTES, 4_000_000);
    for t in &TRACKS {
        let secs = samples_of(t) as f64 / f64::from(SAMPLE_RATE);
        assert!((60.0..=120.0).contains(&secs), "{} is {secs} s", t.file);
    }
    assert!((80..=90).contains(&TRACKS[0].bpm), "calm is 80 to 90 BPM");
    assert!(
        (125..=135).contains(&TRACKS[1].bpm),
        "combat is 125 to 135 BPM"
    );
    assert_eq!(TRACKS[0].bank_name, "music.calm");
    assert_eq!(TRACKS[1].bank_name, "music.combat");
}

#[test]
fn nothing_clips_and_the_peak_is_the_file_peak() {
    for t in &TRACKS {
        let x = render(t);
        let peak = x.iter().map(|s| i32::from(*s).abs()).max().unwrap();
        assert!(
            (FILE_PEAK - 2..=FILE_PEAK).contains(&peak),
            "{} peak {peak}",
            t.file
        );
        assert!(peak < 32_767);
        // No flat top: the largest sample is not repeated in a row.
        let flat = x
            .windows(3)
            .filter(|w| w.iter().all(|s| i32::from(*s).abs() >= FILE_PEAK - 2))
            .count();
        assert_eq!(flat, 0, "{} has a flat top", t.file);
    }
}

/// The step between two neighbouring samples: the join of the loop must be no bigger a step than the music
/// takes inside itself (a cut-off note or a missing tail is a click, and a click is a big step).
fn steps(x: &[i16]) -> (i32, i32) {
    let join = (i32::from(x[0]) - i32::from(x[x.len() - 1])).abs();
    let mut inside: Vec<i32> = x
        .windows(2)
        .map(|w| (i32::from(w[1]) - i32::from(w[0])).abs())
        .collect();
    inside.sort_unstable();
    // The 90th percentile: the biggest steps are the clicks of drums and hats, which are meant.
    (join, inside[inside.len() * 90 / 100])
}

#[test]
fn the_loop_joins_without_a_jump() {
    for t in &TRACKS {
        let x = render(t);
        let (join, p90) = steps(&x);
        println!(
            "{}: join step {join}, 90th percentile of the steps {p90}",
            t.file
        );
        assert!(
            join <= p90,
            "{}: the last and the first sample are {join} apart, music steps of {p90} are normal",
            t.file
        );
    }
}

#[test]
fn the_ends_have_music_in_them_so_the_join_test_means_something() {
    // A track that fades to silence at its ends would pass the join test trivially. These do not: the first
    // and last second are as loud as the whole track within a factor of three.
    for t in &TRACKS {
        let x = render(t);
        let rms = |s: &[i16]| {
            (s.iter().map(|v| i64::from(*v) * i64::from(*v)).sum::<i64>() / s.len() as i64) as f64
        };
        let all = rms(&x);
        let rate = SAMPLE_RATE as usize;
        for (what, part) in [("start", &x[..rate]), ("end", &x[x.len() - rate..])] {
            let r = rms(part);
            assert!(
                r * 9.0 > all,
                "{} {what} is too quiet ({r} of {all})",
                t.file
            );
        }
    }
}

#[test]
fn both_files_have_about_the_same_loudness_so_the_bank_volumes_set_the_mood() {
    // The files are made equally loud on purpose (peak and, within a factor of 1.5, RMS); how loud each mood
    // plays is the `volume` of its entry in bank.ron (calm lower than combat).
    let rms = |x: &[i16]| {
        ((x.iter().map(|v| i64::from(*v) * i64::from(*v)).sum::<i64>() / x.len() as i64) as u64)
            .isqrt()
    };
    let calm = rms(&render(&TRACKS[0]));
    let combat = rms(&render(&TRACKS[1]));
    println!("rms calm {calm} combat {combat}");
    assert!(calm * 2 < combat * 3 && combat * 2 < calm * 3);
}

#[test]
fn the_game_decoder_reads_the_fresh_bytes_and_the_committed_files_back_exactly() {
    // The bytes of a fresh encode (this tests the encoder) and the file on disk (this tests what ships).
    for (t, (_, fresh)) in TRACKS.iter().zip(files()) {
        let want = render(t);
        let on_disk = std::fs::read(music_dir().join(t.file)).expect("committed");
        for (what, sound) in [
            (
                "fresh",
                StaticSoundData::from_cursor(std::io::Cursor::new(fresh)),
            ),
            (
                "committed",
                StaticSoundData::from_cursor(std::io::Cursor::new(on_disk)),
            ),
        ] {
            let sound =
                sound.unwrap_or_else(|e| panic!("{} ({what}) does not decode: {e}", t.file));
            assert_eq!(sound.sample_rate, SAMPLE_RATE);
            assert_eq!(sound.frames.len(), want.len(), "{} {what} length", t.file);
            for (i, (f, w)) in sound.frames.iter().zip(&want).enumerate() {
                let expected = f32::from(*w) / 32_768.0;
                assert!(
                    (f.left - expected).abs() < 1e-6 && (f.right - expected).abs() < 1e-6,
                    "{} {what} sample {i}: {} against {expected}",
                    t.file,
                    f.left
                );
            }
        }
    }
}
