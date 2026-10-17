//! Checks on a parsed [`Scene`]: references, times, overlaps, asset paths.
//!
//! [`validate`] looks only at the scene itself (no files are touched) and
//! separates **errors** (the scene cannot be played as written) from
//! **warnings** (it can, but probably not as intended). [`missing_assets`] is
//! the file-system half: it lists referenced asset files that do not exist. A
//! missing asset is never an error, because placeholders are allowed while the
//! art and the recordings are still being made.
//!
//! Issues carry the *path* of the offending value (`timeline.tracks[1].events[3]`);
//! the loader turns the path into a line number ([`crate::span::SpanIndex`]).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use crate::math::Ms;
use crate::model::{Art, Delivery, Event, EventKind, FORMAT_VERSION, Scene};

/// One problem found in a scene.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// Path of the value, e.g. `timeline.tracks[0].events[2].kind.speaker`.
    pub path: String,
    pub message: String,
    /// 1-based line in the source text, when known.
    pub line: Option<usize>,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "line {l}: {}", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// Result of [`validate`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validation {
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

/// A referenced asset file that does not exist (yet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingAsset {
    /// The path as written in the scene.
    pub file: String,
    /// Where the scene refers to it, as a human sentence.
    pub used_by: String,
    /// Path of the value in the scene (for the line lookup).
    pub path: String,
    pub line: Option<usize>,
}

impl fmt::Display for MissingAsset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "line {l}: missing {} ({})", self.file, self.used_by),
            None => write!(f, "missing {} ({})", self.file, self.used_by),
        }
    }
}

/// A reference to an asset file inside a scene.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetRef {
    pub file: String,
    pub used_by: String,
    pub path: String,
}

/// Every asset file the scene refers to, in scene order.
pub fn asset_refs(scene: &Scene) -> Vec<AssetRef> {
    let mut out = Vec::new();
    let mut push = |file: &str, used_by: String, path: String| {
        out.push(AssetRef {
            file: file.to_string(),
            used_by,
            path,
        });
    };
    for (i, l) in scene.layers.iter().enumerate() {
        if let Art::Image(f) = &l.art {
            push(f, format!("layer {:?}", l.name), format!("layers[{i}].art"));
        }
    }
    for (c, ch) in scene.characters.iter().enumerate() {
        for (p, part) in ch.parts.iter().enumerate() {
            if let Art::Image(f) = &part.art {
                push(
                    f,
                    format!("part {:?} of {:?}", part.name, ch.id),
                    format!("characters[{c}].parts[{p}].art"),
                );
            }
        }
    }
    for (t, track) in scene.timeline.tracks.iter().enumerate() {
        for (i, e) in track.events.iter().enumerate() {
            let base = format!("timeline.tracks[{t}].events[{i}].kind");
            let who = format!(
                "{} at {} ms in track {:?}",
                e.kind.label(),
                e.at,
                track.name
            );
            match &e.kind {
                EventKind::Dialogue { voice, lipsync, .. } => {
                    if let Some(f) = voice {
                        push(f, format!("voice of the {who}"), format!("{base}.voice"));
                    }
                    if let Some(f) = lipsync {
                        push(
                            f,
                            format!("lip-sync of the {who}"),
                            format!("{base}.lipsync"),
                        );
                    }
                }
                EventKind::Sfx { sound: Some(f), .. } => {
                    push(f, format!("sound of the {who}"), format!("{base}.sound"));
                }
                EventKind::Music { file: Some(f), .. }
                | EventKind::Ambience { file: Some(f), .. } => {
                    push(f, format!("file of the {who}"), format!("{base}.file"));
                }
                _ => {}
            }
        }
    }
    out
}

/// Asset files the scene refers to that do not exist under `root`.
pub fn missing_assets(scene: &Scene, root: &Path) -> Vec<MissingAsset> {
    asset_refs(scene)
        .into_iter()
        .filter(|r| check_asset_path(&r.file).is_ok() && !root.join(&r.file).is_file())
        .map(|r| MissingAsset {
            file: r.file,
            used_by: r.used_by,
            path: r.path,
            line: None,
        })
        .collect()
}

/// An asset path must stay inside the asset root: relative, forward slashes, no
/// `..`.
pub fn check_asset_path(file: &str) -> Result<(), String> {
    if file.trim().is_empty() {
        return Err("asset path is empty".into());
    }
    if file.contains('\\') {
        return Err(format!("asset path {file:?} must use forward slashes"));
    }
    let bytes = file.as_bytes();
    if file.starts_with('/') || (bytes.len() >= 2 && bytes[1] == b':') {
        return Err(format!(
            "asset path {file:?} must be relative to the asset root, not absolute"
        ));
    }
    if file.split('/').any(|c| c == "..") {
        return Err(format!("asset path {file:?} must not contain '..'"));
    }
    Ok(())
}

/// Check a scene. See the module docs.
pub fn validate(scene: &Scene) -> Validation {
    let mut v = V {
        scene,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    v.header();
    v.layers();
    v.characters();
    v.timeline();
    v.stage_and_speech();
    Validation {
        errors: v.errors,
        warnings: v.warnings,
    }
}

struct V<'a> {
    scene: &'a Scene,
    errors: Vec<Issue>,
    warnings: Vec<Issue>,
}

/// Names given to shots by `CameraCut` events.
fn shot_names(scene: &Scene) -> BTreeSet<&str> {
    let mut set = BTreeSet::new();
    for t in &scene.timeline.tracks {
        for e in &t.events {
            if let EventKind::CameraCut {
                shot: Some(name), ..
            } = &e.kind
            {
                set.insert(name.as_str());
            }
        }
    }
    set
}

fn finite_pos(x: f32) -> bool {
    x.is_finite()
}

impl V<'_> {
    fn err(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.errors.push(Issue {
            path: path.into(),
            message: message.into(),
            line: None,
        });
    }

    fn warn(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(Issue {
            path: path.into(),
            message: message.into(),
            line: None,
        });
    }

    fn header(&mut self) {
        let s = self.scene;
        if s.format != FORMAT_VERSION {
            self.err(
                "format",
                format!(
                    "unsupported format version {} (this reader understands {FORMAT_VERSION})",
                    s.format
                ),
            );
        }
        if s.name.trim().is_empty() {
            self.err("name", "scene name is empty");
        }
        if s.stage.width <= 0 || s.stage.height <= 0 {
            self.err(
                "stage",
                format!(
                    "stage size {}x{} must be positive",
                    s.stage.width, s.stage.height
                ),
            );
        }
    }

    fn art(&mut self, path: &str, what: &str, art: &Art) {
        match art {
            Art::Image(f) => {
                if let Err(m) = check_asset_path(f) {
                    self.err(path, format!("{what}: {m}"));
                }
            }
            Art::Rect { w, h, .. } => {
                if !(w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0) {
                    self.err(
                        path,
                        format!("{what}: rectangle size {w} x {h} must be positive"),
                    );
                }
            }
            Art::Circle { radius, .. } => {
                if !(radius.is_finite() && *radius > 0.0) {
                    self.err(
                        path,
                        format!("{what}: circle radius {radius} must be positive"),
                    );
                }
            }
        }
    }

    fn layers(&mut self) {
        let scene = self.scene;
        let shots = shot_names(scene);
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for (i, l) in scene.layers.iter().enumerate() {
            let p = format!("layers[{i}]");
            if let Some(first) = seen.insert(&l.name, i) {
                self.err(
                    &p,
                    format!(
                        "layer name {:?} is used twice (also layers[{first}])",
                        l.name
                    ),
                );
            }
            self.art(&format!("{p}.art"), &format!("layer {:?}", l.name), &l.art);
            if !l.pos.is_finite() || !finite_pos(l.parallax) {
                self.err(&p, format!("layer {:?} has a non-finite number", l.name));
            }
            for (k, s) in l.shots.iter().enumerate() {
                if !shots.contains(s.as_str()) {
                    self.err(
                        format!("{p}.shots[{k}]"),
                        format!(
                            "layer {:?} is shown in shot {s:?}, but no CameraCut creates a shot of that name",
                            l.name
                        ),
                    );
                }
            }
        }
    }

    fn characters(&mut self) {
        let scene = self.scene;
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for (c, ch) in scene.characters.iter().enumerate() {
            let cp = format!("characters[{c}]");
            if ch.id.trim().is_empty() {
                self.err(&cp, "character id is empty");
            }
            if let Some(first) = seen.insert(&ch.id, c) {
                self.err(
                    &cp,
                    format!(
                        "character id {:?} is used twice (also characters[{first}])",
                        ch.id
                    ),
                );
            }
            if !(ch.scale.is_finite() && ch.scale > 0.0) {
                self.err(
                    &cp,
                    format!("character {:?}: scale {} must be positive", ch.id, ch.scale),
                );
            }

            // Parts.
            let mut parts: BTreeMap<&str, usize> = BTreeMap::new();
            for (p, part) in ch.parts.iter().enumerate() {
                let pp = format!("{cp}.parts[{p}]");
                if let Some(first) = parts.insert(&part.name, p) {
                    self.err(
                        &pp,
                        format!(
                            "character {:?}: part name {:?} is used twice (also parts[{first}])",
                            ch.id, part.name
                        ),
                    );
                }
                self.art(
                    &format!("{pp}.art"),
                    &format!("part {:?} of {:?}", part.name, ch.id),
                    &part.art,
                );
                if !part.pivot.is_finite() || !part.offset.is_finite() {
                    self.err(
                        &pp,
                        format!(
                            "character {:?}: part {:?} has a non-finite pivot or offset",
                            ch.id, part.name
                        ),
                    );
                }
            }
            for (p, part) in ch.parts.iter().enumerate() {
                if let Some(parent) = &part.parent {
                    if !parts.contains_key(parent.as_str()) {
                        self.err(
                            format!("{cp}.parts[{p}].parent"),
                            format!(
                                "character {:?}: part {:?} has unknown parent part {parent:?}",
                                ch.id, part.name
                            ),
                        );
                    } else if parent == &part.name {
                        self.err(
                            format!("{cp}.parts[{p}].parent"),
                            format!(
                                "character {:?}: part {:?} is its own parent",
                                ch.id, part.name
                            ),
                        );
                    }
                }
            }
            // Cycles: follow parents; more steps than parts means a loop.
            for (p, part) in ch.parts.iter().enumerate() {
                let mut cur = part;
                let mut steps = 0;
                while let Some(parent) = cur.parent.as_deref() {
                    let Some(&idx) = parts.get(parent) else { break };
                    cur = &ch.parts[idx];
                    steps += 1;
                    if steps > ch.parts.len() {
                        self.err(
                            format!("{cp}.parts[{p}].parent"),
                            format!(
                                "character {:?}: the parents of part {:?} form a loop",
                                ch.id, part.name
                            ),
                        );
                        break;
                    }
                }
            }

            // Poses.
            let mut poses: BTreeMap<&str, usize> = BTreeMap::new();
            for (k, pose) in ch.poses.iter().enumerate() {
                let kp = format!("{cp}.poses[{k}]");
                if let Some(first) = poses.insert(&pose.name, k) {
                    self.err(
                        &kp,
                        format!(
                            "character {:?}: pose name {:?} is used twice (also poses[{first}])",
                            ch.id, pose.name
                        ),
                    );
                }
                let mut in_pose: BTreeSet<&str> = BTreeSet::new();
                for (q, pp) in pose.parts.iter().enumerate() {
                    let qp = format!("{kp}.parts[{q}]");
                    if !parts.contains_key(pp.part.as_str()) {
                        self.err(
                            format!("{qp}.part"),
                            format!(
                                "character {:?}: pose {:?} refers to unknown part {:?}",
                                ch.id, pose.name, pp.part
                            ),
                        );
                    } else if !in_pose.insert(&pp.part) {
                        self.err(
                            &qp,
                            format!(
                                "character {:?}: pose {:?} lists part {:?} twice",
                                ch.id, pose.name, pp.part
                            ),
                        );
                    }
                    if !(pp.angle.is_finite() && pp.offset.is_finite() && pp.scale.is_finite()) {
                        self.err(
                            &qp,
                            format!(
                                "character {:?}: pose {:?}, part {:?} has a non-finite number",
                                ch.id, pose.name, pp.part
                            ),
                        );
                    }
                }
            }
        }
    }

    fn describe(&self, t: usize, i: usize, e: &Event) -> String {
        format!(
            "track {:?}, event {i}: {} at {} ms",
            self.scene.timeline.tracks[t].name,
            e.kind.label(),
            e.at
        )
    }

    fn timeline(&mut self) {
        let scene = self.scene;
        let mut names: BTreeMap<&str, usize> = BTreeMap::new();
        for (t, track) in scene.timeline.tracks.iter().enumerate() {
            let tp = format!("timeline.tracks[{t}]");
            if let Some(first) = names.insert(&track.name, t) {
                self.err(
                    &tp,
                    format!(
                        "track name {:?} is used twice (also tracks[{first}])",
                        track.name
                    ),
                );
            }
            let mut prev: Option<Ms> = None;
            for (i, e) in track.events.iter().enumerate() {
                let ep = format!("{tp}.events[{i}]");
                let who = self.describe(t, i, e);
                if e.at < 0 {
                    self.err(format!("{ep}.at"), format!("{who}: time is negative"));
                }
                if let Some(p) = prev
                    && e.at < p
                {
                    self.err(
                        format!("{ep}.at"),
                        format!(
                            "{who}: comes before the previous event of this track ({p} ms); \
                             events in a track must be sorted by time"
                        ),
                    );
                }
                prev = Some(e.at);
                self.event(&ep, &who, e);
            }
        }
    }

    fn character_known(&mut self, path: String, who: &str, id: &str) -> bool {
        if self.scene.character(id).is_some() {
            true
        } else {
            self.err(path, format!("{who}: unknown character {id:?}"));
            false
        }
    }

    fn duration(&mut self, ep: &str, who: &str, d: Ms, positive: bool) {
        if d < 0 {
            self.err(
                format!("{ep}.kind.duration_ms"),
                format!("{who}: duration {d} ms is negative"),
            );
        } else if positive && d == 0 {
            self.err(
                format!("{ep}.kind.duration_ms"),
                format!("{who}: duration must be more than 0 ms"),
            );
        }
    }

    fn asset(&mut self, path: String, who: &str, what: &str, file: &str) {
        if let Err(m) = check_asset_path(file) {
            self.err(path, format!("{who}: {what}: {m}"));
        }
    }

    fn event(&mut self, ep: &str, who: &str, e: &Event) {
        let kp = format!("{ep}.kind");
        match &e.kind {
            EventKind::Enter {
                character,
                pos,
                pose,
                ..
            } => {
                let known = self.character_known(format!("{kp}.character"), who, character);
                if !pos.is_finite() {
                    self.err(
                        format!("{kp}.pos"),
                        format!("{who}: position is not finite"),
                    );
                }
                if let (Some(pose), true) = (pose, known) {
                    self.pose_known(&format!("{kp}.pose"), who, character, pose);
                }
            }
            EventKind::Exit { character } => {
                self.character_known(format!("{kp}.character"), who, character);
            }
            EventKind::Move {
                character,
                to,
                duration_ms,
                ..
            } => {
                self.character_known(format!("{kp}.character"), who, character);
                if !to.is_finite() {
                    self.err(format!("{kp}.to"), format!("{who}: target is not finite"));
                }
                self.duration(ep, who, *duration_ms, false);
            }
            EventKind::SetPose {
                character,
                pose,
                duration_ms,
                ..
            } => {
                if self.character_known(format!("{kp}.character"), who, character) {
                    self.pose_known(&format!("{kp}.pose"), who, character, pose);
                }
                self.duration(ep, who, *duration_ms, false);
            }
            EventKind::Dialogue {
                speaker,
                text,
                duration_ms,
                voice,
                lipsync,
                ..
            } => {
                self.character_known(format!("{kp}.speaker"), who, speaker);
                if text.trim().is_empty() {
                    self.err(format!("{kp}.text"), format!("{who}: the line is empty"));
                }
                self.duration(ep, who, *duration_ms, true);
                if let Some(f) = voice {
                    self.asset(format!("{kp}.voice"), who, "voice", f);
                }
                if let Some(f) = lipsync {
                    self.asset(format!("{kp}.lipsync"), who, "lip-sync", f);
                }
            }
            EventKind::Sfx { sound, volume, .. } => {
                if let Some(f) = sound {
                    self.asset(format!("{kp}.sound"), who, "sound", f);
                }
                if !(volume.is_finite() && *volume >= 0.0) {
                    self.err(
                        format!("{kp}.volume"),
                        format!("{who}: volume {volume} must be 0 or more"),
                    );
                }
            }
            EventKind::Music { file, fade_ms, .. } | EventKind::Ambience { file, fade_ms, .. } => {
                if let Some(f) = file {
                    self.asset(format!("{kp}.file"), who, "file", f);
                }
                if *fade_ms < 0 {
                    self.err(
                        format!("{kp}.fade_ms"),
                        format!("{who}: fade {fade_ms} ms is negative"),
                    );
                }
            }
            EventKind::CameraPan {
                to, duration_ms, ..
            } => {
                if !to.is_finite() {
                    self.err(format!("{kp}.to"), format!("{who}: target is not finite"));
                }
                self.duration(ep, who, *duration_ms, false);
            }
            EventKind::CameraZoom {
                to, duration_ms, ..
            } => {
                if !(to.is_finite() && *to > 0.0) {
                    self.err(
                        format!("{kp}.to"),
                        format!("{who}: zoom {to} must be more than 0"),
                    );
                }
                self.duration(ep, who, *duration_ms, false);
            }
            EventKind::CameraCut { center, zoom, shot } => {
                if !center.is_finite() {
                    self.err(
                        format!("{kp}.center"),
                        format!("{who}: centre is not finite"),
                    );
                }
                if !(zoom.is_finite() && *zoom > 0.0) {
                    self.err(
                        format!("{kp}.zoom"),
                        format!("{who}: zoom {zoom} must be more than 0"),
                    );
                }
                if shot.as_deref().is_some_and(|s| s.trim().is_empty()) {
                    self.err(format!("{kp}.shot"), format!("{who}: shot name is empty"));
                }
            }
            EventKind::CameraShake {
                amplitude,
                duration_ms,
            } => {
                if !(amplitude.is_finite() && *amplitude >= 0.0) {
                    self.err(
                        format!("{kp}.amplitude"),
                        format!("{who}: amplitude {amplitude} must be 0 or more"),
                    );
                }
                self.duration(ep, who, *duration_ms, true);
            }
            EventKind::Transition { duration_ms, .. } | EventKind::Wait { duration_ms } => {
                self.duration(ep, who, *duration_ms, false);
            }
            EventKind::MapOverlay {
                markers,
                duration_ms,
                ..
            } => {
                self.duration(ep, who, *duration_ms, true);
                for (m, marker) in markers.iter().enumerate() {
                    if !marker.at.is_finite() || marker.to.is_some_and(|t| !t.is_finite()) {
                        self.err(
                            format!("{kp}.markers[{m}]"),
                            format!("{who}: marker {:?} is not finite", marker.label),
                        );
                    }
                }
            }
        }
    }

    fn pose_known(&mut self, path: &str, who: &str, character: &str, pose: &str) {
        let known = self
            .scene
            .character(character)
            .is_some_and(|c| c.pose(pose).is_some());
        if !known {
            self.err(
                path,
                format!("{who}: character {character:?} has no pose {pose:?}"),
            );
        }
    }

    /// Checks that need the events in play order: who is on stage, who talks
    /// over whom.
    fn stage_and_speech(&mut self) {
        let scene = self.scene;
        let ordered = scene.events_in_order();
        // Character -> time of the Enter that put it on stage.
        let mut on_stage: BTreeMap<&str, Ms> = BTreeMap::new();
        // Speaker -> (start, end) of the previous line.
        let mut last_line: BTreeMap<&str, (Ms, i64)> = BTreeMap::new();
        for o in ordered {
            let track = &scene.timeline.tracks[o.track];
            let e = &track.events[o.index];
            let who = self.describe(o.track, o.index, e);
            let kp = format!("timeline.tracks[{}].events[{}].kind", o.track, o.index);
            match o.kind {
                EventKind::Enter { character, .. } => {
                    if scene.character(character).is_none() {
                        continue;
                    }
                    if let Some(since) = on_stage.insert(character, o.at) {
                        self.err(
                            format!("{kp}.character"),
                            format!(
                                "{who}: {character:?} is already on stage (entered at {since} ms)"
                            ),
                        );
                    }
                }
                EventKind::Exit { character } => {
                    if scene.character(character).is_some()
                        && on_stage.remove(character.as_str()).is_none()
                    {
                        self.err(
                            format!("{kp}.character"),
                            format!("{who}: {character:?} is not on stage"),
                        );
                    }
                }
                EventKind::Move { character, .. } | EventKind::SetPose { character, .. } => {
                    if scene.character(character).is_some()
                        && !on_stage.contains_key(character.as_str())
                    {
                        self.err(
                            format!("{kp}.character"),
                            format!("{who}: {character:?} is not on stage at that time"),
                        );
                    }
                }
                EventKind::Dialogue {
                    speaker,
                    duration_ms,
                    delivery,
                    ..
                } => {
                    if scene.character(speaker).is_none() {
                        continue;
                    }
                    if *delivery == Delivery::OnScreen && !on_stage.contains_key(speaker.as_str()) {
                        self.warn(
                            format!("{kp}.speaker"),
                            format!(
                                "{who}: {speaker:?} speaks on screen but is not on stage \
                                 (use delivery: VoiceOver or OffScreen, or add an Enter)"
                            ),
                        );
                    }
                    if *duration_ms <= 0 {
                        continue;
                    }
                    let end = i64::from(o.at) + i64::from(*duration_ms);
                    if let Some(&(prev_start, prev_end)) = last_line.get(speaker.as_str())
                        && i64::from(o.at) < prev_end
                    {
                        self.err(
                            format!("{kp}.at"),
                            format!(
                                "{who}: {speaker:?} is still talking (the line from \
                                 {prev_start} ms runs to {prev_end} ms)"
                            ),
                        );
                    }
                    let keep = last_line
                        .get(speaker.as_str())
                        .is_none_or(|&(_, prev_end)| end > prev_end);
                    if keep {
                        last_line.insert(speaker, (o.at, end));
                    }
                }
                _ => {}
            }
        }
    }
}
