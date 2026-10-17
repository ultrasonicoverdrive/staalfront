//! 16-bit PCM WAV, the output of `sf_ra_extract` and an input of the
//! Red Alert loader (a "WAV folder").
//!
//! Format: the RIFF/WAVE layout of Microsoft's "Multimedia Programming
//! Interface and Data Specifications 1.0" (1991): `RIFF` size `WAVE`, then
//! chunks `id size data` padded to even length; `fmt ` with format tag 1
//! (PCM), channels, sample rate, byte rate, block align, bits per sample;
//! `data` with interleaved little-endian samples. Only 16-bit PCM, mono or
//! stereo, is read; anything else is an error, never a panic.

use super::aud::{MAX_OUTPUT, Pcm};

/// Encode as a 16-bit PCM WAV file.
pub fn write(pcm: &Pcm) -> Vec<u8> {
    let channels = pcm.channels.max(1);
    let data_len = pcm.samples.len() * 2;
    let mut v = Vec::with_capacity(44 + data_len);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&pcm.sample_rate.to_le_bytes());
    v.extend_from_slice(&(pcm.sample_rate * u32::from(channels) * 2).to_le_bytes());
    v.extend_from_slice(&(channels * 2).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in &pcm.samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Decode a 16-bit PCM WAV file.
pub fn read(b: &[u8]) -> Result<Pcm, String> {
    if b.get(0..4) != Some(b"RIFF") || b.get(8..12) != Some(b"WAVE") {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut at = 12usize;
    let mut fmt: Option<(u16, u32)> = None;
    while let (Some(id), Some(len)) = (b.get(at..at + 4), u32_at(b, at + 4)) {
        let start = at + 8;
        let len = len as usize;
        let end = start
            .checked_add(len)
            .filter(|&e| e <= b.len())
            .ok_or("WAV chunk runs past the end")?;
        let body = &b[start..end];
        match id {
            b"fmt " => {
                let tag = u16_at(body, 0).ok_or("short fmt chunk")?;
                let channels = u16_at(body, 2).ok_or("short fmt chunk")?;
                let rate = u32_at(body, 4).ok_or("short fmt chunk")?;
                let bits = u16_at(body, 14).ok_or("short fmt chunk")?;
                if tag != 1 || bits != 16 {
                    return Err(format!("only 16-bit PCM WAV (tag {tag}, {bits} bits)"));
                }
                if !(1..=2).contains(&channels) || rate == 0 {
                    return Err(format!("unsupported WAV: {channels} channels, {rate} Hz"));
                }
                fmt = Some((channels, rate));
            }
            b"data" => {
                let (channels, sample_rate) = fmt.ok_or("WAV data before fmt")?;
                if body.len() > MAX_OUTPUT {
                    return Err("WAV data too large".into());
                }
                let samples = body
                    .chunks_exact(2)
                    .map(|p| i16::from_le_bytes([p[0], p[1]]))
                    .collect();
                return Ok(Pcm {
                    sample_rate,
                    channels,
                    samples,
                });
            }
            _ => {}
        }
        at = end + (len & 1);
    }
    Err("WAV has no data chunk".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_garbage() {
        let pcm = Pcm {
            sample_rate: 22050,
            channels: 2,
            samples: vec![1, -2, 300, -32768, 32767, 0],
        };
        let w = write(&pcm);
        assert_eq!(w.len(), 44 + 12);
        assert_eq!(read(&w).unwrap(), pcm);
        for cut in 0..w.len() {
            assert!(read(&w[..cut]).is_err(), "cut at {cut}");
        }
        assert!(read(b"RIFF\xff\xff\xff\xffWAVEdata\xff\xff\xff\xff").is_err());
        let mut eight = w.clone();
        eight[34] = 8;
        assert!(read(&eight).is_err());
    }
}
