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
    m.add(&burst(rng, n, b.noise_lp, None, ms(2), b.noise_tau), 90, 0);
    m.add(
        &burst(rng, n, 200, Some(30), ms(1), b.crack_tau),
        b.crack_percent,
        0,
    );
    if let Some((delay, tau, percent)) = b.tail {
        m.add(
            &burst(rng, n, 22, None, ms(20), ms(tau)),
            percent,
            ms(delay),
        );
    }
    m
}

fn rifle(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&burst(rng, n, 210, Some(40), ms(1), ms(22)), 100, 0);
    m.add(&thump(n, 180, 70, ms(1), ms(35)), 40, 0);
    m
}

fn precision(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&burst(rng, n, 235, Some(30), ms(1), ms(16)), 100, 0);
    m.add(&thump(n, 260, 90, ms(1), ms(70)), 50, 0);
    m.add(&burst(rng, n, 40, None, ms(5), ms(150)), 25, ms(60));
    m.add(&burst(rng, n, 30, None, ms(5), ms(120)), 15, ms(180));
    m
}

fn cannon(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 150,
            f1: 40,
            body_tau: ms(120),
            noise_lp: 60,
            noise_tau: ms(90),
            crack_tau: ms(15),
            crack_percent: 50,
            tail: None,
        },
    )
}

fn rocket(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&burst(rng, n, 90, Some(20), ms(120), ms(350)), 100, 0);
    m.add(&burst(rng, n, 200, Some(40), ms(1), ms(8)), 60, 0);
    m
}

fn shell(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&thump(n, 220, 75, ms(2), ms(55)), 100, 0);
    m.add(&burst(rng, n, 120, None, ms(2), ms(40)), 70, 0);
    m.add(&burst(rng, n, 220, Some(30), ms(1), ms(10)), 40, 0);
    m
}

fn howitzer(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&thump(n, 100, 28, ms(5), ms(220)), 100, 0);
    m.add(&burst(rng, n, 40, None, ms(4), ms(250)), 80, 0);
    m.add(&burst(rng, n, 150, Some(30), ms(1), ms(20)), 40, 0);
    m.add(&burst(rng, n, 25, None, ms(20), ms(260)), 30, ms(250));
    m
}

fn demolition(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    for at in [0u32, 130] {
        m.add(&thump(ms(60), 1400, 1400, ms(1), ms(8)), 100, ms(at));
        m.add(&burst(rng, ms(40), 220, Some(30), ms(1), ms(3)), 50, ms(at));
    }
    m.add(&thump(ms(180), 700, 900, ms(3), ms(60)), 90, ms(260));
    m
}

fn impact_bullet(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&burst(rng, n, 256, Some(60), ms(1), ms(12)), 100, 0);
    m.add(&thump(n, 2400, 1800, ms(1), ms(25)), 40, 0);
    m
}

fn impact_precision(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&thump(n, 1100, 700, ms(1), ms(60)), 60, 0);
    m.add(&burst(rng, n, 256, Some(60), ms(1), ms(15)), 70, 0);
    m.add(&thump(n, 140, 90, ms(1), ms(50)), 50, 0);
    m
}

fn impact_cannon(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 110,
            f1: 45,
            body_tau: ms(100),
            noise_lp: 70,
            noise_tau: ms(130),
            crack_tau: ms(12),
            crack_percent: 50,
            tail: None,
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
            body_tau: ms(160),
            noise_lp: 60,
            noise_tau: ms(200),
            crack_tau: ms(14),
            crack_percent: 60,
            tail: Some((120, 300, 35)),
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
            body_tau: ms(90),
            noise_lp: 80,
            noise_tau: ms(100),
            crack_tau: ms(10),
            crack_percent: 40,
            tail: None,
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
            body_tau: ms(300),
            noise_lp: 50,
            noise_tau: ms(260),
            crack_tau: ms(20),
            crack_percent: 50,
            tail: Some((100, 400, 50)),
        },
    )
}

fn impact_miss(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&thump(n, 2600, 900, ms(4), ms(70)), 70, 0);
    m.add(&burst(rng, n, 150, Some(40), ms(2), ms(40)), 30, 0);
    m
}

fn destroyed_infantry(rng: &mut Rng, n: usize) -> Mix {
    let mut m = Mix::new(n);
    m.add(&thump(n, 160, 60, ms(2), ms(60)), 100, 0);
    m.add(&burst(rng, n, 100, None, ms(2), ms(70)), 70, 0);
    m.add(&thump(n, 520, 180, ms(5), ms(120)), 25, 0);
    m
}

fn explosion_small(rng: &mut Rng, n: usize) -> Mix {
    boom(
        rng,
        n,
        &Boom {
            f0: 120,
            f1: 50,
            body_tau: ms(110),
            noise_lp: 80,
            noise_tau: ms(140),
            crack_tau: ms(12),
            crack_percent: 50,
            tail: None,
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
            body_tau: ms(190),
            noise_lp: 55,
            noise_tau: ms(240),
            crack_tau: ms(16),
            crack_percent: 50,
            tail: Some((90, 350, 40)),
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
            body_tau: ms(380),
            noise_lp: 40,
            noise_tau: ms(450),
            crack_tau: ms(20),
            crack_percent: 60,
            tail: Some((150, 700, 50)),
        },
    )
}

macro_rules! spec {
    ($event:expr, $file:expr, $seed:expr, $ms:expr, $recipe:ident, $what:expr) => {
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

/// Every fight sound, in the order of the sound table of `docs/rules/audio.md`.
#[rustfmt::skip]
pub const SOUNDS: [SoundSpec; 18] = [
    spec!("weapon.fire.bullet", "weapon_fire_bullet.wav", 101, 250, rifle,
        "dry, sharp crack with a short low thump"),
    spec!("weapon.fire.precision", "weapon_fire_precision.wav", 102, 500, precision,
        "harder crack, a body and two soft echoes"),
    spec!("weapon.fire.cannon", "weapon_fire_cannon.wav", 103, 600, cannon,
        "deep boom (150 to 40 Hz) with a rumble and a small crack"),
    spec!("weapon.fire.rocket", "weapon_fire_rocket.wav", 104, 900, rocket,
        "pop, then a swelling whoosh that dies away"),
    spec!("weapon.fire.shell", "weapon_fire_shell.wav", 105, 500, shell,
        "mid-pitched thump (220 to 75 Hz), shorter than the cannon"),
    spec!("weapon.fire.howitzer", "weapon_fire_howitzer.wav", 106, 1000, howitzer,
        "very deep, long boom (100 to 28 Hz) and a low echo"),
    spec!("weapon.fire.demolition", "weapon_fire_demolition.wav", 107, 450, demolition,
        "two ticks and a rising beep: a charge being set"),
    spec!("impact.bullet", "impact_bullet.wav", 108, 150, impact_bullet,
        "very short tick with a high ping"),
    spec!("impact.precision", "impact_precision.wav", 109, 250, impact_precision,
        "ping (1.1 kHz falling) over a dull thud"),
    spec!("impact.cannon", "impact_cannon.wav", 110, 700, impact_cannon,
        "small blast: low body, noise and a crack"),
    spec!("impact.rocket", "impact_rocket.wav", 111, 900, impact_rocket,
        "bigger blast with a rumbling tail"),
    spec!("impact.shell", "impact_shell.wav", 112, 600, impact_shell,
        "medium blast, shorter and higher than the rocket's"),
    spec!("impact.demolition", "impact_demolition.wav", 113, 1200, impact_demolition,
        "heavy blast (80 to 25 Hz) with a long rumble"),
    spec!("impact.miss", "impact_miss.wav", 114, 300, impact_miss,
        "thin falling zing (2.6 kHz to 900 Hz): a near miss"),
    spec!("unit.destroyed.infantry", "unit_destroyed_infantry.wav", 115, 500, destroyed_infantry,
        "dull thud and a rattle with a faint falling tone"),
    spec!("explosion.small", "explosion_small.wav", 116, 800, explosion_small,
        "small explosion, no tail"),
    spec!("explosion.medium", "explosion_medium.wav", 117, 1200, explosion_medium,
        "medium explosion with a rumbling tail"),
    spec!("explosion.large", "explosion_large.wav", 118, 2000, explosion_large,
        "large explosion (80 to 26 Hz), long rumble"),
];

/// All files as (name, bytes), for the writer and the checker.
pub fn files() -> Vec<(String, Vec<u8>)> {
    SOUNDS
        .iter()
        .map(|s| (s.file.to_string(), s.bytes()))
        .collect()
}
