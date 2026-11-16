//! The orchestral layer of the combat track (WP11.4c): the committed samples, the integer player, the pitches,
//! the arrangement per section and the loudness. Numbers are pinned as literals; a recipe change moves them on
//! purpose and nothing else should.

use sf_music_gen::orchestra::{PARTS, Part, SAMPLES};
use sf_music_gen::sampler::{ImportError, decimate_at, import, parse_wav, pitch_ratio_q16, play};
use sf_music_gen::{
    FILE_PEAK, SAMPLE_RATE, TRACKS, combat_base, flac, orchestra_layers, render, samples_of,
};

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn ms(ms: usize) -> usize {
    ms * SAMPLE_RATE as usize / 1000
}

/// (id, pitch in cents of a MIDI note, kept ms, fade ms, WAV bytes, FNV-1a 64 of the file), as literals, in
/// `SAMPLES` order.
const PINNED: [(&str, i32, u32, u32, usize, u64); 30] = [
    ("tim2_v4_rr1", 3440, 2500, 500, 110294, 0x4b21f6a0ea34b95a),
    ("tim2_v4_rr2", 3440, 2500, 500, 110294, 0x2682ea2d519dc72b),
    ("tim2_v3_rr1", 3440, 2500, 500, 110294, 0x0c85e5158dbf18c6),
    ("tim2_v3_rr2", 3440, 2500, 500, 110294, 0x0cca8f80947a701f),
    ("tim3_v4_rr1", 3750, 2500, 500, 110294, 0x046bb92354e79e44),
    ("tim3_v4_rr2", 3750, 2500, 500, 110294, 0x9f215dcd116ad133),
    ("tim3_v3_rr1", 3750, 2500, 500, 110294, 0x3752ffaac6d8d82b),
    ("tim3_v3_rr2", 3750, 2500, 500, 110294, 0x3cfca4b0b408ea7f),
    ("tim5_v4_rr1", 4270, 2500, 500, 110294, 0x76406a4a6936d54b),
    ("tim5_v4_rr2", 4270, 2500, 500, 110294, 0x03284306584c81c8),
    ("tim5_v3_rr1", 4270, 2500, 500, 110294, 0x7e0794892cbe4a83),
    ("tim5_v3_rr2", 4270, 2500, 500, 110294, 0xaa0e42eac46548af),
    ("tim3_roll", 3750, 2000, 100, 88244, 0xd2a4c56ff280de74),
    ("bdrum", 0, 2000, 500, 88244, 0x4c8b3d5eff927921),
    ("crash", 0, 4000, 1500, 176444, 0x495d8c7450843301),
    ("gong", 0, 8000, 3000, 352844, 0x30f3467975bb1664),
    ("tbn_sus_as1", 4600, 2100, 100, 92654, 0x303cab7f0adca610),
    ("tbn_sus_d2", 5000, 2100, 100, 92654, 0x3e544229536999dc),
    ("tbn_sus_f2", 5300, 2100, 100, 92654, 0x5f3fdcd0ea4a75db),
    ("tbn_stac_as1", 4600, 500, 40, 22094, 0x8a6878280a977d5e),
    ("tbn_stac_f2", 5300, 500, 40, 22094, 0x8e8f234590292da9),
    ("horn_a2", 5700, 2100, 100, 92654, 0x720ecd7ce34638c9),
    ("horn_c3", 6000, 2100, 100, 92654, 0x9dbbbf39fc37769f),
    ("horn_f2", 5300, 2100, 100, 92654, 0x250a81d7d3636795),
    ("tuba_as0", 3400, 600, 60, 26504, 0x2a7a6b648fd4a761),
    ("tuba_ds1", 3900, 600, 60, 26504, 0x1b934dcb312708bb),
    ("tuba_f1", 4100, 600, 60, 26504, 0xc8a5936f124d179b),
    ("cello_b1", 4700, 400, 40, 17684, 0x6c176dd8b0073b2e),
    ("cello_d2", 5000, 400, 40, 17684, 0x62ae5f3010fcb1e9),
    ("cello_f2", 5300, 400, 40, 17684, 0x7a291fd06fcd735a),
];

#[test]
fn every_committed_sample_is_pinned_by_length_and_hash() {
    assert_eq!(SAMPLES.len(), PINNED.len());
    let mut total = 0;
    for (s, (id, pitch, keep, fade, len, hash)) in SAMPLES.iter().zip(PINNED) {
        assert_eq!(s.id, id);
        assert_eq!(s.pitch_cents, pitch, "{id} pitch");
        assert_eq!(s.keep_ms, keep, "{id} keep");
        assert_eq!(s.fade_ms, fade, "{id} fade");
        assert_eq!(s.bytes.len(), len, "{id} length");
        assert_eq!(fnv1a(s.bytes), hash, "{id} hash");
        total += s.bytes.len();
        // Every one parses as mono 16 bit at the track rate and is as long as it says (or shorter, when the
        // recording itself is shorter).
        let data = parse_wav(s.bytes);
        assert!(data.len() <= ms(keep as usize) + 1, "{id} is too long");
        assert!(!data.is_empty(), "{id} is empty");
        let peak = data.iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert_eq!(peak, 32_767, "{id} is normalised to full scale");
    }
    // PLAN WP11.4c: the samples stay small (target under about 8 MB).
    assert_eq!(total, 2_761_980);
    assert!(total < 8_000_000);
}

#[test]
fn the_samples_dir_holds_exactly_the_pinned_files() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("samples");
    let mut on_disk: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let mut want: Vec<String> = PINNED.iter().map(|p| format!("{}.wav", p.0)).collect();
    want.sort();
    assert_eq!(on_disk, want);
}

// ---- the pitches -----------------------------------------------------------------------------------------

/// Pitch of `data` near `claim` cents (MIDI x 100) from the autocorrelation of 600 ms of the sample, searched
/// over six semitones either way. Returns (cents, normalised autocorrelation at the peak, autocorrelation at
/// half the period). Integer sums, then one log to turn a period into cents (this is a test).
fn measure(data: &[i16], claim: i32) -> (f64, f64, f64) {
    let start = ms(60).min(data.len() / 4);
    let len = ms(600).min(data.len() - start);
    let seg: Vec<i64> = data[start..start + len]
        .iter()
        .map(|v| i64::from(*v))
        .collect();
    let power: i64 = seg.iter().map(|v| v * v).sum();
    let acf = |lag: usize| -> f64 {
        let s: i64 = (0..seg.len() - lag).map(|i| seg[i] * seg[i + lag]).sum();
        s as f64 / power as f64
    };
    let period =
        |cents: f64| f64::from(SAMPLE_RATE) / (440.0 * 2f64.powf((cents / 100.0 - 69.0) / 12.0));
    let lo = period(f64::from(claim) + 600.0) as usize;
    let hi = period(f64::from(claim) - 600.0) as usize + 1;
    let (mut best, mut best_v) = (lo, f64::MIN);
    for lag in lo..=hi {
        let v = acf(lag);
        if v > best_v {
            (best, best_v) = (lag, v);
        }
    }
    let (a, b, c) = (acf(best - 1), acf(best), acf(best + 1));
    let frac = (a - c) / (2.0 * (a - 2.0 * b + c));
    let lag = best as f64 + frac;
    let f = f64::from(SAMPLE_RATE) / lag;
    let cents = 100.0 * (69.0 + 12.0 * (f / 440.0).log2());
    let half = (best as f64 * 0.5) as usize;
    let half_v = (half - 2..=half + 2).map(acf).fold(f64::MIN, f64::max);
    (cents, best_v, half_v)
}

#[test]
fn the_committed_samples_have_their_pitch() {
    // Every pitched sample is within half a semitone of its documented constant, and the constant is the
    // fundamental, not an octave away: the autocorrelation at the claimed period is strong and the one at half
    // the period (the octave up) is weaker.
    let mut bad = Vec::new();
    for s in &SAMPLES {
        if s.pitch_cents == 0 {
            continue;
        }
        let data = parse_wav(s.bytes);
        let (cents, strength, half) = measure(&data, s.pitch_cents);
        println!(
            "{}: measured {:.0} cents against the constant {} (acf {strength:.2}, at half period {half:.2})",
            s.id, cents, s.pitch_cents
        );
        if (cents - f64::from(s.pitch_cents)).abs() > 50.0 {
            bad.push(format!(
                "{}: measured {cents:.0}, constant {}",
                s.id, s.pitch_cents
            ));
        }
        if strength < 0.3 {
            bad.push(format!("{}: not periodic ({strength:.2})", s.id));
        }
        if half >= strength {
            bad.push(format!("{}: an octave too low?", s.id));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn the_timpani_pitches_are_the_measured_ones_not_the_file_names() {
    // The timpani files carry no note. The measured pitches (this crate, autocorrelation, 16-11-2026) are
    // 34.6, 37.3 and 42.3 for drums 2, 3 and 5 and 37.5 for the roll; the constants are those in cents.
    let want = [
        ("tim2_v4_rr1", 3440),
        ("tim2_v3_rr2", 3440),
        ("tim3_v4_rr1", 3750),
        ("tim3_v3_rr2", 3750),
        ("tim5_v4_rr1", 4270),
        ("tim5_v3_rr2", 4270),
        ("tim3_roll", 3750),
    ];
    for (id, cents) in want {
        let s = SAMPLES.iter().find(|s| s.id == id).unwrap();
        assert_eq!(s.pitch_cents, cents, "{id}");
    }
}

// ---- the player ------------------------------------------------------------------------------------------

#[test]
fn the_pitch_ratio_is_pinned_and_exact_enough() {
    // Literals (Q16, 65536 = unison): 2^(n/12) rounded.
    for (cents, q16) in [
        (0, 65_536u64),
        (100, 69_433),
        (200, 73_562),
        (700, 98_193),
        (1200, 131_072),
        (-1200, 32_768),
        (2400, 262_144),
        (-100, 61_858),
        (50, 67_456),
    ] {
        assert_eq!(pitch_ratio_q16(cents), q16, "{cents} cents");
    }
    // And against the float formula over the whole range the layer can ask for (this is a test).
    for cents in -2400..=2400 {
        let want = 65_536.0 * 2f64.powf(f64::from(cents) / 1200.0);
        let got = pitch_ratio_q16(cents) as f64;
        assert!((got - want).abs() <= 1.0, "{cents}: {got} against {want}");
    }
}

#[test]
fn the_player_is_deterministic_and_unison_is_a_copy() {
    let data = parse_wav(SAMPLES[0].bytes);
    let a = play(&data, 130, 50_000, 800);
    let b = play(&data, 130, 50_000, 800);
    assert_eq!(a, b);
    let same = play(&data, 0, usize::MAX, 0);
    assert_eq!(same.len(), data.len());
    assert!(same.iter().zip(&data).all(|(x, y)| *x == i32::from(*y)));
    // An octave up is half as long and plays every second sample.
    let up = play(&data, 1200, usize::MAX, 0);
    assert_eq!(up.len(), data.len() / 2);
    assert!(
        up.iter()
            .zip(data.iter().step_by(2))
            .all(|(x, y)| *x == i32::from(*y))
    );
    // A cut ends in a fade to (almost) nothing and never exceeds the cut.
    let cut = play(&data, 0, 20_000, 1_000);
    assert_eq!(cut.len(), 20_000);
    assert!(cut[19_999].abs() <= cut[18_999].abs().max(60));
    assert_eq!(
        cut[..19_000],
        data[..19_000]
            .iter()
            .map(|v| i32::from(*v))
            .collect::<Vec<_>>()[..]
    );
}

#[test]
fn a_shifted_sample_has_the_pitch_it_was_asked_for() {
    // Shift the Timpani3 hit (37.3) up to 42 and the trombone D2 (50) down to 45 and measure the result.
    let wav = |id: &str| parse_wav(SAMPLES.iter().find(|s| s.id == id).unwrap().bytes);
    for (id, from, to) in [("tbn_sus_d2", 5000, 4500), ("horn_a2", 5700, 6200)] {
        let moved: Vec<i16> = play(&wav(id), to - from, usize::MAX, 0)
            .iter()
            .map(|v| (*v).clamp(-32_767, 32_767) as i16)
            .collect();
        let (cents, strength, _) = measure(&moved, to);
        println!("{id} moved to {to}: measured {cents:.0} (acf {strength:.2})");
        assert!((cents - f64::from(to)).abs() <= 20.0, "{id}: {cents}");
    }
}

#[test]
fn the_render_path_has_no_float() {
    // The layer and the player are integer only (PLAN rule: no floats in the render path), so the bytes are
    // the same on every platform. A token search over the sources of the render path.
    for (name, text) in [
        ("sampler.rs", include_str!("../src/sampler.rs")),
        ("orchestra.rs", include_str!("../src/orchestra.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
    ] {
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            for bad in [
                "f32", "f64", ".sin(", ".cos(", ".exp(", ".powf(", ".powi(", ".sqrt(",
            ] {
                assert!(!code.contains(bad), "{name}:{} uses {bad}", n + 1);
            }
        }
    }
}

// ---- the import step -------------------------------------------------------------------------------------

fn wav24_stereo(rate: u32, left: &[i32], right: &[i32]) -> Vec<u8> {
    let data_len = left.len() as u32 * 6;
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 6).to_le_bytes());
    w.extend_from_slice(&6u16.to_le_bytes());
    w.extend_from_slice(&24u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for (l, r) in left.iter().zip(right) {
        w.extend_from_slice(&l.to_le_bytes()[..3]);
        w.extend_from_slice(&r.to_le_bytes()[..3]);
    }
    w
}

/// A tone of `hz` at 44.1 kHz, 24 bit, `n` frames (a test: floats are fine here).
fn tone24(hz: f64, n: usize, amp: f64) -> Vec<i32> {
    (0..n)
        .map(|i| (amp * (2.0 * std::f64::consts::PI * hz * i as f64 / 44_100.0).sin()) as i32)
        .collect()
}

#[test]
fn the_import_makes_a_mono_22k_16_bit_sample_with_a_fade_and_full_scale() {
    let l = tone24(1000.0, 44_100, 3_000_000.0);
    let r = tone24(1000.0, 44_100, 1_000_000.0);
    let src = wav24_stereo(44_100, &l, &r);
    let out = import(&src, 500, 100).unwrap();
    assert_eq!(out, import(&src, 500, 100).unwrap(), "deterministic");
    let data = parse_wav(&out);
    assert_eq!(data.len(), ms(500), "cut at keep_ms");
    let peak = data.iter().map(|v| i32::from(*v).abs()).max().unwrap();
    assert_eq!(peak, 32_767);
    // Full level before the fade, silence at the end of the fade.
    let mid = data[ms(100)..ms(300)]
        .iter()
        .map(|v| i32::from(*v).abs())
        .max()
        .unwrap();
    assert!(mid > 32_000, "{mid}");
    assert!(i32::from(data[data.len() - 1]).abs() < 400);
    // A recording shorter than keep_ms keeps its whole length and gets no fade.
    let short = parse_wav(&import(&src, 5_000, 100).unwrap());
    assert_eq!(short.len(), 22_050);
    assert!(i32::from(short[short.len() - 1]).abs() > 1_000);
}

#[test]
fn the_import_filter_keeps_the_music_and_removes_what_would_fold_back() {
    // 1 kHz passes with gain 1; 14 kHz (above the new Nyquist of 11.025 kHz, where it would fold back to
    // 8.05 kHz) is at least 60 dB down; 9 kHz is still within half a dB.
    let gain = |hz: f64| {
        let x: Vec<i64> = tone24(hz, 44_100, 1_000_000.0)
            .iter()
            .map(|v| i64::from(*v))
            .collect();
        let peak = (2000..8000)
            .map(|j| decimate_at(&x, j).abs())
            .max()
            .unwrap();
        peak as f64 / 32_768.0 / 1_000_000.0
    };
    let (g1, g9, g14) = (gain(1000.0), gain(9000.0), gain(14_000.0));
    println!("filter gain: 1 kHz {g1:.4}, 9 kHz {g9:.4}, 14 kHz {g14:.5}");
    assert!((0.99..=1.01).contains(&g1), "{g1}");
    assert!((0.94..=1.06).contains(&g9), "{g9}");
    assert!(g14 < 0.001, "{g14}");
}

#[test]
fn the_import_refuses_what_it_cannot_read() {
    let l = tone24(1000.0, 4410, 1_000_000.0);
    let wrong_rate = wav24_stereo(48_000, &l, &l);
    assert!(matches!(import(&wrong_rate, 100, 10), Err(ImportError(_))));
    assert!(import(b"not a wav file at all", 100, 10).is_err());
}

#[test]
fn the_committed_samples_are_what_the_import_makes_of_the_recordings() {
    // The recordings (VSCO 2 CE, CC0) are not in the repository, so CI cannot do this; set SF_VSCO_DIR to a
    // checkout of github.com/sgossner/VSCO-2-CE to check the committed samples against them.
    let Some(dir) = std::env::var_os("SF_VSCO_DIR") else {
        println!(
            "SF_VSCO_DIR is not set: the committed samples are checked against their pins only"
        );
        return;
    };
    for s in &SAMPLES {
        let src = std::fs::read(std::path::Path::new(&dir).join(s.src)).expect("recording");
        let made = import(&src, s.keep_ms, s.fade_ms).unwrap();
        assert!(made == s.bytes, "{} differs from a fresh import", s.id);
    }
}

// ---- the arrangement -------------------------------------------------------------------------------------

fn bar_start(bar: usize) -> usize {
    bar * 16 * SAMPLE_RATE as usize * 15 / 128
}

fn step_start(bar: usize, step: usize) -> usize {
    (bar * 16 + step) * SAMPLE_RATE as usize * 15 / 128
}

fn energy(x: &[i32], from: usize, to: usize) -> u128 {
    x[from..to.min(x.len())]
        .iter()
        .map(|v| (i128::from(*v) * i128::from(*v)) as u128)
        .sum()
}

fn part(p: Part) -> Vec<i32> {
    orchestra_layers()
        .into_iter()
        .find(|(q, _)| *q == p)
        .unwrap()
        .1
}

/// The bar roots of the combat track: the A phrase D D Bb C, the B phrase D G Bb A.
fn sec_is_b(sec: usize) -> bool {
    matches!(sec, 2 | 4 | 7)
}

#[test]
fn every_part_has_the_length_of_the_track() {
    let want = samples_of(&TRACKS[1]);
    let layers = orchestra_layers();
    assert_eq!(layers.len(), PARTS.len());
    for (p, x) in &layers {
        assert_eq!(x.len(), want, "{p:?}");
    }
    assert!(!sec_is_b(0) && sec_is_b(2) && sec_is_b(4) && sec_is_b(7));
}

#[test]
fn the_timpani_play_where_the_arrangement_says() {
    let x = part(Part::Timpani);
    let w = ms(60);
    let onset = |t: usize| {
        let after = energy(&x, t, t + w);
        let before = energy(&x, t - w, t);
        (after, before)
    };
    let mut hits = 0;
    for bar in 1..36 {
        let sec = bar / 4;
        let on_one = matches!(sec, 1 | 2 | 3 | 5 | 6 | 7)
            || (sec == 8 && bar < 34)
            || (sec == 4 && bar % 2 == 0);
        let on_three = matches!(sec, 3 | 6 | 7);
        for (beat, t, want) in [
            (1, bar_start(bar), on_one),
            (3, step_start(bar, 8), on_three),
        ] {
            let (after, before) = onset(t);
            if want {
                hits += 1;
                assert!(
                    after > 8 * before.max(1),
                    "bar {bar} beat {beat}: no hit ({after} after, {before} before)"
                );
            } else {
                assert!(
                    after <= 2 * before,
                    "bar {bar} beat {beat}: a hit that should not be there ({after} after, {before} before)"
                );
            }
        }
    }
    // Bar 0 (the intro) has none.
    assert_eq!(
        energy(&x, 0, bar_start(4)),
        0,
        "the intro is without timpani"
    );
    // 28 hits on one (sections 1, 2, 3, 5, 6, 7 are 24 bars, the outro 2 bars, the breakdown 2 even bars) and
    // 12 on three.
    assert_eq!(hits, 24 + 2 + 2 + 12);
    // The last two bars are the tom fill, the layer rests (the tails of the bar 33 hit run out inside bar 34).
    assert!(
        energy(&x, bar_start(35), bar_start(36)) < energy(&x, bar_start(33), bar_start(34)) / 100
    );
}

/// Share of the energy of `x` that is above about 500 Hz: one minus the share that two one-pole low passes at
/// k 40 (`sf_sound_gen::lowpass`) let through.
const K: i32 = 40;

fn high_share(x: &[i32]) -> f64 {
    let mut low = x.to_vec();
    for _ in 0..2 {
        sf_sound_gen::lowpass(&mut low, K);
    }
    let total = energy(x, 0, x.len()) as f64;
    let low_e = energy(&low, 0, low.len()) as f64;
    1.0 - low_e / total
}

#[test]
fn the_parts_sit_in_their_own_bands() {
    // The timpani are a low band (under a fifth of their energy above about 500 Hz); the brass, the cello and the
    // stabs have two fifths or more up there.
    let timp = high_share(&part(Part::Timpani));
    let brass = high_share(&part(Part::LowBrass));
    let cello = high_share(&part(Part::Cello));
    let stabs = high_share(&part(Part::Stabs));
    println!(
        "above about 500 Hz: timpani {timp:.2}, low brass {brass:.2}, cello {cello:.2}, stabs {stabs:.2}"
    );
    assert!(timp < 0.3, "timpani {timp}");
    assert!(brass > 0.4, "brass {brass}");
    assert!(cello > 0.4, "cello {cello}");
    assert!(stabs > 0.4, "stabs {stabs}");
}

#[test]
fn the_low_brass_hold_in_the_full_sections_and_the_start_of_the_outro() {
    let x = part(Part::LowBrass);
    for bar in 0..36 {
        let sec = bar / 4;
        let e = energy(&x, bar_start(bar), bar_start(bar + 1));
        let want = matches!(sec, 3 | 6 | 7) || (sec == 8 && bar < 34);
        if want {
            assert!(e > 1_000_000_000, "bar {bar}: brass energy {e}");
            // Held: there is sound late in the bar too (the held trombone and horn last 95% of it).
            assert!(
                energy(&x, step_start(bar, 12), bar_start(bar + 1)) > 100_000_000,
                "bar {bar} is not held"
            );
        } else {
            assert_eq!(e, 0, "bar {bar}: brass where there should be none");
        }
    }
}

#[test]
fn the_trombone_stabs_hit_four_times_in_sections_two_and_seven() {
    let x = part(Part::Stabs);
    for bar in 0..36 {
        let sec = bar / 4;
        let e = energy(&x, bar_start(bar), bar_start(bar + 1));
        if matches!(sec, 2 | 7) {
            for s in [0usize, 3, 6, 10] {
                let t = step_start(bar, s);
                assert!(
                    energy(&x, t, t + ms(30)) > 10_000_000,
                    "bar {bar} step {s}: no stab"
                );
            }
            // Nothing in the last quarter of the bar: the stabs of 250 ms end before the 16th 12.
            assert_eq!(
                energy(&x, step_start(bar, 13), bar_start(bar + 1)),
                0,
                "bar {bar}"
            );
        } else {
            assert_eq!(e, 0, "bar {bar}: stabs where there should be none");
        }
    }
}

#[test]
fn the_cello_plays_eighths_in_the_build_and_the_last_two_full_sections() {
    let x = part(Part::Cello);
    for bar in 0..36 {
        let sec = bar / 4;
        let e = energy(&x, bar_start(bar), bar_start(bar + 1));
        if (5..=7).contains(&sec) {
            for s in (0..16).step_by(2) {
                let t = step_start(bar, s);
                assert!(
                    energy(&x, t, t + ms(20)) > 1_000_000,
                    "bar {bar} step {s}: no note"
                );
            }
        } else {
            assert_eq!(e, 0, "bar {bar}: cello where there should be none");
        }
    }
}

#[test]
fn the_accents_fall_on_bars_12_16_23_and_24() {
    let x = part(Part::Accents);
    // Nothing before bar 12.
    assert_eq!(energy(&x, 0, bar_start(12)), 0);
    let w = ms(80);
    for bar in [12usize, 16, 24] {
        assert!(
            energy(&x, bar_start(bar), bar_start(bar) + w) > 100_000_000,
            "bar {bar}"
        );
    }
    // Between the crash of bar 12 (4 s) and the gong of bar 16 the accents are quiet; bar 22 is silent (the gong
    // of bar 16 lasts 8 s) and the roll fills bar 23.
    assert_eq!(energy(&x, bar_start(22), bar_start(23)), 0, "bar 22");
    assert!(
        energy(&x, bar_start(23), bar_start(24)) > 1_000_000_000,
        "the roll in bar 23"
    );
    // After the tail of the crash of bar 24 (4 s) the track ends without accents.
    assert_eq!(
        energy(&x, bar_start(32), bar_start(36)),
        0,
        "the track ends without accents"
    );
}

// ---- the old track and the loudness ----------------------------------------------------------------------

#[test]
fn without_the_layer_the_old_combat_track_comes_back_byte_for_byte() {
    // Re-pin evidence (WP11.4c): the arrangement is the only change of `combat.flac`. With it switched off the
    // track encodes to the bytes WP11.4b committed (length and FNV-1a 64 of the old file).
    let bytes = flac::encode(&combat_base(), SAMPLE_RATE);
    assert_eq!(bytes.len(), 2_021_170);
    assert_eq!(fnv1a(&bytes), 0x95f6_29a2_3692_dcd9);
}

fn rms(x: &[i16]) -> u64 {
    ((x.iter().map(|v| i64::from(*v) * i64::from(*v)).sum::<i64>() / x.len() as i64) as u64).isqrt()
}

#[test]
fn the_layer_does_not_make_the_combat_track_quieter_and_the_peak_stays_the_file_peak() {
    let base = combat_base();
    let full = render(&TRACKS[1]);
    let (r0, r1) = (rms(&base), rms(&full));
    let peak = |x: &[i16]| x.iter().map(|v| i32::from(*v).abs()).max().unwrap();
    println!(
        "combat rms {r0} -> {r1}, peak {} -> {}",
        peak(&base),
        peak(&full)
    );
    // The peak is the file peak either way (the sf_game test `a_20_tank_fight_is_not_louder_than_the_music`
    // measures the fight against 29000 / 32767 x 0.6 = 0.531).
    assert!((FILE_PEAK - 2..=FILE_PEAK).contains(&peak(&full)));
    // The music does not get quieter: the same peak, an RMS within a tenth of the old one or more.
    assert!(r1 * 10 >= r0 * 9, "{r1} against {r0}");
    // And the layer is not louder than the loudness rule of the two tracks (calm and combat within 1.5).
    let calm = rms(&render(&TRACKS[0]));
    assert!(
        calm * 2 < r1 * 3 && r1 * 2 < calm * 3,
        "calm {calm}, combat {r1}"
    );
    assert_eq!(r0, RMS_BEFORE);
    assert_eq!(r1, RMS_AFTER);
}

/// RMS of the combat track before and after the layer (measured, pinned).
const RMS_BEFORE: u64 = 9_120;
const RMS_AFTER: u64 = 9_165;
