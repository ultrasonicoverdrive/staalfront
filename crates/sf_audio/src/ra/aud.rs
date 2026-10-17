//! AUD sound files (Westwood).
//!
//! **Provenance (clean-room, B5).** Written only from these public format
//! descriptions, never from the EA source release or OpenRA:
//! - ModdingWiki, "Westwood AUD Format",
//!   <https://moddingwiki.shikadi.net/wiki/Westwood_AUD_Format>: header and
//!   chunk layout, flags, compression codes 1 and 99;
//! - MultimediaWiki, "Westwood ADPCM Audio",
//!   <https://wiki.multimedia.cx/index.php/Westwood_ADPCM_Audio> (based on
//!   Valery V. Anisimovsky's WS-AUD notes): the Westwood ADPCM codes and
//!   tables, the 0x80 start per chunk, "equal sizes = stored raw", the
//!   sign-extended 5-bit delta, chunks independent of each other, and the
//!   `LL RR` stereo nibble layout of IMA sound in C&C/Red Alert VQA files;
//! - MultimediaWiki, "Westwood IMA ADPCM",
//!   <https://wiki.multimedia.cx/index.php/Westwood_IMA_ADPCM>: predictor
//!   and index start at 0 and are kept across chunks; low nibble first;
//!   stereo bytes alternate `L1L0 R1R0 ...`;
//! - IMA Digital Audio Focus and Technical Working Groups, "Recommended
//!   Practices for Enhancing Digital Audio Compatibility in Multimedia
//!   Systems" (1992): IMA ADPCM step and index tables.
//!
//! Caveats, unverified against real files: the IMA chunk carry-over and
//! the stereo byte layout are described for VQA / Westwood IMA and applied
//! to AUD as well; stereo or 16-bit Westwood ADPCM is not described (only
//! mono 8-bit has been seen) and its layout here is our assumption.
//!
//! - **Header** (12 bytes, little-endian): `u16 sample rate`, `u32 size of
//!   the compressed data`, `u32 size of the output`, `u8 flags` (bit 0
//!   stereo, bit 1 16-bit), `u8 compression` (1 = Westwood ADPCM, 99 = IMA
//!   ADPCM).
//! - **Chunks**: `u16 compressed size`, `u16 output size`, `u32 0x0000DEAF`,
//!   then the compressed bytes.
//! - **Westwood ADPCM**: 8-bit unsigned output. Each command byte has a
//!   2-bit code and a 6-bit count: 0 = `count+1` bytes of four 2-bit deltas,
//!   1 = `count+1` bytes of two 4-bit deltas (low nibble first), 2 = a 5-bit
//!   signed delta when bit 5 of the count is set (output as a byte, not
//!   clipped), otherwise `count+1` raw samples, 3 = repeat the last sample
//!   `count+1` times. ADPCM deltas are clipped to 0..=255.
//! - **IMA ADPCM**: 4-bit codes, low nibble first, 16-bit output. Stereo:
//!   one byte (two nibbles) left, then one byte right.

use std::fmt;

/// Westwood ADPCM 2-bit and 4-bit delta tables.
const WS_STEP2: [i32; 4] = [-2, -1, 0, 1];
const WS_STEP4: [i32; 16] = [-9, -8, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 8];

/// IMA ADPCM step size table (89 entries) and index adjustment.
const IMA_STEP: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const IMA_INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

const CHUNK_ID: u32 = 0x0000_DEAF;
const HEADER_LEN: usize = 12;
const CHUNK_HEADER_LEN: usize = 8;
/// Upper bound on decoded output (bytes), against absurd headers.
pub const MAX_OUTPUT: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    Westwood,
    Ima,
}

impl Compression {
    fn code(self) -> u8 {
        match self {
            Compression::Westwood => 1,
            Compression::Ima => 99,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudHeader {
    pub sample_rate: u16,
    pub size: u32,
    pub out_size: u32,
    pub stereo: bool,
    pub sixteen_bit: bool,
    pub compression: Compression,
}

impl AudHeader {
    pub fn channels(&self) -> u16 {
        if self.stereo { 2 } else { 1 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudError {
    Truncated,
    Invalid(String),
}

impl fmt::Display for AudError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudError::Truncated => write!(f, "AUD data is truncated"),
            AudError::Invalid(why) => write!(f, "invalid AUD: {why}"),
        }
    }
}

impl std::error::Error for AudError {}

/// Decoded audio: interleaved signed 16-bit samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        self.frames() as u64 * 1000 / u64::from(self.sample_rate)
    }

    /// As a kira static sound (stereo frames, `-1..1`).
    pub fn to_static_sound(&self) -> kira::sound::static_sound::StaticSoundData {
        use kira::Frame;
        let s = |v: i16| f32::from(v) / 32768.0;
        let frames: Vec<Frame> = if self.channels == 2 {
            self.samples
                .chunks_exact(2)
                .map(|p| Frame::new(s(p[0]), s(p[1])))
                .collect()
        } else {
            self.samples
                .iter()
                .map(|&v| Frame::from_mono(s(v)))
                .collect()
        };
        kira::sound::static_sound::StaticSoundData {
            sample_rate: self.sample_rate,
            frames: frames.into(),
            settings: Default::default(),
            slice: None,
        }
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Parse the header; also checks the first chunk id, so random data is
/// rarely taken for an AUD file.
pub fn parse_header(b: &[u8]) -> Result<AudHeader, AudError> {
    let (Some(rate), Some(size), Some(out), Some(&flags), Some(&comp)) = (
        u16_at(b, 0),
        u32_at(b, 2),
        u32_at(b, 6),
        b.get(10),
        b.get(11),
    ) else {
        return Err(AudError::Truncated);
    };
    let compression = match comp {
        1 => Compression::Westwood,
        99 => Compression::Ima,
        c => return Err(AudError::Invalid(format!("unknown compression {c}"))),
    };
    if flags & !3 != 0 {
        return Err(AudError::Invalid(format!("unknown flags {flags:#04x}")));
    }
    if !(1000..=96000).contains(&u32::from(rate)) {
        return Err(AudError::Invalid(format!("sample rate {rate}")));
    }
    if size > 0 {
        match u32_at(b, HEADER_LEN + 4) {
            Some(CHUNK_ID) => {}
            Some(_) => return Err(AudError::Invalid("no chunk marker".into())),
            None => return Err(AudError::Truncated),
        }
    }
    Ok(AudHeader {
        sample_rate: rate,
        size,
        out_size: out,
        stereo: flags & 1 != 0,
        sixteen_bit: flags & 2 != 0,
        compression,
    })
}

/// Westwood ADPCM: decode one chunk to `out_len` unsigned 8-bit samples.
pub fn ws_decode_chunk(input: &[u8], out_len: usize, out: &mut Vec<u8>) -> Result<(), AudError> {
    if input.len() == out_len {
        out.extend_from_slice(input);
        return Ok(());
    }
    let end = out.len() + out_len;
    let mut sample: i32 = 0x80;
    let mut i = 0;
    let next = |i: &mut usize| -> Result<u8, AudError> {
        let b = *input.get(*i).ok_or(AudError::Truncated)?;
        *i += 1;
        Ok(b)
    };
    let push = |out: &mut Vec<u8>, s: i32| {
        if out.len() < end {
            out.push(s as u8);
        }
    };
    while out.len() < end {
        let cmd = next(&mut i)?;
        let count = usize::from(cmd & 0x3F);
        match cmd >> 6 {
            0 => {
                for _ in 0..=count {
                    let b = next(&mut i)?;
                    for k in 0..4 {
                        sample = (sample + WS_STEP2[usize::from((b >> (2 * k)) & 3)]).clamp(0, 255);
                        push(out, sample);
                    }
                }
            }
            1 => {
                for _ in 0..=count {
                    let b = next(&mut i)?;
                    for nib in [b & 0x0F, b >> 4] {
                        sample = (sample + WS_STEP4[usize::from(nib)]).clamp(0, 255);
                        push(out, sample);
                    }
                }
            }
            2 => {
                if count & 0x20 != 0 {
                    // 5-bit two's complement delta.
                    let delta = ((cmd << 3) as i8 >> 3) as i32;
                    sample += delta;
                    if out.len() < end {
                        out.push(sample as u8);
                    }
                } else {
                    for _ in 0..=count {
                        let b = next(&mut i)?;
                        sample = i32::from(b);
                        push(out, sample);
                    }
                }
            }
            _ => {
                for _ in 0..=count {
                    push(out, sample);
                }
            }
        }
    }
    Ok(())
}

/// IMA ADPCM state of one channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImaState {
    pub predictor: i32,
    pub index: i32,
}

impl ImaState {
    /// Decode one 4-bit code.
    pub fn decode(&mut self, code: u8) -> i16 {
        let step = IMA_STEP[self.index as usize];
        let mut diff = step >> 3;
        if code & 4 != 0 {
            diff += step;
        }
        if code & 2 != 0 {
            diff += step >> 1;
        }
        if code & 1 != 0 {
            diff += step >> 2;
        }
        self.predictor = if code & 8 != 0 {
            self.predictor - diff
        } else {
            self.predictor + diff
        }
        .clamp(-32768, 32767);
        self.index = (self.index + IMA_INDEX[usize::from(code & 7)]).clamp(0, 88);
        self.predictor as i16
    }

    /// Encode one sample (the standard IMA encoder) and stay in step with
    /// the decoder.
    pub fn encode(&mut self, sample: i16) -> u8 {
        let step = IMA_STEP[self.index as usize];
        let mut delta = i32::from(sample) - self.predictor;
        let mut code = 0u8;
        if delta < 0 {
            code = 8;
            delta = -delta;
        }
        let mut s = step;
        for bit in [4u8, 2, 1] {
            if delta >= s {
                code |= bit;
                delta -= s;
            }
            s >>= 1;
        }
        self.decode(code);
        code
    }
}

/// Decode a whole AUD file to PCM.
pub fn decode(bytes: &[u8]) -> Result<Pcm, AudError> {
    let h = parse_header(bytes)?;
    let channels = usize::from(h.channels());
    let mut raw: Vec<u8> = Vec::new(); // Westwood: 8-bit output bytes
    let mut samples: Vec<i16> = Vec::new();
    let mut ima = [ImaState::default(); 2];
    let mut at = HEADER_LEN;
    let data_end = HEADER_LEN
        .checked_add(h.size as usize)
        .filter(|&e| e <= bytes.len())
        .ok_or(AudError::Truncated)?;
    while at < data_end {
        let (Some(comp), Some(out), Some(id)) = (
            u16_at(bytes, at),
            u16_at(bytes, at + 2),
            u32_at(bytes, at + 4),
        ) else {
            return Err(AudError::Truncated);
        };
        if id != CHUNK_ID {
            return Err(AudError::Invalid(format!("bad chunk marker at {at}")));
        }
        let start = at + CHUNK_HEADER_LEN;
        let end = start + usize::from(comp);
        let input = bytes.get(start..end).ok_or(AudError::Truncated)?;
        if raw.len() + samples.len() * 2 + usize::from(out) > MAX_OUTPUT {
            return Err(AudError::Invalid("output too large".into()));
        }
        match h.compression {
            Compression::Westwood => ws_decode_chunk(input, usize::from(out), &mut raw)?,
            Compression::Ima => {
                if channels == 2 {
                    for pair in input.chunks_exact(2) {
                        let (l, r) = (pair[0], pair[1]);
                        let l1 = ima[0].decode(l & 0x0F);
                        let l2 = ima[0].decode(l >> 4);
                        let r1 = ima[1].decode(r & 0x0F);
                        let r2 = ima[1].decode(r >> 4);
                        samples.extend_from_slice(&[l1, r1, l2, r2]);
                    }
                } else {
                    for &b in input {
                        samples.push(ima[0].decode(b & 0x0F));
                        samples.push(ima[0].decode(b >> 4));
                    }
                }
            }
        }
        at = end;
    }
    if h.compression == Compression::Westwood {
        samples = if h.sixteen_bit {
            raw.chunks_exact(2)
                .map(|p| i16::from_le_bytes([p[0], p[1]]))
                .collect()
        } else {
            raw.iter().map(|&b| (i16::from(b) - 128) << 8).collect()
        };
    }
    samples.truncate(samples.len() - samples.len() % channels);
    Ok(Pcm {
        sample_rate: u32::from(h.sample_rate),
        channels: h.channels(),
        samples,
    })
}

/// Westwood ADPCM encoder for one chunk of 8-bit samples (our own, for test
/// fixtures). Uses 4-bit groups where two samples fit within `tolerance`,
/// runs for repeats and raw samples otherwise; the decoder then stays
/// within `tolerance` of the input.
pub fn ws_encode_chunk(input: &[u8], tolerance: i32) -> Vec<u8> {
    let best4 = |cur: i32, target: i32| -> (u8, i32) {
        let mut best = (8u8, cur);
        for (k, d) in WS_STEP4.iter().enumerate() {
            let v = (cur + d).clamp(0, 255);
            if (v - target).abs() < (best.1 - target).abs() {
                best = (k as u8, v);
            }
        }
        best
    };
    let mut out = Vec::new();
    let mut cur: i32 = 0x80;
    let mut i = 0;
    let mut group: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, group: &mut Vec<u8>| {
        if !group.is_empty() {
            out.push(0x40 | (group.len() as u8 - 1));
            out.append(group);
        }
    };
    while i < input.len() {
        let t = |k: usize| i32::from(input[k]);
        // A run of samples equal to the current one.
        let run = input[i..]
            .iter()
            .take(64)
            .take_while(|&&s| i32::from(s) == cur)
            .count();
        if run >= 3 {
            flush(&mut out, &mut group);
            out.push(0xC0 | (run as u8 - 1));
            i += run;
            continue;
        }
        if i + 1 < input.len() {
            let (n1, v1) = best4(cur, t(i));
            let (n2, v2) = best4(v1, t(i + 1));
            if (v1 - t(i)).abs() <= tolerance && (v2 - t(i + 1)).abs() <= tolerance {
                group.push(n1 | (n2 << 4));
                cur = v2;
                i += 2;
                if group.len() == 64 {
                    flush(&mut out, &mut group);
                }
                continue;
            }
        }
        flush(&mut out, &mut group);
        let d = t(i) - cur;
        if (-16..=15).contains(&d) {
            out.push(0x80 | 0x20 | (d as u8 & 0x1F));
        } else {
            out.push(0x80);
            out.push(input[i]);
        }
        cur = t(i);
        i += 1;
    }
    flush(&mut out, &mut group);
    out
}

/// Write an AUD file from PCM with our own encoders (test fixtures). For
/// Westwood ADPCM the samples are reduced to 8 bits first.
pub fn encode(pcm: &Pcm, compression: Compression) -> Vec<u8> {
    let channels = usize::from(pcm.channels.clamp(1, 2));
    let mut body = Vec::new();
    let mut out_total = 0usize;
    let mut chunk = |input: &[u8], out_len: usize, body: &mut Vec<u8>| {
        body.extend_from_slice(&(input.len() as u16).to_le_bytes());
        body.extend_from_slice(&(out_len as u16).to_le_bytes());
        body.extend_from_slice(&CHUNK_ID.to_le_bytes());
        body.extend_from_slice(input);
        out_total += out_len;
    };
    match compression {
        Compression::Westwood => {
            let bytes: Vec<u8> = pcm
                .samples
                .iter()
                .map(|&s| ((i32::from(s) + 32768 + 128) >> 8).clamp(0, 255) as u8)
                .collect();
            for part in bytes.chunks(1024) {
                let mut enc = ws_encode_chunk(part, 2);
                if enc.len() >= part.len() {
                    enc = part.to_vec(); // stored raw
                }
                chunk(&enc, part.len(), &mut body);
            }
        }
        Compression::Ima => {
            let mut st = [ImaState::default(); 2];
            for part in pcm.samples.chunks(2048) {
                let mut enc = Vec::with_capacity(part.len() / 2 + 1);
                // Two frames at a time: mono = one byte, stereo = L byte, R byte.
                for group in part.chunks(2 * channels) {
                    for (c, state) in st.iter_mut().enumerate().take(channels) {
                        let first = group.get(c).copied().unwrap_or(0);
                        let second = group.get(c + channels).copied().unwrap_or(first);
                        let lo = state.encode(first);
                        let hi = state.encode(second);
                        enc.push(lo | (hi << 4));
                    }
                }
                chunk(&enc, part.len() * 2, &mut body);
            }
        }
    }
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(&(pcm.sample_rate as u16).to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&(out_total as u32).to_le_bytes());
    let mut flags = if channels == 2 { 1 } else { 0 };
    if compression == Compression::Ima {
        flags |= 2;
    }
    out.push(flags);
    out.push(compression.code());
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, channels: u16, frames: usize) -> Pcm {
        // A deterministic integer "sine": a triangle wave plus a slow ramp.
        let mut samples = Vec::new();
        for i in 0..frames {
            let tri = |p: usize, a: i32| {
                let x = (i % p) as i32 * 4 * a / p as i32;
                if x < a {
                    x
                } else if x < 3 * a {
                    2 * a - x
                } else {
                    x - 4 * a
                }
            };
            samples.push(tri(100, 12000) as i16);
            if channels == 2 {
                samples.push(tri(37, 6000) as i16);
            }
        }
        Pcm {
            sample_rate: rate,
            channels,
            samples,
        }
    }

    fn max_err(a: &[i16], b: &[i16]) -> i32 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn ima_round_trip_mono_and_stereo() {
        for channels in [1, 2] {
            let pcm = tone(22050, channels, 5000);
            let aud = encode(&pcm, Compression::Ima);
            let back = decode(&aud).unwrap();
            assert_eq!(back.channels, channels);
            assert_eq!(back.sample_rate, 22050);
            assert_eq!(back.samples.len(), pcm.samples.len());
            // After the step size has adapted (first 50 frames), within 4 %
            // of full scale.
            let skip = 50 * usize::from(channels);
            let e = max_err(&back.samples[skip..], &pcm.samples[skip..]);
            assert!(e < 1300, "channels {channels}: max error {e}");
        }
    }

    #[test]
    fn westwood_round_trip_mono_and_stereo() {
        for channels in [1, 2] {
            let pcm = tone(11025, channels, 3000);
            let aud = encode(&pcm, Compression::Westwood);
            let back = decode(&aud).unwrap();
            assert_eq!(back.channels, channels);
            assert_eq!(back.samples.len(), pcm.samples.len());
            // 8-bit quantisation (128) plus the encoder tolerance (2 steps).
            let e = max_err(&back.samples, &pcm.samples);
            assert!(e <= 3 * 256 + 128, "channels {channels}: max error {e}");
            if channels == 1 {
                assert!(aud.len() < pcm.samples.len(), "it compresses");
            }
        }
    }

    /// Hand-built from the IMA specification: from predictor 0 / index 0,
    /// code 7 adds 7/8 + 7 + 7/2 + 7/4 = 0 + 7 + 3 + 1 = 11 and the index
    /// rises by 8; code 0xF then subtracts with step 16 (index 8):
    /// 2 + 16 + 8 + 4 = 30, so 11 - 30 = -19, index 16.
    #[test]
    fn ima_known_answers() {
        let mut s = ImaState::default();
        assert_eq!(s.decode(7), 11);
        assert_eq!(s.index, 8);
        assert_eq!(s.decode(0xF), -19);
        assert_eq!(s.index, 16);
        assert_eq!(s.decode(0), -19 + (34 >> 3));
        assert_eq!(s.index, 15);
        // Clamping at the ends of the table and the range.
        let mut hi = ImaState {
            predictor: 32760,
            index: 88,
        };
        assert_eq!(hi.decode(7), 32767);
        assert_eq!(hi.index, 88);
    }

    /// Hand-built from the Westwood ADPCM description, one command each.
    #[test]
    fn westwood_known_answers() {
        let mut out = Vec::new();
        // code 0, one byte: 2-bit deltas 3,0,1,2 -> +1,-2,-1,0 from 0x80.
        ws_decode_chunk(&[0x00, 0b10_01_00_11], 4, &mut out).unwrap();
        assert_eq!(out, [0x81, 0x7F, 0x7E, 0x7E]);
        // code 1, one byte: nibbles 0xF (+8) then 0x0 (-9); code 3 twice more.
        out.clear();
        ws_decode_chunk(&[0x40, 0x0F, 0xC1], 4, &mut out).unwrap();
        assert_eq!(out, [0x88, 0x7F, 0x7F, 0x7F]);
        // code 2 with bit 5: delta -1 (0x1F); then raw 2 bytes; then code 3
        // repeating the last raw sample 3 times.
        out.clear();
        ws_decode_chunk(&[0xBF, 0x81, 0x10, 0x20, 0xC2], 6, &mut out).unwrap();
        assert_eq!(out, [0x7F, 0x10, 0x20, 0x20, 0x20, 0x20]);
        // 5-bit delta is output as a byte without clipping: raw 0xFF, +15.
        out.clear();
        ws_decode_chunk(&[0x80, 0xFF, 0xAF, 0xC0], 3, &mut out).unwrap();
        assert_eq!(out, [0xFF, 0x0E, 0x0E]);
        // Equal sizes: stored raw.
        out.clear();
        ws_decode_chunk(&[1, 2, 3], 3, &mut out).unwrap();
        assert_eq!(out, [1, 2, 3]);
        // Clamping at 255.
        out.clear();
        ws_decode_chunk(&[0x80, 0xFE, 0x40, 0xFF], 3, &mut out).unwrap();
        assert_eq!(out, [0xFE, 0xFF, 0xFF]);
        // Runs out of input.
        out.clear();
        assert_eq!(
            ws_decode_chunk(&[0x41, 0x00], 4, &mut out),
            Err(AudError::Truncated)
        );
    }

    /// Stereo IMA: one byte of left nibbles, then one of right (`LL RR`).
    #[test]
    fn ima_stereo_layout() {
        let mut aud = Vec::new();
        aud.extend_from_slice(&22050u16.to_le_bytes());
        aud.extend_from_slice(&10u32.to_le_bytes());
        aud.extend_from_slice(&8u32.to_le_bytes());
        aud.push(3);
        aud.push(99);
        aud.extend_from_slice(&2u16.to_le_bytes());
        aud.extend_from_slice(&8u16.to_le_bytes());
        aud.extend_from_slice(&CHUNK_ID.to_le_bytes());
        aud.extend_from_slice(&[0x07, 0x00]);
        let pcm = decode(&aud).unwrap();
        assert_eq!(pcm.channels, 2);
        assert_eq!(pcm.samples, [11, 0, 13, 0]);
    }

    #[test]
    fn sixteen_bit_westwood_raw_chunks_decode_exactly() {
        let samples: Vec<i16> = vec![0, 1000, -1000, 32767, -32768, 7];
        let raw: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut aud = Vec::new();
        aud.extend_from_slice(&8000u16.to_le_bytes());
        aud.extend_from_slice(&((raw.len() + 8) as u32).to_le_bytes());
        aud.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        aud.push(2);
        aud.push(1);
        aud.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        aud.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        aud.extend_from_slice(&CHUNK_ID.to_le_bytes());
        aud.extend_from_slice(&raw);
        let pcm = decode(&aud).unwrap();
        assert_eq!(pcm.samples, samples);
        assert_eq!(pcm.duration_ms(), 0);
        let k = pcm.to_static_sound();
        assert_eq!(k.frames.len(), 6);
    }

    #[test]
    fn truncated_and_corrupt_files_are_errors() {
        let aud = encode(&tone(22050, 1, 3000), Compression::Ima);
        for cut in 0..aud.len() {
            assert!(decode(&aud[..cut]).is_err(), "cut at {cut}");
        }
        let mut bad = aud.clone();
        bad[11] = 7;
        assert!(matches!(decode(&bad), Err(AudError::Invalid(_))));
        let mut bad = aud.clone();
        bad[HEADER_LEN + 4] = 0;
        assert!(decode(&bad).is_err());
        let mut bad = aud;
        bad[0..2].copy_from_slice(&5u16.to_le_bytes());
        assert!(decode(&bad).is_err());
    }
}
