//! Lip-sync (WP12.3): which mouth shape a character shows at a given moment.
//!
//! * [`shape`]: the nine shapes (Rhubarb's basic six `A`-`F`, the extended `G`,
//!   `H` and `X` for rest), the fixed table of similar shapes used when a rig
//!   lacks one, and the geometric placeholder mouths.
//! * [`parse`]: reads Rhubarb's JSON and TSV output into [`MouthCues`], an
//!   integer-millisecond time series, with line-numbered errors and warnings.
//! * [`flap`]: the fallback for a line without a cue file, made from the text.
//!
//! A `Dialogue` event's `lipsync` field names the cue file. The loader reads
//! the files into the scene's [`MouthCueSet`] (when it has an asset root), and
//! [`Scene::sample`](crate::Scene::sample) reports the shape per character in
//! [`CharacterState::mouth`](crate::CharacterState::mouth): from the cue file
//! while the character's line is running, otherwise `X`. Time zero of a cue
//! file is the start of the line, as for the voice recording.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::math::Ms;
use crate::model::{EventKind, Scene};

pub mod flap;
pub mod parse;
pub mod shape;

#[cfg(test)]
mod parse_tests;
#[cfg(test)]
mod tests;

pub use parse::{LipsyncError, LipsyncWarning, Parsed, parse as parse_cues, parse_json, parse_tsv};
pub use shape::{MouthShape, placeholder_mouth_shapes};

/// Cue files larger than this are refused: a line of speech is a few hundred
/// cues, a megabyte is a mistake.
pub const MAX_CUE_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// One change of mouth shape: from `start_ms` (counted from the start of the
/// line) the mouth shows `shape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouthCue {
    pub start_ms: Ms,
    pub shape: MouthShape,
}

/// A time series of mouth shapes. Each cue holds until the next one starts;
/// before the first cue the mouth is at rest (`X`). Start times are strictly
/// increasing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MouthCues {
    cues: Vec<MouthCue>,
}

impl MouthCues {
    /// Build from cues the caller has already put in strictly increasing
    /// order of start time (the parsers and the flap do).
    pub(crate) fn from_sorted(cues: Vec<MouthCue>) -> MouthCues {
        debug_assert!(cues.windows(2).all(|w| w[0].start_ms < w[1].start_ms));
        MouthCues { cues }
    }

    /// Build from any list: sorted by time, and where two cues share a time
    /// the later one in the list wins.
    pub fn new(mut cues: Vec<MouthCue>) -> MouthCues {
        cues.sort_by_key(|c| c.start_ms); // stable
        let mut out: Vec<MouthCue> = Vec::with_capacity(cues.len());
        for c in cues {
            match out.last_mut() {
                Some(last) if last.start_ms == c.start_ms => *last = c,
                _ => out.push(c),
            }
        }
        MouthCues { cues: out }
    }

    pub fn cues(&self) -> &[MouthCue] {
        &self.cues
    }

    pub fn len(&self) -> usize {
        self.cues.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }

    /// The shape shown `elapsed_ms` after the start of the sound: the last cue
    /// that has started (a cue is in force from its own start time), `X`
    /// before the first.
    pub fn shape_at(&self, elapsed_ms: Ms) -> MouthShape {
        let started = self.cues.partition_point(|c| c.start_ms <= elapsed_ms);
        match started.checked_sub(1) {
            Some(i) => self.cues[i].shape,
            None => MouthShape::X,
        }
    }

    /// The fallback for a line without a cue file; see [`flap`].
    pub fn flap(text: &str, duration_ms: Ms) -> MouthCues {
        flap::flap(text, duration_ms)
    }
}

/// The cue files of a scene, by the asset path the `Dialogue` events name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MouthCueSet {
    files: BTreeMap<String, MouthCues>,
}

impl MouthCueSet {
    pub fn insert(&mut self, path: impl Into<String>, cues: MouthCues) {
        self.files.insert(path.into(), cues);
    }

    pub fn get(&self, path: &str) -> Option<&MouthCues> {
        self.files.get(path)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
}

/// The mouth shape of a speaker `elapsed_ms` into a line of `duration_ms`: from
/// the cue file when `lipsync` names one that was loaded, else from the flap
/// made of the text. Outside the line (before it starts, at or after its end)
/// the mouth is at rest.
pub fn line_shape(
    set: &MouthCueSet,
    lipsync: Option<&str>,
    text: &str,
    elapsed_ms: Ms,
    duration_ms: Ms,
) -> MouthShape {
    if elapsed_ms < 0 || elapsed_ms >= duration_ms {
        return MouthShape::X;
    }
    match lipsync.and_then(|p| set.get(p)) {
        Some(cues) => cues.shape_at(elapsed_ms),
        None => flap::flap(text, duration_ms).shape_at(elapsed_ms),
    }
}

/// A problem with a cue file, found while loading a scene. It never stops the
/// scene from loading: the line is lip-synced with the flap instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LipsyncIssue {
    /// The cue file, as the scene names it (relative to the asset root).
    pub file: String,
    /// Line in the cue file, when the problem is in the content.
    pub line: Option<usize>,
    pub message: String,
    /// True: the file could not be used at all. False: it was used after
    /// being corrected.
    pub unusable: bool,
}

/// Read the cue file of every `Dialogue` with a `lipsync` path that exists
/// below `root` into the scene's [`Scene::mouth_cues`]. A file that does not
/// exist is not an issue here (the loader lists it as a missing asset); one
/// that cannot be read or understood is, and that line keeps the flap.
pub fn load_cue_files(scene: &mut Scene, root: &Path) -> Vec<LipsyncIssue> {
    // The longest line that uses each file: that is what the cues are
    // checked against.
    let mut longest: BTreeMap<String, Ms> = BTreeMap::new();
    for track in &scene.timeline.tracks {
        for e in &track.events {
            if let EventKind::Dialogue {
                lipsync: Some(path),
                duration_ms,
                ..
            } = &e.kind
            {
                let d = longest.entry(path.clone()).or_insert(0);
                *d = (*d).max(*duration_ms);
            }
        }
    }
    let mut issues = Vec::new();
    for (path, duration_ms) in longest {
        if crate::validate::check_asset_path(&path).is_err() {
            continue; // the validator has reported it
        }
        let full = root.join(&path);
        let text = match read_limited(&full) {
            Ok(Some(t)) => t,
            Ok(None) => continue,
            Err(message) => {
                issues.push(LipsyncIssue {
                    file: path,
                    line: None,
                    message,
                    unusable: true,
                });
                continue;
            }
        };
        match parse::parse(&text, duration_ms) {
            Ok(parsed) => {
                for w in parsed.warnings {
                    issues.push(LipsyncIssue {
                        file: path.clone(),
                        line: Some(w.line),
                        message: w.message,
                        unusable: false,
                    });
                }
                scene.mouth_cues.insert(path, parsed.cues);
            }
            Err(e) => issues.push(LipsyncIssue {
                file: path,
                line: Some(e.line),
                message: e.message,
                unusable: true,
            }),
        }
    }
    issues
}

/// The text of a file, `None` when there is no such file (a folder is no file).
fn read_limited(path: &Path) -> Result<Option<String>, String> {
    match fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read: {e}")),
        Ok(m) if !m.is_file() => return Ok(None),
        Ok(_) => {}
    }
    let file = fs::File::open(path).map_err(|e| format!("cannot read: {e}"))?;
    let mut text = String::new();
    file.take(MAX_CUE_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read: {e}"))?;
    if text.len() as u64 > MAX_CUE_FILE_BYTES {
        return Err(format!(
            "cue file is larger than {} MiB",
            MAX_CUE_FILE_BYTES / (1024 * 1024)
        ));
    }
    Ok(Some(text))
}
