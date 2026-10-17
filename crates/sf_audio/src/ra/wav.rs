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

/// Format and data location of a 16-bit PCM WAV.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub channels: u16,
    pub sample_rate: u32,
    /// Byte offset and length of the sample data as the file declares them.
    pub data_start: usize,
    pub data_len: usize,
}

impl Header {
    /// Playing time of the declared data in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        let frames = (self.data_len / 2 / usize::from(self.channels)) as u64;
        frames * 1000 / u64::from(self.sample_rate)
    }
}

/// Read the chunk headers of `b` up to the data chunk. `b` may be only the
/// first bytes of a file of `file_len` bytes (the scanner reads a window of
/// 64 KiB, not whole tracks); the data chunk's declared length is returned,
/// not checked against `b`. If the chunks before the data chunk do not fit in
/// `b` although the file goes on, the error says so ("WAV header larger than
/// 64 KiB") instead of claiming the data chunk is missing. A whole file is
/// `header(b, b.len())`.
pub fn header(b: &[u8], file_len: usize) -> Result<Header, String> {
    if b.get(0..4) != Some(b"RIFF") || b.get(8..12) != Some(b"WAVE") {
        return Err("not a RIFF/WAVE file".into());
    }
    let truncated = b.len() < file_len;
    let window = || format!("WAV header larger than {} KiB", b.len() / 1024);
    let mut at = 12usize;
    let mut fmt: Option<(u16, u32)> = None;
    while let (Some(id), Some(len)) = (b.get(at..at + 4), u32_at(b, at + 4)) {
        let start = at + 8;
        let len = len as usize;
        match id {
            b"fmt " => {
                let body = b.get(start..start.saturating_add(len)).ok_or_else(|| {
                    if truncated && start.saturating_add(len) <= file_len {
                        window()
                    } else {
                        "WAV chunk runs past the end".to_string()
                    }
                })?;
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
                return Ok(Header {
                    channels,
                    sample_rate,
                    data_start: start,
                    data_len: len,
                });
            }
            _ => {}
        }
        at = start
            .checked_add(len)
            .and_then(|e| e.checked_add(len & 1))
            .ok_or("WAV chunk runs past the end")?;
    }
    if at > file_len {
        Err("WAV chunk runs past the end".into())
    } else if truncated && at < file_len {
        Err(window())
    } else {
        Err("WAV has no data chunk".into())
    }
}

/// Decode a 16-bit PCM WAV file.
pub fn read(b: &[u8]) -> Result<Pcm, String> {
    let h = header(b, b.len())?;
    let end = h
        .data_start
        .checked_add(h.data_len)
        .filter(|&e| e <= b.len())
        .ok_or("WAV chunk runs past the end")?;
    if h.data_len > MAX_OUTPUT {
        return Err("WAV data too large".into());
    }
    let samples = b[h.data_start..end]
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]))
        .collect();
    Ok(Pcm {
        sample_rate: h.sample_rate,
        channels: h.channels,
        samples,
    })
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
        // The header alone is enough for the scanner.
        let h = header(&w[..44], w.len()).unwrap();
        assert_eq!(
            (h.channels, h.sample_rate, h.data_start, h.data_len),
            (2, 22050, 44, 12)
        );
        assert_eq!(h.duration_ms(), 0);
        let mut eight = w.clone();
        eight[34] = 8;
        assert!(read(&eight).is_err());
    }

    /// RIFF + `fmt ` + a `LIST` chunk of `pad` bytes + `data`.
    fn with_list_chunk(pad: usize, samples: &[i16]) -> Vec<u8> {
        let plain = write(&Pcm {
            sample_rate: 22050,
            channels: 1,
            samples: samples.to_vec(),
        });
        let mut v = plain[..36].to_vec(); // RIFF header and fmt chunk
        v.extend_from_slice(b"LIST");
        v.extend_from_slice(&(pad as u32).to_le_bytes());
        v.resize(v.len() + pad, 0);
        v.extend_from_slice(&plain[36..]);
        v
    }

    #[test]
    fn a_header_beyond_the_scan_window_is_reported_as_such() {
        const WINDOW: usize = 64 * 1024;
        let w = with_list_chunk(70_000, &[1, 2, 3, 4]);
        // Whole file: the big chunk is just skipped.
        let pcm = read(&w).unwrap();
        assert_eq!(pcm.samples, [1, 2, 3, 4]);
        // Only the first 64 KiB seen: the data chunk lies beyond them.
        let err = header(&w[..WINDOW], w.len()).unwrap_err();
        assert_eq!(err, "WAV header larger than 64 KiB");
        // A window that ends inside a chunk header or inside the fmt chunk.
        for cut in [38, 40, 44] {
            let err = header(&w[..cut], w.len()).unwrap_err();
            assert!(err.contains("larger than"), "cut {cut}: {err}");
        }
        // A genuinely missing data chunk is still that, in a whole file and
        // in a window that covers the whole file.
        let none = &w[..36 + 8 + 70_000];
        assert_eq!(
            header(none, none.len()).unwrap_err(),
            "WAV has no data chunk"
        );
        // A chunk that claims more than the file holds runs past the end.
        let mut lie = w[..36 + 8 + 100].to_vec();
        lie[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            header(&lie, lie.len()).unwrap_err(),
            "WAV chunk runs past the end"
        );
    }
}
