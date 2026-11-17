//! The CC0 unit voices of Staalfront (WP11.2b): short English replies, spoken by the Piper voice
//! `en_US-joe-medium` (chosen by Tijmen by ear, 15-11-2026) and passed through a **walkie-talkie filter**
//! written here, so the units sound as if they speak over a field radio.
//!
//! The raw Piper renders are committed in `tools/sf_voice_gen/voice_src/` and are the pinned input; they are
//! made once, offline, by `tools/sf_voice_gen/piper_lines.py` (CI never runs Piper). This crate turns them into
//! `assets/audio/voice/*.wav`: `cargo run -p sf_voice_gen` rewrites the outputs, `-- --check` compares them
//! with a fresh run (a test does the same).
//!
//! **Integer maths only**, so the bytes are the same on every platform: the filter coefficients come from
//! cosine and exponential series summed in 128-bit fixed point ([`cos_q30`], [`exp_neg_q30`]), never from a
//! platform `sin` or `exp`.
//!
//! The chain, per line ([`render`]):
//! 1. **Trim** the silence Piper leaves before and after the words ([`trim`]).
//! 2. **Walkie-talkie band** ([`radio_band`]): two high passes at 300 Hz and two low passes at 3000 Hz
//!    (fourth order on each side, steep), and a presence bump of +6 dB at 1800 Hz.
//! 3. **Radio compressor** ([`compress`]) and **soft saturation** ([`saturate`]): dense and a little gritty;
//!    the band's low passes run once more after the saturation so the grit stays inside the band.
//! 4. A faint **hiss** in the same band under the words, a **squelch** "ksst" when the talk key opens and a
//!    short squelch **tail** when it is released.
//!
//! Every file is normalised to the same peak ([`FILE_PEAK`]); how loud the voices are in the game is the
//! `volume` of their bank entries.

use std::path::{Path, PathBuf};

use sf_sound_gen::{FILE_PEAK, Rng, SAMPLE_RATE, ms, noise, wav};

const FS: i128 = SAMPLE_RATE as i128;
/// π in Q60 (π × 2^60, rounded down).
const PI_Q60: i128 = 3_622_009_729_038_561_421;
const Q60: i128 = 1 << 60;
const Q30: i64 = 1 << 30;

/// cos(x) in Q30 for `x` in Q60 radians, 0 ≤ x ≤ 2π: a Taylor series summed in 128-bit fixed point.
pub fn cos_q30(x: i128) -> i64 {
    let x = if x > PI_Q60 { 2 * PI_Q60 - x } else { x };
    let x2 = (x * x) >> 60;
    let (mut term, mut sum) = (Q60, Q60);
    for k in 0..24i128 {
        term = -((term * x2) >> 60) / ((2 * k + 1) * (2 * k + 2));
        sum += term;
    }
    (sum >> 30) as i64
}

/// sin(x) in Q30 for `x` in Q60 radians, 0 ≤ x ≤ π.
pub fn sin_q30(x: i128) -> i64 {
    cos_q30((x - PI_Q60 / 2).abs())
}

/// e^(-x) in Q30 for a small `x` ≥ 0 in Q60 (here at most about 0.7).
pub fn exp_neg_q30(x: i128) -> i64 {
    let (mut term, mut sum) = (Q60, Q60);
    for k in 1..24i128 {
        term = -((term * x) >> 60) / k;
        sum += term;
    }
    (sum >> 30) as i64
}

/// A biquad filter, coefficients in Q30 (`a0` divided out): `y = b0 x + b1 x1 + b2 x2 - a1 y1 - a2 y2`.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b: [i64; 3],
    a: [i64; 2],
    x1: i64,
    x2: i64,
    y1: i64,
    y2: i64,
}

/// What a [`Biquad`] does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    LowPass,
    HighPass,
    /// A bell around the frequency; the gain is in thousandths (1995 = +6 dB in amplitude).
    Peak {
        gain_permille: i64,
    },
}

impl Biquad {
    /// The usual cookbook biquad at `f` Hz with quality `q_permille` / 1000, in integers.
    pub fn new(kind: Kind, f: u32, q_permille: i64) -> Biquad {
        let w = 2 * PI_Q60 * i128::from(f) / FS;
        let cos = i128::from(cos_q30(w));
        let sin = i128::from(sin_q30(w));
        let one = i128::from(Q30);
        let alpha = sin * 1000 / (2 * i128::from(q_permille));
        let (b, a0, a1, a2) = match kind {
            Kind::LowPass => {
                let b0 = (one - cos) / 2;
                ([b0, one - cos, b0], one + alpha, -2 * cos, one - alpha)
            }
            Kind::HighPass => {
                let b0 = (one + cos) / 2;
                ([b0, -(one + cos), b0], one + alpha, -2 * cos, one - alpha)
            }
            Kind::Peak { gain_permille } => {
                // The cookbook's A is the square root of the amplitude gain at the centre: in thousandths,
                // A = isqrt(gain_permille × 1000).
                let a = i128::from(isqrt(gain_permille * 1000));
                (
                    [one + alpha * a / 1000, -2 * cos, one - alpha * a / 1000],
                    one + alpha * 1000 / a,
                    -2 * cos,
                    one - alpha * 1000 / a,
                )
            }
        };
        let n = |v: i128| (v * one / a0) as i64;
        Biquad {
            b: [n(b[0]), n(b[1]), n(b[2])],
            a: [n(a1), n(a2)],
            x1: 0,
            x2: 0,
            y1: 0,
            y2: 0,
        }
    }

    pub fn step(&mut self, x: i64) -> i64 {
        let acc = i128::from(self.b[0]) * i128::from(x)
            + i128::from(self.b[1]) * i128::from(self.x1)
            + i128::from(self.b[2]) * i128::from(self.x2)
            - i128::from(self.a[0]) * i128::from(self.y1)
            - i128::from(self.a[1]) * i128::from(self.y2);
        let y = (acc >> 30) as i64;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    /// Run the filter over `x` in place.
    pub fn run(mut self, x: &mut [i64]) {
        for s in x.iter_mut() {
            *s = self.step(*s);
        }
    }
}

/// Integer square root (floor).
fn isqrt(v: i64) -> i64 {
    let (mut lo, mut hi) = (0i64, v.max(1));
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if mid.saturating_mul(mid) <= v {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// The walkie-talkie band: edges in Hz, quality of the edge filters (Butterworth, 0.707), and the presence bump.
pub const BAND_LOW_HZ: u32 = 300;
pub const BAND_HIGH_HZ: u32 = 3000;
pub const EDGE_Q_PERMILLE: i64 = 707;
pub const PRESENCE_HZ: u32 = 1800;
pub const PRESENCE_Q_PERMILLE: i64 = 1000;
/// +6 dB: an amplitude gain of 1.995.
pub const PRESENCE_GAIN_PERMILLE: i64 = 1995;

fn low_passes(x: &mut [i64]) {
    for _ in 0..2 {
        Biquad::new(Kind::LowPass, BAND_HIGH_HZ, EDGE_Q_PERMILLE).run(x);
    }
}

/// The walkie-talkie band in place: two high passes at [`BAND_LOW_HZ`], two low passes at [`BAND_HIGH_HZ`] and
/// the presence bump at [`PRESENCE_HZ`].
pub fn radio_band(x: &mut [i64]) {
    for _ in 0..2 {
        Biquad::new(Kind::HighPass, BAND_LOW_HZ, EDGE_Q_PERMILLE).run(x);
    }
    low_passes(x);
    Biquad::new(
        Kind::Peak {
            gain_permille: PRESENCE_GAIN_PERMILLE,
        },
        PRESENCE_HZ,
        PRESENCE_Q_PERMILLE,
    )
    .run(x);
}

/// Scale so the largest magnitude is `peak` (a silent buffer stays silent).
fn scale_to(x: &mut [i64], peak: i64) {
    let top = x.iter().map(|s| s.abs()).max().unwrap_or(0);
    if top == 0 {
        return;
    }
    for s in x.iter_mut() {
        *s = (i128::from(*s) * i128::from(peak) / i128::from(top)) as i64;
    }
}

/// Knee of the radio compressor in percent of the peak, and its release in milliseconds.
pub const COMPRESS_KNEE_PERCENT: i64 = 20;
pub const COMPRESS_RELEASE_MS: u32 = 40;

/// A radio's compressor, in place: an envelope that jumps up with the signal and falls back over
/// [`COMPRESS_RELEASE_MS`], and a gain `knee / (envelope + knee)`, so quiet sounds pass and loud ones are held
/// down. The words come out denser (less difference between peak and average), which is what lets them be
/// heard over a fight without a higher peak.
pub fn compress(x: &mut [i64]) {
    let top = x.iter().map(|s| s.abs()).max().unwrap_or(0).max(1);
    let knee = top * COMPRESS_KNEE_PERCENT / 100;
    let release = ms(COMPRESS_RELEASE_MS).max(1) as i64;
    let mut env = 0i64;
    for s in x.iter_mut() {
        let a = s.abs();
        env = if a > env { a } else { env - env / release };
        *s = (i128::from(*s) * i128::from(knee) / i128::from(env + knee)) as i64;
    }
}

/// Drive of the soft saturation in percent: the peak goes in at this share of the curve's knee.
pub const SATURATION_DRIVE_PERCENT: i64 = 140;

/// Soft saturation in place: the peak is driven to [`SATURATION_DRIVE_PERCENT`] of 1 and bent by
/// `1.5 u - 0.5 u³` (flat at 1 beyond), which rounds the loudest parts off and adds the grit of a small radio.
pub fn saturate(x: &mut [i64]) {
    let top = x.iter().map(|s| s.abs()).max().unwrap_or(0).max(1);
    let one = 1i64 << 15;
    for s in x.iter_mut() {
        let u = (i128::from(*s) * i128::from(one) * i128::from(SATURATION_DRIVE_PERCENT)
            / 100
            / i128::from(top)) as i64;
        let u = u.clamp(-one, one);
        let u3 = (((u * u) >> 15) * u) >> 15;
        *s = (3 * u - u3) / 2;
    }
}

/// Samples below this share of the peak (thousandths) count as silence for the trim; the margins kept before
/// and after the words, in milliseconds.
pub const TRIM_PERMILLE: i64 = 20;
pub const TRIM_LEAD_MS: u32 = 15;
pub const TRIM_TAIL_MS: u32 = 40;

/// The words without Piper's silence around them: from [`TRIM_LEAD_MS`] before the first sample above
/// [`TRIM_PERMILLE`] of the peak to [`TRIM_TAIL_MS`] after the last.
pub fn trim(x: &[i64]) -> &[i64] {
    let top = x.iter().map(|s| s.abs()).max().unwrap_or(0);
    let gate = top * TRIM_PERMILLE / 1000;
    let first = x.iter().position(|s| s.abs() > gate).unwrap_or(0);
    let last = x.iter().rposition(|s| s.abs() > gate).unwrap_or(0);
    let from = first.saturating_sub(ms(TRIM_LEAD_MS));
    let to = (last + ms(TRIM_TAIL_MS)).min(x.len());
    &x[from..to.max(from)]
}

/// Length of the squelch when the key opens and of the tail when it is released, in milliseconds; the hiss
/// and the two squelch peaks in thousandths of the file peak.
pub const OPEN_MS: u32 = 45;
pub const TAIL_MS: u32 = 70;
pub const HISS_PERMILLE: i64 = 20;
pub const OPEN_PERMILLE: i64 = 450;
pub const TAIL_PERMILLE: i64 = 300;

/// A squelch burst of `n` samples: noise in the radio band (with more top: one low pass less), a click at the
/// start and a fast decay.
fn squelch(rng: &mut Rng, n: usize) -> Vec<i64> {
    let mut x: Vec<i64> = noise(rng, n).into_iter().map(i64::from).collect();
    for _ in 0..2 {
        Biquad::new(Kind::HighPass, 900, EDGE_Q_PERMILLE).run(&mut x);
    }
    Biquad::new(Kind::LowPass, 3400, EDGE_Q_PERMILLE).run(&mut x);
    let len = n.max(1) as i64;
    for (i, s) in x.iter_mut().enumerate() {
        let left = len - i as i64;
        *s = *s * left * left / (len * len);
    }
    x
}

/// A finished line from the raw source samples: trimmed words through the walkie-talkie chain, the squelch on
/// opening, the hiss, the squelch tail; normalised to [`FILE_PEAK`] and faded out over the last 5 ms.
pub fn render(source: &[i16], seed: u32) -> Vec<i16> {
    let raw: Vec<i64> = source.iter().map(|s| i64::from(*s) << 8).collect();
    let mut words = trim(&raw).to_vec();
    radio_band(&mut words);
    compress(&mut words);
    saturate(&mut words);
    low_passes(&mut words);
    let (open_n, tail_n) = (ms(OPEN_MS), ms(TAIL_MS));
    let n = open_n + words.len() + tail_n;
    let mut out = vec![0i64; n];
    for (d, s) in out[open_n..].iter_mut().zip(&words) {
        *d = *s;
    }
    scale_to(
        &mut out,
        i64::from(FILE_PEAK) * (1000 - HISS_PERMILLE) / 1000,
    );
    let mut rng = Rng::new(seed);
    let mut hiss: Vec<i64> = noise(&mut rng, n).into_iter().map(i64::from).collect();
    radio_band(&mut hiss);
    scale_to(&mut hiss, i64::from(FILE_PEAK) * HISS_PERMILLE / 1000);
    let mut open = squelch(&mut rng, open_n);
    scale_to(&mut open, i64::from(FILE_PEAK) * OPEN_PERMILLE / 1000);
    let mut tail = squelch(&mut rng, tail_n);
    scale_to(&mut tail, i64::from(FILE_PEAK) * TAIL_PERMILLE / 1000);
    for (i, d) in out.iter_mut().enumerate() {
        *d += hiss[i];
        if i < open_n {
            *d += open[i];
        }
        if i >= n - tail_n {
            *d += tail[i - (n - tail_n)];
        }
    }
    // The words' peak plus at most the hiss: pin the whole to the file peak exactly.
    scale_to(&mut out, i64::from(FILE_PEAK));
    let fade = ms(5).min(n);
    for j in 0..fade {
        let i = n - fade + j;
        out[i] = out[i] * (fade - j) as i64 / fade as i64;
    }
    out.iter().map(|s| *s as i16).collect()
}

/// The samples of a 16-bit mono PCM WAV at [`SAMPLE_RATE`] (the `data` chunk, wherever it is).
pub fn read_wav(bytes: &[u8]) -> Result<Vec<i16>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF WAVE file".into());
    }
    let mut at = 12;
    let mut format_ok = false;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let body = bytes
            .get(at + 8..at + 8 + len)
            .ok_or("chunk runs past the end")?;
        if id == b"fmt " {
            if len < 16 {
                return Err("fmt chunk too short".into());
            }
            let u16_at = |o: usize| u16::from_le_bytes([body[o], body[o + 1]]);
            let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            if u16_at(0) != 1 || u16_at(2) != 1 || u16_at(14) != 16 {
                return Err("not 16-bit mono PCM".into());
            }
            if rate != SAMPLE_RATE {
                return Err(format!("sample rate {rate}, want {SAMPLE_RATE}"));
            }
            format_ok = true;
        } else if id == b"data" {
            if !format_ok {
                return Err("data before fmt".into());
            }
            return Ok(body
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect());
        }
        at += 8 + len + (len & 1);
    }
    Err("no data chunk".into())
}

/// The folder of the committed Piper renders.
pub fn source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("voice_src")
}

/// One spoken line: the event it answers, its file (the same name in `voice_src/` and `assets/audio/voice/`),
/// the words and the hiss seed.
pub struct Line {
    pub event: &'static str,
    pub file: &'static str,
    pub text: &'static str,
    pub seed: u32,
}

impl Line {
    /// The raw Piper render of this line.
    pub fn source(&self) -> Vec<i16> {
        let path = source_dir().join(self.file);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (the Piper source is committed)", path.display()));
        read_wav(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    pub fn samples(&self) -> Vec<i16> {
        render(&self.source(), self.seed)
    }

    pub fn bytes(&self) -> Vec<u8> {
        wav(&self.samples())
    }

    /// Length of the finished file in milliseconds.
    pub fn ms(&self) -> u32 {
        (self.samples().len() as u64 * 1000 / u64::from(SAMPLE_RATE)) as u32
    }
}

/// Every line, three per event, in the order of the bank entries. The texts are the ones `piper_lines.py`
/// renders (with a full stop).
#[rustfmt::skip]
pub const LINES: [Line; 9] = [
    Line { event: "unit.select", file: "unit_select_ready.wav", text: "Ready", seed: 201 },
    Line { event: "unit.select", file: "unit_select_online.wav", text: "Online", seed: 202 },
    Line { event: "unit.select", file: "unit_select_standing_by.wav", text: "Standing by", seed: 203 },
    Line { event: "unit.move", file: "unit_move_moving.wav", text: "Moving", seed: 204 },
    Line { event: "unit.move", file: "unit_move_route_set.wav", text: "Route set", seed: 205 },
    Line { event: "unit.move", file: "unit_move_on_it.wav", text: "On it", seed: 206 },
    Line { event: "unit.attack", file: "unit_attack_engaging.wav", text: "Engaging", seed: 207 },
    Line { event: "unit.attack", file: "unit_attack_target_locked.wav", text: "Target locked", seed: 208 },
    Line { event: "unit.attack", file: "unit_attack_firing.wav", text: "Firing", seed: 209 },
];

/// The announcements (WP11.2c): the status lines of the player's own base ("Unit ready", "Base under attack"),
/// one file per event, same voice, same filter, same settings as the unit voices. They are longer than a unit
/// reply; the longest finished file is 1.31 s, so the unit voices' 1.5 s limit holds for them too (no separate limit,
/// no word cut). The texts are the ones `piper_lines.py` renders (with a full stop).
#[rustfmt::skip]
pub const ANNOUNCEMENTS: [Line; 19] = [
    Line { event: "announce.structure_ready", file: "announce_structure_ready.wav", text: "Structure ready", seed: 301 },
    Line { event: "announce.unit_ready", file: "announce_unit_ready.wav", text: "Unit ready", seed: 302 },
    Line { event: "announce.building", file: "announce_building.wav", text: "Building", seed: 303 },
    Line { event: "announce.training", file: "announce_training.wav", text: "Training", seed: 304 },
    Line { event: "announce.on_hold", file: "announce_on_hold.wav", text: "On hold", seed: 305 },
    Line { event: "announce.cancelled", file: "announce_cancelled.wav", text: "Cancelled", seed: 306 },
    Line { event: "announce.base_under_attack", file: "announce_base_under_attack.wav", text: "Base under attack", seed: 307 },
    Line { event: "announce.harvester_under_attack", file: "announce_harvester_under_attack.wav", text: "Harvester under attack", seed: 308 },
    Line { event: "announce.low_power", file: "announce_low_power.wav", text: "Power low", seed: 309 },
    Line { event: "announce.insufficient_funds", file: "announce_insufficient_funds.wav", text: "Not enough credits", seed: 310 },
    Line { event: "announce.storage_full", file: "announce_storage_full.wav", text: "Storage full", seed: 311 },
    Line { event: "announce.unit_lost", file: "announce_unit_lost.wav", text: "Unit lost", seed: 312 },
    Line { event: "announce.structure_lost", file: "announce_structure_lost.wav", text: "Structure lost", seed: 313 },
    Line { event: "announce.new_options", file: "announce_new_options.wav", text: "New options available", seed: 314 },
    Line { event: "announce.cannot_deploy", file: "announce_cannot_deploy.wav", text: "Cannot build there", seed: 315 },
    Line { event: "announce.victory", file: "announce_victory.wav", text: "Victory", seed: 316 },
    Line { event: "announce.defeat", file: "announce_defeat.wav", text: "Defeat", seed: 317 },
    Line { event: "announce.structure_sold", file: "announce_structure_sold.wav", text: "Structure sold", seed: 318 },
    Line { event: "announce.repairing", file: "announce_repairing.wav", text: "Repairing", seed: 319 },
];

/// All files as (name, bytes), for the writer and the checker: the unit voices first, then the announcements.
pub fn files() -> Vec<(String, Vec<u8>)> {
    LINES
        .iter()
        .chain(ANNOUNCEMENTS.iter())
        .map(|l| (l.file.to_string(), l.bytes()))
        .collect()
}
