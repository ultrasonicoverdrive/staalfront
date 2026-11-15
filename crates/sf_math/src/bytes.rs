//! A little-endian byte writer and a checking reader (WP13.1a, `docs/rules/save.md`).
//!
//! The save file of the simulation is written with these two and nothing else: fixed-width
//! integers in little-endian order, a `usize` as a `u32`, byte runs with a `u32` length in front,
//! and sections of a tag byte and a `u32` length. The same bytes come out on every platform.
//!
//! The reader is meant for untrusted input. Every read returns `Err` on input that is too short or
//! malformed and never panics, and nothing it hands out is larger than the input that is left (a
//! length field cannot make it allocate more than the file holds).
//!
//! ```
//! use sf_math::bytes::{ByteReader, ByteWriter};
//! let mut w = ByteWriter::new();
//! w.u32(7);
//! w.bytes(b"ore").unwrap();
//! let data = w.finish();
//! let mut r = ByteReader::new(&data);
//! assert_eq!(r.u32(), Ok(7));
//! assert_eq!(r.bytes(), Ok(&b"ore"[..]));
//! assert!(r.is_empty());
//! assert!(r.u8().is_err());
//! ```

use alloc::vec::Vec;
use core::fmt;

/// Why a read or a write failed.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ByteError {
    /// The input ended before the value did.
    Truncated,
    /// A `usize` (a length, a count) does not fit the `u32` it is written as.
    TooLong,
    /// A byte that should be 0 or 1 (a bool) was neither.
    BadBool,
    /// A section ended with bytes left over, or its tag was not the one expected.
    BadSection,
}

impl fmt::Display for ByteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ByteError::Truncated => "the input ended early",
            ByteError::TooLong => "a length does not fit 32 bits",
            ByteError::BadBool => "a flag byte is neither 0 nor 1",
            ByteError::BadSection => "a section has the wrong tag or length",
        })
    }
}

impl core::error::Error for ByteError {}

/// Appends values to a byte vector, little-endian.
#[derive(Clone, Debug, Default)]
pub struct ByteWriter {
    out: Vec<u8>,
}

impl ByteWriter {
    /// An empty writer.
    pub fn new() -> ByteWriter {
        ByteWriter::default()
    }

    /// The bytes written so far.
    pub fn finish(self) -> Vec<u8> {
        self.out
    }

    /// The number of bytes written so far.
    pub fn len(&self) -> usize {
        self.out.len()
    }

    /// True if nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    /// One byte.
    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    /// A bool as one byte: 1 for true, 0 for false.
    pub fn bool(&mut self, v: bool) {
        self.out.push(u8::from(v));
    }

    /// Two bytes.
    pub fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    /// Four bytes.
    pub fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    /// Eight bytes.
    pub fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    /// Four bytes, two's complement.
    pub fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    /// Eight bytes, two's complement.
    pub fn i64(&mut self, v: i64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    /// A `usize` as a `u32`; [`ByteError::TooLong`] (and nothing written) if it does not fit.
    pub fn usize(&mut self, v: usize) -> Result<(), ByteError> {
        let v = u32::try_from(v).map_err(|_| ByteError::TooLong)?;
        self.u32(v);
        Ok(())
    }

    /// A `u32` length, then the bytes.
    pub fn bytes(&mut self, v: &[u8]) -> Result<(), ByteError> {
        self.usize(v.len())?;
        self.out.extend_from_slice(v);
        Ok(())
    }

    /// A section: the tag, a `u32` length, then the bytes `body` writes.
    pub fn section(
        &mut self,
        tag: u8,
        body: impl FnOnce(&mut ByteWriter) -> Result<(), ByteError>,
    ) -> Result<(), ByteError> {
        let mut inner = ByteWriter::new();
        body(&mut inner)?;
        self.u8(tag);
        self.bytes(&inner.out)
    }
}

/// Reads values back from a byte slice, checking every length.
#[derive(Clone, Debug)]
pub struct ByteReader<'a> {
    rest: &'a [u8],
}

impl<'a> ByteReader<'a> {
    /// A reader over `data`.
    pub fn new(data: &'a [u8]) -> ByteReader<'a> {
        ByteReader { rest: data }
    }

    /// The number of bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.rest.len()
    }

    /// True if everything has been read.
    pub fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }

    /// The next `n` bytes, or [`ByteError::Truncated`] (and nothing consumed).
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], ByteError> {
        if n > self.rest.len() {
            return Err(ByteError::Truncated);
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ByteError> {
        let mut a = [0u8; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }

    /// One byte.
    pub fn u8(&mut self) -> Result<u8, ByteError> {
        Ok(self.array::<1>()?[0])
    }

    /// A bool; [`ByteError::BadBool`] for any other byte than 0 and 1.
    pub fn bool(&mut self) -> Result<bool, ByteError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ByteError::BadBool),
        }
    }

    /// Two bytes.
    pub fn u16(&mut self) -> Result<u16, ByteError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// Four bytes.
    pub fn u32(&mut self) -> Result<u32, ByteError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// Eight bytes.
    pub fn u64(&mut self) -> Result<u64, ByteError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// Four bytes, two's complement.
    pub fn i32(&mut self) -> Result<i32, ByteError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    /// Eight bytes, two's complement.
    pub fn i64(&mut self) -> Result<i64, ByteError> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    /// A `usize` written as a `u32`.
    pub fn usize(&mut self) -> Result<usize, ByteError> {
        usize::try_from(self.u32()?).map_err(|_| ByteError::TooLong)
    }

    /// A count of items that take at least `min_bytes` each: [`ByteError::Truncated`] if the rest
    /// of the input cannot hold that many, so a caller may reserve `count` items safely.
    pub fn count(&mut self, min_bytes: usize) -> Result<usize, ByteError> {
        let n = self.usize()?;
        if n.saturating_mul(min_bytes.max(1)) > self.rest.len() {
            return Err(ByteError::Truncated);
        }
        Ok(n)
    }

    /// A byte run with a `u32` length in front.
    pub fn bytes(&mut self) -> Result<&'a [u8], ByteError> {
        let n = self.usize()?;
        self.take(n)
    }

    /// A section with the given tag: its body, as a reader of its own. [`ByteError::BadSection`]
    /// for another tag.
    pub fn section(&mut self, tag: u8) -> Result<ByteReader<'a>, ByteError> {
        if self.u8()? != tag {
            return Err(ByteError::BadSection);
        }
        Ok(ByteReader::new(self.bytes()?))
    }

    /// [`ByteError::BadSection`] unless everything has been read.
    pub fn end(&self) -> Result<(), ByteError> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(ByteError::BadSection)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn sample() -> Vec<u8> {
        let mut w = ByteWriter::new();
        w.u8(0xAB);
        w.bool(true);
        w.bool(false);
        w.u16(0xBEEF);
        w.u32(0xDEAD_BEEF);
        w.u64(0x0123_4567_89AB_CDEF);
        w.i32(-5);
        w.i64(i64::MIN);
        w.usize(70_000).unwrap();
        w.bytes(b"staal").unwrap();
        w.section(7, |s| {
            s.u32(9);
            Ok(())
        })
        .unwrap();
        w.finish()
    }

    /// Reads `sample` back without judging the values (so a flipped bit cannot trip an assert).
    fn parse(data: &[u8]) -> Result<Vec<u64>, ByteError> {
        let mut r = ByteReader::new(data);
        let mut v = Vec::new();
        v.push(u64::from(r.u8()?));
        v.push(u64::from(r.bool()?));
        v.push(u64::from(r.bool()?));
        v.push(u64::from(r.u16()?));
        v.push(u64::from(r.u32()?));
        v.push(r.u64()?);
        v.push(r.i32()? as u64);
        v.push(r.i64()? as u64);
        v.push(r.usize()? as u64);
        v.extend(r.bytes()?.iter().map(|&b| u64::from(b)));
        let mut s = r.section(7)?;
        v.push(u64::from(s.u32()?));
        s.end()?;
        r.end()?;
        Ok(v)
    }

    fn read_all(data: &[u8]) -> Result<(), ByteError> {
        let v = parse(data)?;
        let mut want = std::vec![
            0xAB,
            1,
            0,
            0xBEEF,
            0xDEAD_BEEF,
            0x0123_4567_89AB_CDEF,
            -5i64 as u64,
            i64::MIN as u64,
            70_000,
        ];
        want.extend(b"staal".iter().map(|&b| u64::from(b)));
        want.push(9);
        assert_eq!(v, want);
        Ok(())
    }

    #[test]
    fn every_primitive_round_trips_little_endian() {
        let data = sample();
        read_all(&data).unwrap();
        // Little-endian, fixed width: the u16 is EF BE, the u32 follows it.
        assert_eq!(&data[3..9], &[0xEF, 0xBE, 0xEF, 0xBE, 0xAD, 0xDE]);
        assert_eq!(data.len(), 1 + 2 + 2 + 4 + 8 + 4 + 8 + 4 + 4 + 5 + 1 + 4 + 4);
    }

    #[test]
    fn every_truncation_is_an_error() {
        let data = sample();
        for cut in 0..data.len() {
            let r = std::panic::catch_unwind(|| read_all(&data[..cut]));
            assert!(matches!(r, Ok(Err(_))), "cut at {cut}");
        }
    }

    #[test]
    fn bit_flips_never_panic() {
        let data = sample();
        for byte in 0..data.len() {
            for bit in 0..8 {
                let mut d = data.clone();
                d[byte] ^= 1 << bit;
                // Generic reads that do not know the values: lengths, sections, bools.
                let r = std::panic::catch_unwind(|| {
                    let mut r = ByteReader::new(&d);
                    let _ = r.u8().and_then(|_| r.bool()).and_then(|_| r.bool());
                    let _ = r.take(16);
                    let _ = r.usize().and_then(|_| r.bytes());
                    let _ = r.section(7).and_then(|s| s.end());
                    let _ = parse(&d);
                });
                assert!(r.is_ok(), "flip {byte}:{bit} panicked");
            }
        }
    }

    #[test]
    fn a_length_beyond_the_input_is_refused_before_anything_is_allocated() {
        let mut w = ByteWriter::new();
        w.u32(u32::MAX);
        let data = w.finish();
        assert_eq!(ByteReader::new(&data).bytes(), Err(ByteError::Truncated));
        assert_eq!(ByteReader::new(&data).count(1), Err(ByteError::Truncated));
        let mut w = ByteWriter::new();
        w.u32(3);
        w.u64(0);
        let data = w.finish();
        assert_eq!(ByteReader::new(&data).count(4), Err(ByteError::Truncated));
        assert_eq!(ByteReader::new(&data).count(2), Ok(3));
    }

    #[test]
    fn bad_bools_and_sections_are_errors() {
        assert_eq!(ByteReader::new(&[2]).bool(), Err(ByteError::BadBool));
        let mut w = ByteWriter::new();
        w.section(1, |_| Ok(())).unwrap();
        let data = w.finish();
        assert_eq!(
            ByteReader::new(&data).section(2).err(),
            Some(ByteError::BadSection)
        );
        assert!(ByteReader::new(&[1]).end().is_err());
    }

    #[test]
    fn a_usize_over_32_bits_is_refused() {
        let mut w = ByteWriter::new();
        if let Ok(big) = usize::try_from(u64::from(u32::MAX) + 1) {
            assert_eq!(w.usize(big), Err(ByteError::TooLong));
            assert!(w.is_empty());
        }
    }
}
