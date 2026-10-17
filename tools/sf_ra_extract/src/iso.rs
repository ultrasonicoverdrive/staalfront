//! A small ISO 9660 reader: enough to list the files of a CD image and
//! where their bytes are.
//!
//! Written from ECMA-119 (2nd edition, 1987; the public text of ISO 9660):
//! volume descriptors from logical sector 16 (§8: type byte, `CD001`,
//! version; type 1 = primary, 255 = terminator), the primary volume
//! descriptor's logical block size (BP 129-132, both-endian 16-bit) and root
//! directory record (BP 157-190), and directory records (§9.1: length,
//! extended attribute length, extent location and data length as both-endian
//! 32-bit at BP 3 and 11, file flags at BP 26 (bit 1 directory, bit 7
//! multi-extent), interleave at BP 27-28, identifier length and identifier at
//! BP 33-34). Directory records never cross a sector boundary; a zero length
//! byte means "continue in the next sector" (§6.8.1.1). File identifiers end
//! in `;version`, which is dropped. Joliet and Rock Ridge are not read.
//!
//! Hostile images: every read is bounds-checked against the image size,
//! directories are followed at most [`MAX_DEPTH`] deep and each directory
//! extent once, at most [`MAX_ENTRIES`] records are listed and a directory
//! extent larger than [`MAX_DIR_BYTES`] is refused. Errors, never panics.

use std::collections::BTreeSet;
use std::fmt;
use std::io::{Read, Seek, SeekFrom};

pub const SECTOR: u64 = 2048;
pub const MAX_DEPTH: usize = 16;
pub const MAX_ENTRIES: usize = 100_000;
pub const MAX_DIR_BYTES: u32 = 4 * 1024 * 1024;
/// Volume descriptors looked at before giving up on finding the primary one.
const MAX_DESCRIPTORS: u64 = 64;

#[derive(Debug)]
pub enum IsoError {
    Io(std::io::Error),
    Invalid(String),
}

impl fmt::Display for IsoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IsoError::Io(e) => write!(f, "ISO read error: {e}"),
            IsoError::Invalid(why) => write!(f, "invalid ISO 9660 image: {why}"),
        }
    }
}

impl std::error::Error for IsoError {}

impl From<std::io::Error> for IsoError {
    fn from(e: std::io::Error) -> Self {
        IsoError::Io(e)
    }
}

fn bad(why: impl Into<String>) -> IsoError {
    IsoError::Invalid(why.into())
}

/// A regular file in the image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoFile {
    /// `DIR/SUB/NAME.EXT`, without the `;1` version.
    pub path: String,
    /// Byte offset of the data in the image.
    pub offset: u64,
    pub size: u32,
}

/// What [`list_files`] found, plus records it skipped and why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub files: Vec<IsoFile>,
    pub skipped: Vec<String>,
}

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

struct Record {
    extent: u32,
    size: u32,
    flags: u8,
    interleaved: bool,
    name: Vec<u8>,
}

/// Parse one directory record (`b` starts at its length byte).
fn record(b: &[u8]) -> Result<Record, IsoError> {
    let len = usize::from(*b.first().ok_or_else(|| bad("empty record"))?);
    if len < 34 || len > b.len() {
        return Err(bad(format!("directory record of {len} bytes")));
    }
    let name_len = usize::from(b[32]);
    if 33 + name_len > len {
        return Err(bad("file identifier runs past its record"));
    }
    Ok(Record {
        extent: u32_le(b, 2),
        size: u32_le(b, 10),
        flags: b[25],
        interleaved: b[26] != 0 || b[27] != 0,
        name: b[33..33 + name_len].to_vec(),
    })
}

struct Walker<'a, R> {
    r: &'a mut R,
    block: u64,
    image_len: u64,
    seen_dirs: BTreeSet<u32>,
    records: usize,
    out: Listing,
}

impl<R: Read + Seek> Walker<'_, R> {
    fn read(&mut self, offset: u64, len: u32) -> Result<Vec<u8>, IsoError> {
        let end = offset
            .checked_add(u64::from(len))
            .ok_or_else(|| bad("extent overflows"))?;
        if end > self.image_len {
            return Err(bad(format!(
                "extent {offset}+{len} past the end of the image ({} bytes)",
                self.image_len
            )));
        }
        self.r.seek(SeekFrom::Start(offset))?;
        let mut v = vec![0u8; len as usize];
        self.r.read_exact(&mut v)?;
        Ok(v)
    }

    fn dir(&mut self, extent: u32, size: u32, prefix: &str, depth: usize) -> Result<(), IsoError> {
        if depth > MAX_DEPTH {
            self.out.skipped.push(format!(
                "{prefix}: directories nested deeper than {MAX_DEPTH}"
            ));
            return Ok(());
        }
        if !self.seen_dirs.insert(extent) {
            return Ok(());
        }
        if size > MAX_DIR_BYTES {
            return Err(bad(format!("directory '{prefix}' of {size} bytes")));
        }
        let data = self.read(u64::from(extent) * self.block, size)?;
        let block = self.block as usize;
        let mut at = 0usize;
        let mut subdirs = Vec::new();
        while at < data.len() {
            if data[at] == 0 {
                // Rest of this sector is padding.
                at = (at / block + 1) * block;
                continue;
            }
            self.records += 1;
            if self.records > MAX_ENTRIES {
                return Err(bad(format!("more than {MAX_ENTRIES} directory records")));
            }
            let sector_end = ((at / block + 1) * block).min(data.len());
            let rec = record(&data[at..sector_end])?;
            at += usize::from(data[at]);
            if rec.name == [0] || rec.name == [1] {
                continue; // "." and ".."
            }
            let mut name = String::from_utf8_lossy(&rec.name).into_owned();
            if let Some(i) = name.find(';') {
                name.truncate(i);
            }
            let name = name.trim_end_matches('.').to_string();
            if name.is_empty() || name.contains('/') || name == "." || name == ".." {
                self.out.skipped.push(format!("{prefix}: bad file name"));
                continue;
            }
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if rec.flags & 0x02 != 0 {
                subdirs.push((rec.extent, rec.size, path));
            } else if rec.flags & 0x80 != 0 || rec.interleaved {
                self.out
                    .skipped
                    .push(format!("{path}: multi-extent or interleaved file"));
            } else {
                let offset = u64::from(rec.extent) * self.block;
                match offset.checked_add(u64::from(rec.size)) {
                    Some(end) if end <= self.image_len => self.out.files.push(IsoFile {
                        path,
                        offset,
                        size: rec.size,
                    }),
                    _ => self
                        .out
                        .skipped
                        .push(format!("{path}: data past the end of the image")),
                }
            }
        }
        for (extent, size, path) in subdirs {
            if let Err(e) = self.dir(extent, size, &path, depth + 1) {
                self.out.skipped.push(format!("{path}: {e}"));
            }
        }
        Ok(())
    }
}

/// List every regular file of the image, in directory order.
pub fn list_files<R: Read + Seek>(r: &mut R) -> Result<Listing, IsoError> {
    let image_len = r.seek(SeekFrom::End(0))?;
    let mut pvd = None;
    for i in 0..MAX_DESCRIPTORS {
        let at = (16 + i) * SECTOR;
        if at + SECTOR > image_len {
            break;
        }
        r.seek(SeekFrom::Start(at))?;
        let mut d = vec![0u8; SECTOR as usize];
        r.read_exact(&mut d)?;
        if &d[1..6] != b"CD001" {
            return Err(bad("no 'CD001' volume descriptor at sector 16"));
        }
        match d[0] {
            1 => {
                pvd = Some(d);
                break;
            }
            255 => break,
            _ => {}
        }
    }
    let pvd = pvd.ok_or_else(|| bad("no primary volume descriptor"))?;
    let block = u64::from(u16_le(&pvd, 128));
    if !matches!(block, 512 | 1024 | 2048) {
        return Err(bad(format!("logical block size {block}")));
    }
    let root = record(&pvd[156..190])?;
    if root.flags & 0x02 == 0 {
        return Err(bad("root record is not a directory"));
    }
    let mut w = Walker {
        r,
        block,
        image_len,
        seen_dirs: BTreeSet::new(),
        records: 0,
        out: Listing::default(),
    };
    w.dir(root.extent, root.size, "", 0)?;
    Ok(w.out)
}

/// A minimal ISO 9660 writer for tests: directories and files, 2048-byte
/// blocks, plain identifiers. Each `(path, data)` is placed under its
/// directories (created as needed).
#[cfg(test)]
pub fn write_iso(files: &[(&str, &[u8])]) -> Vec<u8> {
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Dir {
        dirs: BTreeMap<String, Dir>,
        files: Vec<(String, Vec<u8>)>,
    }
    let mut root = Dir::default();
    for (path, data) in files {
        let mut parts: Vec<&str> = path.split('/').collect();
        let file = parts.pop().unwrap();
        let mut d = &mut root;
        for p in parts {
            d = d.dirs.entry(p.to_string()).or_default();
        }
        d.files.push((format!("{file};1"), data.to_vec()));
    }
    fn rec(extent: u32, size: u32, dir: bool, name: &[u8]) -> Vec<u8> {
        let mut len = 33 + name.len();
        len += len & 1;
        let mut r = vec![0u8; len];
        r[0] = len as u8;
        r[2..6].copy_from_slice(&extent.to_le_bytes());
        r[6..10].copy_from_slice(&extent.to_be_bytes());
        r[10..14].copy_from_slice(&size.to_le_bytes());
        r[14..18].copy_from_slice(&size.to_be_bytes());
        r[25] = if dir { 2 } else { 0 };
        r[28] = 1;
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r
    }
    let sectors = |n: usize| n.div_ceil(SECTOR as usize).max(1) as u32;
    // Lay out: 16 system sectors, PVD, terminator, then dirs and files.
    let mut img = vec![0u8; 18 * SECTOR as usize];
    fn place(
        d: &Dir,
        img: &mut Vec<u8>,
        parent: u32,
        sectors: &dyn Fn(usize) -> u32,
    ) -> (u32, u32) {
        // A directory takes one sector here (tests stay small).
        let me = (img.len() / SECTOR as usize) as u32;
        img.resize(img.len() + SECTOR as usize, 0);
        let mut body = rec(me, SECTOR as u32, true, &[0]);
        body.extend(rec(parent, SECTOR as u32, true, &[1]));
        for (name, sub) in &d.dirs {
            let (e, s) = place(sub, img, me, sectors);
            body.extend(rec(e, s, true, name.as_bytes()));
        }
        for (name, data) in &d.files {
            let e = (img.len() / SECTOR as usize) as u32;
            img.extend_from_slice(data);
            img.resize(((e + sectors(data.len())) as u64 * SECTOR) as usize, 0);
            body.extend(rec(e, data.len() as u32, false, name.as_bytes()));
        }
        assert!(body.len() <= SECTOR as usize);
        let at = me as usize * SECTOR as usize;
        img[at..at + body.len()].copy_from_slice(&body);
        (me, SECTOR as u32)
    }
    let (re, rs) = place(&root, &mut img, 18, &sectors);
    let pvd = 16 * SECTOR as usize;
    img[pvd] = 1;
    img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
    img[pvd + 6] = 1;
    let total = (img.len() / SECTOR as usize) as u32;
    img[pvd + 80..pvd + 84].copy_from_slice(&total.to_le_bytes());
    img[pvd + 128..pvd + 130].copy_from_slice(&2048u16.to_le_bytes());
    let r = rec(re, rs, true, &[0]);
    img[pvd + 156..pvd + 156 + 34].copy_from_slice(&r);
    let term = 17 * SECTOR as usize;
    img[term] = 255;
    img[term + 1..term + 6].copy_from_slice(b"CD001");
    img[term + 6] = 1;
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn sample() -> Vec<u8> {
        write_iso(&[
            ("README.TXT", b"hello"),
            ("INSTALL/MAIN.MIX", &[7u8; 5000]),
            ("INSTALL/SUB/DEEP.MIX", b"x"),
        ])
    }

    #[test]
    fn lists_files_with_offsets() {
        let img = sample();
        let l = list_files(&mut Cursor::new(&img)).unwrap();
        let paths: Vec<&str> = l.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["README.TXT", "INSTALL/MAIN.MIX", "INSTALL/SUB/DEEP.MIX"]
        );
        let main = &l.files[1];
        assert_eq!(main.size, 5000);
        assert_eq!(&img[main.offset as usize..][..5000], &[7u8; 5000][..]);
        assert!(l.skipped.is_empty());
    }

    #[test]
    fn corrupt_images_give_errors_not_panics() {
        assert!(list_files(&mut Cursor::new(Vec::new())).is_err());
        assert!(list_files(&mut Cursor::new(vec![0u8; 40_000])).is_err());
        let img = sample();
        // Every truncation.
        for cut in (0..img.len()).step_by(97) {
            let _ = list_files(&mut Cursor::new(&img[..cut]));
        }
        // Every single-byte corruption in the descriptors and directories.
        let pvd = 16 * SECTOR as usize;
        for at in (pvd..pvd + 200).chain(18 * 2048..18 * 2048 + 300) {
            for v in [0u8, 1, 2, 0x22, 0x80, 0xff] {
                let mut m = img.clone();
                m[at] = v;
                let _ = list_files(&mut Cursor::new(&m));
            }
        }
        // A directory that contains itself: listed once, no endless loop.
        let mut m = img.clone();
        let root_sector = 18 * 2048;
        // Point the third record (INSTALL) back at the root.
        let mut idx = root_sector;
        let mut n = 0;
        while n < 2 {
            idx += m[idx] as usize;
            n += 1;
        }
        m[idx + 25] = 2;
        m[idx + 2..idx + 6].copy_from_slice(&18u32.to_le_bytes());
        m[idx + 10..idx + 14].copy_from_slice(&2048u32.to_le_bytes());
        assert!(list_files(&mut Cursor::new(&m)).is_ok());
        // A huge directory size is refused, not allocated.
        let mut m = img.clone();
        m[pvd + 156 + 10..pvd + 156 + 14].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(list_files(&mut Cursor::new(&m)).is_err());
        // A file pointing past the end is skipped with a note.
        let mut m = img.clone();
        let l = list_files(&mut Cursor::new(&m)).unwrap();
        let readme_rec = (root_sector..root_sector + 2048)
            .find(|&i| m[i + 33..].starts_with(b"README.TXT"))
            .unwrap();
        m[readme_rec + 2..readme_rec + 6].copy_from_slice(&0x00ff_ffffu32.to_le_bytes());
        let l2 = list_files(&mut Cursor::new(&m)).unwrap();
        assert_eq!(l2.files.len() + 1, l.files.len());
        assert_eq!(l2.skipped.len(), 1);
    }

    #[test]
    fn deep_nesting_is_capped() {
        let path = (0..MAX_DEPTH + 3)
            .map(|i| format!("D{i}"))
            .chain(["X.MIX".to_string()])
            .collect::<Vec<_>>()
            .join("/");
        let img = write_iso(&[(path.as_str(), b"abc")]);
        let l = list_files(&mut Cursor::new(&img)).unwrap();
        assert!(l.files.is_empty());
        assert_eq!(l.skipped.len(), 1);
    }
}
