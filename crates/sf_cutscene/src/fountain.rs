//! Fountain briefing -> skeleton scene.
//!
//! Writers draft briefings in [Fountain](https://fountain.io) following
//! `content/CONVENTIONS.md`. [`convert`] turns such a file into a scene RON
//! that validates and plays (with geometric placeholder figures and silent
//! sound slots), so the artist has something to refine and the cartoon player
//! something to show before any art exists.
//!
//! # Mapping
//!
//! | Fountain | Scene |
//! |---|---|
//! | title page `Title:` | `title` |
//! | scene heading (`INT. FIELD TENT - DAWN`) | a **shot**: `CameraCut` with a shot name, two placeholder background layers for it, the characters named in its action lines and speaking in it `Enter`, a 2 s `Wait` to establish the place; the previous shot's characters `Exit` |
//! | cue + dialogue | `Dialogue`; duration = words / 2.5 per second, at least 1.5 s; 0.3 s pause after every line |
//! | `(V.O.)` / `(O.S.)` after a cue | `delivery: VoiceOver` / `OffScreen` (that character is not put on stage) |
//! | parenthetical `(tired, steady)` | a `SetPose` with `hint` set to the text, choosing one of the placeholder poses `neutral`, `forceful`, `subdued`, `point` by keyword |
//! | `[[SFX: x]]` | `Sfx` with `note: "x"` (no file yet) |
//! | `[[MUSIC: x]]`, `[[AMBIENCE: x]]` | `Music` / `Ambience` with `note: "x"`; `stop`, `off`, `none`, `silence` become `action: Stop` |
//! | `[[SHOT: map]]` | `CameraCut` to the shot `map` (a full-frame placeholder map layer in front of everything); it lasts until the end of the next dialogue line (3 s if none follows), then the camera cuts back |
//! | `[[SHOT: wide]]` | `CameraCut` to zoom 0.85 for the rest of the shot |
//! | `[[SHOT: reaction NAME]]` | a 1.5 s close-up `CameraCut` on that character, then back; any other `SHOT` value has no preset and is reported |
//! | `[[MAP: x]]` | `MapOverlay` with `note: "x"` for the length of the map shot |
//! | `> FADE IN:` / `> FADE OUT.` | `Transition` fade in / out (0.8 s) |
//! | `CUT TO:` | `Transition` cut; other transitions become cuts with a warning |
//!
//! Action lines are used to decide who is on stage and are otherwise dropped
//! (the writer's description is not a cue for the cartoon). Anything the
//! importer cannot place is reported as a warning, never silently lost.
//!
//! Characters are cast in order of their first cue. Those that appear on
//! stage get a placeholder rig (see [`placeholder_character`]); those that only
//! ever speak as `(V.O.)` or `(O.S.)` are voice-only (no parts).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;

use ron::ser::PrettyConfig;

use crate::lipsync::placeholder_mouth_shapes;
use crate::math::{Color, Easing, Ms, Vec2};
use crate::model::{
    Art, Character, Delivery, Event, EventKind, FORMAT_VERSION, Facing, Layer, LoopAction, Part,
    PartPose, Pose, Scene, Stage, Timeline, Track, TransitionKind,
};

/// How long a spoken line lasts at least.
pub const MIN_LINE_MS: Ms = 1500;
/// Speaking speed: 2.5 words per second, i.e. 400 ms per word.
pub const MS_PER_WORD: Ms = 400;
/// Pause after every line of dialogue.
pub const LINE_GAP_MS: Ms = 300;
/// How long a new shot is held before the first line.
pub const ESTABLISHING_MS: Ms = 2000;
/// Length of fade in / fade out.
pub const FADE_MS: Ms = 800;
/// How long the map shot lasts when no dialogue line follows it.
pub const MAP_ONLY_MS: Ms = 3000;
/// How long a pose hint takes to blend in.
pub const POSE_BLEND_MS: Ms = 300;
/// How long a `[[SHOT: reaction NAME]]` cut-away lasts.
pub const REACTION_MS: Ms = 1500;
/// Zoom of `[[SHOT: wide]]` and of a reaction close-up.
pub const WIDE_ZOOM: f32 = 0.85;
pub const CLOSE_ZOOM: f32 = 1.6;

/// Settings for [`convert`].
#[derive(Clone, Debug)]
pub struct ImportOptions {
    /// Scene name (identifier); see [`scene_name`].
    pub name: String,
    /// Where the Fountain text came from (a path), recorded in the scene.
    pub source: Option<String>,
}

/// Something the importer did not understand or had to guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportWarning {
    /// Line in the Fountain file.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// The result of [`convert`].
#[derive(Clone, Debug)]
pub struct Import {
    /// The scene as built.
    pub scene: Scene,
    /// The same scene as commented, readable RON; parsing it gives `scene`
    /// back exactly.
    pub ron: String,
    pub warnings: Vec<ImportWarning>,
}

/// An identifier from a file stem: lower case, runs of anything that is not a
/// letter or digit become one `_` (`EXAMPLE-bridge` -> `example_bridge`).
pub fn scene_name(stem: &str) -> String {
    slug(stem)
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('_');
            }
            pending = false;
            out.extend(c.to_lowercase());
        } else {
            pending = true;
        }
    }
    if out.is_empty() {
        out.push_str("unnamed");
    }
    out
}

/// Estimated length of a spoken line.
pub fn estimate_line_ms(text: &str) -> Ms {
    let words = text
        .split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count();
    let ms = i64::try_from(words).unwrap_or(i64::MAX / 1000) * i64::from(MS_PER_WORD);
    ms.clamp(i64::from(MIN_LINE_MS), i64::from(Ms::MAX / 4)) as Ms
}

// ---------------------------------------------------------------------------
// Pass 1: Fountain text -> elements
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Elem {
    Blank,
    Heading {
        line: usize,
        text: String,
    },
    Action {
        line: usize,
        text: String,
    },
    Transition {
        line: usize,
        text: String,
    },
    Cue {
        line: usize,
        name: String,
        ext: String,
    },
    Paren {
        line: usize,
        text: String,
    },
    Text {
        line: usize,
        text: String,
    },
    Note {
        line: usize,
        key: Option<String>,
        value: String,
    },
}

struct Parsed {
    title: Option<String>,
    elems: Vec<Elem>,
    warnings: Vec<ImportWarning>,
}

/// Pull `[[...]]` notes out of a line. Notes may continue over several lines:
/// `carry` holds the start line and text of an unfinished one.
fn extract_notes(
    raw: &str,
    line_no: usize,
    carry: &mut Option<(usize, String)>,
) -> (String, Vec<(usize, String)>) {
    let mut rest = String::new();
    let mut notes = Vec::new();
    let mut s = raw;
    loop {
        if let Some((start, mut buf)) = carry.take() {
            match s.find("]]") {
                Some(p) => {
                    buf.push(' ');
                    buf.push_str(s[..p].trim());
                    notes.push((start, buf.trim().to_string()));
                    s = &s[p + 2..];
                    continue;
                }
                None => {
                    buf.push(' ');
                    buf.push_str(s.trim());
                    *carry = Some((start, buf));
                    return (rest, notes);
                }
            }
        }
        match s.find("[[") {
            None => {
                rest.push_str(s);
                return (rest, notes);
            }
            Some(p) => {
                rest.push_str(&s[..p]);
                s = &s[p + 2..];
                match s.find("]]") {
                    Some(q) => {
                        notes.push((line_no, s[..q].trim().to_string()));
                        s = &s[q + 2..];
                    }
                    None => {
                        *carry = Some((line_no, s.trim().to_string()));
                        return (rest, notes);
                    }
                }
            }
        }
    }
}

fn note_elem(line: usize, text: &str) -> Elem {
    // `KEY: value` with an upper-case key is a convention note; anything else
    // is a free note for the writers.
    if let Some((k, v)) = text.split_once(':')
        && !k.is_empty()
        && k.chars().all(|c| c.is_ascii_uppercase() || c == '_')
    {
        return Elem::Note {
            line,
            key: Some(k.to_string()),
            value: v.trim().to_string(),
        };
    }
    Elem::Note {
        line,
        key: None,
        value: text.trim().to_string(),
    }
}

fn is_heading(s: &str) -> bool {
    let u = s.to_uppercase();
    [
        "INT.", "EXT.", "EST.", "INT ", "EXT ", "EST ", "INT/EXT", "INT./EXT", "I/E",
    ]
    .iter()
    .any(|p| u.starts_with(p))
}

fn is_cue(s: &str) -> bool {
    let name = s
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('^')
        .trim();
    !name.is_empty()
        && name.chars().any(char::is_alphabetic)
        && name == name.to_uppercase()
        && !s.starts_with('(')
}

fn split_cue(s: &str) -> (String, String) {
    let s = s.trim().trim_end_matches('^').trim();
    match s.split_once('(') {
        Some((name, ext)) => (
            name.trim().to_string(),
            ext.trim_end_matches(|c: char| c == ')' || c.is_whitespace())
                .trim()
                .to_string(),
        ),
        None => (s.to_string(), String::new()),
    }
}

fn parse(text: &str) -> Parsed {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut warnings = Vec::new();
    let mut elems = Vec::new();
    let mut title = None;
    let mut i = 0;

    // Title page: `Key: value` lines at the very top, up to the first blank.
    let looks_like_title = lines.first().is_some_and(|l| {
        l.split_once(':').is_some_and(|(k, _)| {
            !k.is_empty()
                && !l.starts_with(char::is_whitespace)
                && k.chars().all(|c| c.is_alphanumeric() || c == ' ')
        })
    });
    if looks_like_title {
        while i < lines.len() && !lines[i].trim().is_empty() {
            if let Some((k, v)) = lines[i].split_once(':')
                && k.trim().eq_ignore_ascii_case("title")
            {
                title = Some(v.trim().to_string());
            }
            i += 1;
        }
    }

    let mut carry: Option<(usize, String)> = None;
    let mut in_dialogue = false;
    let mut prev_blank = true;
    while i < lines.len() {
        let raw = lines[i];
        let line_no = i + 1;
        let next_nonblank = lines.get(i + 1).is_some_and(|l| !l.trim().is_empty());
        i += 1;

        if raw.trim().is_empty() && carry.is_none() {
            elems.push(Elem::Blank);
            in_dialogue = false;
            prev_blank = true;
            continue;
        }
        let (rest, notes) = extract_notes(raw, line_no, &mut carry);
        for (l, n) in notes {
            elems.push(note_elem(l, &n));
        }
        let s = rest.trim();
        if s.is_empty() {
            continue; // a line of notes only
        }
        let was_blank = prev_blank;
        prev_blank = false;

        if in_dialogue {
            if s.starts_with('(') && s.ends_with(')') {
                elems.push(Elem::Paren {
                    line: line_no,
                    text: s[1..s.len() - 1].trim().to_string(),
                });
            } else {
                elems.push(Elem::Text {
                    line: line_no,
                    text: s.to_string(),
                });
            }
        } else if let Some(h) = s.strip_prefix('.').filter(|h| !h.starts_with('.')) {
            elems.push(Elem::Heading {
                line: line_no,
                text: h.trim().to_string(),
            });
        } else if was_blank && is_heading(s) {
            elems.push(Elem::Heading {
                line: line_no,
                text: s.to_string(),
            });
        } else if let Some(t) = s.strip_prefix('>') {
            if t.trim_end().ends_with('<') {
                elems.push(Elem::Action {
                    line: line_no,
                    text: t.trim().trim_end_matches('<').trim().to_string(),
                });
            } else {
                elems.push(Elem::Transition {
                    line: line_no,
                    text: t.trim().to_string(),
                });
            }
        } else if was_blank && s == s.to_uppercase() && s.ends_with("TO:") {
            elems.push(Elem::Transition {
                line: line_no,
                text: s.to_string(),
            });
        } else if let Some(c) = s.strip_prefix('@').filter(|c| !split_cue(c).0.is_empty()) {
            let (name, ext) = split_cue(c);
            elems.push(Elem::Cue {
                line: line_no,
                name,
                ext,
            });
            in_dialogue = true;
        } else if let Some(a) = s.strip_prefix('!') {
            elems.push(Elem::Action {
                line: line_no,
                text: a.trim().to_string(),
            });
        } else if s.starts_with('#') || s.starts_with('=') {
            // Section, synopsis, page break: not part of the film.
        } else if was_blank && next_nonblank && is_cue(s) {
            let (name, ext) = split_cue(s);
            elems.push(Elem::Cue {
                line: line_no,
                name,
                ext,
            });
            in_dialogue = true;
        } else {
            elems.push(Elem::Action {
                line: line_no,
                text: s.to_string(),
            });
        }
    }
    if let Some((start, _)) = carry {
        warnings.push(ImportWarning {
            line: start,
            message: "note starting here is never closed with ]]; ignored".into(),
        });
    }
    Parsed {
        title,
        elems,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Placeholder figures and backgrounds
// ---------------------------------------------------------------------------

const BODY_COLORS: [Color; 8] = [
    Color::rgb(0x3a, 0x5f, 0x8a),
    Color::rgb(0x8a, 0x3a, 0x3a),
    Color::rgb(0x3a, 0x8a, 0x5f),
    Color::rgb(0x8a, 0x6b, 0x3a),
    Color::rgb(0x6b, 0x3a, 0x8a),
    Color::rgb(0x3a, 0x8a, 0x8a),
    Color::rgb(0x8a, 0x3a, 0x6b),
    Color::rgb(0x5f, 0x8a, 0x3a),
];

/// Where the feet of a standing character are (stage units, `+y` down).
const GROUND_Y: f32 = 330.0;
/// The rig's origin is the middle of the torso, this far above the feet.
const ORIGIN_Y: f32 = 100.0;

/// The colour of the placeholder mouth.
const MOUTH_COLOR: Color = Color::rgb(0x7a, 0x2e, 0x2e);

/// The geometric stand-in for a cutout figure: legs, torso, two arms, head,
/// eye and mouth, plus the four poses the importer can choose from.
pub fn placeholder_character(id: &str, name: &str, body: Color) -> Character {
    let skin = Color::rgb(0xe8, 0xc3, 0x9e);
    let rect = |w: f32, h: f32, color: Color| Art::Rect { w, h, color };
    let part =
        |name: &str, parent: Option<&str>, pivot: Vec2, offset: Vec2, z: i32, art: Art| Part {
            name: name.into(),
            parent: parent.map(str::to_string),
            pivot,
            offset,
            z,
            art,
            mouth_shapes: BTreeMap::new(),
        };
    let parts = vec![
        part(
            "legs",
            Some("torso"),
            Vec2::ZERO,
            Vec2::new(0.0, 170.0),
            -1,
            rect(90.0, 120.0, body.scaled(60)),
        ),
        part(
            "torso",
            None,
            Vec2::ZERO,
            Vec2::ZERO,
            0,
            rect(130.0, 220.0, body),
        ),
        // Arms hang from the shoulder: the joint is the top of the picture.
        part(
            "arm_l",
            Some("torso"),
            Vec2::new(0.0, -75.0),
            Vec2::new(-83.0, -95.0),
            2,
            rect(36.0, 150.0, body.scaled(80)),
        ),
        part(
            "arm_r",
            Some("torso"),
            Vec2::new(0.0, -75.0),
            Vec2::new(83.0, -95.0),
            2,
            rect(36.0, 150.0, body.scaled(80)),
        ),
        part(
            "head",
            Some("torso"),
            Vec2::ZERO,
            Vec2::new(0.0, -165.0),
            3,
            Art::Circle {
                radius: 55.0,
                color: skin,
            },
        ),
        part(
            "eye",
            Some("head"),
            Vec2::ZERO,
            Vec2::new(18.0, -8.0),
            4,
            Art::Circle {
                radius: 7.0,
                color: Color::BLACK,
            },
        ),
        // The resting mouth, with a variant for every mouth shape (WP12.3), so
        // the figure talks as soon as the scene has a line for it.
        Part {
            mouth_shapes: placeholder_mouth_shapes(36.0, 8.0, MOUTH_COLOR),
            ..part(
                "mouth",
                Some("head"),
                Vec2::ZERO,
                Vec2::new(8.0, 22.0),
                4,
                rect(36.0, 8.0, MOUTH_COLOR),
            )
        },
    ];
    let pp = |part: &str, angle: f32, offset: Vec2| PartPose {
        part: part.into(),
        angle,
        offset,
        scale: Vec2::ONE,
    };
    let poses = vec![
        Pose {
            name: "neutral".into(),
            parts: vec![],
        },
        Pose {
            name: "forceful".into(),
            parts: vec![
                pp("torso", 3.0, Vec2::ZERO),
                pp("head", -4.0, Vec2::new(4.0, -3.0)),
                pp("arm_r", -20.0, Vec2::ZERO),
            ],
        },
        Pose {
            name: "subdued".into(),
            parts: vec![
                pp("torso", 2.0, Vec2::ZERO),
                pp("head", 12.0, Vec2::new(0.0, 6.0)),
                pp("arm_l", 6.0, Vec2::ZERO),
                pp("arm_r", -6.0, Vec2::ZERO),
            ],
        },
        Pose {
            name: "point".into(),
            parts: vec![
                pp("torso", 2.0, Vec2::ZERO),
                pp("head", -3.0, Vec2::ZERO),
                pp("arm_r", -75.0, Vec2::ZERO),
            ],
        },
    ];
    Character {
        id: id.into(),
        name: name.into(),
        z: 0,
        scale: 1.0,
        parts,
        poses,
    }
}

/// Which placeholder pose a stage direction suggests.
pub fn pose_for_hint(hint: &str) -> &'static str {
    let h = hint.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| h.contains(w));
    if has(&["point"]) {
        "point"
    } else if has(&[
        "tired",
        "quiet",
        "wary",
        "worr",
        "concern",
        "soft",
        "sad",
        "grim",
        "hesita",
        "uncertain",
    ]) {
        "subdued"
    } else if has(&[
        "firm",
        "sharp",
        "clipped",
        "urgent",
        "irritat",
        "alert",
        "determin",
        "resolute",
        "serious",
        "breathless",
        "angry",
        "shout",
        "intense",
        "stern",
        "gravel",
    ]) {
        "forceful"
    } else {
        "neutral"
    }
}

/// Colours of the placeholder background by time of day in the heading.
fn wall_color(heading: &str) -> Color {
    let u = heading.to_uppercase();
    let time = u.rsplit(" - ").next().unwrap_or("");
    let has = |w: &str| time.contains(w);
    if has("NIGHT") {
        Color::rgb(0x1f, 0x2a, 0x44)
    } else if has("DUSK") || has("EVENING") {
        Color::rgb(0x7a, 0x5c, 0x6e)
    } else if has("DAWN") || has("BEFORE") || has("FIRST LIGHT") {
        Color::rgb(0x6b, 0x7a, 0x99)
    } else if has("MORNING") {
        Color::rgb(0x8a, 0xa0, 0xb8)
    } else if has("DAY") || has("AFTERNOON") || has("NOON") {
        Color::rgb(0x9f, 0xb6, 0xc9)
    } else {
        Color::rgb(0x7c, 0x87, 0x94)
    }
}

/// Where the `k`-th of `n` characters in a shot stands, and which way it
/// faces (towards the middle).
fn slot(k: usize, n: usize) -> (Vec2, Facing) {
    let x = match n {
        0 | 1 => 0,
        2 => [-360, 360][k.min(1)],
        3 => [-600, 0, 600][k.min(2)],
        _ => -720 + i32::try_from(k * 1440 / (n - 1)).unwrap_or(0),
    };
    let facing = if x > 0 { Facing::Left } else { Facing::Right };
    (Vec2::new(x as f32, ORIGIN_Y), facing)
}

// ---------------------------------------------------------------------------
// Pass 2: elements -> events
// ---------------------------------------------------------------------------

/// The lanes of a generated scene, in file order.
const TRACK_NAMES: [&str; 6] = [
    "transition",
    "camera",
    "stage",
    "dialogue",
    "audio",
    "overlay",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    Transition = 0,
    Camera = 1,
    Stage = 2,
    Dialogue = 3,
    Audio = 4,
    Overlay = 5,
}

struct Pending {
    at: Ms,
    kind: EventKind,
    line: usize,
}

struct CastMember {
    id: String,
    display: String,
    /// Appears on stage at least once (gets a rig).
    rig: bool,
}

struct MapState {
    start: Ms,
    /// Indices into the overlay lane of the `MapOverlay`s to stretch.
    overlays: Vec<usize>,
    dialogue_seen: bool,
}

struct Builder {
    lanes: [Vec<Pending>; 6],
    warnings: Vec<ImportWarning>,
    cast: Vec<CastMember>,
    cursor: Ms,
    layers: Vec<Layer>,
    used_slugs: BTreeSet<String>,
    shot: Option<String>,
    /// When the current shot began, and whether anyone has spoken in it yet.
    /// Audio notes before the first line belong to the start of the shot (the
    /// rain, the music), not to the end of the establishing hold.
    shot_start: Ms,
    shot_spoke: bool,
    /// Zoom the camera returns to after a cut-away (`wide` changes it).
    base_zoom: f32,
    on_stage: Vec<String>,
    /// Where each character stood when it entered the current shot.
    positions: Vec<(String, Vec2)>,
    map: Option<MapState>,
    map_layer_added: bool,
}

fn title_case(name: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in name.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        start = c.is_whitespace() || c == '-';
    }
    out
}

fn delivery_of(ext: &str) -> Delivery {
    let e: String = ext
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    match e.as_str() {
        "VO" => Delivery::VoiceOver,
        "OS" | "OC" => Delivery::OffScreen,
        _ => Delivery::OnScreen,
    }
}

/// Does `hay` contain `name` as a whole word? Returns the byte position.
fn find_word(hay: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(p) = hay[from..].find(name) {
        let start = from + p;
        let end = start + name.len();
        let before = hay[..start].chars().next_back();
        let after = hay[end..].chars().next();
        if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
            return Some(start);
        }
        from = start + name.len().max(1);
    }
    None
}

impl Builder {
    fn warn(&mut self, line: usize, message: impl Into<String>) {
        self.warnings.push(ImportWarning {
            line,
            message: message.into(),
        });
    }

    fn push(&mut self, lane: Lane, at: Ms, kind: EventKind, line: usize) -> usize {
        let v = &mut self.lanes[lane as usize];
        v.push(Pending { at, kind, line });
        v.len() - 1
    }

    fn cast_index(&self, id: &str) -> Option<usize> {
        self.cast.iter().position(|c| c.id == id)
    }

    /// Who is on stage in the shot that starts at `from` (the element after the
    /// heading): characters named in capitals in its action lines, then those
    /// who speak on screen, in order of appearance.
    fn stage_for(&self, elems: &[Elem], from: usize) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let add = |id: &str, found: &mut Vec<String>| {
            if !found.iter().any(|f| f == id) {
                found.push(id.to_string());
            }
        };
        // Action mentions, in text order within each action line.
        let mut spoken: Vec<String> = Vec::new();
        for e in &elems[from..] {
            match e {
                Elem::Heading { .. } => break,
                Elem::Action { text, .. } => {
                    let mut hits: Vec<(usize, &str)> = self
                        .cast
                        .iter()
                        .filter_map(|c| {
                            find_word(text, &c.display.to_uppercase()).map(|p| (p, c.id.as_str()))
                        })
                        .collect();
                    hits.sort_by_key(|h| h.0);
                    for (_, id) in hits {
                        add(id, &mut found);
                    }
                }
                Elem::Cue { name, ext, .. } if delivery_of(ext) == Delivery::OnScreen => {
                    spoken.push(slug(name));
                }
                _ => {}
            }
        }
        for id in spoken {
            add(&id, &mut found);
        }
        found
    }

    fn ensure_shot(&mut self, elems: &[Elem], idx: usize, line: usize) {
        if self.shot.is_none() {
            self.warn(
                line,
                "no scene heading before this; starting an unnamed location (INT. UNSPECIFIED - DAY)",
            );
            self.begin_shot("INT. UNSPECIFIED - DAY", line, elems, idx);
        }
    }

    fn begin_shot(&mut self, heading: &str, line: usize, elems: &[Elem], from: usize) {
        self.close_map(None);
        let at = self.cursor;
        for id in std::mem::take(&mut self.on_stage) {
            self.push(Lane::Stage, at, EventKind::Exit { character: id }, line);
        }
        // Unique shot name from the location part of the heading.
        let place = heading
            .trim_start_matches(|c: char| c.is_alphabetic() || c == '.' || c == '/')
            .trim();
        let mut base = slug(place);
        if base == "unnamed" {
            base = slug(heading);
        }
        let mut name = base.clone();
        let mut n = 2;
        while !self.used_slugs.insert(name.clone()) {
            name = format!("{base}_{n}");
            n += 1;
        }
        let wall = wall_color(heading);
        self.layers.push(Layer {
            name: format!("{name}_wall"),
            art: Art::Rect {
                w: 2400.0,
                h: 1400.0,
                color: wall,
            },
            pos: Vec2::ZERO,
            parallax: 0.8,
            z: -20,
            shots: vec![name.clone()],
        });
        self.layers.push(Layer {
            name: format!("{name}_floor"),
            art: Art::Rect {
                w: 2400.0,
                h: 420.0,
                color: wall.scaled(60),
            },
            pos: Vec2::new(0.0, GROUND_Y + 210.0),
            parallax: 1.0,
            z: -19,
            shots: vec![name.clone()],
        });
        self.push(
            Lane::Camera,
            at,
            EventKind::CameraCut {
                center: Vec2::ZERO,
                zoom: 1.0,
                shot: Some(name.clone()),
            },
            line,
        );
        let stage = self.stage_for(elems, from);
        let n = stage.len();
        self.positions.clear();
        for (k, id) in stage.into_iter().enumerate() {
            let (pos, facing) = slot(k, n);
            self.positions.push((id.clone(), pos));
            // Whoever stands on stage at least once gets a rig.
            if let Some(ci) = self.cast_index(&id) {
                self.cast[ci].rig = true;
            }
            self.push(
                Lane::Stage,
                at,
                EventKind::Enter {
                    character: id.clone(),
                    pos,
                    facing,
                    pose: Some("neutral".into()),
                },
                line,
            );
            self.on_stage.push(id);
        }
        self.push(
            Lane::Camera,
            at,
            EventKind::Wait {
                duration_ms: ESTABLISHING_MS,
            },
            line,
        );
        self.cursor = at + ESTABLISHING_MS;
        self.shot = Some(name);
        self.shot_start = at;
        self.shot_spoke = false;
        self.base_zoom = 1.0;
    }

    fn open_map(&mut self, elems: &[Elem], idx: usize, line: usize) {
        self.ensure_shot(elems, idx, line);
        if self.map.is_some() {
            return;
        }
        if !self.map_layer_added {
            self.map_layer_added = true;
            self.layers.push(Layer {
                name: "map_table".into(),
                art: Art::Rect {
                    w: 1920.0,
                    h: 1080.0,
                    color: Color::rgb(0xd9, 0xc9, 0xa0),
                },
                pos: Vec2::ZERO,
                parallax: 0.0,
                z: 40,
                shots: vec!["map".into()],
            });
        }
        let at = self.cursor;
        self.push(
            Lane::Camera,
            at,
            EventKind::CameraCut {
                center: Vec2::ZERO,
                zoom: 1.0,
                shot: Some("map".into()),
            },
            line,
        );
        self.map = Some(MapState {
            start: at,
            overlays: Vec::new(),
            dialogue_seen: false,
        });
    }

    /// End the map shot: cut back to the location and stretch the overlay over
    /// it. `end` is the end of the dialogue line that was shown over the map;
    /// without it the shot lasts at least [`MAP_ONLY_MS`].
    fn close_map(&mut self, end: Option<Ms>) {
        let Some(m) = self.map.take() else { return };
        let stand_alone = end.is_none();
        let end = end
            .unwrap_or_else(|| self.cursor.max(m.start + MAP_ONLY_MS))
            .max(m.start + 1);
        for &o in &m.overlays {
            let p = &mut self.lanes[Lane::Overlay as usize][o];
            if let EventKind::MapOverlay { duration_ms, .. } = &mut p.kind {
                *duration_ms = (end - p.at).max(1);
            }
        }
        let back = self.shot.clone();
        let zoom = self.base_zoom;
        self.push(
            Lane::Camera,
            end,
            EventKind::CameraCut {
                center: Vec2::ZERO,
                zoom,
                shot: back,
            },
            0,
        );
        if stand_alone {
            self.cursor = end;
        }
    }

    /// `[[SHOT: ...]]`: `map`, `wide`, `reaction NAME`. Anything else has no
    /// preset (the auto-director of WP12.9 is meant for those) and is reported.
    fn shot_hint(&mut self, elems: &[Elem], idx: usize, line: usize, value: &str) {
        let at = self.cursor;
        let lower = value.trim().to_lowercase();
        if lower == "map" {
            self.open_map(elems, idx, line);
        } else if self.map.is_some() {
            self.warn(
                line,
                format!("shot {value:?} ignored while the map is shown"),
            );
        } else if lower == "wide" {
            self.base_zoom = WIDE_ZOOM;
            self.push(
                Lane::Camera,
                at,
                EventKind::CameraCut {
                    center: Vec2::ZERO,
                    zoom: WIDE_ZOOM,
                    shot: None,
                },
                line,
            );
        } else if let Some(who) = lower.strip_prefix("reaction") {
            let id = slug(who);
            let pos = self
                .positions
                .iter()
                .find(|(c, _)| *c == id)
                .map(|(_, p)| *p)
                .filter(|_| self.on_stage.contains(&id));
            if let Some(p) = pos {
                self.push(
                    Lane::Camera,
                    at,
                    EventKind::CameraCut {
                        center: Vec2::new(p.x, p.y - 120.0),
                        zoom: CLOSE_ZOOM,
                        shot: None,
                    },
                    line,
                );
                let zoom = self.base_zoom;
                self.push(
                    Lane::Camera,
                    at + REACTION_MS,
                    EventKind::CameraCut {
                        center: Vec2::ZERO,
                        zoom,
                        shot: None,
                    },
                    line,
                );
                self.cursor = at + REACTION_MS;
            } else {
                self.warn(
                    line,
                    format!(
                        "reaction shot: {:?} is not on stage in this shot; ignored",
                        who.trim()
                    ),
                );
            }
        } else {
            self.warn(
                line,
                format!("shot {value:?} has no preset; cutting to the default framing"),
            );
            self.push(
                Lane::Camera,
                at,
                EventKind::CameraCut {
                    center: Vec2::ZERO,
                    zoom: self.base_zoom,
                    shot: None,
                },
                line,
            );
        }
    }

    fn transition(&mut self, line: usize, text: &str) {
        let u = text
            .trim()
            .trim_end_matches([':', '.'])
            .trim()
            .to_uppercase();
        match u.as_str() {
            "FADE IN" => {
                self.push(
                    Lane::Transition,
                    self.cursor,
                    EventKind::Transition {
                        kind: TransitionKind::FadeIn,
                        duration_ms: FADE_MS,
                        color: Color::BLACK,
                    },
                    line,
                );
            }
            "FADE OUT" | "FADE TO BLACK" | "FADE TO" => {
                self.close_map(None);
                self.push(
                    Lane::Transition,
                    self.cursor,
                    EventKind::Transition {
                        kind: TransitionKind::FadeOut,
                        duration_ms: FADE_MS,
                        color: Color::BLACK,
                    },
                    line,
                );
                self.cursor += FADE_MS;
            }
            other => {
                if other != "CUT TO" && other != "CUT" {
                    self.warn(
                        line,
                        format!("transition {text:?} is not supported; using a hard cut"),
                    );
                }
                self.close_map(None);
                self.push(
                    Lane::Transition,
                    self.cursor,
                    EventKind::Transition {
                        kind: TransitionKind::Cut,
                        duration_ms: 0,
                        color: Color::BLACK,
                    },
                    line,
                );
            }
        }
    }

    fn note(&mut self, elems: &[Elem], idx: usize, line: usize, key: Option<&str>, value: &str) {
        let at = self.cursor;
        let audio_at = if self.shot_spoke { at } else { self.shot_start };
        let Some(key) = key else { return };
        match key {
            "SFX" | "MUSIC" | "AMBIENCE" if value.is_empty() => {
                self.warn(line, format!("empty {key} note ignored"));
            }
            "SFX" => {
                self.push(
                    Lane::Audio,
                    audio_at,
                    EventKind::Sfx {
                        sound: None,
                        note: value.to_string(),
                        volume: 1.0,
                    },
                    line,
                );
            }
            "MUSIC" | "AMBIENCE" => {
                let v = value.to_lowercase();
                let action = if ["stop", "off", "none", "silence"].contains(&v.as_str()) {
                    LoopAction::Stop
                } else {
                    LoopAction::Play
                };
                let note = if action == LoopAction::Stop {
                    String::new()
                } else {
                    value.to_string()
                };
                let kind = if key == "MUSIC" {
                    EventKind::Music {
                        action,
                        file: None,
                        note,
                        fade_ms: 0,
                    }
                } else {
                    EventKind::Ambience {
                        action,
                        file: None,
                        note,
                        fade_ms: 0,
                    }
                };
                self.push(Lane::Audio, audio_at, kind, line);
            }
            "SHOT" => self.shot_hint(elems, idx, line, value),
            "MAP" => {
                if self.map.is_none() {
                    self.warn(
                        line,
                        "[[MAP: ...]] without [[SHOT: map]] before it; opening the map shot",
                    );
                    self.open_map(elems, idx, line);
                }
                // A second MAP in the same shot ends the first one.
                if let Some(m) = &self.map {
                    let prev = m.overlays.last().copied();
                    if let Some(p) = prev {
                        let pending = &mut self.lanes[Lane::Overlay as usize][p];
                        if let EventKind::MapOverlay { duration_ms, .. } = &mut pending.kind {
                            *duration_ms = (at - pending.at).max(1);
                        }
                    }
                }
                let o = self.push(
                    Lane::Overlay,
                    at,
                    EventKind::MapOverlay {
                        note: value.to_string(),
                        markers: Vec::new(),
                        duration_ms: 1,
                    },
                    line,
                );
                if let Some(m) = &mut self.map {
                    m.overlays.push(o);
                }
            }
            other => self.warn(line, format!("unknown note [[{other}: ...]] ignored")),
        }
    }

    fn dialogue(
        &mut self,
        speaker: &str,
        delivery: Delivery,
        hint: Option<&str>,
        text: &str,
        line: usize,
    ) {
        let start = self.cursor;
        if let Some(h) = hint
            && delivery == Delivery::OnScreen
            && self.on_stage.iter().any(|s| s == speaker)
        {
            self.push(
                Lane::Stage,
                start,
                EventKind::SetPose {
                    character: speaker.to_string(),
                    pose: pose_for_hint(h).to_string(),
                    duration_ms: POSE_BLEND_MS,
                    easing: Easing::EaseInOut,
                    hint: Some(h.to_string()),
                },
                line,
            );
        }
        let dur = estimate_line_ms(text);
        self.shot_spoke = true;
        self.push(
            Lane::Dialogue,
            start,
            EventKind::Dialogue {
                speaker: speaker.to_string(),
                text: text.to_string(),
                duration_ms: dur,
                voice: None,
                lipsync: None,
                delivery,
            },
            line,
        );
        let end = start + dur;
        let first_over_map = self.map.as_mut().is_some_and(|m| {
            let first = !m.dialogue_seen;
            m.dialogue_seen = true;
            first
        });
        if first_over_map {
            self.close_map(Some(end));
        }
        self.cursor = end + LINE_GAP_MS;
    }
}

struct Segment {
    line: usize,
    hint: Option<String>,
    text: String,
}

fn build(parsed: &Parsed) -> Builder {
    let elems = &parsed.elems;
    let mut b = Builder {
        lanes: Default::default(),
        warnings: Vec::new(),
        cast: Vec::new(),
        cursor: 0,
        layers: Vec::new(),
        used_slugs: BTreeSet::new(),
        shot: None,
        shot_start: 0,
        shot_spoke: false,
        base_zoom: 1.0,
        on_stage: Vec::new(),
        positions: Vec::new(),
        map: None,
        map_layer_added: false,
    };

    // Cast, in order of first cue.
    for e in elems {
        if let Elem::Cue { name, .. } = e {
            let id = slug(name);
            if b.cast_index(&id).is_none() {
                b.cast.push(CastMember {
                    id,
                    display: title_case(name.trim()),
                    rig: false,
                });
            }
        }
    }
    let mut idx = 0;
    while idx < elems.len() {
        match &elems[idx] {
            Elem::Blank | Elem::Paren { .. } | Elem::Text { .. } | Elem::Action { .. } => {}
            Elem::Heading { line, text } => b.begin_shot(text, *line, elems, idx + 1),
            Elem::Transition { line, text } => b.transition(*line, text),
            Elem::Note { line, key, value } => b.note(elems, idx, *line, key.as_deref(), value),
            Elem::Cue { line, name, ext } => {
                b.ensure_shot(elems, idx, *line);
                let speaker = slug(name);
                let delivery = delivery_of(ext);
                // The block: parentheticals, text and notes up to the next
                // blank line.
                let mut segs = vec![Segment {
                    line: *line,
                    hint: None,
                    text: String::new(),
                }];
                let mut j = idx + 1;
                while j < elems.len() {
                    match &elems[j] {
                        Elem::Paren { line, text } => {
                            let last = segs.last_mut().expect("one segment exists");
                            if last.text.is_empty() {
                                last.hint = Some(match last.hint.take() {
                                    Some(h) => format!("{h}, {text}"),
                                    None => text.clone(),
                                });
                                last.line = *line;
                            } else {
                                segs.push(Segment {
                                    line: *line,
                                    hint: Some(text.clone()),
                                    text: String::new(),
                                });
                            }
                        }
                        Elem::Text { line, text } => {
                            let last = segs.last_mut().expect("one segment exists");
                            if !last.text.is_empty() {
                                last.text.push(' ');
                            } else {
                                last.line = *line;
                            }
                            last.text.push_str(text);
                        }
                        Elem::Note { line, key, value } => {
                            b.note(elems, j, *line, key.as_deref(), value);
                        }
                        _ => break,
                    }
                    j += 1;
                }
                idx = j - 1;
                let mut any = false;
                for seg in segs {
                    let text = seg.text.replace('*', "");
                    let text = text.trim();
                    if text.is_empty() {
                        continue;
                    }
                    any = true;
                    b.dialogue(&speaker, delivery, seg.hint.as_deref(), text, seg.line);
                }
                if !any {
                    b.warn(
                        *line,
                        format!("{name} has no dialogue under the cue; skipped"),
                    );
                }
            }
        }
        idx += 1;
    }
    b.close_map(None);
    b
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn compact() -> PrettyConfig {
    PrettyConfig::new()
        .compact_structs(true)
        .compact_arrays(true)
}

/// `ron` writes `'` inside strings as `\'`, which reads badly in a line a
/// writer will edit. A bare `'` is equally valid in a RON string, so drop the
/// backslash (but leave `\\` and the other escapes alone).
fn plain_apostrophes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\'') => out.push('\''),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn ron_inline<T: serde::Serialize>(v: &T) -> String {
    plain_apostrophes(
        &ron::ser::to_string_pretty(v, compact()).unwrap_or_else(|e| format!("/* {e} */")),
    )
}

fn ron_string(s: &str) -> String {
    ron_inline(&s)
}

/// Render the scene as readable RON, with a source-line comment after every
/// event. `lines[t][i]` is the Fountain line of event `i` of track `t`.
fn emit(scene: &Scene, lines: &[Vec<usize>], source: Option<&str>) -> String {
    let mut o = String::new();
    o.push_str("// Skeleton scene generated by fountain2scene");
    if let Some(s) = source {
        let _ = write!(o, " from {s}");
    }
    o.push_str(
        ".\n// Placeholder figures, silent sound slots: replace them (docs/cutscene-format.md).\n",
    );
    o.push_str("// `// line N` after an event is the line in the Fountain file it came from.\n");
    o.push_str("#![enable(implicit_some)]\nScene(\n");
    let _ = writeln!(o, "    format: {FORMAT_VERSION},");
    let _ = writeln!(o, "    name: {},", ron_string(&scene.name));
    if let Some(t) = &scene.title {
        let _ = writeln!(o, "    title: {},", ron_string(t));
    }
    if let Some(s) = &scene.source {
        let _ = writeln!(o, "    source: {},", ron_string(s));
    }
    let _ = writeln!(o, "    stage: {},", ron_inline(&scene.stage));

    o.push_str("    layers: [\n");
    for l in &scene.layers {
        let _ = writeln!(o, "        {},", ron_inline(l));
    }
    o.push_str("    ],\n");

    o.push_str("    characters: [\n");
    for c in &scene.characters {
        let cfg = PrettyConfig::new().compact_structs(true).depth_limit(2);
        let pretty = plain_apostrophes(&ron::ser::to_string_pretty(c, cfg).unwrap_or_default());
        let mut first = true;
        for ln in pretty.lines() {
            if first {
                let _ = write!(o, "        {ln}");
                first = false;
            } else {
                let _ = write!(o, "\n        {ln}");
            }
        }
        o.push_str(",\n");
    }
    o.push_str("    ],\n");

    o.push_str("    timeline: (\n        tracks: [\n");
    for (t, track) in scene.timeline.tracks.iter().enumerate() {
        let _ = writeln!(
            o,
            "            (\n                name: {},",
            ron_string(&track.name)
        );
        o.push_str("                events: [\n");
        for (i, e) in track.events.iter().enumerate() {
            let line = lines.get(t).and_then(|l| l.get(i)).copied().unwrap_or(0);
            let mut text = format!("                    {},", ron_inline(e));
            if line > 0 {
                let _ = write!(text, "  // line {line}");
            }
            o.push_str(&text);
            o.push('\n');
        }
        o.push_str("                ],\n            ),\n");
    }
    o.push_str("        ],\n    ),\n)\n");
    o
}

/// Convert a Fountain briefing into a skeleton scene.
///
/// Never fails: whatever cannot be placed becomes a warning. The result is a
/// valid scene (see the tests, which run every briefing in the repository
/// through [`crate::load_str`]).
pub fn convert(text: &str, options: &ImportOptions) -> Import {
    let parsed = parse(text);
    let mut b = build(&parsed);
    let mut warnings = parsed.warnings.clone();
    warnings.append(&mut b.warnings);
    if b.shot.is_none() {
        warnings.push(ImportWarning {
            line: 1,
            message: "no scene heading and no dialogue found; the scene is empty".into(),
        });
    }

    let characters: Vec<Character> = b
        .cast
        .iter()
        .enumerate()
        .map(|(n, c)| {
            if c.rig {
                placeholder_character(&c.id, &c.display, BODY_COLORS[n % BODY_COLORS.len()])
            } else {
                Character {
                    id: c.id.clone(),
                    name: c.display.clone(),
                    z: 0,
                    scale: 1.0,
                    parts: Vec::new(),
                    poses: Vec::new(),
                }
            }
        })
        .collect();

    let mut tracks = Vec::new();
    let mut lines: Vec<Vec<usize>> = Vec::new();
    for (lane, name) in b.lanes.iter_mut().zip(TRACK_NAMES) {
        if lane.is_empty() {
            continue;
        }
        lane.sort_by_key(|p| p.at); // stable: equal times keep their order
        lines.push(lane.iter().map(|p| p.line).collect());
        tracks.push(Track {
            name: name.to_string(),
            events: lane
                .drain(..)
                .map(|p| Event {
                    at: p.at,
                    kind: p.kind,
                })
                .collect(),
        });
    }

    let scene = Scene {
        format: FORMAT_VERSION,
        name: options.name.clone(),
        title: parsed.title.clone().filter(|t| !t.is_empty()),
        source: options.source.clone(),
        stage: Stage::default(),
        layers: b.layers,
        characters,
        timeline: Timeline { tracks },
        mouth_cues: Default::default(),
    };
    let ron = emit(&scene, &lines, options.source.as_deref());
    Import {
        scene,
        ron,
        warnings,
    }
}

/// What [`convert_file`] did.
#[derive(Clone, Debug)]
pub struct FileReport {
    pub warnings: Vec<ImportWarning>,
    /// Scene problems found when the written file was read back; empty when
    /// the conversion is sound.
    pub errors: Vec<String>,
    pub missing_assets: Vec<crate::validate::MissingAsset>,
    pub tracks: usize,
    pub events: usize,
    pub duration_ms: Ms,
}

/// Convert `input` (a `.fountain` file) to `output` (a scene `.ron`), then
/// read the output back through the loader to prove it is valid. Failing to
/// read or write a file is an `Err`; an invalid result is reported in
/// [`FileReport::errors`]. The output file is written either way, so it can be
/// inspected.
///
/// `name` defaults to the output file's stem. `assets_root` enables the
/// missing-asset list.
pub fn convert_file(
    input: &std::path::Path,
    output: &std::path::Path,
    name: Option<&str>,
    assets_root: Option<&std::path::Path>,
) -> Result<FileReport, String> {
    let text = std::fs::read_to_string(input)
        .map_err(|e| format!("{}: cannot read: {e}", input.display()))?;
    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("scene");
    let options = ImportOptions {
        name: name.map_or_else(|| scene_name(stem), str::to_string),
        source: Some(input.display().to_string().replace('\\', "/")),
    };
    let import = convert(&text, &options);
    std::fs::write(output, &import.ron)
        .map_err(|e| format!("{}: cannot write: {e}", output.display()))?;
    let (errors, missing_assets) = match crate::load::load_str(&import.ron, assets_root) {
        Ok(l) => (Vec::new(), l.missing_assets),
        Err(e) => (
            e.in_file(&output.display().to_string())
                .to_string()
                .lines()
                .map(str::to_string)
                .collect(),
            Vec::new(),
        ),
    };
    Ok(FileReport {
        warnings: import.warnings,
        errors,
        missing_assets,
        tracks: import.scene.timeline.tracks.len(),
        events: import
            .scene
            .timeline
            .tracks
            .iter()
            .map(|t| t.events.len())
            .sum(),
        duration_ms: import.scene.duration_ms(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load_str;

    fn opts() -> ImportOptions {
        ImportOptions {
            name: "t".into(),
            source: Some("t.fountain".into()),
        }
    }

    fn events<'a>(s: &'a Scene, track: &str) -> Vec<&'a Event> {
        s.timeline
            .tracks
            .iter()
            .filter(|t| t.name == track)
            .flat_map(|t| t.events.iter())
            .collect()
    }

    const SMALL: &str = "Title: Small Test\nAuthor: x\n\n> FADE IN:\n\nINT. OFFICE - DAY\n\n[[AMBIENCE: hum]]\n\nA desk. BOSS sits. CLERK stands.\n\nBOSS\n(tired)\nOne two three four five.\n\nCLERK (V.O.)\nYes.\n\n[[SFX: door]]\n\n> FADE OUT.\n";

    #[test]
    fn names_are_slugged() {
        assert_eq!(scene_name("EXAMPLE-bridge"), "example_bridge");
        assert_eq!(scene_name("F1-M01"), "f1_m01");
        assert_eq!(scene_name("---"), "unnamed");
    }

    #[test]
    fn line_length_is_words_over_2_5_per_second_with_a_floor() {
        assert_eq!(estimate_line_ms("Yes."), 1500);
        assert_eq!(estimate_line_ms("one two three"), 1500);
        assert_eq!(estimate_line_ms("one two three four"), 1600);
        let twenty = vec!["word"; 20].join(" ");
        assert_eq!(estimate_line_ms(&twenty), 8000);
        // Dashes and other punctuation-only tokens are not words.
        assert_eq!(estimate_line_ms("a - b"), 1500);
        let ten_with_dashes = "one - two - three - four - five - six - seven - eight - nine - ten";
        assert_eq!(estimate_line_ms(ten_with_dashes), 4000);
    }

    #[test]
    fn small_script_maps_every_element() {
        let imp = convert(SMALL, &opts());
        let s = &imp.scene;
        assert_eq!(s.title.as_deref(), Some("Small Test"));
        // Heading -> shot with two layers.
        let cuts: Vec<_> = events(s, "camera")
            .into_iter()
            .filter_map(|e| match &e.kind {
                EventKind::CameraCut { shot, .. } => shot.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(cuts, ["office_day"]);
        assert_eq!(s.layers.len(), 2);
        assert!(s.layers.iter().all(|l| l.shots == ["office_day"]));
        // Both named characters are on stage: BOSS (cue) and CLERK (named in the
        // action; his only cue is V.O.).
        let enters: Vec<_> = events(s, "stage")
            .into_iter()
            .filter_map(|e| match &e.kind {
                EventKind::Enter { character, .. } => Some(character.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(enters, ["boss", "clerk"]);
        // Dialogue: 5 words -> 2 s; at 2 s after the heading (establishing hold).
        let d = events(s, "dialogue");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].at, ESTABLISHING_MS);
        match &d[0].kind {
            EventKind::Dialogue {
                speaker,
                text,
                duration_ms,
                delivery,
                voice,
                lipsync,
            } => {
                assert_eq!(speaker, "boss");
                assert_eq!(text, "One two three four five.");
                assert_eq!(*duration_ms, 2000);
                assert_eq!(*delivery, Delivery::OnScreen);
                assert!(voice.is_none() && lipsync.is_none());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(d[1].at, ESTABLISHING_MS + 2000 + LINE_GAP_MS);
        assert!(matches!(
            &d[1].kind,
            EventKind::Dialogue {
                delivery: Delivery::VoiceOver,
                duration_ms: 1500,
                ..
            }
        ));
        // Parenthetical -> pose hint at the start of the line.
        let pose = events(s, "stage")
            .into_iter()
            .find_map(|e| match &e.kind {
                EventKind::SetPose { pose, hint, .. } => Some((e.at, pose.clone(), hint.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            pose,
            (
                ESTABLISHING_MS,
                "subdued".to_string(),
                Some("tired".to_string())
            )
        );
        // Notes, transitions.
        let audio = events(s, "audio");
        assert!(matches!(
            &audio[0].kind,
            EventKind::Ambience { action: LoopAction::Play, note, .. } if note == "hum"
        ));
        assert_eq!(
            audio[0].at, 0,
            "before the first line: at the start of the shot"
        );
        assert!(matches!(&audio[1].kind, EventKind::Sfx { note, .. } if note == "door"));
        assert_eq!(
            audio[1].at,
            d[1].at + 1500 + LINE_GAP_MS,
            "after the last line"
        );
        let tr = events(s, "transition");
        assert!(matches!(
            tr[0].kind,
            EventKind::Transition {
                kind: TransitionKind::FadeIn,
                ..
            }
        ));
        assert_eq!(tr[0].at, 0);
        assert!(matches!(
            tr[1].kind,
            EventKind::Transition {
                kind: TransitionKind::FadeOut,
                ..
            }
        ));
        // Fade out comes after the last line plus its pause.
        assert_eq!(tr[1].at, d[1].at + 1500 + LINE_GAP_MS);
        // Cast: CLERK only ever speaks V.O. but is named in the action, so he has
        // a rig; both are valid.
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
        load_str(&imp.ron, None).unwrap_or_else(|e| panic!("{e}\n{}", imp.ron));
    }

    #[test]
    fn voice_only_speakers_have_no_rig() {
        let src =
            "INT. ROOM - DAY\n\nA table. ALPHA waits.\n\nALPHA\nHello.\n\nRADIO (V.O.)\nCome in.\n";
        let imp = convert(src, &opts());
        let radio = imp.scene.character("radio").unwrap();
        assert!(radio.parts.is_empty() && radio.poses.is_empty());
        assert_eq!(radio.name, "Radio");
        assert!(!imp.scene.character("alpha").unwrap().parts.is_empty());
        let loaded = load_str(&imp.ron, None).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    #[test]
    fn off_screen_cue_is_not_put_on_stage() {
        let src = "INT. ROOM - DAY\n\nALPHA\nHello.\n\nBETA (O.S.)\nOver here.\n";
        let imp = convert(src, &opts());
        let enters = events(&imp.scene, "stage")
            .into_iter()
            .filter(|e| matches!(e.kind, EventKind::Enter { .. }))
            .count();
        assert_eq!(enters, 1);
        assert!(matches!(
            &events(&imp.scene, "dialogue")[1].kind,
            EventKind::Dialogue {
                delivery: Delivery::OffScreen,
                ..
            }
        ));
    }

    #[test]
    fn map_shot_covers_the_next_line_then_cuts_back() {
        let src = "INT. TENT - DAWN\n\nA. B.\n\nA\nFirst line.\n\n[[SHOT: map]]\n[[MAP: two arrows]]\n\nA\nThis is shown over the map for a while.\n\nB\nBack in the tent.\n";
        let imp = convert(src, &opts());
        let s = &imp.scene;
        let cam: Vec<_> = events(s, "camera")
            .into_iter()
            .filter_map(|e| match &e.kind {
                EventKind::CameraCut { shot, .. } => Some((e.at, shot.clone())),
                _ => None,
            })
            .collect();
        // Heading at 0, map after the first line (2000 + 1500 + 300), back at the
        // end of the second line.
        let map_at = ESTABLISHING_MS + 1500 + LINE_GAP_MS;
        let second = 9 * MS_PER_WORD; // "This is shown over the map for a while." = 9 words
        assert_eq!(
            cam,
            [
                (0, Some("tent_dawn".to_string())),
                (map_at, Some("map".to_string())),
                (map_at + second, Some("tent_dawn".to_string())),
            ]
        );
        let ov = events(s, "overlay");
        assert_eq!(ov.len(), 1);
        assert_eq!(ov[0].at, map_at);
        assert!(matches!(
            &ov[0].kind,
            EventKind::MapOverlay { note, duration_ms, .. } if note == "two arrows" && *duration_ms == second
        ));
        assert!(
            s.layers
                .iter()
                .any(|l| l.name == "map_table" && l.shots == ["map"])
        );
        // The third line starts after the cut back.
        let d = events(s, "dialogue");
        assert!(d[2].at > map_at + second);
        load_str(&imp.ron, None).unwrap_or_else(|e| panic!("{e}\n{}", imp.ron));
    }

    #[test]
    fn map_without_following_dialogue_lasts_three_seconds() {
        let src = "INT. TENT - DAWN\n\nA. \n\nA\nHi.\n\n[[SHOT: map]]\n\n> FADE OUT.\n";
        let imp = convert(src, &opts());
        let cam: Vec<_> = events(&imp.scene, "camera")
            .into_iter()
            .filter_map(|e| match &e.kind {
                EventKind::CameraCut { shot, .. } => Some((e.at, shot.clone())),
                _ => None,
            })
            .collect();
        let map_at = ESTABLISHING_MS + 1500 + LINE_GAP_MS;
        assert_eq!(cam[1], (map_at, Some("map".into())));
        assert_eq!(cam[2], (map_at + MAP_ONLY_MS, Some("tent_dawn".into())));
        let fade = events(&imp.scene, "transition");
        assert_eq!(fade[0].at, map_at + MAP_ONLY_MS);
    }

    #[test]
    fn two_headings_make_two_shots_and_swap_the_cast() {
        let src = "INT. ONE - DAY\n\nA\nHi.\n\nINT. ONE - NIGHT\n\nA\nAgain.\n\nB\nHello.\n";
        let imp = convert(src, &opts());
        let s = &imp.scene;
        let names: Vec<_> = s.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "one_day_wall",
                "one_day_floor",
                "one_night_wall",
                "one_night_floor"
            ]
        );
        let stage: Vec<_> = events(s, "stage")
            .into_iter()
            .map(|e| (e.at, e.kind.label()))
            .collect();
        let second = ESTABLISHING_MS + 1500 + LINE_GAP_MS;
        assert_eq!(
            stage,
            [
                (0, "Enter"),
                (second, "Exit"),
                (second, "Enter"),
                (second, "Enter"),
            ]
        );
        load_str(&imp.ron, None).unwrap_or_else(|e| panic!("{e}\n{}", imp.ron));
    }

    #[test]
    fn duplicate_location_names_get_distinct_shots() {
        let src = "INT. ROOM - DAY\n\nA\nHi.\n\nINT. ROOM - DAY\n\nA\nHi.\n";
        let imp = convert(src, &opts());
        let shots: Vec<_> = imp
            .scene
            .layers
            .iter()
            .flat_map(|l| l.shots.clone())
            .collect();
        assert!(shots.contains(&"room_day".to_string()));
        assert!(shots.contains(&"room_day_2".to_string()));
        load_str(&imp.ron, None).unwrap();
    }

    #[test]
    fn multi_line_dialogue_and_mid_block_parentheticals() {
        let src =
            "INT. ROOM - DAY\n\nA\n(firm)\nFirst half\nsecond half.\n(quieter)\nAfter a beat.\n";
        let imp = convert(src, &opts());
        let d = events(&imp.scene, "dialogue");
        assert_eq!(d.len(), 2);
        assert!(
            matches!(&d[0].kind, EventKind::Dialogue { text, .. } if text == "First half second half.")
        );
        assert!(matches!(&d[1].kind, EventKind::Dialogue { text, .. } if text == "After a beat."));
        let poses: Vec<_> = events(&imp.scene, "stage")
            .into_iter()
            .filter_map(|e| match &e.kind {
                EventKind::SetPose { pose, .. } => Some(pose.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(poses, ["forceful", "subdued"]);
    }

    #[test]
    fn multi_line_notes_and_free_notes() {
        let src = "INT. ROOM - DAY\n\n[[A free note for the writers.]]\n[[SFX: a long\nsound]]\n\nA\nHi.\n";
        let imp = convert(src, &opts());
        let audio = events(&imp.scene, "audio");
        assert_eq!(audio.len(), 1);
        assert!(matches!(&audio[0].kind, EventKind::Sfx { note, .. } if note == "a long sound"));
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    }

    #[test]
    fn warnings_for_what_cannot_be_placed() {
        let src = "INT. ROOM - DAY\n\n[[AMBIENCE: ]]\n[[WIBBLE: x]]\n[[SHOT: close-up]]\n[[MAP: a map]]\n\nDISSOLVE TO:\n\nA\n(smiling)\n\n";
        let imp = convert(src, &opts());
        let msgs: Vec<_> = imp.warnings.iter().map(|w| w.message.as_str()).collect();
        let has = |frag: &str| msgs.iter().any(|m| m.contains(frag));
        assert!(has("empty AMBIENCE"), "{msgs:?}");
        assert!(has("unknown note [[WIBBLE"), "{msgs:?}");
        assert!(has("close-up"), "{msgs:?}");
        assert!(has("without [[SHOT: map]]"), "{msgs:?}");
        assert!(has("DISSOLVE TO"), "{msgs:?}");
        assert!(has("no dialogue under the cue"), "{msgs:?}");
        load_str(&imp.ron, None).unwrap_or_else(|e| panic!("{e}\n{}", imp.ron));
    }

    const POOL: &[&str] = &[
        "",
        "",
        "",
        "INT. ROOM - DAY",
        "EXT. YARD - NIGHT",
        "INT. ROOM - DAY",
        ".SOMEWHERE ELSE",
        "ALPHA",
        "BETA",
        "GAMMA (V.O.)",
        "ALPHA (O.S.)",
        "@Zed (CONT'D)",
        "@",
        "(tired)",
        "(pointing at map)",
        "(urgent, over radio)",
        "()",
        "Hello there.",
        "Go now, quickly, to the north bridge and hold it.",
        "*Emphasis* line.",
        "ALPHA and BETA stand by the table. GAMMA is elsewhere.",
        "[[SFX: bang]]",
        "[[MUSIC: stop]]",
        "[[MUSIC: drums]]",
        "[[AMBIENCE: wind]]",
        "[[AMBIENCE: ]]",
        "[[SHOT: map]]",
        "[[SHOT: wide]]",
        "[[SHOT: reaction BETA]]",
        "[[SHOT: close]]",
        "[[MAP: arrows]]",
        "[[MAP:]]",
        "[[a free note]]",
        "[[X: y]]",
        "[[never closed",
        "]] stray close",
        "> FADE IN:",
        "> FADE OUT.",
        "CUT TO:",
        "DISSOLVE TO:",
        "> CENTERED <",
        "# Act one",
        "= synopsis",
        "===",
        "Title: not a title page",
        "\u{feff}",
        "ÉCLAIR",
    ];

    proptest::proptest! {
        /// Whatever a writer throws at the importer, the scene it returns is
        /// valid and its RON reads back as exactly that scene.
        #[test]
        fn any_mix_of_lines_converts_to_a_valid_scene(
            picks in proptest::collection::vec(proptest::sample::select(POOL), 0..80),
            crlf in proptest::bool::ANY,
        ) {
            let text = picks.join(if crlf { "\r\n" } else { "\n" });
            let imp = convert(&text, &opts());
            let loaded = load_str(&imp.ron, None);
            proptest::prop_assert!(
                loaded.is_ok(),
                "{}\n--- input ---\n{text}\n--- output ---\n{}",
                loaded.err().map(|e| e.to_string()).unwrap_or_default(),
                imp.ron
            );
            proptest::prop_assert_eq!(crate::load::parse_str(&imp.ron).unwrap(), imp.scene);
        }
    }

    #[test]
    fn wide_and_reaction_shots() {
        let src = "INT. ROOM - DAY\n\nA. B.\n\nA\nOne.\n\n[[SHOT: wide]]\n\nA\nTwo.\n\n[[SHOT: reaction B]]\n\nB\nThree.\n\n[[SHOT: reaction GHOST]]\n";
        let imp = convert(src, &opts());
        let cuts: Vec<_> = events(&imp.scene, "camera")
            .into_iter()
            .filter_map(|e| match &e.kind {
                EventKind::CameraCut { center, zoom, .. } => Some((e.at, *center, *zoom)),
                _ => None,
            })
            .collect();
        let wide_at = ESTABLISHING_MS + 1500 + LINE_GAP_MS;
        let react_at = wide_at + 1500 + LINE_GAP_MS;
        assert_eq!(cuts[1], (wide_at, Vec2::ZERO, WIDE_ZOOM));
        // B is the second of two: right side, x = 360; head height is above the origin.
        assert_eq!(cuts[2], (react_at, Vec2::new(360.0, -20.0), CLOSE_ZOOM));
        // Back to the wide framing, not to zoom 1, and the line follows the cut-away.
        assert_eq!(cuts[3], (react_at + REACTION_MS, Vec2::ZERO, WIDE_ZOOM));
        let d = events(&imp.scene, "dialogue");
        assert_eq!(d[2].at, react_at + REACTION_MS);
        let msgs: Vec<_> = imp.warnings.iter().map(|w| w.message.as_str()).collect();
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        assert!(
            msgs[0].contains("GHOST") || msgs[0].contains("ghost"),
            "{msgs:?}"
        );
        load_str(&imp.ron, None).unwrap_or_else(|e| panic!("{e}\n{}", imp.ron));
    }

    #[test]
    fn dialogue_before_any_heading_gets_an_implicit_location() {
        let imp = convert("A\nHi there.\n", &opts());
        // "A" on its own line directly followed by text is a cue only after a
        // blank line; at the top of the file the previous line counts as blank.
        assert!(
            imp.warnings
                .iter()
                .any(|w| w.message.contains("no scene heading")),
            "{:?}",
            imp.warnings
        );
        load_str(&imp.ron, None).unwrap_or_else(|e| panic!("{e}\n{}", imp.ron));
    }

    #[test]
    fn empty_input_gives_an_empty_but_valid_scene() {
        let imp = convert("", &opts());
        assert!(imp.scene.timeline.tracks.is_empty());
        assert!(imp.warnings.iter().any(|w| w.message.contains("empty")));
        load_str(&imp.ron, None).unwrap();
    }

    #[test]
    fn output_parses_back_to_exactly_the_scene() {
        let imp = convert(SMALL, &opts());
        let back = crate::load::parse_str(&imp.ron).unwrap_or_else(|e| panic!("{e}\n{}", imp.ron));
        assert_eq!(back, imp.scene);
    }

    #[test]
    fn quotes_and_backslashes_in_dialogue_survive_the_round_trip() {
        let src = "INT. ROOM - DAY\n\nA\nDon't say \"stop\" at C:\\temp\\\\x, it's 'fine'.\n";
        let imp = convert(src, &opts());
        let text = match &events(&imp.scene, "dialogue")[0].kind {
            EventKind::Dialogue { text, .. } => text.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(text, "Don't say \"stop\" at C:\\temp\\\\x, it's 'fine'.");
        // Apostrophes are written bare for the writer; everything else escaped.
        assert!(imp.ron.contains("Don't say"), "{}", imp.ron);
        let back = crate::load::parse_str(&imp.ron).unwrap_or_else(|e| panic!("{e}\n{}", imp.ron));
        assert_eq!(back, imp.scene);
    }

    #[test]
    fn events_carry_their_fountain_line_as_a_comment() {
        let imp = convert(SMALL, &opts());
        // The first dialogue line is on line 12 of SMALL ("One two ..." follows
        // "BOSS" and "(tired)" at 10 and 11... find it by text instead).
        let l = imp
            .ron
            .lines()
            .find(|l| l.contains("One two three"))
            .unwrap();
        assert!(l.contains("// line "), "{l}");
        let n: usize = l.rsplit("// line ").next().unwrap().trim().parse().unwrap();
        assert_eq!(
            SMALL.lines().nth(n - 1).unwrap(),
            "One two three four five."
        );
    }

    #[test]
    fn pose_hints_pick_a_pose() {
        for (hint, pose) in [
            ("pointing at map", "point"),
            ("tired, steady", "subdued"),
            ("quieter", "subdued"),
            ("urgent, over radio", "forceful"),
            ("dry", "neutral"),
            ("<emotion>", "neutral"),
        ] {
            assert_eq!(pose_for_hint(hint), pose, "{hint}");
        }
    }

    #[test]
    fn placeholder_rig_is_valid_and_poses_move_parts() {
        let c = placeholder_character("x", "X", Color::rgb(1, 2, 3));
        let scene = Scene {
            format: 1,
            name: "p".into(),
            title: None,
            source: None,
            stage: Stage::default(),
            layers: vec![],
            characters: vec![c],
            timeline: Timeline::default(),
            mouth_cues: Default::default(),
        };
        let v = crate::validate::validate(&scene);
        assert!(v.errors.is_empty(), "{:?}", v.errors);
        // The pointing arm is raised away from the body.
        let c = &scene.characters[0];
        let point = c.pose("point").unwrap();
        assert!(
            point
                .parts
                .iter()
                .any(|p| p.part == "arm_r" && p.angle < -45.0)
        );
    }
}
