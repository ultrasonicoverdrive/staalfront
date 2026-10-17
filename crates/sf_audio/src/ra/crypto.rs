//! The two ciphers of an encrypted Red Alert MIX header (clean-room, from the
//! public format description; see `docs/architecture.md`, "Original Red Alert
//! audio"):
//!
//! - **RSA**: the 80-byte "key source" after the flags is two 40-byte blocks.
//!   Each block is a little-endian number `c`; `m = c^e mod n` with the
//!   public key gives 39 bytes of plaintext (little-endian). The first 56
//!   bytes of the 78 are the Blowfish key.
//! - **Blowfish** (Schneier 1993) in ECB mode, with the two 32-bit halves of
//!   each 8-byte block read **little-endian** (the standard reads them
//!   big-endian). We swap each 4-byte word around a standard implementation.

use blowfish::Blowfish;
use blowfish::cipher::generic_array::GenericArray;
use blowfish::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use num_bigint::BigUint;

/// Westwood's public RSA key as published with the format (base64 of a DER
/// INTEGER holding the 320-bit modulus, big-endian); the exponent is 65537.
/// Source: the MIX format description on the C&C ModdingWiki
/// (<https://moddingwiki.shikadi.net/wiki/MIX_Format_(Westwood)>), which
/// reproduces the key Westwood shipped in its games. A format parameter, not
/// game content.
pub const WESTWOOD_PUBLIC_KEY_B64: &str =
    "AihRvNoIbTn85FZRYNZRcT+i6KpU+maCsEqr3Q5q+LDB5tH7Tz2qQ38V";
/// The public exponent used with [`WESTWOOD_PUBLIC_KEY_B64`].
pub const WESTWOOD_PUBLIC_EXPONENT: u32 = 65537;

/// Length of the RSA key source in an encrypted header.
pub const KEY_SOURCE_LEN: usize = 80;
/// Length of the Blowfish key taken from the decrypted key source.
pub const BLOWFISH_KEY_LEN: usize = 56;

/// An RSA key (`n`, exponent). The public key has `e`; a private test key
/// carries `d` in the same field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RsaKey {
    pub n: BigUint,
    pub e: BigUint,
}

impl RsaKey {
    /// Westwood's key, parsed from [`WESTWOOD_PUBLIC_KEY_B64`].
    pub fn westwood() -> RsaKey {
        let der = base64_decode(WESTWOOD_PUBLIC_KEY_B64).unwrap_or_default();
        // DER INTEGER: 0x02, length, big-endian magnitude.
        let n = match der.as_slice() {
            [0x02, len, rest @ ..] if usize::from(*len) == rest.len() => {
                BigUint::from_bytes_be(rest)
            }
            _ => BigUint::from(1u32),
        };
        RsaKey {
            n,
            e: BigUint::from(WESTWOOD_PUBLIC_EXPONENT),
        }
    }

    /// Bytes per encrypted block (the modulus length in bytes).
    pub fn block_len(&self) -> usize {
        usize::try_from(self.n.bits().div_ceil(8)).unwrap_or(0)
    }

    /// Apply the key to `input` in blocks of `in_block` little-endian bytes,
    /// each giving `out_block` bytes. Decrypting uses the public key with
    /// `(block_len, block_len - 1)`; a test fixture encrypts with the private
    /// key and `(block_len - 1, block_len)`.
    pub fn apply(&self, input: &[u8], in_block: usize, out_block: usize) -> Option<Vec<u8>> {
        if in_block == 0 || out_block == 0 || !input.len().is_multiple_of(in_block) {
            return None;
        }
        let mut out = Vec::with_capacity(input.len() / in_block * out_block);
        for block in input.chunks(in_block) {
            let m = BigUint::from_bytes_le(block).modpow(&self.e, &self.n);
            let mut bytes = m.to_bytes_le();
            if bytes.len() > out_block {
                return None;
            }
            bytes.resize(out_block, 0);
            out.extend_from_slice(&bytes);
        }
        Some(out)
    }

    /// Decrypt an 80-byte key source to the 56-byte Blowfish key.
    pub fn blowfish_key(&self, key_source: &[u8]) -> Option<[u8; BLOWFISH_KEY_LEN]> {
        let len = self.block_len();
        if len < 2 || key_source.len() != KEY_SOURCE_LEN {
            return None;
        }
        let plain = self.apply(key_source, len, len - 1)?;
        plain.get(..BLOWFISH_KEY_LEN)?.try_into().ok()
    }
}

/// Blowfish-ECB with Westwood's little-endian word order.
pub struct WwBlowfish(Blowfish);

fn swap_words(block: &mut [u8; 8]) {
    block[..4].reverse();
    block[4..].reverse();
}

impl WwBlowfish {
    pub fn new(key: &[u8]) -> Option<WwBlowfish> {
        Blowfish::new_from_slice(key).ok().map(WwBlowfish)
    }

    pub fn decrypt_block(&self, block: &mut [u8; 8]) {
        swap_words(block);
        self.0
            .decrypt_block(GenericArray::from_mut_slice(&mut block[..]));
        swap_words(block);
    }

    pub fn encrypt_block(&self, block: &mut [u8; 8]) {
        swap_words(block);
        self.0
            .encrypt_block(GenericArray::from_mut_slice(&mut block[..]));
        swap_words(block);
    }

    /// Decrypt whole blocks in place; a trailing partial block is left alone.
    pub fn decrypt(&self, data: &mut [u8]) {
        for chunk in data.chunks_exact_mut(8) {
            let mut b = [0u8; 8];
            b.copy_from_slice(chunk);
            self.decrypt_block(&mut b);
            chunk.copy_from_slice(&b);
        }
    }

    /// Encrypt whole blocks in place (fixtures).
    pub fn encrypt(&self, data: &mut [u8]) {
        for chunk in data.chunks_exact_mut(8) {
            let mut b = [0u8; 8];
            b.copy_from_slice(chunk);
            self.encrypt_block(&mut b);
            chunk.copy_from_slice(&b);
        }
    }
}

/// Standard base64 (RFC 4648) without line breaks; `None` on a bad character.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes().filter(|&c| c != b'=') {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A key pair for MIX test fixtures, never for real files. Multi-prime RSA
/// over five Mersenne primes (2^127-1, 2^89-1, 2^61-1, 2^31-1, 2^7-1): a
/// 315-bit modulus, so 40-byte blocks with 39 bytes of plaintext like
/// Westwood's key. Returns `(public, private)`.
pub fn test_keypair() -> (RsaKey, RsaKey) {
    let one = BigUint::from(1u32);
    let primes: Vec<BigUint> = [127u32, 89, 61, 31, 7]
        .iter()
        .map(|&k| (BigUint::from(1u32) << k) - &one)
        .collect();
    let n = primes.iter().fold(one.clone(), |a, p| a * p);
    let phi = primes.iter().fold(one.clone(), |a, p| a * (p - &one));
    let e = BigUint::from(WESTWOOD_PUBLIC_EXPONENT);
    let d = e.modinv(&phi).unwrap_or_else(|| one.clone());
    (RsaKey { n: n.clone(), e }, RsaKey { n, e: d })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn westwood_key_parses_to_a_320_bit_modulus() {
        let k = RsaKey::westwood();
        assert_eq!(k.block_len(), 40);
        assert!(k.n.bits() > 312);
    }

    #[test]
    fn base64_known_answers() {
        // RFC 4648 section 10.
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        assert!(base64_decode("a$b").is_none());
    }

    /// Schneier's published test vectors (standard, big-endian Blowfish):
    /// with the words swapped around our cipher it must give exactly these.
    #[test]
    fn blowfish_known_answer_vectors() {
        let vectors: [([u8; 8], [u8; 8], [u8; 8]); 3] = [
            (
                [0; 8],
                [0; 8],
                [0x4E, 0xF9, 0x97, 0x45, 0x61, 0x98, 0xDD, 0x78],
            ),
            (
                [0xFF; 8],
                [0xFF; 8],
                [0x51, 0x86, 0x6F, 0xD5, 0xB8, 0x5E, 0xCB, 0x8A],
            ),
            (
                [0x30, 0, 0, 0, 0, 0, 0, 0],
                [0x10, 0, 0, 0, 0, 0, 0, 0x01],
                [0x7D, 0x85, 0x6F, 0x9A, 0x61, 0x30, 0x63, 0xF2],
            ),
        ];
        for (key, plain, cipher) in vectors {
            let bf = WwBlowfish::new(&key).unwrap();
            let mut b = plain;
            swap_words(&mut b);
            bf.encrypt_block(&mut b);
            swap_words(&mut b);
            assert_eq!(b, cipher);
            swap_words(&mut b);
            bf.decrypt_block(&mut b);
            swap_words(&mut b);
            assert_eq!(b, plain);
        }
    }

    #[test]
    fn little_endian_words_differ_from_the_standard_order() {
        let bf = WwBlowfish::new(&[0; 8]).unwrap();
        let mut b = [0x01, 0, 0, 0, 0, 0, 0, 0];
        bf.encrypt_block(&mut b);
        let mut s = [0, 0, 0, 0x01, 0, 0, 0, 0];
        bf.encrypt_block(&mut s);
        assert_ne!(b, s);
        let mut data = [7u8; 20];
        bf.encrypt(&mut data);
        assert_eq!(&data[16..], &[7; 4], "partial block untouched");
        bf.decrypt(&mut data);
        assert_eq!(data, [7u8; 20]);
    }

    #[test]
    fn rsa_round_trip_with_the_test_key() {
        let (public, private) = test_keypair();
        assert_eq!(public.block_len(), 40);
        let key: Vec<u8> = (0..78u8).map(|i| i.wrapping_mul(37) ^ 0x5A).collect();
        let source = private.apply(&key, 39, 40).unwrap();
        assert_eq!(source.len(), KEY_SOURCE_LEN);
        let bf = public.blowfish_key(&source).unwrap();
        assert_eq!(&bf[..], &key[..BLOWFISH_KEY_LEN]);
        assert!(public.blowfish_key(&source[..79]).is_none());
    }
}
