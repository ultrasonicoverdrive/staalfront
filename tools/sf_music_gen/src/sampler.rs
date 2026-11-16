//! The integer sample player and the integer import step of the orchestral layer (WP11.4c).
//!
//! The layer of the combat track is made of real recordings from VSCO 2 Community Edition (CC0, see
//! `assets/LICENSES.md`). The recordings are converted once by [`import`] (`sf_music_gen --import <dir>`, not
//! run in CI) into the small 16-bit mono 22.05 kHz WAVs in `tools/sf_music_gen/samples/`, which are committed
//! and pinned; every render then plays them with [`play`], whose maths is **integer only** (a Q16 pitch
//! ratio from a Taylor series, a four-point cubic interpolation, a linear fade), so the output is
//! bit-identical on every platform and the `--check` of the committed FLAC keeps working.

use sf_sound_gen::SAMPLE_RATE;

/// Unison in the Q16 pitch ratio of [`pitch_ratio_q16`].
pub const UNISON: u64 = 1 << 16;

/// `2^(cents / 1200)` in Q16 (65536 = unison, 131072 = an octave up), in integer maths: the octaves are a
/// shift, the rest a Taylor series of `e^y` with `y = r * ln 2 / 1200` in Q32 (twelve terms, error far below
/// one unit of Q16).
pub fn pitch_ratio_q16(cents: i32) -> u64 {
    const LN2_Q32: i128 = 2_977_044_472; // ln 2 * 2^32, rounded
    let octaves = cents.div_euclid(1200);
    let rest = i128::from(cents.rem_euclid(1200));
    let y = rest * LN2_Q32 / 1200;
    let mut term: i128 = 1 << 32;
    let mut sum: i128 = 1 << 32;
    for k in 1..=12 {
        term = term * y / (k << 32);
        sum += term;
    }
    // sum is Q32 and in 1..2; bring it to Q16 with rounding, then apply the octaves.
    let q16 = ((sum + (1 << 15)) >> 16) as u64;
    if octaves >= 0 {
        q16 << octaves
    } else {
        (q16 + (1 << (-octaves - 1))) >> -octaves
    }
}

/// The samples of a mono 16-bit PCM WAV at [`SAMPLE_RATE`] (the format [`import`] writes). Panics on
/// anything else, because a committed sample that is not in this format is a bug in the repository.
pub fn parse_wav(bytes: &[u8]) -> Vec<i16> {
    assert!(
        bytes.len() >= 44 && &bytes[0..4] == b"RIFF" && &bytes[8..16] == b"WAVEfmt ",
        "not a WAV file"
    );
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at =
        |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    assert_eq!(u32_at(16), 16, "fmt chunk");
    assert_eq!(u16_at(20), 1, "PCM");
    assert_eq!(u16_at(22), 1, "mono");
    assert_eq!(u32_at(24), SAMPLE_RATE, "sample rate");
    assert_eq!(u16_at(34), 16, "16 bit");
    assert_eq!(&bytes[36..40], b"data", "data chunk");
    let len = u32_at(40) as usize;
    assert_eq!(bytes.len(), 44 + len, "data length");
    bytes[44..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Play `data` shifted by `cents` (100 per semitone), at most `max_len` output samples, with a linear fade
/// over the last `fade` samples when the cut happens. The output is at the same sample rate; 32767 is full
/// scale (the samples are normalised to it), the caller sets the gain.
///
/// The position in the source advances by the Q16 pitch ratio per output sample; the value at a fractional
/// position is a four-point cubic (Catmull-Rom) through the neighbours, in integers. At unison (ratio exactly
/// 1.0) the fraction is always 0 and the output is the input.
pub fn play(data: &[i16], cents: i32, max_len: usize, fade: usize) -> Vec<i32> {
    let ratio = pitch_ratio_q16(cents);
    let natural = ((data.len() as u64) << 16) / ratio;
    let n = (natural as usize).min(max_len);
    let at = |i: i64| -> i64 {
        if i < 0 || i >= data.len() as i64 {
            0
        } else {
            i64::from(data[i as usize])
        }
    };
    let mut out = Vec::with_capacity(n);
    for j in 0..n {
        let pos = j as u64 * ratio;
        let i = (pos >> 16) as i64;
        let t = (pos & 0xFFFF) as i64;
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let t2 = (t * t) >> 16;
        let t3 = (t2 * t) >> 16;
        let sum = 2 * p1 * 65_536
            + (p2 - p0) * t
            + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2
            + (3 * p1 - p0 - 3 * p2 + p3) * t3;
        out.push(((sum + 65_536) >> 17) as i32);
    }
    if n == max_len && n < natural as usize {
        let f = fade.min(n);
        for k in 0..f {
            let idx = n - f + k;
            let w = (f - k) as i64; // from f down to 1
            out[idx] = (i64::from(out[idx]) * w / f as i64) as i32;
        }
    }
    out
}

// ---- the import step -------------------------------------------------------------------------------------

/// Low-pass FIR of the 2:1 decimation, 95 taps, Q15, sum exactly 32768: a Blackman-windowed sinc with the
/// cut-off at 10 kHz of 44.1 kHz (0 dB up to 9 kHz, -6 dB at 10 kHz, -45 dB at 11.025 kHz, below -70 dB from
/// 12 kHz on). Computed once with a script and written out here, so the import step needs no float.
const DECIMATE_FIR: [i32; 95] = [
    0, 0, 0, 0, -2, 0, 4, 2, -6, -7, 8, 14, -8, -25, 3, 38, 8, -52, -30, 62, 63, -64, -106, 50,
    158, -13, -212, -53, 257, 154, -281, -290, 265, 459, -188, -654, 27, 861, 253, -1066, -710,
    1250, 1484, -1396, -3097, 1491, 10300, 14866, 10300, 1491, -3097, -1396, 1484, 1250, -710,
    -1066, 253, 861, 27, -654, -188, 459, 265, -290, -281, 154, 257, -53, -212, -13, 158, 50, -106,
    -64, 63, 62, -30, -52, 8, 38, 3, -25, -8, 14, 8, -7, -6, 2, 4, 0, -2, 0, 0, 0, 0,
];

/// Output sample `j` of the 2:1 decimation of `x`: the FIR [`DECIMATE_FIR`] centred on input sample `2 * j`
/// (zero outside the input), in the units of `x` times 32768.
pub fn decimate_at(x: &[i64], j: usize) -> i64 {
    let half = DECIMATE_FIR.len() as i64 / 2;
    let centre = 2 * j as i64;
    let mut acc = 0i64;
    for (k, h) in DECIMATE_FIR.iter().enumerate() {
        let i = centre + k as i64 - half;
        if i >= 0 && (i as usize) < x.len() {
            acc += x[i as usize] * i64::from(*h);
        }
    }
    acc
}

/// What [`import`] cannot read.
#[derive(Debug, PartialEq, Eq)]
pub struct ImportError(pub String);

/// The channels of a PCM WAV (16 or 24 bit, any number of channels) summed per frame, in units of the
/// source's own steps, with its sample rate and bits per sample.
pub fn read_pcm_sum(bytes: &[u8]) -> Result<(Vec<i64>, u32, u16), ImportError> {
    let err = |s: &str| ImportError(s.to_string());
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(err("not a RIFF WAVE file"));
    }
    let (mut pos, mut fmt, mut data) = (12usize, None, None);
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body = pos + 8;
        let end = (body + size).min(bytes.len());
        if id == b"fmt " && end - body >= 16 {
            let b = &bytes[body..end];
            fmt = Some((
                u16::from_le_bytes([b[0], b[1]]),
                u16::from_le_bytes([b[2], b[3]]),
                u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                u16::from_le_bytes([b[14], b[15]]),
            ));
        } else if id == b"data" {
            data = Some(&bytes[body..end]);
            break;
        }
        pos = body + size + (size & 1);
    }
    let (tag, channels, rate, bits) = fmt.ok_or_else(|| err("no fmt chunk"))?;
    let data = data.ok_or_else(|| err("no data chunk"))?;
    // 1 = PCM, 0xFFFE = extensible (the bits are still integer PCM in these files).
    if (tag != 1 && tag != 0xFFFE) || channels == 0 || (bits != 16 && bits != 24) {
        return Err(err("only 16 and 24 bit integer PCM is read"));
    }
    let width = usize::from(bits / 8);
    let frame = width * usize::from(channels);
    let mut out = Vec::with_capacity(data.len() / frame);
    for f in data.chunks_exact(frame) {
        let mut sum = 0i64;
        for c in f.chunks_exact(width) {
            sum += if width == 2 {
                i64::from(i16::from_le_bytes([c[0], c[1]]))
            } else {
                i64::from(i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8)
            };
        }
        out.push(sum);
    }
    Ok((out, rate, bits))
}

/// Convert a VSCO recording to a committed sample: mono sum, 2:1 decimation from 44.1 kHz through
/// [`DECIMATE_FIR`], the first `keep_ms` milliseconds, a linear fade over the last `fade_ms` of them (only when
/// the recording is longer than that), scaled so the peak is 32767, rounded once to 16 bit. Returns the WAV file
/// bytes. Integer maths only, so the same recording always gives the same bytes.
pub fn import(src: &[u8], keep_ms: u32, fade_ms: u32) -> Result<Vec<u8>, ImportError> {
    let (x, rate, _bits) = read_pcm_sum(src)?;
    if rate != 2 * SAMPLE_RATE {
        return Err(ImportError(format!(
            "sample rate {rate}, expected {}",
            2 * SAMPLE_RATE
        )));
    }
    let keep = sf_sound_gen::ms(keep_ms);
    let n = keep.min(x.len() / 2);
    let fade = sf_sound_gen::ms(fade_ms);
    // Filter and fade in 64 bits; the scale factor of the fade is Q15.
    let mut v: Vec<i64> = Vec::with_capacity(n);
    for j in 0..n {
        let acc = decimate_at(&x, j);
        let mut w = 32_768i64;
        if x.len() / 2 > keep && fade > 0 && j + fade >= n {
            w = (n - j) as i64 * 32_768 / fade as i64;
        }
        v.push(acc * w);
    }
    let peak = v.iter().map(|s| s.abs()).max().unwrap_or(0).max(1);
    let out: Vec<i16> = v
        .iter()
        .map(|s| {
            let num = i128::from(*s) * 32_767;
            let den = i128::from(peak);
            // round half away from zero
            let q = if num >= 0 {
                (num + den / 2) / den
            } else {
                (num - den / 2) / den
            };
            q as i16
        })
        .collect();
    Ok(sf_sound_gen::wav(&out))
}
