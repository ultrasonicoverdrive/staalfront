//! The CC0 fight sounds of Staalfront, synthesised from seeded noise, filters, envelopes and pitch sweeps
//! (WP11.3b). One sound per sound event of the fight (`docs/rules/audio.md`, "Fight sounds"); the files are
//! committed in `assets/audio/sfx/` and this crate is their source: `cargo run -p sf_sound_gen` rewrites
//! them, `cargo run -p sf_sound_gen -- --check` compares them with a fresh run (CI does, as a test).
//!
//! **Integer maths only** (no float anywhere), so the bytes are identical on every platform and compiler and a
//! pinned hash per file holds everywhere. Nothing here imitates, samples or derives from any game's sound
//! (B11): each recipe is plain noise, a filter and a falling or rising sine, described in words in
//! [`SoundSpec::character`].
//!
//! The building blocks: [`Rng`] (xorshift32), [`noise`], [`lowpass`] and [`highpass`] (one pole), [`tone`] (a
//! sine with a sweep), [`envelope`] (linear attack, exponential decay), mixed with [`Mix`]. Every file is
//! normalised to the same peak ([`FILE_PEAK`]); how loud each event is in the game is the `volume` of its entry
//! in `assets/audio/bank.ron`, not the file.

/// Samples per second of every file (the same as the CC0 test click).
pub const SAMPLE_RATE: u32 = 22_050;
/// Peak of every file, of 32767 (about -2 dBFS). Loudness between events is set by the bank volumes.
pub const FILE_PEAK: i32 = 26_000;

/// Samples in `ms` milliseconds.
pub const fn ms(ms: u32) -> usize {
    (SAMPLE_RATE as u64 * ms as u64 / 1000) as usize
}

/// xorshift32: a fixed, tiny generator, so a seed gives the same noise forever.
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

/// `n` samples of white noise in -32768..=32767.
pub fn noise(rng: &mut Rng, n: usize) -> Vec<i32> {
    (0..n).map(|_| (rng.next() as i32) >> 16).collect()
}

/// One-pole low pass in place; `k` in 1..=256 is the share of the step taken per sample (256 = no filter, small
/// = dark: k 20 is a rumble below about 250 Hz at this sample rate, k 200 still has most of the hiss).
pub fn lowpass(x: &mut [i32], k: i32) {
    let k = i64::from(k.clamp(1, 256));
    let mut y = 0i64;
    for s in x.iter_mut() {
        y += (i64::from(*s) - y) * k / 256;
        *s = y as i32;
    }
}

/// One-pole high pass in place: the input minus its own low pass at `k`.
pub fn highpass(x: &mut [i32], k: i32) {
    let mut low = x.to_vec();
    lowpass(&mut low, k);
    for (s, l) in x.iter_mut().zip(low) {
        *s -= l;
    }
}

/// Scale so the largest sample is 32767 (a silent buffer stays silent).
pub fn normalise(x: &mut [i32]) {
    scale_to(x, 32_767);
}

fn scale_to(x: &mut [i32], peak: i32) {
    let top = x.iter().map(|s| s.abs()).max().unwrap_or(0);
    if top == 0 {
        return;
    }
    for s in x.iter_mut() {
        *s = (i64::from(*s) * i64::from(peak) / i64::from(top)) as i32;
    }
}

/// A sine of `n` samples whose frequency glides from `f0` to `f1` Hz, fast at first (squared), so a thump
/// drops quickly and settles. The sine is a parabola (integer only), full scale 32767.
pub fn tone(n: usize, f0: u32, f1: u32) -> Vec<i32> {
    let mut phase = 0u32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let u = (n - i) as i64 * 65_536 / n as i64; // 65536 at the start, 0 at the end
        let u2 = (u * u) >> 16;
        let f = i64::from(f1) + (((i64::from(f0) - i64::from(f1)) * u2) >> 16);
        // phase step in 1/2^32 of a period
        let step = ((f.max(0) as u64) << 32) / u64::from(SAMPLE_RATE);
        phase = phase.wrapping_add(step as u32);
        let a = i64::from((phase >> 16) as u16 as i16);
        out.push(((a * (32_768 - a.abs())) >> 13).clamp(-32_767, 32_767) as i32);
    }
    out
}

/// Envelope in 0..=32768 per sample: a linear attack of `att` samples, then exponential decay with time
/// constant `tau` samples (the level falls to about a third every `tau`).
pub fn envelope(n: usize, att: usize, tau: usize) -> Vec<i32> {
    let tau = tau.max(1) as i64;
    let k = (1i64 << 24) - (1i64 << 24) / tau;
    let mut v = 1i64 << 24;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let level = (v >> 9) as i32; // 0..=32768
        let a = if i < att {
            (i as i64 * 32_768 / att as i64) as i32
        } else {
            32_768
        };
        out.push((i64::from(level) * i64::from(a) / 32_768) as i32);
        if i >= att {
            v = (v * k) >> 24;
        }
    }
    out
}

/// Multiply a signal by an envelope.
pub fn shape(x: &mut [i32], env: &[i32]) {
    for (s, e) in x.iter_mut().zip(env) {
        *s = (i64::from(*s) * i64::from(*e) / 32_768) as i32;
    }
}

/// Filtered noise shaped by an envelope, normalised before the shaping so a part's percent means something.
pub fn burst(
    rng: &mut Rng,
    n: usize,
    lp: i32,
    hp: Option<i32>,
    att: usize,
    tau: usize,
) -> Vec<i32> {
    let mut x = noise(rng, n);
    lowpass(&mut x, lp);
    if let Some(k) = hp {
        highpass(&mut x, k);
    }
    normalise(&mut x);
    shape(&mut x, &envelope(n, att, tau));
    x
}

/// A sweep (see [`tone`]) with an envelope.
pub fn thump(n: usize, f0: u32, f1: u32, att: usize, tau: usize) -> Vec<i32> {
    let mut x = tone(n, f0, f1);
    shape(&mut x, &envelope(n, att, tau));
    x
}

/// Soft saturation in place (WP11.3c): the buffer is scaled so its peak is `drive` percent of full scale (100 just
/// reaches it, 250 is two and a half times over) and then passed through the cubic `1.5 v - 0.5 v^3`, flat at
/// full scale. Quiet parts stay almost as they were, the loud parts are rounded and held, so the sound has
/// more body at the same peak (the file is normalised again after this).
pub fn saturate(x: &mut [i32], drive: i32) {
    const F: i64 = 32_767;
    let top = x.iter().map(|s| i64::from(s.abs())).max().unwrap_or(0);
    if top == 0 {
        return;
    }
    for s in x.iter_mut() {
        let v = i64::from(*s) * F * i64::from(drive) / 100 / top;
        *s = if v.abs() >= F {
            (F * v.signum()) as i32
        } else {
            ((3 * v * F * F - v * v * v) / (2 * F * F)) as i32
        };
    }
}

/// Noise through a one-pole low pass whose `k` glides linearly from `k0` to `k1` over the `n` samples (a whiz
/// that falls in brightness, a hiss that dies down), then an optional high pass, normalised, no envelope.
pub fn sweep_noise(rng: &mut Rng, n: usize, k0: i32, k1: i32, hp: Option<i32>) -> Vec<i32> {
    let mut x = noise(rng, n);
    let mut y = 0i64;
    for (i, s) in x.iter_mut().enumerate() {
        let k = i64::from(k0) + (i64::from(k1) - i64::from(k0)) * i as i64 / n.max(1) as i64;
        y += (i64::from(*s) - y) * k.clamp(1, 256) / 256;
        *s = y as i32;
    }
    if let Some(k) = hp {
        highpass(&mut x, k);
    }
    normalise(&mut x);
    x
}

/// Sparse noise: a sample is noise with probability `density`/256, else zero (a crackle or sizzle), smoothed by
/// a low pass at `lp`, normalised, shaped by an envelope.
pub fn crackle(rng: &mut Rng, n: usize, density: u32, lp: i32, att: usize, tau: usize) -> Vec<i32> {
    let mut x: Vec<i32> = (0..n)
        .map(|_| {
            let v = (rng.next() as i32) >> 16;
            if (rng.next() & 255) < density { v } else { 0 }
        })
        .collect();
    lowpass(&mut x, lp);
    normalise(&mut x);
    shape(&mut x, &envelope(n, att, tau));
    x
}

/// A sawtooth of `n` samples whose frequency glides linearly from `f0` to `f1` Hz (full scale 32767): the raw
/// voice of a cry, to be filtered.
pub fn saw(n: usize, f0: u32, f1: u32) -> Vec<i32> {
    let mut phase = 0u32;
    (0..n)
        .map(|i| {
            let f = i64::from(f0) + (i64::from(f1) - i64::from(f0)) * i as i64 / n.max(1) as i64;
            phase = phase.wrapping_add((((f.max(0) as u64) << 32) / u64::from(SAMPLE_RATE)) as u32);
            ((phase >> 16) as i32 - 32_768).clamp(-32_767, 32_767)
        })
        .collect()
}

/// The sum of the parts of a sound, each with a gain in percent and a start offset.
pub struct Mix {
    buf: Vec<i32>,
}

impl Mix {
    pub fn new(n: usize) -> Mix {
        Mix { buf: vec![0; n] }
    }

    /// Add `part` at `offset` samples, times `percent`/100 (what runs past the end is cut).
    pub fn add(&mut self, part: &[i32], percent: i32, offset: usize) -> &mut Mix {
        for (i, s) in part.iter().enumerate() {
            if let Some(d) = self.buf.get_mut(offset + i) {
                *d += (i64::from(*s) * i64::from(percent) / 100) as i32;
            }
        }
        self
    }

    /// Soft saturation of the sum so far (see [`saturate`]).
    pub fn drive(&mut self, percent: i32) -> &mut Mix {
        saturate(&mut self.buf, percent);
        self
    }

    /// A short outdoor echo: a copy of the sum so far, low-passed at `lp`, `delay_ms` later at `percent`
    /// (a delay past the end adds nothing). Call twice for two slaps.
    pub fn slap(&mut self, delay_ms: u32, percent: i32, lp: i32) -> &mut Mix {
        let mut copy = self.buf.clone();
        lowpass(&mut copy, lp);
        let d = ms(delay_ms);
        for i in d..self.buf.len() {
            self.buf[i] += (i64::from(copy[i - d]) * i64::from(percent) / 100) as i32;
        }
        self
    }

    /// Normalise to [`FILE_PEAK`], fade out the last 5 ms (no click at the end) and give 16-bit samples.
    pub fn finish(mut self) -> Vec<i16> {
        scale_to(&mut self.buf, FILE_PEAK);
        let n = self.buf.len();
        let fade = ms(5).min(n);
        for j in 0..fade {
            let i = n - fade + j;
            self.buf[i] = (i64::from(self.buf[i]) * (fade - j) as i64 / fade as i64) as i32;
        }
        self.buf.iter().map(|s| *s as i16).collect()
    }
}

/// 16-bit mono PCM WAV.
pub fn wav(samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    w.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

/// One sound: the event it is for, its file, its seed and length, a description for the listener, and the recipe.
pub struct SoundSpec {
    /// The sound event name (`weapon.fire.cannon`) the bank entry of this file has.
    pub event: &'static str,
    /// File name inside `assets/audio/sfx/`.
    pub file: &'static str,
    pub seed: u32,
    pub ms: u32,
    /// What to listen for (nobody can listen in the build container).
    pub character: &'static str,
    recipe: fn(&mut Rng, usize) -> Mix,
}

impl SoundSpec {
    /// The samples of this sound.
    pub fn samples(&self) -> Vec<i16> {
        let n = ms(self.ms);
        (self.recipe)(&mut Rng::new(self.seed), n).finish()
    }

    /// The file bytes.
    pub fn bytes(&self) -> Vec<u8> {
        wav(&self.samples())
    }
}

// ---- recipes ---------------------------------------------------------------------------------------------
//
// WP11.3c: every recipe has more low end (a sub thump under the body), a harder first few milliseconds (a
// click), soft saturation of the sum (`Mix::drive`) and, on the loud ones, a slap echo; the redesigned sounds
// are new recipes with 3 variants each (`<fn>::<0>`, `::<1>`, `::<2>`, one const generic parameter picks the
// variant).

/// A few grains of short filtered noise at random times in `from_ms..from_ms + span_ms`, each quieter than the
/// one before (earth and bits falling, a rattle).
#[allow(clippy::too_many_arguments)]
fn grains(
    m: &mut Mix,
    rng: &mut Rng,
    count: u32,
    from_ms: u32,
    span_ms: u32,
    lp: i32,
    tau_ms: u32,
    percent: i32,
) {
    for g in 0..count {
        let at = from_ms + rng.next() % span_ms.max(1);
        let len = ms(tau_ms * 5);
        let gain = percent * (count - g) as i32 / count as i32;
        m.add(&burst(rng, len, lp, Some(50), 0, ms(tau_ms)), gain, ms(at));
    }
}

/// [`sweep_noise`] with an envelope.
fn sweep_noise_env(
    rng: &mut Rng,
    n: usize,
    k0: i32,
    k1: i32,
    hp: Option<i32>,
    att: usize,
    tau: usize,
) -> Vec<i32> {
    let mut x = sweep_noise(rng, n, k0, k1, hp);
    shape(&mut x, &envelope(n, att, tau));
    x
}

/// Parameters of a boom: the sweep of the body, the noise body, the crack on top and an optional dark tail.
struct Boom {
    f0: u32,
    f1: u32,
    body_tau: usize,
    noise_lp: i32,
    noise_tau: usize,
    crack_tau: usize,
    crack_percent: i32,
    /// (delay ms, tau ms, percent) of a rumbling tail, or none.
    tail: Option<(u32, u32, i32)>,
}

fn boom(rng: &mut Rng, n: usize, b: &Boom) -> Mix {
    let mut m = Mix::new(n);
    m.add(&thump(n, b.f0, b.f1, ms(3), b.body_tau), 100, 0);
    // The sub: a sine just above the final pitch of the body that lasts half as long again.
    m.add(&thump(n, b.f1 + 25, b.f1, ms(6), b.body_tau * 3 / 2), 90, 0);
    m.add(&burst(rng, n, b.noise_lp, None, ms(2), b.noise_tau), 100, 0);
    m.add(
        &burst(rng, n, 200, Some(30), ms(1), b.crack_tau),
        b.crack_percent,
        0,
    );
    // The click: the first 12 ms of full-band noise, no attack.
    m.add(&burst(rng, ms(12), 256, Some(100), 0, ms(2)), 70, 0);
    // A rumble every boom has now, under the explicit tail.
    m.add(
        &burst(rng, n, 24, None, ms(25), b.noise_tau * 2),
        30,
        ms(60),
    );
    if let Some((delay, tau, percent)) = b.tail {
        m.add(
            &burst(rng, n, 22, None, ms(20), ms(tau)),
            percent,
            ms(delay),
        );
    }
    m.drive(240);
    m.slap(110, 28, 60).slap(240, 14, 45);
    m
}

/// `weapon.fire.bullet`, redesigned: a sharp report with body, a short muzzle blast with low end, a mechanical
/// click of the action and a short outdoor slap. 0: rifle, 1: machine gun (three reports), 2: heavy rifle.
fn rifle<const V: u32>(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    let shots: &[(u32, i32)] = match V {
        1 => &[(0, 100), (62, 85), (124, 70)],
        _ => &[(0, 100)],
    };
    for (at, g) in shots {
        let (lp, tau, body) = match V {
            2 => (160, 26, 150),
            1 => (200, 16, 200),
            _ => (190, 20, 180),
        };
        let at = ms(*at);
        // The report: sharp, full-band.
        m.add(&burst(rng, ms(120), 256, Some(45), 0, ms(tau)), *g, at);
        // Body: noise in the mid range under it.
        m.add(
            &burst(rng, ms(150), lp, Some(12), ms(1), ms(28)),
            80 * g / 100,
            at,
        );
        // Muzzle blast with low end: a thump and a sub.
        m.add(&thump(ms(200), body, 55, ms(1), ms(48)), 85 * g / 100, at);
        m.add(&thump(ms(220), 70, 42, ms(3), ms(70)), 60 * g / 100, at);
        // The action: a metal click a little after the shot.
        let click = at + ms(if V == 1 { 38 } else { 52 });
        m.add(&thump(ms(40), 1900, 1700, 0, ms(4)), 22 * g / 100, click);
        m.add(
            &burst(rng, ms(20), 256, Some(80), 0, ms(2)),
            20 * g / 100,
            click,
        );
    }
    m.drive(230);
    m.slap(75, 30, 80).slap(165, 15, 60);
    m
}

fn precision(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&burst(rng, n, 235, Some(30), 0, ms(18)), 100, 0);
    m.add(&thump(n, 260, 70, ms(1), ms(80)), 70, 0);
    m.add(&thump(n, 90, 45, ms(3), ms(120)), 80, 0);
    m.add(&burst(rng, n, 40, None, ms(5), ms(150)), 40, ms(60));
    m.add(&burst(rng, n, 30, None, ms(5), ms(120)), 25, ms(180));
    m.drive(230);
    m.slap(120, 30, 60).slap(260, 18, 45);
    m
}

fn cannon(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 150,
            f1: 40,
            body_tau: ms(130),
            noise_lp: 60,
            noise_tau: ms(100),
            crack_tau: ms(15),
            crack_percent: 70,
            tail: Some((80, 280, 30)),
        },
    )
}

fn rocket(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&burst(rng, n, 90, Some(20), ms(120), ms(380)), 100, 0);
    m.add(&burst(rng, n, 200, Some(40), 0, ms(10)), 80, 0);
    m.add(&thump(n, 120, 50, ms(4), ms(120)), 70, 0);
    m.add(&burst(rng, n, 40, None, ms(150), ms(420)), 50, ms(40));
    m.drive(200);
    m.slap(130, 22, 60);
    m
}

fn shell(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&thump(n, 220, 75, ms(2), ms(60)), 100, 0);
    m.add(&thump(n, 90, 50, ms(4), ms(110)), 80, 0);
    m.add(&burst(rng, n, 120, None, ms(2), ms(45)), 80, 0);
    m.add(&burst(rng, n, 220, Some(30), 0, ms(12)), 60, 0);
    m.drive(230);
    m.slap(100, 26, 60).slap(210, 12, 45);
    m
}

fn howitzer(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&thump(n, 100, 28, ms(5), ms(240)), 100, 0);
    m.add(&thump(n, 52, 32, ms(8), ms(330)), 90, 0);
    m.add(&burst(rng, n, 40, None, ms(4), ms(270)), 90, 0);
    m.add(&burst(rng, n, 150, Some(30), 0, ms(22)), 60, 0);
    m.add(&burst(rng, n, 25, None, ms(20), ms(320)), 40, ms(250));
    m.drive(250);
    m.slap(150, 30, 50).slap(330, 18, 40).slap(520, 10, 35);
    m
}

/// `weapon.fire.demolition`, redesigned: a heavy satchel set down with a metal clunk, then the hiss of a
/// fuse. 0: set down on hard ground, 1: dropped from a little higher (an early second clunk), 2: slower,
/// longer fuse.
fn demolition<const V: u32>(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    let (drop_hz, hiss_at, hiss_tau) = match V {
        1 => (130, 190, 220),
        2 => (100, 240, 320),
        _ => (115, 170, 260),
    };
    // The sack lands: a heavy thud with a bit of canvas noise.
    m.add(&thump(ms(160), drop_hz, 48, ms(2), ms(55)), 100, 0);
    m.add(&burst(rng, ms(120), 70, None, ms(1), ms(30)), 70, 0);
    // The metal clunk: two bar-like tones that do not fit together, and a tick.
    m.add(
        &thump(ms(180), 840 + V * 60, 820 + V * 60, 0, ms(26)),
        45,
        ms(8),
    );
    m.add(
        &thump(ms(180), 1330 + V * 90, 1300 + V * 90, 0, ms(20)),
        30,
        ms(8),
    );
    m.add(&burst(rng, ms(30), 256, Some(60), 0, ms(3)), 60, ms(8));
    // The clasp settles, a smaller clunk.
    let second = if V == 1 { 75 } else { 105 };
    m.add(&thump(ms(100), 1100, 1000, 0, ms(12)), 28, ms(second));
    m.add(&burst(rng, ms(20), 256, Some(80), 0, ms(2)), 30, ms(second));
    // The fuse: a hiss with sparks.
    let len = n - ms(hiss_at);
    m.add(
        &sweep_noise_env(rng, len, 256, 140, Some(14), ms(25), ms(hiss_tau)),
        55,
        ms(hiss_at),
    );
    m.add(
        &crackle(rng, len, 40, 256, ms(25), ms(hiss_tau)),
        45,
        ms(hiss_at),
    );
    m.drive(190);
    m
}

/// `impact.bullet`, redesigned: a dry thwack into dirt or metal with debris, one variant with a short
/// ricochet. 0: dirt, 1: metal, 2: ricochet off stone.
fn impact_bullet<const V: u32>(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    let (lp, body) = match V {
        1 => (256, 260),
        _ => (170, 190),
    };
    m.add(&burst(rng, n, lp, Some(20), 0, ms(14)), 100, 0);
    m.add(&thump(n, body, 80, ms(1), ms(26)), 90, 0);
    m.add(&thump(n, 90, 50, ms(2), ms(45)), 60, 0);
    match V {
        1 => {
            m.add(&thump(n, 1500, 1420, 0, ms(30)), 32, ms(2));
            m.add(&thump(n, 2330, 2250, 0, ms(18)), 18, ms(2));
            grains(&mut m, rng, 3, 20, 70, 230, 4, 40);
        }
        2 => {
            grains(&mut m, rng, 3, 15, 60, 220, 4, 40);
            // The ricochet: a rough falling whine, not a sine ping.
            let mut w = tone(ms(110), 2900, 1100);
            let g = sweep_noise(rng, ms(110), 256, 120, Some(40));
            for (a, b) in w.iter_mut().zip(g) {
                *a = (*a + b) / 2;
            }
            shape(&mut w, &envelope(ms(110), ms(3), ms(40)));
            m.add(&w, 34, ms(14));
        }
        _ => {
            grains(&mut m, rng, 5, 15, 110, 200, 5, 55);
        }
    }
    m.drive(220);
    m
}

/// `impact.precision`, redesigned: a sharp crack and a heavy thud, nothing tonal above 300 Hz (no ping).
/// 0: crack and thud, 1: a bigger thud with an extra low tail, 2: a crack, thud and a slap back.
fn impact_precision<const V: u32>(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    let (thud0, thud_tau) = match V {
        1 => (110, 130),
        _ => (150, 100),
    };
    m.add(&burst(rng, n, 256, Some(25), 0, ms(9 + V)), 190, 0);
    m.add(&burst(rng, n, 170, Some(60), ms(1), ms(25)), 110, 0);
    m.add(&thump(n, thud0, 48, ms(1), ms(thud_tau)), 100, 0);
    m.add(&thump(n, 70, 38, ms(3), ms(thud_tau + 40)), 90, 0);
    m.add(&burst(rng, n, 80, None, ms(1), ms(55)), 60, 0);
    if V == 1 {
        m.add(&burst(rng, n, 30, None, ms(10), ms(160)), 40, ms(40));
    }
    m.drive(170);
    if V == 2 {
        m.slap(85, 28, 70);
    }
    m
}

fn impact_cannon(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 110,
            f1: 45,
            body_tau: ms(110),
            noise_lp: 70,
            noise_tau: ms(140),
            crack_tau: ms(12),
            crack_percent: 70,
            tail: Some((70, 250, 30)),
        },
    )
}

fn impact_rocket(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 90,
            f1: 35,
            body_tau: ms(180),
            noise_lp: 60,
            noise_tau: ms(220),
            crack_tau: ms(14),
            crack_percent: 80,
            tail: Some((120, 340, 45)),
        },
    )
}

fn impact_shell(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 130,
            f1: 50,
            body_tau: ms(100),
            noise_lp: 80,
            noise_tau: ms(110),
            crack_tau: ms(10),
            crack_percent: 60,
            tail: Some((60, 220, 25)),
        },
    )
}

fn impact_demolition(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 80,
            f1: 25,
            body_tau: ms(330),
            noise_lp: 50,
            noise_tau: ms(280),
            crack_tau: ms(20),
            crack_percent: 70,
            tail: Some((100, 450, 60)),
        },
    )
}

/// `impact.miss`, redesigned: a whiz-by (noise that brightens up to the crack and darkens after it), the
/// crack of the bullet passing, and a puff of dirt where it lands. 0: close, 1: farther (softer crack,
/// longer whiz), 2: close with a bigger puff of dirt.
fn impact_miss<const V: u32>(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    let crack_at = match V {
        1 => 55,
        _ => 38,
    };
    // The whiz in: rising brightness up to the crack.
    m.add(
        &sweep_noise_env(
            rng,
            ms(crack_at + 8),
            50,
            256,
            Some(30),
            ms(crack_at),
            ms(120),
        ),
        55,
        0,
    );
    // The crack.
    let crack = if V == 1 { 70 } else { 100 };
    m.add(
        &burst(rng, ms(80), 256, Some(25), 0, ms(7)),
        crack,
        ms(crack_at),
    );
    m.add(&thump(ms(80), 240, 90, ms(1), ms(24)), 50, ms(crack_at));
    // The whiz out: darker and falling.
    m.add(
        &sweep_noise_env(
            rng,
            n - ms(crack_at),
            230,
            40,
            Some(40),
            ms(2),
            ms(70 + 15 * V),
        ),
        70,
        ms(crack_at),
    );
    // The dirt puff, a little later.
    let puff = crack_at + 30;
    let big = if V == 2 { 120 } else { 80 };
    m.add(&burst(rng, ms(150), 90, None, ms(2), ms(40)), big, ms(puff));
    m.add(&thump(ms(150), 95, 55, ms(2), ms(50)), 70, ms(puff));
    grains(&mut m, rng, 3, puff + 10, 60, 190, 4, 30);
    m.drive(200);
    m
}

/// `unit.destroyed.infantry`, redesigned: a short, rough pain cry (a synthesised voice: a glided sawtooth,
/// two formants, roughness and saturation; no recorded or modelled speech) and the body and gear falling.
/// 0: a falling cry, 1: a rising shout cut short, 2: a low grunt.
fn destroyed_infantry<const V: u32>(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    let (f0, f1, len_ms, fall_at) = match V {
        1 => (210, 330, 170, 200),
        2 => (150, 105, 220, 230),
        _ => (400, 190, 260, 240),
    };
    let len = ms(len_ms);
    let src = saw(len, f0, f1);
    // Roughness: the voice is amplitude-modulated by slow noise.
    let mut rough = noise(rng, len);
    lowpass(&mut rough, 14);
    normalise(&mut rough);
    let mut voice: Vec<i32> = src
        .iter()
        .zip(&rough)
        .map(|(s, r)| (i64::from(*s) * i64::from(24_576 + r / 4) / 32_768) as i32)
        .collect();
    // Two formants: one near 800 Hz and one band around 1.5 kHz.
    let mut a = voice.clone();
    lowpass(&mut a, 62);
    let mut b = voice.clone();
    highpass(&mut b, 50);
    lowpass(&mut b, 125);
    for ((v, x), y) in voice.iter_mut().zip(&a).zip(&b) {
        *v = *x + *y / 2;
    }
    normalise(&mut voice);
    saturate(&mut voice, 320);
    let shaped = if V == 1 {
        envelope(len, ms(10), ms(70))
    } else {
        envelope(len, ms(14), ms(120))
    };
    shape(&mut voice, &shaped);
    m.add(&voice, 100, 0);
    // The breath in the cry.
    m.add(&burst(rng, len, 200, Some(40), ms(8), ms(60)), 22, 0);
    // The fall: a body thud and a sub, and the gear: metal ticks and a rattle.
    let fall = ms(fall_at);
    m.add(&thump(ms(240), 150, 52, ms(2), ms(60)), 120, fall);
    m.add(&thump(ms(260), 70, 40, ms(4), ms(90)), 80, fall);
    m.add(&burst(rng, ms(140), 110, None, ms(1), ms(35)), 70, fall);
    for (i, extra) in [30u32, 62, 98].iter().enumerate() {
        let at = ms(fall_at + extra + 7 * V * (i as u32 + 1));
        let hz = 1750 + 260 * i as u32;
        m.add(&thump(ms(60), hz, hz - 100, 0, ms(6)), 20, at);
        m.add(
            &burst(rng, ms(40), 256, Some(70), 0, ms(4)),
            26 - 5 * i as i32,
            at,
        );
    }
    m.drive(140);
    m
}

fn explosion_small(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 120,
            f1: 50,
            body_tau: ms(120),
            noise_lp: 80,
            noise_tau: ms(150),
            crack_tau: ms(12),
            crack_percent: 70,
            tail: Some((70, 260, 30)),
        },
    )
}

fn explosion_medium(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 100,
            f1: 38,
            body_tau: ms(210),
            noise_lp: 55,
            noise_tau: ms(260),
            crack_tau: ms(16),
            crack_percent: 70,
            tail: Some((90, 400, 50)),
        },
    )
}

fn explosion_large(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 80,
            f1: 26,
            body_tau: ms(420),
            noise_lp: 40,
            noise_tau: ms(500),
            crack_tau: ms(20),
            crack_percent: 80,
            tail: Some((150, 800, 60)),
        },
    )
}

macro_rules! spec {
    ($event:expr, $file:expr, $seed:expr, $ms:expr, $recipe:expr, $what:expr) => {
        SoundSpec {
            event: $event,
            file: $file,
            seed: $seed,
            ms: $ms,
            character: $what,
            recipe: $recipe,
        }
    };
}

/// Every fight sound, in the order of the sound table of `docs/rules/audio.md`. An event with variants has
/// one entry per file: the first is the bank entry's `file`, the rest are its `variants`, in this order.
#[rustfmt::skip]
pub const SOUNDS: [SoundSpec; 30] = [
    spec!("weapon.fire.bullet", "weapon_fire_bullet.wav", 101, 450, rifle::<0>,
        "rifle: sharp report, muzzle blast with low end, action click, outdoor slap"),
    spec!("weapon.fire.bullet", "weapon_fire_bullet_mg.wav", 119, 500, rifle::<1>,
        "machine gun: three quick reports, each with body, click and slap"),
    spec!("weapon.fire.bullet", "weapon_fire_bullet_heavy.wav", 120, 500, rifle::<2>,
        "heavy rifle: a lower, harder report with more body"),
    spec!("weapon.fire.precision", "weapon_fire_precision.wav", 102, 600, precision,
        "harder crack, a heavy body and two echoes"),
    spec!("weapon.fire.cannon", "weapon_fire_cannon.wav", 103, 800, cannon,
        "deep boom (150 to 40 Hz) with sub, rumble, crack and a slap echo"),
    spec!("weapon.fire.rocket", "weapon_fire_rocket.wav", 104, 1000, rocket,
        "pop, then a swelling whoosh with a low body that dies away"),
    spec!("weapon.fire.shell", "weapon_fire_shell.wav", 105, 650, shell,
        "mid-pitched thump (220 to 75 Hz) with sub and echo, shorter than the cannon"),
    spec!("weapon.fire.howitzer", "weapon_fire_howitzer.wav", 106, 1300, howitzer,
        "very deep, long boom (100 to 28 Hz), a sub and three low echoes"),
    spec!("weapon.fire.demolition", "weapon_fire_demolition.wav", 107, 800, demolition::<0>,
        "a heavy satchel set down, a metal clunk, then a fuse hiss with sparks"),
    spec!("weapon.fire.demolition", "weapon_fire_demolition_drop.wav", 121, 800, demolition::<1>,
        "the satchel dropped from higher: a harder thud, an early second clunk, a fuse hiss"),
    spec!("weapon.fire.demolition", "weapon_fire_demolition_slow.wav", 122, 1000, demolition::<2>,
        "a lower thud, a clunk, a slower and longer fuse"),
    spec!("impact.bullet", "impact_bullet.wav", 108, 250, impact_bullet::<0>,
        "dry thwack into dirt, with falling debris"),
    spec!("impact.bullet", "impact_bullet_metal.wav", 123, 250, impact_bullet::<1>,
        "a sharper thwack into metal with a dull ring and a few bits"),
    spec!("impact.bullet", "impact_bullet_ricochet.wav", 124, 250, impact_bullet::<2>,
        "thwack, debris and a short rough ricochet whine"),
    spec!("impact.precision", "impact_precision.wav", 109, 400, impact_precision::<0>,
        "a sharp crack and a heavy thud, nothing tonal above 300 Hz"),
    spec!("impact.precision", "impact_precision_deep.wav", 125, 500, impact_precision::<1>,
        "a crack and a bigger, deeper thud with a low tail"),
    spec!("impact.precision", "impact_precision_slap.wav", 126, 500, impact_precision::<2>,
        "a crack, a thud and a slap back"),
    spec!("impact.cannon", "impact_cannon.wav", 110, 900, impact_cannon,
        "blast: low body, sub, noise, crack and a tail"),
    spec!("impact.rocket", "impact_rocket.wav", 111, 1100, impact_rocket,
        "bigger blast with a long rumbling tail"),
    spec!("impact.shell", "impact_shell.wav", 112, 750, impact_shell,
        "medium blast, shorter and higher than the rocket's"),
    spec!("impact.demolition", "impact_demolition.wav", 113, 1500, impact_demolition,
        "heavy blast (80 to 25 Hz) with a sub and a long rumble"),
    spec!("impact.miss", "impact_miss.wav", 114, 400, impact_miss::<0>,
        "whiz-by that cracks as it passes, then a puff of dirt"),
    spec!("impact.miss", "impact_miss_far.wav", 127, 450, impact_miss::<1>,
        "a farther pass: a longer whiz, a softer crack, a puff"),
    spec!("impact.miss", "impact_miss_skip.wav", 128, 450, impact_miss::<2>,
        "a close pass with a bigger puff of dirt"),
    spec!("unit.destroyed.infantry", "unit_destroyed_infantry.wav", 115, 700, destroyed_infantry::<0>,
        "a rough falling pain cry, then the body falls and gear rattles"),
    spec!("unit.destroyed.infantry", "unit_destroyed_infantry_shout.wav", 129, 600, destroyed_infantry::<1>,
        "a short rising shout cut off, a fall, gear"),
    spec!("unit.destroyed.infantry", "unit_destroyed_infantry_grunt.wav", 130, 700, destroyed_infantry::<2>,
        "a low grunt, a heavy fall, gear"),
    spec!("explosion.small", "explosion_small.wav", 116, 1000, explosion_small,
        "small explosion with sub and a short tail"),
    spec!("explosion.medium", "explosion_medium.wav", 117, 1500, explosion_medium,
        "medium explosion with a rumbling tail and echo"),
    spec!("explosion.large", "explosion_large.wav", 118, 2400, explosion_large,
        "large explosion (80 to 26 Hz), sub, long rumble and echoes"),
];

/// All files as (name, bytes), for the writer and the checker.
pub fn files() -> Vec<(String, Vec<u8>)> {
    SOUNDS
        .iter()
        .map(|s| (s.file.to_string(), s.bytes()))
        .collect()
}
