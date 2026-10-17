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
//! Everything here returns errors; a missing or broken install only means
//! the CC0 sounds keep playing.

pub mod aud;
pub mod crypto;
pub mod mix;

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
const MAX_NEST_DEPTH: usize = 3;
/// Files looked at, at most (a wrong path such as `/` must not take long).
const MAX_FILES: usize = 4096;

/// `assets/config/audio.ron`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioConfig {
    #[serde(default)]
    pub ra_audio: RaAudioConfig,
}

/// "Use original Red Alert audio" and where the player's files are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaAudioConfig {
    #[serde(default)]
    pub enabled: bool,
    /// The install or freeware download directory (or one `.mix` file).
    #[serde(default)]
    pub path: String,
}

impl AudioConfig {
    /// Read `<asset_root>/config/audio.ron`; on any problem the defaults
    /// (Red Alert audio off) and a warning.
    pub fn load(asset_root: &Path) -> (AudioConfig, Option<String>) {
        let path = asset_root.join(CONFIG_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => match ron::from_str(&text) {
                Ok(c) => (c, None),
                Err(e) => (
                    AudioConfig::default(),
                    Some(format!(
                        "cannot parse {}: {e}; using defaults",
                        path.display()
                    )),
                ),
            },
            Err(_) => (AudioConfig::default(), None),
        }
    }
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
    /// Looks like AUD but does not decode.
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
            .filter(|e| matches!(e.kind, EntryKind::Aud { .. }))
            .count()
    }

    /// The first playable entry with this name (or `#hex` id).
    pub fn find(&self, name: &str) -> Option<&EntryInfo> {
        let id = id_for(name);
        let wanted_loose = name.to_ascii_uppercase();
        self.archives
            .iter()
            .flat_map(|a| &a.entries)
            .filter(|e| matches!(e.kind, EntryKind::Aud { .. }))
            .find(|e| e.id == id || e.name.as_deref() == Some(wanted_loose.as_str()))
    }
}

/// Read and decode one entry.
pub fn decode_entry(e: &EntryInfo) -> Result<Pcm, String> {
    let bytes = read_at(&e.path, e.offset, e.size).map_err(|x| x.to_string())?;
    aud::decode(&bytes).map_err(|x| x.to_string())
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

fn classify(path: &Path, offset: u64, size: u32, key: &RsaKey) -> EntryKind {
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
}

impl Scanner<'_> {
    fn name(&self, id: u32) -> Option<String> {
        self.names.get(&id).cloned()
    }

    fn archive(&mut self, archive: &MixArchive, label: String, depth: usize) {
        let mut info = ArchiveInfo {
            label: label.clone(),
            encrypted: archive.index.encrypted,
            entries: Vec::new(),
        };
        let mut nested = Vec::new();
        for e in &archive.index.entries {
            let kind = classify(&archive.path, e.offset, e.size, self.key);
            let entry = EntryInfo {
                id: e.id,
                name: self.name(e.id),
                path: archive.path.clone(),
                offset: e.offset,
                size: e.size,
                kind,
            };
            if entry.kind == EntryKind::Mix {
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

/// List every MIX archive (nested ones too) and loose `.aud` file under
/// `root` (a directory or one `.mix` file). Never fails: what cannot be read
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
                kind: classify(&path, 0, size, key),
                path,
                offset: 0,
                size,
            });
        }
    }
    if !loose.entries.is_empty() {
        s.catalog.archives.push(loose);
    }
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
