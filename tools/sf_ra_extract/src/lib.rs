//! `sf_ra_extract`: game sounds and music from the player's own Red Alert
//! files (a CD image, a mounted disc or install directory, or one `.mix`)
//! to a folder of 16-bit PCM WAV files that `sf_game --ra-audio` reads.
//!
//! B11: runs only on the player's machine; refuses to write inside a git
//! repository, so extracted audio cannot end up in Staalfront (or any other
//! checkout). Audio only: VQA videos (and their soundtracks) are counted and
//! skipped, never read beyond their first bytes.
//!
//! Clean-room (B5): the ISO 9660 reader is written from ECMA-119
//! ([`iso`]); MIX and AUD come from `sf_audio::ra` (WP11.7). No name list
//! is shipped: unknown names come out as `#XXXXXXXX.wav`; `--names` hashes a
//! player-supplied list.

pub mod iso;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sf_audio::ra::crypto::RsaKey;
use sf_audio::ra::mix::{MixEntry, MixIndex, mix_id, read_entry, read_index};
use sf_audio::ra::{MAX_ARCHIVES, MAX_DECODED_BYTES, MAX_ENTRIES, MAX_NEST_DEPTH, aud, wav};

pub const USAGE: &str = "usage: sf_ra_extract <input> --out <dir> [--names <file>] [--dry-run]

  <input>         a Red Alert CD image (.iso), a directory (mounted disc,
                  install or a folder of .mix files) or one .mix file
  --out <dir>     where the WAV files go: <dir>/<mix>/<name or #id>.wav;
                  must not be inside a git repository
  --names <file>  file names to look for (one per line, e.g. CLICK.AUD);
                  without it files are written as #XXXXXXXX.wav
  --dry-run       list what would be written, write nothing";

/// Directory levels searched below a directory input.
const MAX_DIR_DEPTH: usize = 6;
/// Files looked at below a directory input.
const MAX_FILES: usize = 16_384;
/// Largest `--names` file read.
const MAX_NAMES_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub input: PathBuf,
    pub out: PathBuf,
    pub names: Option<PathBuf>,
    pub dry_run: bool,
}

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
    let mut input = None;
    let mut out = None;
    let mut names = None;
    let mut dry_run = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = Some(PathBuf::from(it.next().ok_or("--out needs a directory")?)),
            "--names" => names = Some(PathBuf::from(it.next().ok_or("--names needs a file")?)),
            "--dry-run" => dry_run = true,
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ if input.is_none() => input = Some(PathBuf::from(a)),
            _ => return Err(format!("more than one input ({a})")),
        }
    }
    Ok(Args {
        input: input.ok_or("no input given")?,
        out: out.ok_or("--out is required")?,
        names,
        dry_run,
    })
}

/// The repository root (the directory holding `.git`) if `out` is inside one.
/// Checks the path as given and, through its nearest existing ancestor, with
/// symlinks resolved.
pub fn git_repo_above(out: &Path) -> Option<PathBuf> {
    let abs = if out.is_absolute() {
        out.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(out)
    };
    let mut candidates = vec![abs.clone()];
    if let Some(existing) = abs.ancestors().find(|p| p.exists())
        && let Ok(c) = existing.canonicalize()
    {
        candidates.push(c);
    }
    candidates
        .iter()
        .flat_map(|c| c.ancestors())
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Read a `--names` file: one name per line, blank lines ignored.
pub fn read_names(path: &Path) -> Result<BTreeMap<u32, String>, String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if len > MAX_NAMES_BYTES {
        return Err(format!("{}: names file larger than 4 MiB", path.display()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| (mix_id(l), l.to_ascii_uppercase()))
        .collect())
}

/// A file name that is safe on Windows and Unix and stays in its directory.
fn safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._#-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() || s.chars().all(|c| c == '.') {
        "_".into()
    } else {
        s
    }
}

/// `CLICK.AUD` -> `CLICK.wav`, unknown -> `#1A2B3C4D.wav` (what
/// `sf_audio::ra::wav_name` reads back).
fn wav_file_name(id: u32, name: Option<&str>) -> String {
    match name {
        Some(n) => {
            let upper = n.to_ascii_uppercase();
            let stem = upper.strip_suffix(".AUD").unwrap_or(&upper);
            format!("{}.wav", safe(stem))
        }
        None => format!("#{id:08X}.wav"),
    }
}

/// Everything the run did (or, with `--dry-run`, would do).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub archives: usize,
    /// Output paths relative to `--out`.
    pub sounds: Vec<String>,
    /// `archive > entry: reason`.
    pub failed: Vec<String>,
    pub videos: usize,
    /// Entries that are neither audio, video nor an archive.
    pub other: usize,
    /// Bytes of WAV written (or that would be).
    pub bytes: u64,
    /// Inputs or archives that could not be read, and caps hit.
    pub problems: Vec<String>,
}

impl Summary {
    pub fn print(&self, out: &mut dyn Write, dry_run: bool) -> std::io::Result<()> {
        let verb = if dry_run { "would write" } else { "written" };
        writeln!(out, "archives found:  {}", self.archives)?;
        writeln!(out, "sounds {verb}: {}", self.sounds.len())?;
        writeln!(out, "videos skipped:  {}", self.videos)?;
        writeln!(out, "other entries:   {} (graphics, text, ...)", self.other)?;
        writeln!(
            out,
            "total size:      {:.1} MiB",
            self.bytes as f64 / (1024.0 * 1024.0)
        )?;
        writeln!(out, "failed entries:  {}", self.failed.len())?;
        for f in &self.failed {
            writeln!(out, "  {f}")?;
        }
        for p in &self.problems {
            writeln!(out, "problem: {p}")?;
        }
        Ok(())
    }
}

/// A MIX archive somewhere in a file: a `.mix` on disk or inside an image.
struct Source {
    label: String,
    file: PathBuf,
    offset: u64,
    len: u64,
}

fn has_ext(name: &str, ext: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext))
}

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

/// The MIX archives of the input; loose videos are counted.
fn sources(input: &Path, s: &mut Summary) -> Vec<Source> {
    let mut v = Vec::new();
    let file_name = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let disk_file = |p: &Path, v: &mut Vec<Source>, s: &mut Summary| {
        let name = file_name(p);
        if has_ext(&name, "mix") {
            match std::fs::metadata(p) {
                Ok(m) => v.push(Source {
                    label: name,
                    file: p.to_path_buf(),
                    offset: 0,
                    len: m.len(),
                }),
                Err(e) => s.problems.push(format!("{}: {e}", p.display())),
            }
        } else if has_ext(&name, "vqa") {
            s.videos += 1;
        }
    };
    if input.is_dir() {
        let mut files = Vec::new();
        walk(input, 0, &mut files);
        for p in files {
            disk_file(&p, &mut v, s);
        }
    } else if has_ext(&file_name(input), "iso") {
        let listing = File::open(input)
            .map_err(|e| e.to_string())
            .and_then(|mut f| iso::list_files(&mut f).map_err(|e| e.to_string()));
        match listing {
            Ok(l) => {
                s.problems
                    .extend(l.skipped.into_iter().map(|x| format!("ISO: {x}")));
                for f in l.files {
                    let name = f.path.rsplit('/').next().unwrap_or("").to_string();
                    if has_ext(&name, "mix") {
                        v.push(Source {
                            label: name,
                            file: input.to_path_buf(),
                            offset: f.offset,
                            len: u64::from(f.size),
                        });
                    } else if has_ext(&name, "vqa") {
                        s.videos += 1;
                    }
                }
            }
            Err(e) => s.problems.push(format!("{}: {e}", input.display())),
        }
    } else if input.is_file() {
        disk_file(input, &mut v, s);
        if v.is_empty() && s.videos == 0 {
            s.problems.push(format!(
                "{}: not an .iso, .mix or directory (zip files: unzip first)",
                input.display()
            ));
        }
    } else {
        s.problems
            .push(format!("{} does not exist", input.display()));
    }
    v
}

struct Extractor<'a> {
    key: &'a RsaKey,
    names: &'a BTreeMap<u32, String>,
    out: &'a Path,
    dry_run: bool,
    log: &'a mut dyn Write,
    s: Summary,
    regions: BTreeSet<(PathBuf, u64, u32)>,
    entries: usize,
    decoded: u64,
    capped: bool,
}

enum Kind {
    Video,
    Aud,
    Other,
}

impl Extractor<'_> {
    fn cap(&mut self, what: &str) {
        if !self.capped {
            self.capped = true;
            self.s
                .problems
                .push(format!("stopped early: more than {what}"));
        }
    }

    fn sniff(file: &mut File, e: &MixEntry) -> std::io::Result<Kind> {
        let mut head = [0u8; 20];
        let n = (e.size as usize).min(head.len());
        file.seek(SeekFrom::Start(e.offset))?;
        file.read_exact(&mut head[..n])?;
        let h = &head[..n];
        Ok(
            if h.len() >= 12 && &h[0..4] == b"FORM" && &h[8..12] == b"WVQA" {
                Kind::Video
            } else if aud::parse_header(h).is_ok() {
                Kind::Aud
            } else {
                Kind::Other
            },
        )
    }

    fn archive(&mut self, file: &Path, index: &MixIndex, rel: &Path, label: &str, depth: usize) {
        if self.s.archives >= MAX_ARCHIVES {
            self.cap(&format!("{MAX_ARCHIVES} archives"));
            return;
        }
        self.s.archives += 1;
        let mut f = match File::open(file) {
            Ok(f) => f,
            Err(e) => {
                self.s.problems.push(format!("{label}: {e}"));
                return;
            }
        };
        let mut used = BTreeSet::new();
        for e in &index.entries {
            if self.entries >= MAX_ENTRIES {
                self.cap(&format!("{MAX_ENTRIES} entries"));
                return;
            }
            self.entries += 1;
            if !self.regions.insert((file.to_path_buf(), e.offset, e.size)) {
                continue;
            }
            let name = self.names.get(&e.id).map(String::as_str);
            let shown = name.map_or_else(|| format!("#{:08X}", e.id), str::to_string);
            let kind = match Self::sniff(&mut f, e) {
                Ok(k) => k,
                Err(err) => {
                    self.s.failed.push(format!("{label} > {shown}: {err}"));
                    continue;
                }
            };
            match kind {
                Kind::Video => self.s.videos += 1,
                Kind::Aud => self.sound(&mut f, e, rel, label, &shown, name),
                Kind::Other => {
                    let nested = read_index(&mut f, e.offset, u64::from(e.size), self.key)
                        .ok()
                        .filter(|i| !i.entries.is_empty());
                    match nested {
                        Some(inner) if depth < MAX_NEST_DEPTH => {
                            let mut dir = safe(&shown);
                            while !used.insert(dir.clone()) {
                                dir.push('~');
                            }
                            self.archive(
                                file,
                                &inner,
                                &rel.join(&dir),
                                &format!("{label} > {shown}"),
                                depth + 1,
                            );
                        }
                        Some(_) => self.s.problems.push(format!(
                            "{label} > {shown}: archives nested deeper than {MAX_NEST_DEPTH}"
                        )),
                        None => self.s.other += 1,
                    }
                }
            }
        }
    }

    fn sound(
        &mut self,
        f: &mut File,
        e: &MixEntry,
        rel: &Path,
        label: &str,
        shown: &str,
        name: Option<&str>,
    ) {
        if self.decoded.saturating_add(u64::from(e.size)) > MAX_DECODED_BYTES {
            self.cap("the decode budget");
            return;
        }
        self.decoded += u64::from(e.size);
        let pcm = match read_entry(f, e)
            .map_err(|x| x.to_string())
            .and_then(|b| aud::decode(&b).map_err(|x| x.to_string()))
        {
            Ok(p) => p,
            Err(err) => {
                self.s.failed.push(format!("{label} > {shown}: {err}"));
                return;
            }
        };
        let out_rel = rel.join(wav_file_name(e.id, name));
        let data = wav::write(&pcm);
        let shown_path = out_rel.display().to_string().replace('\\', "/");
        if self.dry_run {
            let _ = writeln!(
                self.log,
                "{shown_path}  ({} Hz, {}, {} ms)",
                pcm.sample_rate,
                if pcm.channels == 2 { "stereo" } else { "mono" },
                pcm.duration_ms()
            );
        } else {
            let path = self.out.join(&out_rel);
            let written = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, &data));
            if let Err(err) = written {
                self.s.failed.push(format!(
                    "{label} > {shown}: cannot write {}: {err}",
                    path.display()
                ));
                return;
            }
        }
        self.s.bytes += data.len() as u64;
        self.s.sounds.push(shown_path);
    }
}

/// Run the extraction. `Err` only for a refusal or an unusable argument;
/// everything else ends up in the [`Summary`].
pub fn run(args: &Args, key: &RsaKey, log: &mut dyn Write) -> Result<Summary, String> {
    if let Some(repo) = git_repo_above(&args.out) {
        return Err(format!(
            "refusing to write to '{}': it is inside the git repository '{}'. \
             Extracted Red Alert audio must never end up in a repository (B11); \
             choose a folder outside it, e.g. C:\\Games\\RedAlertSounds or ~/RedAlertSounds.",
            args.out.display(),
            repo.display()
        ));
    }
    let names = match &args.names {
        Some(p) => read_names(p)?,
        None => BTreeMap::new(),
    };
    let mut summary = Summary::default();
    let srcs = sources(&args.input, &mut summary);
    let mut x = Extractor {
        key,
        names: &names,
        out: &args.out,
        dry_run: args.dry_run,
        log,
        s: summary,
        regions: BTreeSet::new(),
        entries: 0,
        decoded: 0,
        capped: false,
    };
    let mut used = BTreeSet::new();
    for src in srcs {
        let index = File::open(&src.file)
            .map_err(|e| e.to_string())
            .and_then(|mut f| {
                read_index(&mut f, src.offset, src.len, key).map_err(|e| e.to_string())
            });
        match index {
            Ok(index) => {
                let mut dir = safe(&src.label);
                while !used.insert(dir.clone()) {
                    dir.push('~');
                }
                x.archive(&src.file, &index, Path::new(&dir), &src.label, 0);
            }
            Err(e) => x.s.problems.push(format!("{}: {e}", src.label)),
        }
    }
    Ok(x.s)
}

#[cfg(test)]
mod tests;
