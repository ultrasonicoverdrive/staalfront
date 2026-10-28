//! Optional original Red Alert audio (B11, WP11.7), read at runtime from the
//! player's own Red Alert files; nothing of Red Alert is in the repo.
//!
//! Clean-room: implemented only from public format descriptions (see
//! `docs/architecture.md`, "Original Red Alert audio"), not from the EA
//! source release or OpenRA.
//!
//! - [`mix`]: MIX archives (plain and encrypted headers, nested archives,
//!   lookup by name hash).
//! - [`aud`]: AUD sounds (Westwood ADPCM and IMA ADPCM) to PCM.
//! - [`crypto`]: the RSA + Blowfish step of encrypted MIX headers.
//! - [`scan`] builds a [`Catalog`] of everything under a directory (the
//!   sound browser in the F1 panel); [`load`] resolves a [`Mapping`] from
//!   Staalfront sound events to Red Alert file names against it.
//!
//! The readers return errors on bad input, never panic. A scan visits each
//! (file, offset, size) region once and stops at [`MAX_ARCHIVES`],
//! [`MAX_ENTRIES`] and [`MAX_DECODED_BYTES`] (one problem line), so a
//! crafted archive with overlapping, nested entries cannot blow up time or
//! memory. A missing or broken install only means the CC0 sounds keep
//! playing.

pub mod aud;
pub mod crypto;
pub mod mix;
pub mod wav;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use kira::sound::static_sound::StaticSoundData;
use serde::Deserialize;

use aud::{Compression, Pcm};
use crypto::RsaKey;
use mix::{MixArchive, id_for, read_index};

/// Mapping file relative to the asset root.
pub const MAPPING_FILE: &str = "audio/ra_mapping.ron";
/// Audio settings relative to the asset root.
pub const CONFIG_FILE: &str = "config/audio.ron";

/// Directory levels searched for `.mix` files below the configured path.
const MAX_DIR_DEPTH: usize = 4;
/// Archives inside archives followed this deep.
pub const MAX_NEST_DEPTH: usize = 3;
/// Files looked at, at most (a wrong path such as `/` must not take long).
const MAX_FILES: usize = 16_384;
/// Caps on one scan, against archive bombs (overlapping entries, nesting).
/// The whole Red Alert release (both CDs) is a few dozen archives, a few
/// thousand entries and well under 1 GiB, so real files never get near them.
pub const MAX_ARCHIVES: usize = 512;
pub const MAX_ENTRIES: usize = 100_000;
/// Bytes of AUD data read and decoded per scan.
pub const MAX_DECODED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub use crate::config::AudioConfig;

/// "Use original Red Alert audio" and where the player's files are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaAudioConfig {
    #[serde(default)]
    pub enabled: bool,
    /// The install or freeware download directory, one `.mix` file, or a
    /// folder of WAV files written by `sf_ra_extract`.
    #[serde(default)]
    pub path: String,
}

/// `assets/audio/ra_mapping.ron`: Staalfront sound event -> Red Alert file
/// name (or `#hex` id from the sound browser). Names only, never audio.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    #[serde(default)]
    pub sounds: BTreeMap<String, String>,
    /// Extra file names the sound browser shows instead of a bare id.
    #[serde(default)]
    pub names: Vec<String>,
}

impl Mapping {
    pub fn parse(text: &str) -> Result<Mapping, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }

    pub fn load(asset_root: &Path) -> Result<Mapping, String> {
        let path = asset_root.join(MAPPING_FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// All names known to the browser: mapped files plus the extras.
    pub fn known_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .sounds
            .values()
            .chain(&self.names)
            .filter(|n| !n.starts_with('#'))
            .cloned()
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

/// What an archive entry turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Aud {
        sample_rate: u32,
        channels: u16,
        millis: u64,
        compression: Compression,
    },
    /// A 16-bit PCM WAV file in a WAV folder (from `sf_ra_extract`).
    Wav {
        sample_rate: u32,
        channels: u16,
        millis: u64,
    },
    /// Looks like AUD (or WAV) but does not decode.
    Broken(String),
    /// A nested archive (listed as an archive of its own).
    Mix,
    /// Anything else (graphics, palettes, text).
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryInfo {
    pub id: u32,
    pub name: Option<String>,
    /// File holding the bytes and where (absolute within that file).
    pub path: PathBuf,
    pub offset: u64,
    pub size: u32,
    pub kind: EntryKind,
}

impl EntryInfo {
    /// The name, or `#XXXXXXXX` for the mapping file.
    pub fn label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("#{:08X}", self.id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveInfo {
    /// e.g. `MAIN.MIX > SOUNDS.MIX`.
    pub label: String,
    pub encrypted: bool,
    pub entries: Vec<EntryInfo>,
}

/// Everything found under the configured path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub root: PathBuf,
    pub archives: Vec<ArchiveInfo>,
    /// Archives that could not be read, with the reason.
    pub problems: Vec<String>,
}

impl Catalog {
    /// Number of playable AUD entries.
    pub fn sound_count(&self) -> usize {
        self.archives
            .iter()
            .flat_map(|a| &a.entries)
            .filter(|e| e.kind.playable())
            .count()
    }

    /// The first playable entry with this name (or `#hex` id); a WAV
    /// folder entry wins over an AUD in a MIX archive.
    pub fn find(&self, name: &str) -> Option<&EntryInfo> {
        let id = id_for(name);
        let wanted_loose = name.to_ascii_uppercase();
        let hit = |e: &&EntryInfo| e.id == id || e.name.as_deref() == Some(wanted_loose.as_str());
        let all = || self.archives.iter().flat_map(|a| &a.entries);
        all()
            .filter(|e| matches!(e.kind, EntryKind::Wav { .. }))
            .find(hit)
            .or_else(|| {
                all()
                    .filter(|e| matches!(e.kind, EntryKind::Aud { .. }))
                    .find(hit)
            })
    }
}

impl EntryKind {
    /// AUD or WAV that decodes.
    pub fn playable(&self) -> bool {
        matches!(self, EntryKind::Aud { .. } | EntryKind::Wav { .. })
    }
}

/// Read and decode one entry.
pub fn decode_entry(e: &EntryInfo) -> Result<Pcm, String> {
    let bytes = read_at(&e.path, e.offset, e.size).map_err(|x| x.to_string())?;
    if matches!(e.kind, EntryKind::Wav { .. }) {
        return wav::read(&bytes);
    }
    aud::decode(&bytes).map_err(|x| x.to_string())
}

/// The name and id of a WAV written by `sf_ra_extract`: `#1A2B3C4D.wav` is
/// that id; `CLICK.wav` is `CLICK.AUD` (the tool drops `.AUD`); a stem that
/// still has an extension (`FOO.JUV.wav`) keeps it.
pub fn wav_name(file_name: &str) -> (u32, Option<String>) {
    let stem = file_name
        .len()
        .checked_sub(4)
        .filter(|&n| file_name.is_char_boundary(n))
        .map_or(file_name, |n| &file_name[..n]);
    if let Some(hex) = stem.strip_prefix('#')
        && let Ok(id) = u32::from_str_radix(hex, 16)
    {
        return (id, None);
    }
    let mut name = stem.to_ascii_uppercase();
    if !name.contains('.') {
        name.push_str(".AUD");
    }
    (mix::mix_id(&name), Some(name))
}

fn read_at(path: &Path, offset: u64, size: u32) -> std::io::Result<Vec<u8>> {
    if size > mix::MAX_ENTRY_SIZE {
        return Err(std::io::Error::other("entry too large"));
    }
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut v = vec![0u8; size as usize];
    f.read_exact(&mut v)?;
    Ok(v)
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// Files below `dir`, sorted, at most [`MAX_DIR_DEPTH`] levels deep.
fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DIR_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    items.sort();
    for p in items {
        if out.len() >= MAX_FILES {
            return;
        }
        if p.is_dir() {
            walk(&p, depth + 1, out);
        } else {
            out.push(p);
        }
    }
}

/// Bytes of a WAV file read to classify it (chunks before `data`).
const WAV_HEADER_BYTES: u32 = 64 * 1024;

fn classify(path: &Path, offset: u64, size: u32, key: &RsaKey) -> EntryKind {
    if offset == 0 && has_ext(path, "wav") {
        // Only the header is read here (a music track is tens of MiB); the
        // samples are read when the entry is played.
        let header = if size > mix::MAX_ENTRY_SIZE {
            Err("WAV larger than 64 MiB".to_string())
        } else {
            read_at(path, 0, size.min(WAV_HEADER_BYTES))
                .map_err(|e| e.to_string())
                .and_then(|b| wav::header(&b, size as usize))
                .and_then(|h| {
                    if h.data_start.saturating_add(h.data_len) > size as usize {
                        Err("WAV chunk runs past the end".to_string())
                    } else {
                        Ok(h)
                    }
                })
        };
        return match header {
            Ok(h) => EntryKind::Wav {
                sample_rate: h.sample_rate,
                channels: h.channels,
                millis: h.duration_ms(),
            },
            Err(e) => EntryKind::Broken(e),
        };
    }
    let Ok(mut f) = File::open(path) else {
        return EntryKind::Other;
    };
    let mut head = [0u8; 20];
    let n = (size as usize).min(head.len());
    if f.seek(SeekFrom::Start(offset)).is_err() || f.read_exact(&mut head[..n]).is_err() {
        return EntryKind::Other;
    }
    if aud::parse_header(&head[..n]).is_ok() {
        return match read_at(path, offset, size)
            .map_err(|e| e.to_string())
            .and_then(|b| aud::decode(&b).map_err(|e| e.to_string()))
        {
            Ok(pcm) => EntryKind::Aud {
                sample_rate: pcm.sample_rate,
                channels: pcm.channels,
                millis: pcm.duration_ms(),
                compression: aud::parse_header(&head[..n])
                    .map(|h| h.compression)
                    .unwrap_or(Compression::Ima),
            },
            Err(e) => EntryKind::Broken(e),
        };
    }
    match read_index(&mut f, offset, u64::from(size), key) {
        Ok(idx) if !idx.entries.is_empty() => EntryKind::Mix,
        _ => EntryKind::Other,
    }
}

struct Scanner<'a> {
    key: &'a RsaKey,
    names: BTreeMap<u32, String>,
    catalog: Catalog,
    /// Every (file, offset, size) region classified so far: overlapping or
    /// repeated entries are decoded once, nested archives opened once.
    regions: BTreeMap<(PathBuf, u64, u32), EntryKind>,
    entries: usize,
    decoded: u64,
    capped: bool,
}

impl Scanner<'_> {
    fn name(&self, id: u32) -> Option<String> {
        self.names.get(&id).cloned()
    }

    fn cap(&mut self, what: &str) {
        if !self.capped {
            self.capped = true;
            self.catalog
                .problems
                .push(format!("scan stopped early: more than {what}"));
        }
    }

    /// Classify a region once; later visits reuse the result.
    fn kind(&mut self, path: &Path, offset: u64, size: u32) -> (EntryKind, bool) {
        let key = (path.to_path_buf(), offset, size);
        if let Some(k) = self.regions.get(&key) {
            return (k.clone(), false);
        }
        let kind = if self.decoded.saturating_add(u64::from(size)) > MAX_DECODED_BYTES {
            self.cap("the decode budget");
            EntryKind::Other
        } else {
            let k = classify(path, offset, size, self.key);
            if k.playable() || matches!(k, EntryKind::Broken(_)) {
                self.decoded += u64::from(size);
            }
            k
        };
        self.regions.insert(key, kind.clone());
        (kind, true)
    }

    fn archive(&mut self, archive: &MixArchive, label: String, depth: usize) {
        if self.catalog.archives.len() >= MAX_ARCHIVES {
            self.cap(&format!("{MAX_ARCHIVES} archives"));
            return;
        }
        let mut info = ArchiveInfo {
            label: label.clone(),
            encrypted: archive.index.encrypted,
            entries: Vec::new(),
        };
        let mut nested = Vec::new();
        for e in &archive.index.entries {
            if self.entries >= MAX_ENTRIES {
                self.cap(&format!("{MAX_ENTRIES} entries"));
                break;
            }
            self.entries += 1;
            let (kind, first) = self.kind(&archive.path, e.offset, e.size);
            let entry = EntryInfo {
                id: e.id,
                name: self.name(e.id),
                path: archive.path.clone(),
                offset: e.offset,
                size: e.size,
                kind,
            };
            if entry.kind == EntryKind::Mix && first {
                nested.push((*e, entry.label()));
            }
            info.entries.push(entry);
        }
        self.catalog.archives.push(info);
        if depth >= MAX_NEST_DEPTH {
            return;
        }
        for (e, name) in nested {
            match archive.open_nested(&e, self.key) {
                Ok(inner) => self.archive(&inner, format!("{label} > {name}"), depth + 1),
                Err(err) => self
                    .catalog
                    .problems
                    .push(format!("{label} > {name}: {err}")),
            }
        }
    }
}

/// List every MIX archive (nested ones too), loose `.aud` file and WAV
/// folder (one archive per directory holding `.wav` files, as written by
/// `sf_ra_extract`) under `root` (a directory or one `.mix` file). Never fails: what cannot be read
/// ends up in [`Catalog::problems`].
pub fn scan(root: &Path, key: &RsaKey, known_names: &[String]) -> Catalog {
    let mut s = Scanner {
        key,
        names: known_names
            .iter()
            .map(|n| (mix::mix_id(n), n.to_ascii_uppercase()))
            .collect(),
        catalog: Catalog {
            root: root.to_path_buf(),
            ..Catalog::default()
        },
        regions: BTreeMap::new(),
        entries: 0,
        decoded: 0,
        capped: false,
    };
    let mut files = Vec::new();
    if root.is_file() {
        files.push(root.to_path_buf());
    } else if root.is_dir() {
        walk(root, 0, &mut files);
    } else {
        s.catalog
            .problems
            .push(format!("{} does not exist", root.display()));
        return s.catalog;
    }
    let mut wav_dirs: BTreeMap<String, ArchiveInfo> = BTreeMap::new();
    let mut loose = ArchiveInfo {
        label: "(loose .aud files)".into(),
        encrypted: false,
        entries: Vec::new(),
    };
    for path in files {
        let label = path
            .strip_prefix(root)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(&path)
            .display()
            .to_string()
            .replace('\\', "/");
        if has_ext(&path, "mix") {
            match MixArchive::open(&path, key) {
                Ok(a) => s.archive(&a, label, 0),
                Err(e) => s.catalog.problems.push(format!("{label}: {e}")),
            }
        } else if has_ext(&path, "wav") {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let Ok(size) = u32::try_from(size) else {
                continue;
            };
            let file = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (id, name) = wav_name(&file);
            let dir = label.rsplit_once('/').map_or(".", |(d, _)| d).to_string();
            let kind = s.kind(&path, 0, size).0;
            wav_dirs
                .entry(dir.clone())
                .or_insert_with(|| ArchiveInfo {
                    label: format!("{dir} (WAV folder)"),
                    encrypted: false,
                    entries: Vec::new(),
                })
                .entries
                .push(EntryInfo {
                    id,
                    name,
                    kind,
                    path,
                    offset: 0,
                    size,
                });
        } else if has_ext(&path, "aud") {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let Ok(size) = u32::try_from(size) else {
                continue;
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_uppercase());
            loose.entries.push(EntryInfo {
                id: name.as_deref().map(mix::mix_id).unwrap_or(0),
                name,
                kind: s.kind(&path, 0, size).0,
                path,
                offset: 0,
                size,
            });
        }
    }
    if !loose.entries.is_empty() {
        s.catalog.archives.push(loose);
    }
    s.catalog.archives.extend(wav_dirs.into_values());
    s.catalog
}

/// The result of [`load`]: what was found and the decoded mapped sounds.
pub struct Loaded {
    pub catalog: Catalog,
    /// Event name -> decoded sound.
    pub sounds: BTreeMap<String, StaticSoundData>,
    /// One summary line when something is missing (`None` = all mapped
    /// sounds found).
    pub warning: Option<String>,
}

/// Scan `root` and decode every mapped sound. Blocking: run it on a
/// background thread.
pub fn load(root: &Path, mapping: &Mapping, key: &RsaKey) -> Loaded {
    let catalog = scan(root, key, &mapping.known_names());
    let mut sounds = BTreeMap::new();
    let mut missing = Vec::new();
    for (event, ra_name) in &mapping.sounds {
        match catalog.find(ra_name).map(decode_entry) {
            Some(Ok(pcm)) => {
                sounds.insert(event.clone(), pcm.to_static_sound());
            }
            Some(Err(e)) => missing.push(format!("{ra_name} ({e})")),
            None => missing.push(ra_name.clone()),
        }
    }
    let warning = if catalog.sound_count() == 0 {
        Some(format!(
            "no original Red Alert sounds found under '{}'{}; using the CC0 sounds",
            root.display(),
            catalog
                .problems
                .first()
                .map(|p| format!(" ({p})"))
                .unwrap_or_default()
        ))
    } else if !missing.is_empty() {
        Some(format!(
            "{} of {} mapped Red Alert sounds not found ({}); those use the CC0 sounds",
            missing.len(),
            mapping.sounds.len(),
            missing.join(", ")
        ))
    } else {
        None
    };
    Loaded {
        catalog,
        sounds,
        warning,
    }
}

#[cfg(test)]
mod tests;
