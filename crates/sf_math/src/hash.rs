//! [`Fnv1a64`]: a tiny, explicit, platform-independent hash for state
//! checksums.
//!
//! The simulation checksums its world every tick (see the PLAN.md section on
//! simulation and presentation) and the cross-platform golden tests hash
//! long computation traces with it. It is
//! **not** `std::hash::Hasher` on purpose: the standard `Hash` implementations
//! feed native-endian integers and `usize` length prefixes, which differ
//! between platforms. Here every integer is written as fixed-width
//! little-endian bytes, so the result depends only on the values written.
//!
//! FNV-1a is not cryptographic and not collision resistant against an
//! adversary; it is a fast change detector for desync checks and tests.

use crate::facing::Facing;
use crate::fx::Fx;
use crate::vec2::Vec2;

const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// 64-bit FNV-1a.
///
/// ```
/// use sf_math::hash::Fnv1a64;
/// let mut h = Fnv1a64::new();
/// h.write_bytes(b"foobar");
/// assert_eq!(h.finish(), 0x8594_4171_f739_67e8); // published test vector
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Fnv1a64(u64);

impl Default for Fnv1a64 {
    fn default() -> Fnv1a64 {
        Fnv1a64::new()
    }
}

impl Fnv1a64 {
    /// A fresh hasher.
    #[inline]
    pub const fn new() -> Fnv1a64 {
        Fnv1a64(OFFSET_BASIS)
    }

    /// Mixes in one byte.
    #[inline]
    pub const fn write_u8(&mut self, b: u8) {
        self.0 = (self.0 ^ b as u64).wrapping_mul(PRIME);
    }

    /// Mixes in a byte slice.
    #[inline]
    pub const fn write_bytes(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            self.write_u8(bytes[i]);
            i += 1;
        }
    }

    /// Mixes in a `u32` as 4 little-endian bytes.
    #[inline]
    pub const fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Mixes in a `u64` as 8 little-endian bytes.
    #[inline]
    pub const fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Mixes in an `i32` as 4 little-endian bytes.
    #[inline]
    pub const fn write_i32(&mut self, v: i32) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Mixes in an `i64` as 8 little-endian bytes.
    #[inline]
    pub const fn write_i64(&mut self, v: i64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Mixes in the raw bits of an [`Fx`].
    #[inline]
    pub const fn write_fx(&mut self, v: Fx) {
        self.write_i64(v.raw());
    }

    /// Mixes in both components of a [`Vec2`].
    #[inline]
    pub const fn write_vec2(&mut self, v: Vec2) {
        self.write_fx(v.x);
        self.write_fx(v.y);
    }

    /// Mixes in a [`Facing`].
    #[inline]
    pub const fn write_facing(&mut self, f: Facing) {
        self.write_u8(f.raw());
    }

    /// The hash of everything written so far.
    #[inline]
    pub const fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_test_vectors() {
        // From the FNV reference (isthe.com/chongo/tech/comp/fnv).
        let h = |s: &[u8]| {
            let mut h = Fnv1a64::new();
            h.write_bytes(s);
            h.finish()
        };
        assert_eq!(h(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(h(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(h(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn integers_are_little_endian_and_order_matters() {
        let mut a = Fnv1a64::new();
        a.write_u32(0x0403_0201);
        let mut b = Fnv1a64::new();
        b.write_bytes(&[1, 2, 3, 4]);
        assert_eq!(a, b);

        let mut c = Fnv1a64::new();
        c.write_u8(1);
        c.write_u8(2);
        let mut d = Fnv1a64::new();
        d.write_u8(2);
        d.write_u8(1);
        assert_ne!(c, d);

        let mut e = Fnv1a64::new();
        e.write_i64(-1);
        let mut f = Fnv1a64::new();
        f.write_u64(u64::MAX);
        assert_eq!(e, f);
    }
}
