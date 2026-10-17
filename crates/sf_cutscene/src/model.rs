//! The scene data model: what a `.ron` scene file contains.
//!
//! One [`Scene`] is a cartoon: background [`Layer`]s, cutout [`Character`]s and
//! a [`Timeline`] of [`Track`]s whose [`Event`]s happen at fixed millisecond
//! times. `docs/cutscene-format.md` is the writer's and artist's description of
//! the same thing; this file is the source of truth for field names and
//! defaults.
//!
//! Every struct rejects unknown fields, so a typo in a scene is an error with
//! a line number rather than a silently ignored setting.

use serde::{Deserialize, Serialize};

use crate::math::{Color, Easing, Ms, Vec2};

/// The only format version this crate reads.
pub const FORMAT_VERSION: u32 = 1;

fn one() -> f32 {
    1.0
}

fn unit_scale() -> Vec2 {
    Vec2::ONE
}

fn default_format() -> u32 {
    FORMAT_VERSION
}

fn default_volume() -> f32 {
    1.0
}

/// A whole scene (one briefing, one interlude).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// Format version, currently `1`.
    #[serde(default = "default_format")]
    pub format: u32,
    /// Short identifier, for logs and tools (`example_bridge`).
    pub name: String,
    /// Human title (from the Fountain title page).
    #[serde(default)]
    pub title: Option<String>,
    /// The file this scene was generated from, if any.
    #[serde(default)]
    pub source: Option<String>,
    /// The frame, in stage units. Camera zoom 1 shows exactly this area.
    #[serde(default)]
    pub stage: Stage,
    /// Backgrounds and foregrounds. Order does not matter; `z` does.
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub characters: Vec<Character>,
    #[serde(default)]
    pub timeline: Timeline,
}

/// Size of the frame in stage units (the origin is its centre).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub width: i32,
    pub height: i32,
}

impl Default for Stage {
    fn default() -> Stage {
        Stage {
            width: 1920,
            height: 1080,
        }
    }
}

/// What a layer or a part looks like. Placeholders are plain shapes; the real
/// art is an image file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Art {
    /// A PNG or SVG, path relative to the asset root (`assets/`). Drawn
    /// centred on the layer's or part's origin, one image pixel per stage unit.
    Image(String),
    /// A rectangle of `w` x `h` stage units, centred on the origin.
    Rect { w: f32, h: f32, color: Color },
    /// A rectangle with rounded corners, centred on the origin. A `radius`
    /// larger than half the shorter side is drawn as half of it.
    RoundRect {
        w: f32,
        h: f32,
        radius: f32,
        color: Color,
    },
    /// A circle of the given radius, centred on the origin.
    Circle { radius: f32, color: Color },
}

/// A background or foreground picture with parallax depth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub name: String,
    pub art: Art,
    /// Where the layer sits in the world (stage units).
    #[serde(default)]
    pub pos: Vec2,
    /// How strongly the layer follows the camera pan: `1` moves with the world
    /// (where the characters stand), `0` is glued to the frame (a far sky),
    /// between is further away, above `1` is a foreground that slides past
    /// faster.
    #[serde(default = "one")]
    pub parallax: f32,
    /// Draw order: layers with `z < 0` are behind all characters, layers with
    /// `z >= 0` in front of them; lower `z` is drawn first.
    #[serde(default)]
    pub z: i32,
    /// Shots this layer is visible in (see [`EventKind::CameraCut`]). Empty
    /// means "always visible".
    #[serde(default)]
    pub shots: Vec<String>,
}

/// A cutout figure: a tree of named parts and a set of named poses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    /// The name events use (`major_halversen`).
    pub id: String,
    /// Name shown with the subtitle; falls back to `id`.
    #[serde(default)]
    pub name: String,
    /// Draw order between characters (lower is drawn first, ties by position
    /// in this list).
    #[serde(default)]
    pub z: i32,
    /// Overall size factor of the whole figure.
    #[serde(default = "one")]
    pub scale: f32,
    /// The rig. A character without parts is a voice only (off-screen, V.O.).
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub poses: Vec<Pose>,
}

impl Character {
    /// Name for subtitles.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }

    pub fn part_index(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.name == name)
    }

    pub fn pose(&self, name: &str) -> Option<&Pose> {
        self.poses.iter().find(|p| p.name == name)
    }
}

/// One piece of a cutout figure.
///
/// The part's joint sits at `offset` from its parent's joint (or from the
/// character's position for a root part). The picture is drawn so that the
/// point `pivot` of the picture (measured from the picture's centre) lies on the
/// joint; the pose then rotates the part about that joint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub name: String,
    /// Name of the parent part; absent for a root.
    #[serde(default)]
    pub parent: Option<String>,
    /// Joint position inside the picture, from the picture's centre.
    #[serde(default)]
    pub pivot: Vec2,
    /// Rest position of the joint relative to the parent's joint.
    #[serde(default)]
    pub offset: Vec2,
    /// Draw order inside the character (lower first, ties by position in the
    /// list); independent of the tree.
    #[serde(default)]
    pub z: i32,
    pub art: Art,
}

/// A named arrangement of the parts. Parts not listed stand at rest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub name: String,
    #[serde(default)]
    pub parts: Vec<PartPose>,
}

/// How one part differs from its rest position in a pose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartPose {
    pub part: String,
    /// Rotation about the joint in degrees, positive = clockwise on screen.
    /// Blended linearly (no shortest-way wrap-around), so `-170` to `170` swings
    /// the long way round: the author decides the direction.
    #[serde(default)]
    pub angle: f32,
    /// Added to the part's rest offset.
    #[serde(default)]
    pub offset: Vec2,
    /// Size factor of the part (and, through the tree, its children).
    #[serde(default = "unit_scale")]
    pub scale: Vec2,
}

/// All tracks of a scene.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    #[serde(default)]
    pub tracks: Vec<Track>,
}

/// A lane of events, sorted by time. Tracks are for the author's overview
/// ("camera", "dialogue", "audio"); the player merges them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub name: String,
    #[serde(default)]
    pub events: Vec<Event>,
}

/// Something that happens at `at` milliseconds from the start of the scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub at: Ms,
    pub kind: EventKind,
}

/// Which way a character looks. Art is drawn facing right; `Left` mirrors it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    Left,
    #[default]
    Right,
}

/// How a line of dialogue is delivered (the Fountain `(V.O.)` and `(O.S.)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Delivery {
    /// The speaker is in the picture and talks.
    #[default]
    OnScreen,
    /// Voice over: radio, narrator, a recording. Not drawn talking.
    VoiceOver,
    /// Off screen: in the scene but outside the frame.
    OffScreen,
}

/// What a looping sound (music, ambience) does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopAction {
    #[default]
    Play,
    Stop,
}

/// The kinds of transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    /// Hard cut: any fade is removed at once.
    Cut,
    /// From a solid colour to the picture.
    FadeIn,
    /// From the picture to a solid colour, which then stays until the next
    /// fade in or cut.
    FadeOut,
}

/// An arrow or a pin on the map overlay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapMarker {
    pub label: String,
    /// Where it sits (stage units); for an arrow, where it starts.
    pub at: Vec2,
    /// Present: an arrow from `at` to here. Absent: a pin.
    #[serde(default)]
    pub to: Option<Vec2>,
}

/// Everything that can happen on a timeline.
///
/// Asset fields (`voice`, `lipsync`, `sound`, `file`) are paths relative to the
/// asset root. They may be absent while the asset does not exist yet; the
/// free-text `note` keeps what the writer wanted there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum EventKind {
    /// A character steps onto the stage. Must not already be on it.
    Enter {
        character: String,
        pos: Vec2,
        #[serde(default)]
        facing: Facing,
        /// Pose to start in; absent = every part at rest.
        #[serde(default)]
        pose: Option<String>,
    },
    /// A character leaves the stage.
    Exit { character: String },
    /// Walk to a new position.
    Move {
        character: String,
        to: Vec2,
        duration_ms: Ms,
        #[serde(default)]
        easing: Easing,
        /// Turn to this side when the move starts.
        #[serde(default)]
        facing: Option<Facing>,
    },
    /// Change pose, blending from whatever the character looks like now.
    SetPose {
        character: String,
        pose: String,
        #[serde(default)]
        duration_ms: Ms,
        #[serde(default)]
        easing: Easing,
        /// The writer's stage direction this came from (Fountain parenthetical).
        #[serde(default)]
        hint: Option<String>,
    },
    /// A spoken line, shown as a subtitle for `duration_ms`.
    Dialogue {
        speaker: String,
        text: String,
        duration_ms: Ms,
        /// Recorded voice file.
        #[serde(default)]
        voice: Option<String>,
        /// Mouth-shape time series for the voice file (WP12.3).
        #[serde(default)]
        lipsync: Option<String>,
        #[serde(default)]
        delivery: Delivery,
    },
    /// A one-shot sound effect.
    Sfx {
        #[serde(default)]
        sound: Option<String>,
        #[serde(default)]
        note: String,
        #[serde(default = "default_volume")]
        volume: f32,
    },
    /// Start or stop the music.
    Music {
        #[serde(default)]
        action: LoopAction,
        #[serde(default)]
        file: Option<String>,
        #[serde(default)]
        note: String,
        #[serde(default)]
        fade_ms: Ms,
    },
    /// Start or stop the ambient bed (rain, engines).
    Ambience {
        #[serde(default)]
        action: LoopAction,
        #[serde(default)]
        file: Option<String>,
        #[serde(default)]
        note: String,
        #[serde(default)]
        fade_ms: Ms,
    },
    /// Move the camera centre.
    CameraPan {
        to: Vec2,
        duration_ms: Ms,
        #[serde(default)]
        easing: Easing,
    },
    /// Change the zoom (`1` = the whole stage, `2` = twice as close).
    CameraZoom {
        to: f32,
        duration_ms: Ms,
        #[serde(default)]
        easing: Easing,
    },
    /// Jump to a new framing. With a `shot` name, layers listing that shot
    /// become visible and layers listing other shots disappear.
    CameraCut {
        #[serde(default)]
        center: Vec2,
        #[serde(default = "one")]
        zoom: f32,
        #[serde(default)]
        shot: Option<String>,
    },
    /// Shake the whole frame; the shake fades out linearly.
    CameraShake { amplitude: f32, duration_ms: Ms },
    /// Fade in, fade out or hard cut.
    Transition {
        kind: TransitionKind,
        #[serde(default)]
        duration_ms: Ms,
        #[serde(default = "black")]
        color: Color,
    },
    /// Nothing happens for a while. Gives a scene its length when the last
    /// real event is earlier.
    Wait { duration_ms: Ms },
    /// The map insert of a briefing (Fountain `[[MAP: ...]]`).
    MapOverlay {
        note: String,
        #[serde(default)]
        markers: Vec<MapMarker>,
        duration_ms: Ms,
    },
}

fn black() -> Color {
    Color::BLACK
}

impl EventKind {
    /// Length of the event; 0 for instantaneous ones.
    pub fn duration_ms(&self) -> Ms {
        match self {
            EventKind::Move { duration_ms, .. }
            | EventKind::SetPose { duration_ms, .. }
            | EventKind::Dialogue { duration_ms, .. }
            | EventKind::CameraPan { duration_ms, .. }
            | EventKind::CameraZoom { duration_ms, .. }
            | EventKind::CameraShake { duration_ms, .. }
            | EventKind::Transition { duration_ms, .. }
            | EventKind::Wait { duration_ms }
            | EventKind::MapOverlay { duration_ms, .. } => *duration_ms,
            EventKind::Enter { .. }
            | EventKind::Exit { .. }
            | EventKind::Sfx { .. }
            | EventKind::Music { .. }
            | EventKind::Ambience { .. }
            | EventKind::CameraCut { .. } => 0,
        }
    }

    /// Short name of the variant, for messages.
    pub fn label(&self) -> &'static str {
        match self {
            EventKind::Enter { .. } => "Enter",
            EventKind::Exit { .. } => "Exit",
            EventKind::Move { .. } => "Move",
            EventKind::SetPose { .. } => "SetPose",
            EventKind::Dialogue { .. } => "Dialogue",
            EventKind::Sfx { .. } => "Sfx",
            EventKind::Music { .. } => "Music",
            EventKind::Ambience { .. } => "Ambience",
            EventKind::CameraPan { .. } => "CameraPan",
            EventKind::CameraZoom { .. } => "CameraZoom",
            EventKind::CameraCut { .. } => "CameraCut",
            EventKind::CameraShake { .. } => "CameraShake",
            EventKind::Transition { .. } => "Transition",
            EventKind::Wait { .. } => "Wait",
            EventKind::MapOverlay { .. } => "MapOverlay",
        }
    }

    /// True for the events a sound system has to act on at their start time:
    /// sound effects, music and ambience cues, and dialogue with a voice file.
    pub fn is_sound(&self) -> bool {
        match self {
            EventKind::Sfx { .. } | EventKind::Music { .. } | EventKind::Ambience { .. } => true,
            EventKind::Dialogue { voice, .. } => voice.is_some(),
            _ => false,
        }
    }
}

impl Scene {
    pub fn character(&self, id: &str) -> Option<&Character> {
        self.characters.iter().find(|c| c.id == id)
    }

    /// Every event with its track and index, in play order: by time, then by
    /// track order, then by position in the track.
    pub fn events_in_order(&self) -> Vec<OrderedEvent<'_>> {
        let mut v: Vec<OrderedEvent<'_>> = self
            .timeline
            .tracks
            .iter()
            .enumerate()
            .flat_map(|(t, track)| {
                track
                    .events
                    .iter()
                    .enumerate()
                    .map(move |(i, e)| OrderedEvent {
                        at: e.at,
                        track: t,
                        index: i,
                        kind: &e.kind,
                    })
            })
            .collect();
        v.sort_by_key(|e| (e.at, e.track, e.index));
        v
    }

    /// When the scene ends: the latest `at + duration` of any event (never
    /// negative).
    pub fn duration_ms(&self) -> Ms {
        let end = self
            .timeline
            .tracks
            .iter()
            .flat_map(|t| &t.events)
            .map(|e| i64::from(e.at) + i64::from(e.kind.duration_ms().max(0)))
            .max()
            .unwrap_or(0)
            .clamp(0, i64::from(Ms::MAX));
        end as Ms
    }
}

/// An event with its place in the scene; see [`Scene::events_in_order`].
#[derive(Clone, Copy, Debug)]
pub struct OrderedEvent<'a> {
    pub at: Ms,
    pub track: usize,
    pub index: usize,
    pub kind: &'a EventKind,
}
