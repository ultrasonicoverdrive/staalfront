//! The CC0 music of Staalfront (WP11.4b): one calm and one combat track, composed and synthesised by this
//! crate, so the shipped game is not silent without the player's Red Alert files. The files are committed in
//! `assets/audio/music/` and this crate is their source: `cargo run -p sf_music_gen` rewrites them,
//! `cargo run -p sf_music_gen -- --check` compares them with a fresh run (CI does, as a test).
//!
//! **Integer maths only** (no float, no platform `sin` or `exp`), so the bytes are identical on every platform
//! and one pinned hash per file holds everywhere. The noise, the one-pole filters and the envelopes are those
//! of `sf_sound_gen`; this crate adds oscillators (a phase counter and a parabola for the sine), a small
//! step sequencer, the instruments and a loop-safe effect section.
//!
//! **Own composition, nothing borrowed** (B11): the notes are written out below ([`calm`], [`combat`]) from a
//! plain minor scale and chord roots; the drums are noise and falling sines, the bass and stabs are filtered
//! saws. Nothing is sampled, quoted or made to imitate any game's music.
//!
//! **Seamless loop.** Everything is mixed into a ring: a note, a pad's release or an echo that runs past the
//! end wraps round to the start (`Song::put`), and the echo and reverb run round the ring two or more times
//! ([`comb`]), so the last sample flows into the first one the way every sample flows into the next. A test
//! checks that the join is no bigger a step than the music has inside.

pub mod flac;
pub mod orchestra;
pub mod sampler;

use sf_sound_gen::{Rng, burst, envelope, lowpass, ms, normalise, shape, thump};

/// Samples per second of the files (the same as the fight sounds).
pub const SAMPLE_RATE: u32 = sf_sound_gen::SAMPLE_RATE;
/// Peak of both tracks, of 32767 (about -1 dBFS). How loud they are in the game is the `volume` of their
/// entries in `assets/audio/bank.ron` and the music volume of `audio.ron`.
pub const FILE_PEAK: i32 = 29_000;
/// Both tracks together may take this many bytes (PLAN WP11.4b).
pub const BUDGET_BYTES: usize = 4_000_000;

/// What a track is: the data of `--table`, the documents and the tests.
pub struct TrackSpec {
    /// The name in `bank.ron` and `audio.ron` (`music.fallback`).
    pub bank_name: &'static str,
    /// File name in `assets/audio/music/`.
    pub file: &'static str,
    pub bpm: u32,
    pub key: &'static str,
    /// Bars of four beats; the track loops after these.
    pub bars: u32,
    pub character: &'static str,
}

pub const TRACKS: [TrackSpec; 2] = [
    TrackSpec {
        bank_name: "music.calm",
        file: "calm.flac",
        bpm: 84,
        key: "D minor",
        bars: 24,
        character: "sparse and tense: slow detuned pads, a deep sub bass, a soft heartbeat pulse, a sparse plucked pattern in an echo, far metal hits and a slow lead near the end",
    },
    TrackSpec {
        bank_name: "music.combat",
        file: "combat.flac",
        bpm: 128,
        key: "D minor",
        bars: 36,
        character: "driving: four-on-the-floor kick, snare on two and four, 16th hats, a 16th bass ostinato, short chord stabs, a breakdown and a snare-roll build, a square lead in the last third, and an orchestral layer of real CC0 samples (timpani, trombone, horn, tuba, cello, crash, bass drum, gong)",
    },
];

/// Length of a track in samples. A 16th note is `SAMPLE_RATE * 15 / bpm` samples; the ring holds a whole
/// number of them.
pub fn samples_of(spec: &TrackSpec) -> usize {
    at(spec.bars as usize * 16, spec.bpm)
}

/// Sample at which 16th note number `step` starts.
fn at(step: usize, bpm: u32) -> usize {
    (step as u64 * u64::from(SAMPLE_RATE) * 15 / u64::from(bpm)) as usize
}

// ---- oscillators ---------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Wave {
    Saw,
    Square,
    Tri,
    Sine,
}

/// Frequencies of C0 to B0 in 1/256 Hz (a rounded table; octave n is a left shift).
const C0_TABLE: [u32; 12] = [
    4186, 4435, 4699, 4978, 5274, 5588, 5920, 6272, 6645, 7040, 7459, 7902,
];

/// Phase step per sample (1/2^32 of a period) of MIDI note `midi` (12 = C0, 60 = middle C, 69 = A 440 Hz).
fn step_of(midi: u32) -> u32 {
    let f256 = u64::from(C0_TABLE[(midi % 12) as usize]) << (midi / 12 - 1);
    ((f256 << 32) / (256 * u64::from(SAMPLE_RATE))) as u32
}

fn wave_at(w: Wave, phase: u32) -> i32 {
    match w {
        Wave::Saw => (phase >> 16) as i32 - 32_768,
        Wave::Square => {
            if phase < 0x8000_0000 {
                32_767
            } else {
                -32_767
            }
        }
        Wave::Tri => {
            let p = (phase >> 16) as i32;
            let t = if p < 32_768 { p } else { 65_535 - p };
            (t - 16_384) * 2
        }
        Wave::Sine => {
            let a = i64::from((phase >> 16) as u16 as i16);
            ((a * (32_768 - a.abs())) >> 13).clamp(-32_767, 32_767) as i32
        }
    }
}

/// `n` samples of a wave with phase step `step`, starting at phase `phase0`, full scale about 32767.
fn osc(w: Wave, step: u32, phase0: u32, n: usize) -> Vec<i32> {
    let mut phase = phase0;
    (0..n)
        .map(|_| {
            let v = wave_at(w, phase);
            phase = phase.wrapping_add(step);
            v
        })
        .collect()
}

/// `n` samples of a wave glued to a pitch of `hz` Hz (for the non-musical metal).
fn osc_hz(w: Wave, hz: u32, n: usize) -> Vec<i32> {
    let step = ((u64::from(hz) << 32) / u64::from(SAMPLE_RATE)) as u32;
    osc(w, step, 0, n)
}

fn add_into(dst: &mut [i32], src: &[i32], num: i32, den: i32) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d += (i64::from(*s) * i64::from(num) / i64::from(den)) as i32;
    }
}

/// Two one-pole low passes whose cut-off falls from `k0` to `k1` (the `k` of [`lowpass`]) with the time
/// constant `tau` samples: the "pluck" of a synth bass or stab.
fn sweep_lp(x: &mut [i32], k0: i32, k1: i32, tau: usize) {
    let env = envelope(x.len(), 0, tau);
    let (mut y1, mut y2) = (0i64, 0i64);
    for (s, e) in x.iter_mut().zip(env) {
        let k = i64::from(k1) + i64::from(k0 - k1) * i64::from(e) / 32_768;
        y1 += (i64::from(*s) - y1) * k / 256;
        y2 += (y1 - y2) * k / 256;
        *s = y2 as i32;
    }
}

/// Two low passes at a fixed `k`.
fn lowpass2(x: &mut [i32], k: i32) {
    lowpass(x, k);
    lowpass(x, k);
}

/// Linear attack of `att` samples, flat, linear release over the last `rel` samples, in 0..=32768.
fn swell(n: usize, att: usize, rel: usize) -> Vec<i32> {
    (0..n)
        .map(|i| {
            let a = if i < att {
                i as i64 * 32_768 / att.max(1) as i64
            } else {
                32_768
            };
            let r = if i + rel > n {
                (n - i) as i64 * 32_768 / rel.max(1) as i64
            } else {
                32_768
            };
            a.min(r) as i32
        })
        .collect()
}

// ---- instruments ---------------------------------------------------------------------------------------

/// Three slightly detuned saws, dark: one voice of a pad.
fn pad_voice(midi: u32, n: usize, att: usize, rel: usize, k: i32) -> Vec<i32> {
    let s = step_of(midi);
    let a = osc(Wave::Saw, s, 0, n);
    let b = osc(Wave::Saw, s + s / 190, 0x4000_0000, n);
    let c = osc(Wave::Saw, s - s / 210, 0x9000_0000, n);
    let mut x: Vec<i32> = (0..n).map(|i| (a[i] + b[i] + c[i]) / 3).collect();
    lowpass2(&mut x, k);
    shape(&mut x, &swell(n, att, rel));
    x
}

/// A sine with a triangle an octave up: the deep bass of the calm track.
fn sub_voice(midi: u32, n: usize, tau: usize) -> Vec<i32> {
    let s = step_of(midi);
    let mut x = osc(Wave::Sine, s, 0, n);
    let t = osc(Wave::Tri, s * 2, 0, n);
    add_into(&mut x, &t, 1, 4);
    shape(&mut x, &envelope(n, ms(6), tau));
    x
}

/// A saw and a sine on the same pitch through a closing filter: the bass of the combat track.
fn bass_voice(midi: u32, n: usize, tau: usize) -> Vec<i32> {
    let s = step_of(midi);
    let mut x = osc(Wave::Saw, s, 0, n);
    sweep_lp(&mut x, 130, 26, ms(80));
    let sine = osc(Wave::Sine, s, 0, n);
    add_into(&mut x, &sine, 3, 4);
    shape(&mut x, &envelope(n, ms(2), tau));
    x
}

/// A square and a saw through a closing filter: a pluck.
fn pluck_voice(midi: u32, n: usize, tau: usize) -> Vec<i32> {
    let s = step_of(midi);
    let mut x = osc(Wave::Square, s, 0, n);
    let b = osc(Wave::Saw, s + s / 150, 0x2000_0000, n);
    add_into(&mut x, &b, 1, 2);
    sweep_lp(&mut x, 150, 24, ms(60));
    shape(&mut x, &envelope(n, ms(1), tau));
    x
}

/// A chord of detuned saws through a closing filter, short: a stab.
fn stab_voice(notes: &[u32], n: usize, tau: usize) -> Vec<i32> {
    let mut x = vec![0; n];
    for (i, m) in notes.iter().enumerate() {
        let s = step_of(*m);
        let a = osc(Wave::Saw, s, 0x1000_0000 * i as u32, n);
        let b = osc(Wave::Saw, s + s / 160, 0x5000_0000, n);
        for j in 0..n {
            x[j] += (a[j] + b[j]) / (2 * notes.len() as i32);
        }
    }
    sweep_lp(&mut x, 170, 34, ms(55));
    shape(&mut x, &envelope(n, ms(2), tau));
    x
}

/// A triangle with a quiet sine an octave up, slowly in and out: the lead.
fn lead_voice(midi: u32, n: usize, att: usize, rel: usize, wave: Wave) -> Vec<i32> {
    let s = step_of(midi);
    let mut x = osc(wave, s, 0, n);
    let o = osc(Wave::Sine, s * 2, 0, n);
    add_into(&mut x, &o, 1, 3);
    lowpass2(&mut x, 110);
    shape(&mut x, &swell(n, att, rel));
    x
}

fn kick_hard() -> Vec<i32> {
    let mut x = thump(ms(300), 170, 42, ms(1), ms(75));
    let mut rng = Rng::new(0x4B49_434B);
    let click = burst(&mut rng, ms(14), 130, None, ms(1), ms(3));
    add_into(&mut x, &click, 1, 3);
    x
}

fn kick_soft() -> Vec<i32> {
    thump(ms(320), 95, 46, ms(2), ms(120))
}

fn snare() -> Vec<i32> {
    let mut rng = Rng::new(0x534E_4152);
    let mut x = burst(&mut rng, ms(260), 230, Some(40), ms(1), ms(70));
    let body = thump(ms(180), 215, 150, ms(1), ms(38));
    add_into(&mut x, &body, 3, 4);
    x
}

fn hat(seed: u32, open: bool) -> Vec<i32> {
    let mut rng = Rng::new(seed);
    if open {
        burst(&mut rng, ms(320), 256, Some(55), ms(1), ms(95))
    } else {
        burst(&mut rng, ms(70), 256, Some(55), ms(1), ms(11))
    }
}

fn tom(f0: u32, f1: u32) -> Vec<i32> {
    let mut x = thump(ms(360), f0, f1, ms(1), ms(100));
    let mut rng = Rng::new(f0 ^ 0x544F_4D00);
    let click = burst(&mut rng, ms(10), 100, None, ms(1), ms(3));
    add_into(&mut x, &click, 1, 4);
    x
}

/// A far metal hit: four square waves that do not sit on a musical scale, with a noise burst, decaying slowly.
fn clank() -> Vec<i32> {
    let n = ms(900);
    let mut x = vec![0; n];
    for hz in [1130, 1741, 2467, 3269] {
        let w = osc_hz(Wave::Square, hz, n);
        add_into(&mut x, &w, 1, 4);
    }
    lowpass(&mut x, 170);
    let mut rng = Rng::new(0x434C_414E);
    let rattle = burst(&mut rng, n, 256, Some(30), 0, ms(60));
    add_into(&mut x, &rattle, 1, 2);
    shape(&mut x, &envelope(n, ms(1), ms(170)));
    x
}

// ---- the ring ------------------------------------------------------------------------------------------

/// Where a sound goes: straight to the mix, through an echo of three 16ths, or through the reverb.
#[derive(Clone, Copy)]
enum Bus {
    Dry,
    Echo,
    Verb,
}

/// The track as a ring of samples of length `n`: anything put past the end continues at the start.
struct Song {
    bpm: u32,
    n: usize,
    dry: Vec<i32>,
    echo: Vec<i32>,
    verb: Vec<i32>,
}

impl Song {
    fn new(bpm: u32, bars: u32) -> Song {
        let n = at(bars as usize * 16, bpm);
        Song {
            bpm,
            n,
            dry: vec![0; n],
            echo: vec![0; n],
            verb: vec![0; n],
        }
    }

    /// Add `part` (times `gain`/256) from 16th number `step`, `shift` samples later, wrapping round the ring.
    fn put(&mut self, bus: Bus, step: usize, shift: usize, part: &[i32], gain: i32) {
        let start = at(step, self.bpm) + shift;
        let n = self.n;
        let buf = match bus {
            Bus::Dry => &mut self.dry,
            Bus::Echo => &mut self.echo,
            Bus::Verb => &mut self.verb,
        };
        for (i, s) in part.iter().enumerate() {
            let j = (start + i) % n;
            buf[j] += (i64::from(*s) * i64::from(gain) / 256) as i32;
        }
    }

    fn len_of(&self, steps: usize) -> usize {
        at(steps, self.bpm)
    }

    /// Echo, reverb and the sum, then a soft limit and the normalisation to [`FILE_PEAK`].
    fn finish(self, echo_fb: i64, verb_wet: i32, knee_x10: i64) -> Vec<i16> {
        let n = self.n;
        let d = at(3, self.bpm);
        let echo = comb(&self.echo, d, echo_fb);
        // Reverb: four combs of unrelated lengths, darkened, mixed with the direct sound.
        let mut x = ring_lowpass(&self.verb, 70);
        for v in &mut x {
            *v /= 2;
        }
        let mut wet = vec![0i32; n];
        for (len, g) in [(1553usize, 196i64), (1907, 190), (2381, 184), (2749, 178)] {
            let c = comb(&x, len, g);
            for i in 0..n {
                wet[i] += c[i] / 4;
            }
        }
        let mut out = vec![0i32; n];
        for i in 0..n {
            let direct = self.verb[i];
            out[i] = self.dry[i]
                + echo[i]
                + direct
                + (i64::from(wet[i]) * i64::from(verb_wet) / 256) as i32;
        }
        // Soft limit: smooth, so a peak does not turn into a flat top.
        // The knee is `knee_x10 / 10` times the loudness (RMS) of the mix, so a hard drum hit is pressed down
        // more where the knee is low (combat) than where it is high (calm).
        let power: i64 = out
            .iter()
            .map(|s| i64::from(*s) * i64::from(*s))
            .sum::<i64>()
            / n as i64;
        let knee = (power as u64).isqrt() as i64 * knee_x10 / 10 + 1;
        for s in &mut out {
            let v = i64::from(*s);
            *s = (v * knee / (knee + v.abs())) as i32;
        }
        normalise(&mut out);
        let k = i64::from(FILE_PEAK);
        out.iter()
            .map(|s| (i64::from(*s) * k / 32_767) as i16)
            .collect()
    }
}

/// A feedback delay of `d` samples round the ring (`y[i] = x[i] + g * y[i - d]`, `g` in 1/256), run for
/// several laps so the end of the ring feeds the start the way a long run would.
fn comb(x: &[i32], d: usize, g: i64) -> Vec<i32> {
    let n = x.len();
    let d = d % n;
    let mut y = x.to_vec();
    for _ in 0..6 {
        for i in 0..n {
            let j = (i + n - d) % n;
            y[i] = x[i] + (g * i64::from(y[j]) / 256) as i32;
        }
    }
    y
}

/// A low pass that starts from the end of the ring, so the join has no jump.
fn ring_lowpass(x: &[i32], k: i32) -> Vec<i32> {
    let warm = 4096.min(x.len());
    let mut ext: Vec<i32> = x[x.len() - warm..].to_vec();
    ext.extend_from_slice(x);
    lowpass(&mut ext, k);
    ext[warm..].to_vec()
}

// ---- the notes -----------------------------------------------------------------------------------------

/// Third and fifth above a root: a minor or a major triad.
fn triad(root: u32, minor: bool) -> [u32; 3] {
    [root, root + if minor { 3 } else { 4 }, root + 7]
}

/// Calm: 84 BPM, D minor, 24 bars (68.6 s). Chords change every two bars: Dm Bb Gm A, Dm Bb Eb A, Dm Gm Bb A
/// (the Eb is the dark note, the A major turns back to D). Layers come in by the third: pad and sub bass and a
/// soft pulse for bars 0 to 7, plucks in an echo from bar 8, hats from bar 12, a low tom at the end of every fourth bar from bar 11, a slow lead in
/// bars 16 to 23.
fn calm() -> Vec<i16> {
    let spec = &TRACKS[0];
    let mut song = Song::new(spec.bpm, spec.bars);
    // (root MIDI note, minor)
    const CHORDS: [(u32, bool); 12] = [
        (50, true),
        (46, false),
        (43, true),
        (45, false),
        (50, true),
        (46, false),
        (51, false),
        (45, false),
        (50, true),
        (43, true),
        (46, false),
        (45, false),
    ];
    let kick = kick_soft();
    let hats = [hat(0x4841_5431, false), hat(0x4841_5432, false)];
    let hat_open = hat(0x4841_544F, true);
    let low_tom = tom(125, 72);
    let metal = clank();

    for (ci, (root, minor)) in CHORDS.iter().enumerate() {
        let tri = triad(*root, *minor);
        let step0 = ci * 32;
        // Pad: root and the triad an octave up, slow in, slow out, running into the next chord.
        let len = song.len_of(32) + ms(1600);
        let pad_gain = if ci < 4 { 20 } else { 26 };
        for (i, m) in [tri[0], tri[0] + 12, tri[1] + 12, tri[2] + 12]
            .iter()
            .enumerate()
        {
            let v = pad_voice(*m, len, ms(1300), ms(1500), 34 - 4 * (i as i32).min(2));
            song.put(Bus::Verb, step0, 0, &v, pad_gain);
        }
        // Sub bass on every bar; a second, softer hit from bar 8.
        for b in 0..2 {
            let step = step0 + b * 16;
            let v = sub_voice(root - 12, song.len_of(16) + ms(150), ms(950));
            song.put(Bus::Dry, step, 0, &v, 150);
            if step >= 8 * 16 {
                let v = sub_voice(root - 12, song.len_of(6), ms(260));
                song.put(Bus::Dry, step + 10, 0, &v, 90);
            }
        }
    }
    for bar in 0..spec.bars as usize {
        let step0 = bar * 16;
        let (root, minor) = CHORDS[bar / 2];
        let tri = triad(root, minor);
        // Heartbeat: a soft thump on the first step, a second one a dotted 8th later.
        song.put(Bus::Dry, step0, 0, &kick, 105);
        if bar % 4 != 3 {
            song.put(Bus::Dry, step0 + 6, 0, &kick, 55);
        }
        // Plucks in an echo: the tresillo 3-3-2 over the chord tones.
        if (8..23).contains(&bar) {
            let tones = [tri[0] + 24, tri[1] + 24, tri[2] + 24, tri[0] + 36];
            let order = [0usize, 2, 1, 2, 3, 2];
            for (h, s) in [0usize, 3, 6, 8, 11, 14].iter().enumerate() {
                let v = pluck_voice(tones[order[h]], song.len_of(5), ms(200));
                let g = if *s == 0 { 62 } else { 48 };
                song.put(Bus::Echo, step0 + s, 0, &v, g);
            }
        }
        if bar >= 12 {
            for (h, s) in [2usize, 6, 10, 14].iter().enumerate() {
                song.put(Bus::Dry, step0 + s, 0, &hats[h % 2], 34);
            }
            if bar % 4 == 3 {
                song.put(Bus::Dry, step0 + 14, 0, &hat_open, 22);
            }
        }
        if bar >= 8 && bar % 4 == 3 {
            song.put(Bus::Dry, step0 + 12, 0, &low_tom, 120);
        }
        if bar % 8 == 7 {
            song.put(Bus::Verb, step0 + 8, 0, &metal, 60);
        }
    }
    // Lead: two long notes per chord in bars 16 to 23 (chords 8 to 11), out of D minor and the A major.
    const LEAD: [(u32, u32); 4] = [(69, 65), (70, 67), (74, 72), (73, 69)];
    for (i, (a, b)) in LEAD.iter().enumerate() {
        let step0 = (16 + i * 2) * 16;
        let v = lead_voice(*a, song.len_of(24), ms(500), ms(900), Wave::Tri);
        song.put(Bus::Echo, step0 + 4, 0, &v, 46);
        song.put(Bus::Verb, step0 + 4, 0, &v, 30);
        let v = lead_voice(*b, song.len_of(20), ms(400), ms(900), Wave::Tri);
        song.put(Bus::Echo, step0 + 16 + 8, 0, &v, 40);
        song.put(Bus::Verb, step0 + 16 + 8, 0, &v, 26);
    }
    song.finish(104, 150, 40)
}

/// Root and minor/major of the chord of bar `bar` of the combat track: the A phrase D D Bb C and the B phrase
/// D G Bb A, in the order of [`COMBAT_PHRASES`].
const COMBAT_PHRASE_A: [(u32, bool); 4] = [(38, true), (38, true), (34, false), (36, false)];
const COMBAT_PHRASE_B: [(u32, bool); 4] = [(38, true), (43, true), (34, false), (33, false)];
/// Which phrase each of the nine sections plays (`true` = B).
const COMBAT_PHRASES: [bool; 9] = [false, false, true, false, true, false, false, true, false];

fn combat_chord(bar: usize) -> (u32, bool) {
    if COMBAT_PHRASES[bar / 4] {
        COMBAT_PHRASE_B[bar % 4]
    } else {
        COMBAT_PHRASE_A[bar % 4]
    }
}

/// Combat: 128 BPM, D minor, 36 bars (67.5 s) in nine sections of four bars. Bar roots: D D Bb C (A phrase)
/// and D G Bb A (B phrase). Sections: 0 intro (kick, bass, hats), 1 snare and pad, 2 stabs (B), 3 full (A),
/// 4 breakdown (B: no kick, no bass, toms and a pad), 5 build (A, snare roll in the last bar), 6 full with
/// lead (A), 7 full with lead (B), 8 outro (A, thinner, ends on a tom fill that runs into the intro).
fn combat(with_orchestra: bool) -> Vec<i16> {
    let spec = &TRACKS[1];
    let mut song = Song::new(spec.bpm, spec.bars);
    let kick = kick_hard();
    let snare = snare();
    let hats = [hat(0x4841_5431, false), hat(0x4841_5432, false)];
    let hat_open = hat(0x4841_544F, true);
    let toms = [tom(235, 150), tom(185, 120), tom(145, 95), tom(110, 70)];
    let metal = clank();
    // Bass ostinato: (16th, semitones above the root).
    const OSTINATO: [(usize, u32); 9] = [
        (0, 0),
        (2, 0),
        (3, 0),
        (5, 0),
        (7, 12),
        (8, 0),
        (10, 0),
        (11, 7),
        (13, 0),
    ];

    for bar in 0..spec.bars as usize {
        let sec = bar / 4;
        let (root, minor) = combat_chord(bar);
        let step0 = bar * 16;
        let last_of_sec = bar % 4 == 3;
        let breakdown = sec == 4;
        let tri = triad(root, minor);

        // Kick.
        if !breakdown {
            for s in [0usize, 4, 8, 12] {
                song.put(Bus::Dry, step0 + s, 0, &kick, 170);
            }
            if bar % 2 == 1 && sec >= 2 && sec != 5 {
                song.put(Bus::Dry, step0 + 10, 0, &kick, 110);
            }
        }
        // Bass ostinato.
        if !breakdown && !(sec == 8 && bar >= 34) {
            for (s, up) in OSTINATO {
                let v = bass_voice(root + up, song.len_of(2), ms(95));
                let g = if s == 0 { 105 } else { 88 };
                song.put(Bus::Dry, step0 + s, 0, &v, g);
            }
        } else if breakdown {
            // A held sub note keeps the floor while the drums are gone.
            let v = sub_voice(root, song.len_of(16), ms(1000));
            song.put(Bus::Dry, step0, 0, &v, 120);
        }
        // Snare on two and four.
        if sec >= 1 && !breakdown {
            for s in [4usize, 12] {
                song.put(Bus::Dry, step0 + s, 0, &snare, 120);
            }
        }
        // Snare roll in the last bar of the build: 16ths, getting louder.
        if sec == 5 && last_of_sec {
            for s in 0..16usize {
                song.put(Bus::Dry, step0 + s, 0, &snare, 40 + 5 * s as i32);
            }
        }
        // Hats: 16ths with the offbeat louder; eighths in the intro and the breakdown.
        let eighths = sec == 0 || breakdown;
        for s in 0..16usize {
            if eighths && s % 2 == 1 {
                continue;
            }
            let g = match s % 4 {
                0 => 30,
                2 => 50,
                _ => 22,
            };
            song.put(Bus::Dry, step0 + s, 0, &hats[(s / 2) % 2], g);
        }
        if matches!(sec, 2 | 3 | 6 | 7) {
            for s in [6usize, 14] {
                song.put(Bus::Dry, step0 + s, 0, &hat_open, 32);
            }
        }
        // Stabs: the chord on 16ths 0, 3, 6 and 10 of the bar.
        if matches!(sec, 2 | 3 | 5 | 6 | 7) && !(sec == 5 && last_of_sec) {
            let chord = [tri[0] + 24, tri[1] + 24, tri[2] + 24];
            for s in [0usize, 3, 6, 10] {
                let v = stab_voice(&chord, song.len_of(2), ms(110));
                song.put(Bus::Echo, step0 + s, 0, &v, if s == 0 { 56 } else { 46 });
            }
        }
        // Pad under the whole thing from section 1; loud in the breakdown.
        if sec >= 1 && sec != 8 {
            let len = song.len_of(16) + ms(700);
            let g = if breakdown { 30 } else { 17 };
            for (i, m) in [tri[0] + 24, tri[1] + 24, tri[2] + 24].iter().enumerate() {
                let v = pad_voice(*m, len, ms(500), ms(700), 38 - 3 * i as i32);
                song.put(Bus::Verb, step0, 0, &v, g);
            }
        }
        // Breakdown: a low tom on the pulse.
        if breakdown {
            for s in [0usize, 6, 10] {
                song.put(Bus::Verb, step0 + s, 0, &toms[3], 110);
            }
        }
        // Metal hits in the full sections, far away.
        if matches!(sec, 3 | 7) && bar % 2 == 1 {
            song.put(Bus::Verb, step0 + 10, 0, &metal, 46);
        }
        // Tom fills at the end of every eight bars and of the track.
        if (bar % 8 == 7 && sec != 5) || bar == 35 {
            for (i, s) in [12usize, 13, 14, 15].iter().enumerate() {
                song.put(Bus::Dry, step0 + s, 0, &toms[i], 120);
            }
        }
    }
    // Lead: a square line over the last full sections (bars 24 to 31), two bars to the phrase.
    const MOTIF: [(usize, u32, usize); 4] = [(0, 69, 6), (6, 72, 4), (10, 70, 6), (16, 69, 14)];
    for rep in 0..4usize {
        let step0 = 24 * 16 + rep * 32;
        for (s, m, len) in MOTIF {
            let v = lead_voice(m, song.len_of(len + 2), ms(8), ms(70), Wave::Square);
            song.put(Bus::Echo, step0 + s, 0, &v, 36);
        }
    }
    let base = song.finish(96, 120, 20);
    if with_orchestra {
        layer_over(&base, &orchestra_layers())
    } else {
        base
    }
}

/// The orchestral layer of the combat track (WP11.4c), part by part, as rings of the length of the track; 32767
/// is the full scale of the track. See [`orchestra`].
pub fn orchestra_layers() -> Vec<(orchestra::Part, Vec<i32>)> {
    let spec = &TRACKS[1];
    orchestra::combat_layers(spec.bpm, spec.bars as usize, |bar| combat_chord(bar).0)
}

/// The combat track without the orchestral layer (what WP11.4b shipped).
pub fn combat_base() -> Vec<i16> {
    combat(false)
}

/// Add the layers to a finished track, limit the sum and bring it back to [`FILE_PEAK`].
fn layer_over(base: &[i16], layers: &[(orchestra::Part, Vec<i32>)]) -> Vec<i16> {
    let mut x: Vec<i32> = base.iter().map(|s| i32::from(*s)).collect();
    for (_, l) in layers {
        assert_eq!(l.len(), x.len());
        for (d, s) in x.iter_mut().zip(l) {
            *d += *s;
        }
    }
    limit_ring(&mut x, FILE_PEAK);
    normalise(&mut x);
    let k = i64::from(FILE_PEAK);
    x.iter()
        .map(|s| (i64::from(*s) * k / 32_767) as i16)
        .collect()
}

/// Length of a limiter block (5 ms) in samples.
const LIMIT_BLOCK: usize = ms(5);
/// How fast the gain comes back after a peak: `exp(-5 ms / 150 ms)` per block in Q15 (31696 / 32768).
const LIMIT_RELEASE_Q15: i64 = 31_696;

/// A look-ahead peak limiter in integer maths. The peaks of 5 ms blocks (and of the next block, so the gain is
/// down before the peak) give a gain that falls at once and rises again over about 150 ms; the gain is
/// interpolated linearly between block centres. The track is a ring, so the gain state is run for two laps
/// and the last block feeds the first: no step at the loop point.
fn limit_ring(x: &mut [i32], ceiling: i32) {
    let n = x.len();
    let nb = n.div_ceil(LIMIT_BLOCK);
    let peak: Vec<i64> = (0..nb)
        .map(|b| {
            x[b * LIMIT_BLOCK..((b + 1) * LIMIT_BLOCK).min(n)]
                .iter()
                .map(|s| i64::from(s.abs()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let want: Vec<i64> = (0..nb)
        .map(|b| {
            let p = peak[b].max(peak[(b + 1) % nb]);
            if p <= i64::from(ceiling) {
                32_768
            } else {
                i64::from(ceiling) * 32_768 / p
            }
        })
        .collect();
    let mut gain = vec![32_768i64; nb];
    let mut cur = 32_768i64;
    for lap in 0..2 {
        for b in 0..nb {
            let back = 32_768 - (32_768 - cur) * LIMIT_RELEASE_Q15 / 32_768;
            cur = want[b].min(back);
            if lap == 1 {
                gain[b] = cur;
            }
        }
    }
    let half = LIMIT_BLOCK / 2;
    for (i, s) in x.iter_mut().enumerate() {
        let (b, o) = (i / LIMIT_BLOCK, i % LIMIT_BLOCK);
        // Between the centre of the neighbour before (or after) and the centre of this block.
        let (a, c, w) = if o >= half {
            (gain[b], gain[(b + 1) % nb], o - half)
        } else {
            (gain[(b + nb - 1) % nb], gain[b], o + half)
        };
        let g = a + (c - a) * w as i64 / LIMIT_BLOCK as i64;
        *s = (i64::from(*s) * g / 32_768) as i32;
    }
}

/// The finished 16-bit samples of a track.
pub fn render(spec: &TrackSpec) -> Vec<i16> {
    if spec.bank_name == TRACKS[0].bank_name {
        calm()
    } else {
        combat(true)
    }
}

/// File name and bytes of every track.
pub fn files() -> Vec<(String, Vec<u8>)> {
    TRACKS
        .iter()
        .map(|t| (t.file.to_string(), flac::encode(&render(t), SAMPLE_RATE)))
        .collect()
}
