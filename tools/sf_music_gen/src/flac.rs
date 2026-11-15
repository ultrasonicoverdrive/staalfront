//! A small FLAC encoder: mono, 16 bit, fixed predictors of order 0 to 4, Rice-coded residuals, no
//! dependency (WP11.4b). Integer maths only, so the bytes are the same on every platform. Lossless: a decoder
//! gives back exactly the samples (a test decodes the committed files with the game's decoder and compares).
//!
//! Choices that keep it short: one block size (4096) except for the last block, constant 16 bit mono, the MD5
//! of the audio in STREAMINFO left at zero (the format says zero means "not computed"), no seek table, no
//! tags.

const BLOCK: usize = 4096;

struct Bits {
    bytes: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn new() -> Bits {
        Bits {
            bytes: Vec::new(),
            acc: 0,
            n: 0,
        }
    }

    /// The low `count` bits of `v`, most significant first (`count` up to 32).
    fn put(&mut self, v: u32, count: u32) {
        if count == 0 {
            return;
        }
        self.acc = (self.acc << count) | (u64::from(v) & ((1u64 << count) - 1));
        self.n += count;
        while self.n >= 8 {
            self.n -= 8;
            self.bytes.push((self.acc >> self.n) as u8);
        }
    }

    /// `q` zero bits and a one bit.
    fn unary(&mut self, mut q: u32) {
        while q >= 24 {
            self.put(0, 24);
            q -= 24;
        }
        self.put(1, q + 1);
    }

    fn align(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put(0, pad);
        }
    }
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for b in bytes {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for b in bytes {
        crc ^= u16::from(*b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// The residual of the fixed predictor of `order` over `x` (the first `order` samples are the warm-up).
fn residual(x: &[i32], order: usize) -> Vec<i32> {
    (order..x.len())
        .map(|i| match order {
            0 => x[i],
            1 => x[i] - x[i - 1],
            2 => x[i] - 2 * x[i - 1] + x[i - 2],
            3 => x[i] - 3 * x[i - 1] + 3 * x[i - 2] - x[i - 3],
            _ => x[i] - 4 * x[i - 1] + 6 * x[i - 2] - 4 * x[i - 3] + x[i - 4],
        })
        .collect()
}

fn zigzag(r: i32) -> u32 {
    ((r << 1) ^ (r >> 31)) as u32
}

/// Cheapest Rice parameter (0..=14) of a partition and its size in bits.
fn best_param(part: &[i32]) -> (u32, u64) {
    let mut best = (0, u64::MAX);
    for k in 0..=14u32 {
        let bits: u64 = part
            .iter()
            .map(|r| u64::from(zigzag(*r) >> k) + 1 + u64::from(k))
            .sum();
        if bits < best.1 {
            best = (k, bits);
        }
    }
    best
}

/// (partition order, total bits of the parameters and codes) with the lowest cost.
fn best_partitioning(res: &[i32], block: usize, order: usize) -> (u32, u64) {
    let mut best = (0, u64::MAX);
    for po in 0..=6u32 {
        let parts = 1usize << po;
        if !block.is_multiple_of(parts) || block / parts <= order {
            break;
        }
        let size = block / parts;
        let mut bits = 4u64 * parts as u64;
        let mut start = 0;
        for p in 0..parts {
            let len = if p == 0 { size - order } else { size };
            bits += best_param(&res[start..start + len]).1;
            start += len;
        }
        if bits < best.1 {
            best = (po, bits);
        }
    }
    best
}

fn write_utf8(out: &mut Vec<u8>, v: u32) {
    // Frame numbers stay below 2^21 here.
    if v < 0x80 {
        out.push(v as u8);
    } else if v < 0x800 {
        out.push(0xC0 | (v >> 6) as u8);
        out.push(0x80 | (v & 0x3F) as u8);
    } else if v < 0x1_0000 {
        out.push(0xE0 | (v >> 12) as u8);
        out.push(0x80 | ((v >> 6) & 0x3F) as u8);
        out.push(0x80 | (v & 0x3F) as u8);
    } else {
        out.push(0xF0 | (v >> 18) as u8);
        out.push(0x80 | ((v >> 12) & 0x3F) as u8);
        out.push(0x80 | ((v >> 6) & 0x3F) as u8);
        out.push(0x80 | (v & 0x3F) as u8);
    }
}

fn frame(x: &[i32], number: u32) -> Vec<u8> {
    let block = x.len();
    // Try every fixed order, keep the smallest.
    let mut best: Option<(usize, Vec<i32>, u32, u64)> = None;
    for order in 0..=4usize {
        if block <= order + 1 {
            break;
        }
        let res = residual(x, order);
        let (po, bits) = best_partitioning(&res, block, order);
        let total = bits + 16 * order as u64;
        if best.as_ref().is_none_or(|b| total < b.3) {
            best = Some((order, res, po, total));
        }
    }
    let (order, res, po, _) = best.expect("a block has samples");

    let mut head = vec![0xFF, 0xF8];
    // Block size: 4096 is code 12; any other size is written out after the header (code 7).
    let code = if block == BLOCK { 12u8 } else { 7 };
    head.push(code << 4); // sample rate: from STREAMINFO
    head.push(0x08); // mono, 16 bits
    write_utf8(&mut head, number);
    if code == 7 {
        head.extend_from_slice(&((block - 1) as u16).to_be_bytes());
    }
    head.push(crc8(&head));

    let mut w = Bits::new();
    w.put(0, 1);
    w.put(8 + order as u32, 6);
    w.put(0, 1);
    for s in &x[..order] {
        w.put(*s as u32, 16);
    }
    w.put(0, 2); // Rice with 4-bit parameters
    w.put(po, 4);
    let parts = 1usize << po;
    let size = block >> po;
    let mut start = 0;
    for p in 0..parts {
        let len = if p == 0 { size - order } else { size };
        let part = &res[start..start + len];
        let (k, _) = best_param(part);
        w.put(k, 4);
        for r in part {
            let u = zigzag(*r);
            w.unary(u >> k);
            w.put(u, k);
        }
        start += len;
    }
    w.align();

    let mut out = head;
    out.extend_from_slice(&w.bytes);
    let crc = crc16(&out);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

/// A complete FLAC file of mono 16-bit samples at `rate` Hz.
pub fn encode(samples: &[i16], rate: u32) -> Vec<u8> {
    let mut out = b"fLaC".to_vec();
    // STREAMINFO: the last metadata block, 34 bytes.
    out.push(0x80);
    out.extend_from_slice(&[0, 0, 34]);
    out.extend_from_slice(&(BLOCK as u16).to_be_bytes()); // min block size
    out.extend_from_slice(&(BLOCK as u16).to_be_bytes()); // max block size
    out.extend_from_slice(&[0; 6]); // min and max frame size: unknown
    let total = samples.len() as u64;
    // 20 bits rate, 3 bits channels-1, 5 bits bits-1, 36 bits total samples
    let packed: u64 = (u64::from(rate) << 44) | (15u64 << 36) | total;
    out.extend_from_slice(&packed.to_be_bytes());
    out.extend_from_slice(&[0; 16]); // MD5: not computed
    let ints: Vec<i32> = samples.iter().map(|s| i32::from(*s)).collect();
    for (i, chunk) in ints.chunks(BLOCK).enumerate() {
        out.extend_from_slice(&frame(chunk, i as u32));
    }
    out
}
