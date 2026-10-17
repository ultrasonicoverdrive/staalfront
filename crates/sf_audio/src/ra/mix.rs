//! MIX archives (Westwood, Tiberian Dawn / Red Alert), read from the public
//! format description:
//!
//! - **Old (TD) header**: `u16 count`, `u32 body size`, then `count` entries
//!   of `u32 id, u32 offset, u32 size` (little-endian). The body follows.
//! - **Red Alert header**: a `u16` 0, then `u16 flags` (bit 0: a 20-byte SHA-1
//!   trails the body, ignored here; bit 1: the header is encrypted). Plain:
//!   the old header follows. Encrypted: an 80-byte RSA key source (see
//!   [`super::crypto`]), then the old header Blowfish-encrypted and padded to
//!   whole 8-byte blocks.
//! - Entry offsets count from the start of the body. Entries are looked up by
//!   [`mix_id`], a hash of the upper-cased file name; names are not stored.
//!
//! Every reader returns a [`MixError`] on truncated or inconsistent input,
//! never panics, and never allocates more than the input can justify.

use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::crypto::{BLOWFISH_KEY_LEN, KEY_SOURCE_LEN, RsaKey, WwBlowfish};

const FLAG_CHECKSUM: u16 = 1;
const FLAG_ENCRYPTED: u16 = 2;
/// Bytes per index entry.
const ENTRY_LEN: usize = 12;
/// Largest entry read into memory (the biggest Red Alert files are a few MB).
pub const MAX_ENTRY_SIZE: u32 = 64 * 1024 * 1024;

/// The file-name hash of TD/RA MIX files: upper-case the name, take it four
/// bytes at a time as a little-endian `u32` (the last group zero-padded) and
/// fold `id = rotate_left(id, 1) + group`.
pub fn mix_id(name: &str) -> u32 {
    let upper: Vec<u8> = name.bytes().map(|b| b.to_ascii_uppercase()).collect();
    upper.chunks(4).fold(0u32, |id, group| {
        let mut b = [0u8; 4];
        b[..group.len()].copy_from_slice(group);
        id.rotate_left(1).wrapping_add(u32::from_le_bytes(b))
    })
}

/// Parse a name or a raw id written `#1A2B3C4D` (as the sound browser shows
/// entries whose name is unknown).
pub fn id_for(name_or_id: &str) -> u32 {
    name_or_id
        .strip_prefix('#')
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .unwrap_or_else(|| mix_id(name_or_id))
}

#[derive(Debug)]
pub enum MixError {
    Io(std::io::Error),
    /// Ends before the header or an entry does.
    Truncated,
    /// The header does not describe a MIX file.
    Invalid(String),
    /// Encrypted header whose key source does not decrypt.
    BadKey,
    NotFound(String),
}

impl fmt::Display for MixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MixError::Io(e) => write!(f, "read error: {e}"),
            MixError::Truncated => write!(f, "file is truncated"),
            MixError::Invalid(why) => write!(f, "not a MIX file: {why}"),
            MixError::BadKey => write!(f, "encrypted header does not decrypt"),
            MixError::NotFound(n) => write!(f, "'{n}' is not in the archive"),
        }
    }
}

impl std::error::Error for MixError {}

impl From<std::io::Error> for MixError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            MixError::Truncated
        } else {
            MixError::Io(e)
        }
    }
}

/// One file in an archive; `offset` is absolute within the container that
/// was indexed (the disk file, also for nested archives).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixEntry {
    pub id: u32,
    pub offset: u64,
    pub size: u32,
}

/// The index of one MIX archive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MixIndex {
    pub encrypted: bool,
    pub checksum: bool,
    /// Sorted by id.
    pub entries: Vec<MixEntry>,
}

impl MixIndex {
    pub fn find_id(&self, id: u32) -> Option<&MixEntry> {
        self.entries
            .binary_search_by_key(&id, |e| e.id)
            .ok()
            .map(|i| &self.entries[i])
    }

    /// Look up by file name (or `#hex` id).
    pub fn find(&self, name: &str) -> Option<&MixEntry> {
        self.find_id(id_for(name))
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn read_exact_vec<R: Read>(r: &mut R, len: usize) -> Result<Vec<u8>, MixError> {
    let mut v = vec![0u8; len];
    r.read_exact(&mut v)?;
    Ok(v)
}

/// Read the index of the archive that occupies `len` bytes at `start` in `r`.
/// `key` decrypts an encrypted header (normally [`RsaKey::westwood`]).
pub fn read_index<R: Read + Seek>(
    r: &mut R,
    start: u64,
    len: u64,
    key: &RsaKey,
) -> Result<MixIndex, MixError> {
    r.seek(SeekFrom::Start(start))?;
    let need = |n: u64| {
        if len < n {
            Err(MixError::Truncated)
        } else {
            Ok(())
        }
    };
    need(6)?;
    let first = read_exact_vec(r, 4)?;
    let (mut index, header_start) = if u16_at(&first, 0) == Some(0) {
        let flags = u16_at(&first, 2).unwrap_or(0);
        if flags & !(FLAG_CHECKSUM | FLAG_ENCRYPTED) != 0 {
            return Err(MixError::Invalid(format!("unknown flags {flags:#06x}")));
        }
        let index = MixIndex {
            encrypted: flags & FLAG_ENCRYPTED != 0,
            checksum: flags & FLAG_CHECKSUM != 0,
            entries: Vec::new(),
        };
        (index, 4u64)
    } else {
        (MixIndex::default(), 0u64)
    };

    // (count, body size, raw entry table, body start relative to `start`)
    let (count, body_size, table, body_start) = if index.encrypted {
        need(header_start + KEY_SOURCE_LEN as u64 + 8)?;
        let source = read_exact_vec(r, KEY_SOURCE_LEN)?;
        let bf_key: [u8; BLOWFISH_KEY_LEN] = key.blowfish_key(&source).ok_or(MixError::BadKey)?;
        let bf = WwBlowfish::new(&bf_key).ok_or(MixError::BadKey)?;
        let mut head = read_exact_vec(r, 8)?;
        bf.decrypt(&mut head);
        let count = usize::from(u16_at(&head, 0).unwrap_or(0));
        let body_size = u32_at(&head, 2).unwrap_or(0);
        let plain_len = 6 + count * ENTRY_LEN;
        let enc_len = plain_len.div_ceil(8) * 8;
        let at = header_start + KEY_SOURCE_LEN as u64;
        need(at + enc_len as u64)?;
        let mut rest = read_exact_vec(r, enc_len - 8)?;
        bf.decrypt(&mut rest);
        head.extend_from_slice(&rest);
        let table = head[6..plain_len].to_vec();
        (count, body_size, table, at + enc_len as u64)
    } else {
        if header_start == 0 {
            r.seek(SeekFrom::Start(start))?;
        }
        need(header_start + 6)?;
        let head = read_exact_vec(r, 6)?;
        let count = usize::from(u16_at(&head, 0).unwrap_or(0));
        let body_size = u32_at(&head, 2).unwrap_or(0);
        let table_len = count * ENTRY_LEN;
        need(header_start + 6 + table_len as u64)?;
        let table = read_exact_vec(r, table_len)?;
        (count, body_size, table, header_start + 6 + table_len as u64)
    };

    if body_start + u64::from(body_size) > len {
        return Err(MixError::Invalid(format!(
            "body of {body_size} bytes does not fit in {len}"
        )));
    }
    for i in 0..count {
        let at = i * ENTRY_LEN;
        let (Some(id), Some(off), Some(size)) = (
            u32_at(&table, at),
            u32_at(&table, at + 4),
            u32_at(&table, at + 8),
        ) else {
            return Err(MixError::Truncated);
        };
        if u64::from(off) + u64::from(size) > u64::from(body_size) {
            return Err(MixError::Invalid(format!(
                "entry {id:#010x} lies outside the body"
            )));
        }
        index.entries.push(MixEntry {
            id,
            offset: start + body_start + u64::from(off),
            size,
        });
    }
    index.entries.sort_by_key(|e| e.id);
    if index.entries.windows(2).any(|w| w[0].id == w[1].id) {
        return Err(MixError::Invalid("duplicate entry ids".into()));
    }
    Ok(index)
}

/// Read one entry's bytes.
pub fn read_entry<R: Read + Seek>(r: &mut R, entry: &MixEntry) -> Result<Vec<u8>, MixError> {
    if entry.size > MAX_ENTRY_SIZE {
        return Err(MixError::Invalid(format!(
            "entry of {} bytes is too large",
            entry.size
        )));
    }
    r.seek(SeekFrom::Start(entry.offset))?;
    read_exact_vec(r, entry.size as usize)
}

/// An archive on disk, or nested inside one (same file, an offset into it).
#[derive(Clone, Debug)]
pub struct MixArchive {
    pub path: PathBuf,
    pub index: MixIndex,
}

impl MixArchive {
    pub fn open(path: &Path, key: &RsaKey) -> Result<MixArchive, MixError> {
        let mut f = File::open(path)?;
        let len = f.metadata()?.len();
        let index = read_index(&mut f, 0, len, key)?;
        Ok(MixArchive {
            path: path.to_path_buf(),
            index,
        })
    }

    /// Open an entry of this archive as a MIX archive of its own.
    pub fn open_nested(&self, entry: &MixEntry, key: &RsaKey) -> Result<MixArchive, MixError> {
        let mut f = File::open(&self.path)?;
        let index = read_index(&mut f, entry.offset, u64::from(entry.size), key)?;
        Ok(MixArchive {
            path: self.path.clone(),
            index,
        })
    }

    pub fn read(&self, entry: &MixEntry) -> Result<Vec<u8>, MixError> {
        read_entry(&mut File::open(&self.path)?, entry)
    }

    /// Read a file by name (or `#hex` id).
    pub fn read_named(&self, name: &str) -> Result<Vec<u8>, MixError> {
        let e = self
            .index
            .find(name)
            .ok_or_else(|| MixError::NotFound(name.to_string()))?;
        self.read(e)
    }
}

/// Header layout for [`write_mix`].
pub enum MixFormat<'a> {
    /// Tiberian Dawn: no flags word.
    Old,
    /// Red Alert, plain header.
    Plain,
    /// Red Alert, encrypted header: the private test key and the Blowfish key.
    Encrypted {
        private: &'a RsaKey,
        blowfish: [u8; BLOWFISH_KEY_LEN],
    },
}

/// Write an archive (test fixtures and tools; never used on real files).
/// Entries are `(id, data)`; ids must be distinct.
pub fn write_mix(files: &[(u32, &[u8])], format: MixFormat) -> Vec<u8> {
    let mut sorted: Vec<(u32, &[u8])> = files.to_vec();
    sorted.sort_by_key(|f| f.0);
    let mut header = Vec::new();
    let mut body = Vec::new();
    header.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
    let body_size: usize = sorted.iter().map(|f| f.1.len()).sum();
    header.extend_from_slice(&(body_size as u32).to_le_bytes());
    for (id, data) in &sorted {
        header.extend_from_slice(&id.to_le_bytes());
        header.extend_from_slice(&(body.len() as u32).to_le_bytes());
        header.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
    }
    let mut out = Vec::new();
    match format {
        MixFormat::Old => out.extend_from_slice(&header),
        MixFormat::Plain => {
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&header);
        }
        MixFormat::Encrypted { private, blowfish } => {
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(&FLAG_ENCRYPTED.to_le_bytes());
            let block = private.block_len();
            let mut plain = blowfish.to_vec();
            plain.resize(2 * (block - 1), 0);
            let source = private.apply(&plain, block - 1, block).unwrap_or_default();
            out.extend_from_slice(&source);
            header.resize(header.len().div_ceil(8) * 8, 0);
            if let Some(bf) = WwBlowfish::new(&blowfish) {
                bf.encrypt(&mut header);
            }
            out.extend_from_slice(&header);
        }
    }
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::super::crypto::test_keypair;
    use super::*;
    use std::io::Cursor;

    /// Values worked out by hand from the documented algorithm.
    #[test]
    fn id_hash_matches_the_documented_algorithm() {
        assert_eq!(mix_id(""), 0);
        assert_eq!(mix_id("A"), 0x41);
        assert_eq!(mix_id("a"), 0x41, "case-insensitive");
        // "ABCD" = one group, little-endian.
        assert_eq!(mix_id("ABCD"), 0x4443_4241);
        // "ABCDE": rotl(0x44434241, 1) = 0x88868482, + 0x45.
        assert_eq!(mix_id("ABCDE"), 0x8886_84C7);
        // "ABCDEFGHI": rotl(rotl(0x44434241) + 0x48474645, 1) + 0x49.
        let g2 = 0x8886_8482u32.wrapping_add(0x4847_4645);
        assert_eq!(mix_id("abcdefghi"), g2.rotate_left(1).wrapping_add(0x49));
        assert_eq!(id_for("#0000ABCD"), 0xABCD);
        assert_eq!(id_for("ABCD"), 0x4443_4241);
    }

    fn files() -> Vec<(u32, Vec<u8>)> {
        vec![
            (mix_id("ONE.AUD"), b"first file".to_vec()),
            (mix_id("TWO.AUD"), vec![0xAB; 1000]),
            (mix_id("EMPTY"), Vec::new()),
        ]
    }

    fn round_trip(format: MixFormat, key: &RsaKey) {
        let f = files();
        let refs: Vec<(u32, &[u8])> = f.iter().map(|(i, d)| (*i, d.as_slice())).collect();
        let bytes = write_mix(&refs, format);
        let mut c = Cursor::new(&bytes);
        let idx = read_index(&mut c, 0, bytes.len() as u64, key).unwrap();
        assert_eq!(idx.entries.len(), 3);
        for (id, data) in &f {
            let e = idx.find_id(*id).unwrap();
            assert_eq!(&read_entry(&mut c, e).unwrap(), data);
        }
        assert_eq!(
            read_entry(&mut c, idx.find("one.aud").unwrap()).unwrap(),
            b"first file"
        );
        assert!(idx.find("THREE.AUD").is_none());
    }

    #[test]
    fn write_read_round_trips() {
        let (public, private) = test_keypair();
        round_trip(MixFormat::Old, &public);
        round_trip(MixFormat::Plain, &public);
        let mut bf = [0u8; BLOWFISH_KEY_LEN];
        for (i, b) in bf.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(73).wrapping_add(11);
        }
        round_trip(
            MixFormat::Encrypted {
                private: &private,
                blowfish: bf,
            },
            &public,
        );
    }

    #[test]
    fn encrypted_header_is_unreadable_without_the_key() {
        let (public, private) = test_keypair();
        let bytes = write_mix(
            &[(1, b"x"), (2, b"y")],
            MixFormat::Encrypted {
                private: &private,
                blowfish: [9; BLOWFISH_KEY_LEN],
            },
        );
        // The plain header (count 2, body size 2) must not appear as such.
        assert!(!bytes.windows(6).any(|w| w == [2, 0, 2, 0, 0, 0]));
        let mut c = Cursor::new(&bytes);
        assert!(read_index(&mut c, 0, bytes.len() as u64, &public).is_ok());
        // Westwood's key on our fixture: garbage key, so a clean error.
        let r = read_index(&mut c, 0, bytes.len() as u64, &RsaKey::westwood());
        assert!(r.is_err());
    }

    #[test]
    fn nested_archive_on_disk() {
        let (public, _) = test_keypair();
        let inner = write_mix(&[(mix_id("IN.AUD"), b"inner data")], MixFormat::Plain);
        let outer = write_mix(
            &[(mix_id("SOUNDS.MIX"), &inner), (mix_id("X"), b"x")],
            MixFormat::Old,
        );
        let dir = std::env::temp_dir().join(format!("sf_audio_mix_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("MAIN.MIX");
        std::fs::write(&path, &outer).unwrap();
        let a = MixArchive::open(&path, &public).unwrap();
        let e = *a.index.find("sounds.mix").unwrap();
        let n = a.open_nested(&e, &public).unwrap();
        assert_eq!(n.read_named("IN.AUD").unwrap(), b"inner data");
        assert!(matches!(n.read_named("NOPE"), Err(MixError::NotFound(_))));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn truncated_and_corrupt_headers_are_errors() {
        let (public, private) = test_keypair();
        for format in [
            MixFormat::Old,
            MixFormat::Plain,
            MixFormat::Encrypted {
                private: &private,
                blowfish: [3; BLOWFISH_KEY_LEN],
            },
        ] {
            let bytes = write_mix(&[(1, b"abc"), (2, b"defg")], format);
            for cut in 0..bytes.len() {
                let part = &bytes[..cut];
                let r = read_index(&mut Cursor::new(part), 0, cut as u64, &public);
                assert!(r.is_err(), "cut at {cut} of {}", bytes.len());
            }
        }
        // An entry pointing past the body.
        let mut bytes = write_mix(&[(1, b"abc")], MixFormat::Old);
        bytes[10] = 0xFF;
        let r = read_index(&mut Cursor::new(&bytes), 0, bytes.len() as u64, &public);
        assert!(matches!(r, Err(MixError::Invalid(_))));
        // Unknown flags.
        let r = read_index(
            &mut Cursor::new(&[0u8, 0, 0x80, 0, 0, 0, 0, 0]),
            0,
            8,
            &public,
        );
        assert!(matches!(r, Err(MixError::Invalid(_))));
    }
}
