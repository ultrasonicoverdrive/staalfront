//! Evaluating a scene at a point in time, without drawing anything.
//!
//! [`Scene::sample`] answers "what does the frame look like at `t` ms":
//! which characters are on stage, where every part of every rig is (pose
//! blending and easing already applied), where the camera is, which subtitle
//! is up, how far the fade has gone. [`Scene::sample_step`] additionally lists
//! the sound events that start in `(prev, t]`, so a player that calls it once
//! per frame fires every cue exactly once, whatever the frame length.
//!
//! All functions are pure: the result depends only on the scene and the
//! arguments. The renderer (WP12.2) walks a [`SceneState`] and draws it.
//!
//! # How values change over time
//!
//! Positions, poses, pan and zoom are *channels*: a list of segments, each
//! "from `start`, over `dur`, move to `to` with this easing". A segment starts
//! from whatever the channel's value is at its start time, so a new move or
//! pose change that begins while an earlier one is still running continues
//! smoothly from where the earlier one had got to.

use crate::math::{Affine, Color, Easing, Lerp, Ms, Vec2};
use crate::model::{
    Character, Delivery, EventKind, Facing, Layer, MapMarker, Part, Pose, Scene, TransitionKind,
};

/// Everything about a frame; see the module docs.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneState<'a> {
    pub time_ms: Ms,
    /// True once `time_ms` has reached [`Scene::duration_ms`].
    pub finished: bool,
    pub camera: CameraState,
    /// Name of the current shot (the last `CameraCut` that named one).
    pub shot: Option<&'a str>,
    /// Layers behind the characters (`z < 0`), in draw order.
    pub back_layers: Vec<LayerState<'a>>,
    /// Characters on stage, in draw order.
    pub characters: Vec<CharacterState<'a>>,
    /// Layers in front of the characters (`z >= 0`), in draw order.
    pub front_layers: Vec<LayerState<'a>>,
    /// The line being spoken now. If lines of different speakers overlap, the
    /// one that started last.
    pub subtitle: Option<Subtitle<'a>>,
    pub fade: Fade,
    pub map_overlay: Option<MapOverlayState<'a>>,
    /// Sound events that started in `(prev, t]`; empty after [`Scene::sample`],
    /// filled by [`Scene::sample_step`].
    pub sounds: Vec<FiredEvent<'a>>,
}

/// The camera at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraState {
    /// Centre of the view in the world (stage units), without the shake.
    pub center: Vec2,
    /// `1` shows the whole stage.
    pub zoom: f32,
    /// Offset to add to everything drawn (layers included), in frame units
    /// before zoom. `ZERO` when no shake is running.
    pub shake: Vec2,
}

/// A layer placed for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerState<'a> {
    pub layer: &'a Layer,
    /// Where to draw the layer's origin **in world coordinates**, parallax
    /// already folded in: the renderer draws it like any other world object, at
    /// `(position - camera.center) * zoom`.
    pub position: Vec2,
}

/// A character on stage.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterState<'a> {
    pub character: &'a Character,
    /// Where the character's origin stands in the world.
    pub position: Vec2,
    pub facing: Facing,
    /// Parts in draw order (lowest `z` first).
    pub parts: Vec<PartState<'a>>,
}

/// One part of a rig, posed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartState<'a> {
    pub part: &'a Part,
    /// The blended pose values for this part.
    pub pose: PartXform,
    /// Where the joint is in the world.
    pub joint: Vec2,
    /// Maps the part's art, **centred on its own origin**, into the world: the
    /// pivot offset, the joint rotation, the whole tree above the part and the
    /// character's position, scale and facing are all folded in.
    pub world: Affine,
}

/// The line on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Subtitle<'a> {
    /// The speaker's character id.
    pub speaker: &'a str,
    /// The name to print.
    pub speaker_name: &'a str,
    pub text: &'a str,
    pub delivery: Delivery,
    pub voice: Option<&'a str>,
    pub lipsync: Option<&'a str>,
    /// Time since the line started.
    pub elapsed_ms: Ms,
    pub duration_ms: Ms,
}

/// The screen-wide fade colour at one instant. `alpha` 0 is the picture, 1 is
/// solid `color`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub color: Color,
    pub alpha: f32,
}

/// The map insert.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapOverlayState<'a> {
    pub note: &'a str,
    pub markers: &'a [MapMarker],
    pub elapsed_ms: Ms,
}

/// A sound event that started in the sampled interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiredEvent<'a> {
    pub at: Ms,
    /// Index of the track and of the event in it.
    pub track: usize,
    pub index: usize,
    pub kind: &'a EventKind,
}

/// The resolved transform of one part: pose values added to the rest position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartXform {
    /// Degrees, clockwise on screen.
    pub angle: f32,
    pub offset: Vec2,
    pub scale: Vec2,
}

impl PartXform {
    pub const REST: PartXform = PartXform {
        angle: 0.0,
        offset: Vec2::ZERO,
        scale: Vec2::ONE,
    };
}

impl Lerp for PartXform {
    fn lerp(&self, other: &PartXform, w: f32) -> PartXform {
        PartXform {
            angle: self.angle.lerp(&other.angle, w),
            offset: self.offset.lerp(&other.offset, w),
            scale: self.scale.lerp(&other.scale, w),
        }
    }
}

impl Lerp for Vec<PartXform> {
    fn lerp(&self, other: &Vec<PartXform>, w: f32) -> Vec<PartXform> {
        self.iter().zip(other).map(|(a, b)| a.lerp(b, w)).collect()
    }
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

struct Seg<T> {
    start: Ms,
    dur: Ms,
    to: T,
    easing: Easing,
}

impl<T> Seg<T> {
    fn end(&self) -> i64 {
        i64::from(self.start) + i64::from(self.dur.max(0))
    }
}

/// A value that changes over time through a list of segments whose starts are
/// in non-decreasing order.
struct Channel<T> {
    initial: T,
    segs: Vec<Seg<T>>,
}

impl<T: Lerp> Channel<T> {
    fn new(initial: T) -> Channel<T> {
        Channel {
            initial,
            segs: Vec::new(),
        }
    }

    fn push(&mut self, start: Ms, dur: Ms, to: T, easing: Easing) {
        debug_assert!(self.segs.last().is_none_or(|s| s.start <= start));
        self.segs.push(Seg {
            start,
            dur,
            to,
            easing,
        });
    }

    fn eval(&self, t: Ms) -> T {
        let n = self.segs.partition_point(|s| s.start <= t);
        let Some(mut idx) = n.checked_sub(1) else {
            return self.initial.clone();
        };
        // Walk back through the chain of segments that were still running when
        // the next one began; the value they blend from is the previous
        // segment's value at that moment.
        let mut chain: Vec<(usize, Ms)> = Vec::new();
        let mut at = t;
        let base = loop {
            let s = &self.segs[idx];
            if i64::from(at) >= s.end() {
                break s.to.clone();
            }
            chain.push((idx, at));
            if idx == 0 {
                break self.initial.clone();
            }
            at = s.start;
            idx -= 1;
        };
        chain.iter().rev().fold(base, |from, &(i, at)| {
            let s = &self.segs[i];
            let p = (i64::from(at) - i64::from(s.start)) as f32 / s.dur as f32;
            from.lerp(&s.to, s.easing.apply(p))
        })
    }
}

// ---------------------------------------------------------------------------
// Rig
// ---------------------------------------------------------------------------

/// The pose values of every part of `ch` for `pose` (rest for parts the pose
/// does not mention).
fn pose_values(ch: &Character, pose: Option<&Pose>) -> Vec<PartXform> {
    let mut v = vec![PartXform::REST; ch.parts.len()];
    if let Some(pose) = pose {
        for pp in &pose.parts {
            if let Some(i) = ch.part_index(&pp.part) {
                v[i] = PartXform {
                    angle: pp.angle,
                    offset: pp.offset,
                    scale: pp.scale,
                };
            }
        }
    }
    v
}

/// Place every part of `ch`. `root` maps the character's origin into the
/// world. Parts come back sorted for drawing.
fn resolve_parts<'a>(ch: &'a Character, pose: &[PartXform], root: &Affine) -> Vec<PartState<'a>> {
    let n = ch.parts.len();
    let parent: Vec<Option<usize>> = ch
        .parts
        .iter()
        .map(|p| p.parent.as_deref().and_then(|name| ch.part_index(name)))
        .collect();
    let local = |k: usize| {
        let x = pose[k];
        Affine::translation(ch.parts[k].offset + x.offset)
            .then_after(&Affine::rotation_deg(x.angle))
            .then_after(&Affine::scaling(x.scale))
    };
    let mut joint: Vec<Option<Affine>> = vec![None; n];
    for i in 0..n {
        // Walk up from `i` to the first part that is already placed (or to the
        // root), then place the parts on the way down. A walk longer than the
        // part count means a cycle, which validation rejects: here it is just
        // cut and the loop treated as hanging off the root.
        let mut chain = Vec::new();
        let mut cur = Some(i);
        while let Some(c) = cur {
            if joint[c].is_some() || chain.len() > n {
                break;
            }
            chain.push(c);
            cur = parent[c];
        }
        let mut m = cur.and_then(|c| joint[c]).unwrap_or(*root);
        for &k in chain.iter().rev() {
            m = m.then_after(&local(k));
            joint[k] = Some(m);
        }
    }
    let mut parts: Vec<PartState<'a>> = (0..n)
        .map(|i| {
            let part = &ch.parts[i];
            let j = joint[i].unwrap_or(*root);
            PartState {
                part,
                pose: pose[i],
                joint: j.apply(Vec2::ZERO),
                world: j.then_after(&Affine::translation(Vec2::ZERO - part.pivot)),
            }
        })
        .collect();
    // Stable sort: ties keep declaration order.
    parts.sort_by_key(|p| p.part.z);
    parts
}

// ---------------------------------------------------------------------------
// Shake
// ---------------------------------------------------------------------------

/// A deterministic integer hash (finalizer of splitmix64).
fn hash64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A value in `[-1, 1]` from 16 bits of `h`.
fn unit_from(h: u64, shift: u32) -> f32 {
    ((h >> shift) & 0xffff) as f32 / 32767.5 - 1.0
}

/// How often the shake jumps to a new offset.
const SHAKE_STEP_MS: i64 = 20;

fn shake_offset(start: Ms, dur: Ms, amplitude: f32, t: Ms) -> Vec2 {
    let elapsed = i64::from(t) - i64::from(start);
    if elapsed < 0 || elapsed >= i64::from(dur) || dur <= 0 {
        return Vec2::ZERO;
    }
    let decay = 1.0 - elapsed as f32 / dur as f32;
    let frame = elapsed / SHAKE_STEP_MS;
    let h = hash64(((i64::from(start) as u64) << 32) ^ frame as u64);
    Vec2::new(unit_from(h, 0), unit_from(h, 24)) * (amplitude * decay)
}

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

struct CharRun {
    on_stage: bool,
    facing: Facing,
    pos: Channel<Vec2>,
    pose: Channel<Vec<PartXform>>,
}

impl Scene {
    /// The frame at `t_ms`, without sound events (see [`Scene::sample_step`]).
    pub fn sample(&self, t_ms: Ms) -> SceneState<'_> {
        let events: Vec<_> = self
            .events_in_order()
            .into_iter()
            .take_while(|e| e.at <= t_ms)
            .collect();

        let mut pan = Channel::new(Vec2::ZERO);
        let mut zoom = Channel::new(1.0_f32);
        let mut shot: Option<&str> = None;
        let mut shake = Vec2::ZERO;
        let mut fade_alpha = Channel::new(0.0_f32);
        let mut fade_color = Color::BLACK;
        let mut subtitle: Option<Subtitle<'_>> = None;
        let mut overlay: Option<MapOverlayState<'_>> = None;
        let mut runs: Vec<CharRun> = self
            .characters
            .iter()
            .map(|c| CharRun {
                on_stage: false,
                facing: Facing::Right,
                pos: Channel::new(Vec2::ZERO),
                pose: Channel::new(pose_values(c, None)),
            })
            .collect();
        let index_of = |id: &str| self.characters.iter().position(|c| c.id == id);

        for e in &events {
            let at = e.at;
            match e.kind {
                EventKind::Enter {
                    character,
                    pos,
                    facing,
                    pose,
                } => {
                    if let Some(ci) = index_of(character) {
                        let ch = &self.characters[ci];
                        let run = &mut runs[ci];
                        run.on_stage = true;
                        run.facing = *facing;
                        run.pos.push(at, 0, *pos, Easing::Linear);
                        let p = pose.as_deref().and_then(|name| ch.pose(name));
                        run.pose.push(at, 0, pose_values(ch, p), Easing::Linear);
                    }
                }
                EventKind::Exit { character } => {
                    if let Some(ci) = index_of(character) {
                        runs[ci].on_stage = false;
                    }
                }
                EventKind::Move {
                    character,
                    to,
                    duration_ms,
                    easing,
                    facing,
                } => {
                    if let Some(ci) = index_of(character) {
                        let run = &mut runs[ci];
                        run.pos.push(at, *duration_ms, *to, *easing);
                        if let Some(f) = facing {
                            run.facing = *f;
                        }
                    }
                }
                EventKind::SetPose {
                    character,
                    pose,
                    duration_ms,
                    easing,
                    ..
                } => {
                    if let Some(ci) = index_of(character) {
                        let ch = &self.characters[ci];
                        if let Some(p) = ch.pose(pose) {
                            runs[ci]
                                .pose
                                .push(at, *duration_ms, pose_values(ch, Some(p)), *easing);
                        }
                    }
                }
                EventKind::Dialogue {
                    speaker,
                    text,
                    duration_ms,
                    voice,
                    lipsync,
                    delivery,
                } => {
                    let elapsed = i64::from(t_ms) - i64::from(at);
                    if elapsed < i64::from(*duration_ms) {
                        subtitle = Some(Subtitle {
                            speaker,
                            speaker_name: self
                                .character(speaker)
                                .map_or(speaker.as_str(), Character::display_name),
                            text,
                            delivery: *delivery,
                            voice: voice.as_deref(),
                            lipsync: lipsync.as_deref(),
                            elapsed_ms: elapsed as Ms,
                            duration_ms: *duration_ms,
                        });
                    }
                }
                EventKind::CameraPan {
                    to,
                    duration_ms,
                    easing,
                } => pan.push(at, *duration_ms, *to, *easing),
                EventKind::CameraZoom {
                    to,
                    duration_ms,
                    easing,
                } => zoom.push(at, *duration_ms, *to, *easing),
                EventKind::CameraCut {
                    center,
                    zoom: z,
                    shot: s,
                } => {
                    pan.push(at, 0, *center, Easing::Linear);
                    zoom.push(at, 0, *z, Easing::Linear);
                    if let Some(s) = s {
                        shot = Some(s.as_str());
                    }
                }
                EventKind::CameraShake {
                    amplitude,
                    duration_ms,
                } => shake = shake + shake_offset(at, *duration_ms, *amplitude, t_ms),
                EventKind::Transition {
                    kind,
                    duration_ms,
                    color,
                } => {
                    fade_color = *color;
                    match kind {
                        TransitionKind::Cut => fade_alpha.push(at, 0, 0.0, Easing::Linear),
                        TransitionKind::FadeIn => {
                            fade_alpha.push(at, 0, 1.0, Easing::Linear);
                            fade_alpha.push(at, *duration_ms, 0.0, Easing::Linear);
                        }
                        TransitionKind::FadeOut => {
                            fade_alpha.push(at, 0, 0.0, Easing::Linear);
                            fade_alpha.push(at, *duration_ms, 1.0, Easing::Linear);
                        }
                    }
                }
                EventKind::MapOverlay {
                    note,
                    markers,
                    duration_ms,
                } => {
                    let elapsed = i64::from(t_ms) - i64::from(at);
                    if elapsed < i64::from(*duration_ms) {
                        overlay = Some(MapOverlayState {
                            note,
                            markers,
                            elapsed_ms: elapsed as Ms,
                        });
                    }
                }
                EventKind::Sfx { .. }
                | EventKind::Music { .. }
                | EventKind::Ambience { .. }
                | EventKind::Wait { .. } => {}
            }
        }

        let center = pan.eval(t_ms);
        let camera = CameraState {
            center,
            zoom: zoom.eval(t_ms),
            shake,
        };

        let mut back_layers = Vec::new();
        let mut front_layers = Vec::new();
        let mut visible: Vec<&Layer> = self
            .layers
            .iter()
            .filter(|l| l.shots.is_empty() || shot.is_some_and(|s| l.shots.iter().any(|x| x == s)))
            .collect();
        visible.sort_by_key(|l| l.z);
        for layer in visible {
            let state = LayerState {
                layer,
                position: layer.pos + center * (1.0 - layer.parallax),
            };
            if layer.z < 0 {
                back_layers.push(state);
            } else {
                front_layers.push(state);
            }
        }

        let mut order: Vec<usize> = (0..self.characters.len())
            .filter(|&i| runs[i].on_stage)
            .collect();
        order.sort_by_key(|&i| self.characters[i].z);
        let characters = order
            .into_iter()
            .map(|i| {
                let ch = &self.characters[i];
                let run = &runs[i];
                let position = run.pos.eval(t_ms);
                let sx = match run.facing {
                    Facing::Right => ch.scale,
                    Facing::Left => -ch.scale,
                };
                let root = Affine::translation(position)
                    .then_after(&Affine::scaling(Vec2::new(sx, ch.scale)));
                CharacterState {
                    character: ch,
                    position,
                    facing: run.facing,
                    parts: resolve_parts(ch, &run.pose.eval(t_ms), &root),
                }
            })
            .collect();

        SceneState {
            time_ms: t_ms,
            finished: t_ms >= self.duration_ms(),
            camera,
            shot,
            back_layers,
            characters,
            front_layers,
            subtitle,
            fade: Fade {
                color: fade_color,
                alpha: fade_alpha.eval(t_ms),
            },
            map_overlay: overlay,
            sounds: Vec::new(),
        }
    }

    /// The frame at `t_ms` plus the sound events that started in
    /// `(prev_ms, t_ms]`. `prev_ms` is the time of the previous call, or `None`
    /// on the first call (then everything from the start up to `t_ms` fires,
    /// including events at time 0). If `t_ms` is not later than `prev_ms`
    /// (a seek backwards, a pause) nothing fires.
    pub fn sample_step(&self, prev_ms: Option<Ms>, t_ms: Ms) -> SceneState<'_> {
        let mut state = self.sample(t_ms);
        state.sounds = self.fired_between(prev_ms, t_ms);
        state
    }

    /// The sound events (sound effects, music and ambience cues, dialogue with
    /// a voice file) whose start `at` satisfies `prev_ms < at <= t_ms`, in
    /// play order. `prev_ms = None` means "from the very start". Two calls
    /// with consecutive intervals `(a, b]` and `(b, c]` return every event
    /// exactly once between them, whatever `a`, `b` and `c` are.
    pub fn fired_between(&self, prev_ms: Option<Ms>, t_ms: Ms) -> Vec<FiredEvent<'_>> {
        self.events_in_order()
            .into_iter()
            .filter(|e| e.kind.is_sound() && e.at <= t_ms && prev_ms.is_none_or(|p| e.at > p))
            .map(|e| FiredEvent {
                at: e.at,
                track: e.track,
                index: e.index,
                kind: e.kind,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load_str;
    use proptest::prelude::*;

    const FIXTURE: &str = r##"
#![enable(implicit_some)]
Scene(
    name: "fixture",
    layers: [
        (name: "sky", art: Rect(w: 100, h: 100, color: "#112233"), parallax: 0.5, z: -10, shots: ["a"]),
        (name: "ground", art: Rect(w: 100, h: 100, color: "#223344"), z: -5, shots: ["a", "b"]),
        (name: "other", art: Rect(w: 10, h: 10, color: "#ffffff"), z: -1, shots: ["b"]),
        (name: "mist", art: Rect(w: 10, h: 10, color: "#ffffff"), parallax: 2.0, z: 5),
    ],
    characters: [
        (
            id: "hero",
            name: "The Hero",
            parts: [
                (name: "torso", art: Rect(w: 40, h: 80, color: "#ff0000")),
                (name: "arm", parent: "torso", offset: (20, -30), pivot: (0, -20), z: 1,
                 art: Rect(w: 10, h: 40, color: "#00ff00")),
            ],
            poses: [
                (name: "raise", parts: [(part: "arm", angle: 90)]),
                (name: "lean", parts: [(part: "torso", angle: 10, offset: (4, 0))]),
            ],
        ),
        (id: "radio"),
    ],
    timeline: (tracks: [
        (name: "stage", events: [
            (at: 0, kind: Enter(character: "hero", pos: (100, 50))),
            (at: 1000, kind: SetPose(character: "hero", pose: "raise", duration_ms: 1000, easing: Linear)),
            (at: 1500, kind: SetPose(character: "hero", pose: "lean", duration_ms: 1000, easing: Linear)),
            (at: 3000, kind: Move(character: "hero", to: (300, 50), duration_ms: 1000, easing: Linear, facing: Left)),
            (at: 5000, kind: Exit(character: "hero")),
        ]),
        (name: "camera", events: [
            (at: 0, kind: CameraCut(center: (0, 0), zoom: 1, shot: "a")),
            (at: 2000, kind: CameraPan(to: (200, 100), duration_ms: 1000, easing: Linear)),
            (at: 2000, kind: CameraZoom(to: 2, duration_ms: 1000, easing: EaseIn)),
            (at: 3500, kind: CameraShake(amplitude: 10, duration_ms: 400)),
            (at: 4000, kind: CameraCut(center: (-50, 0), zoom: 1, shot: "b")),
        ]),
        (name: "dialogue", events: [
            (at: 200, kind: Dialogue(speaker: "hero", text: "Hello.", duration_ms: 1000, voice: "v/hello.wav")),
            (at: 700, kind: Dialogue(speaker: "radio", text: "Static.", duration_ms: 400, delivery: VoiceOver)),
            (at: 1200, kind: Dialogue(speaker: "hero", text: "Again.", duration_ms: 500)),
        ]),
        (name: "audio", events: [
            (at: 0, kind: Music(note: "drums")),
            (at: 300, kind: Sfx(note: "click")),
            (at: 300, kind: Sfx(note: "clack")),
            (at: 1000, kind: Ambience(note: "rain")),
            (at: 4500, kind: Music(action: Stop, fade_ms: 200)),
        ]),
        (name: "fx", events: [
            (at: 0, kind: Transition(kind: FadeIn, duration_ms: 1000)),
            (at: 6000, kind: Transition(kind: FadeOut, duration_ms: 500, color: "#102030")),
            (at: 7000, kind: Transition(kind: Cut)),
        ]),
    ]),
)
"##;

    fn fixture() -> Scene {
        fixture_from(FIXTURE)
    }

    fn fixture_from(text: &str) -> Scene {
        load_str(text, None).unwrap_or_else(|e| panic!("{e}")).scene
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn part<'a, 's>(s: &'s SceneState<'a>, name: &str) -> &'s PartState<'a> {
        s.characters[0]
            .parts
            .iter()
            .find(|p| p.part.name == name)
            .unwrap()
    }

    #[test]
    fn nothing_on_stage_before_the_first_enter() {
        let sc = fixture();
        assert!(sc.sample(-5).characters.is_empty());
        let s = sc.sample(0);
        assert_eq!(s.characters.len(), 1);
        assert_eq!(s.characters[0].position, Vec2::new(100.0, 50.0));
    }

    #[test]
    fn pose_midpoint_is_halfway_and_endpoints_are_exact() {
        // Without the second pose change at 1500 (moved to 2900) the first one
        // runs undisturbed from 1000 to 2000.
        let sc =
            fixture_from(&FIXTURE.replace("(at: 1500, kind: SetPose", "(at: 2900, kind: SetPose"));
        assert_eq!(part(&sc.sample(1000), "arm").pose.angle, 0.0);
        assert!(near(part(&sc.sample(1250), "arm").pose.angle, 22.5));
        assert!(near(part(&sc.sample(1500), "arm").pose.angle, 45.0));
        assert!(near(part(&sc.sample(1999), "arm").pose.angle, 89.91));
        assert_eq!(part(&sc.sample(2000), "arm").pose.angle, 90.0);
        assert_eq!(part(&sc.sample(2800), "arm").pose.angle, 90.0);
    }

    #[test]
    fn pose_change_started_mid_blend_continues_from_where_it_was() {
        let sc = fixture();
        // At 1500 the arm is at 45; `lean` has no arm entry, so it heads back
        // to 0 over 1000 ms while the torso goes 0 -> 10.
        let at_1500 = sc.sample(1500);
        assert!(near(part(&at_1500, "arm").pose.angle, 45.0));
        assert!(near(part(&at_1500, "torso").pose.angle, 0.0));
        let at_2000 = sc.sample(2000);
        assert!(near(part(&at_2000, "arm").pose.angle, 22.5));
        assert!(near(part(&at_2000, "torso").pose.angle, 5.0));
        assert!(near(part(&at_2000, "torso").pose.offset.x, 2.0));
        let done = sc.sample(2500);
        assert!(near(part(&done, "arm").pose.angle, 0.0));
        assert!(near(part(&done, "torso").pose.angle, 10.0));
        // And it stays put afterwards.
        assert_eq!(sc.sample(2500).characters, sc.sample(2900).characters);
    }

    #[test]
    fn eased_pose_uses_the_curve() {
        let text = FIXTURE.replace(
            "pose: \"raise\", duration_ms: 1000, easing: Linear",
            "pose: \"raise\", duration_ms: 1000, easing: EaseIn",
        );
        let sc = fixture_from(&text);
        // EaseIn at p = 0.25 is 0.0625 of 90 degrees.
        assert!(near(part(&sc.sample(1250), "arm").pose.angle, 5.625));
    }

    #[test]
    fn child_parts_follow_the_parent_in_world_space() {
        let sc = fixture();
        let s = sc.sample(0);
        // Rest pose, facing right, character at (100, 50): the arm joint sits at
        // the torso origin plus the arm offset.
        let arm = part(&s, "arm");
        assert!(
            near(arm.joint.x, 120.0) && near(arm.joint.y, 20.0),
            "{:?}",
            arm.joint
        );
        // The art centre is the pivot (0, -20) below... i.e. the centre is
        // 20 units below the joint: pivot is the joint's place in the picture.
        let c = arm.world.apply(Vec2::ZERO);
        assert!(near(c.x, 120.0) && near(c.y, 40.0), "{c:?}");

        // Torso leaning 10 degrees drags the arm joint round the torso origin.
        let s = sc.sample(2500);
        let torso = part(&s, "torso").joint;
        assert!(near(torso.x, 104.0) && near(torso.y, 50.0), "{torso:?}");
        let (sin, cos) = 10.0_f32.to_radians().sin_cos();
        let want = Vec2::new(
            104.0 + 20.0 * cos - (-30.0) * sin,
            50.0 + 20.0 * sin + (-30.0) * cos,
        );
        let got = part(&s, "arm").joint;
        assert!(
            near(got.x, want.x) && near(got.y, want.y),
            "{got:?} vs {want:?}"
        );
    }

    #[test]
    fn parts_are_sorted_by_z_with_ties_in_declaration_order() {
        let sc = fixture();
        let s = sc.sample(0);
        let names: Vec<_> = s.characters[0]
            .parts
            .iter()
            .map(|p| p.part.name.as_str())
            .collect();
        assert_eq!(names, ["torso", "arm"]);
    }

    #[test]
    fn move_interpolates_and_turns() {
        let sc = fixture();
        let s = sc.sample(3500);
        assert!(near(s.characters[0].position.x, 200.0));
        assert_eq!(s.characters[0].facing, Facing::Left);
        // Facing left mirrors the rig: the arm joint (+20 right of the torso) is
        // now to the left of it.
        let torso = part(&s, "torso").joint;
        let arm = part(&s, "arm").joint;
        assert!(arm.x < torso.x, "{arm:?} {torso:?}");
        assert_eq!(sc.sample(4000).characters[0].position.x, 300.0);
        assert!(sc.sample(5000).characters.is_empty());
    }

    #[test]
    fn camera_pan_zoom_and_cut() {
        let sc = fixture();
        let c = sc.sample(1000).camera;
        assert_eq!((c.center, c.zoom, c.shake), (Vec2::ZERO, 1.0, Vec2::ZERO));
        let c = sc.sample(2500).camera;
        assert!(near(c.center.x, 100.0) && near(c.center.y, 50.0));
        // EaseIn zoom at p = 0.5 is 0.25 of the way from 1 to 2.
        assert!(near(c.zoom, 1.25));
        let c = sc.sample(3000).camera;
        assert_eq!((c.center, c.zoom), (Vec2::new(200.0, 100.0), 2.0));
        // The cut at 4000 jumps, it does not blend.
        let c = sc.sample(4000).camera;
        assert_eq!((c.center, c.zoom), (Vec2::new(-50.0, 0.0), 1.0));
        assert_eq!(sc.sample(3999).camera.center, Vec2::new(200.0, 100.0));
    }

    #[test]
    fn shake_is_bounded_decaying_deterministic_and_ends() {
        let sc = fixture();
        assert_eq!(sc.sample(3499).camera.shake, Vec2::ZERO);
        assert_eq!(sc.sample(3900).camera.shake, Vec2::ZERO);
        let mut any = false;
        for t in 3500..3900 {
            let s = sc.sample(t).camera.shake;
            let left = 1.0 - (t - 3500) as f32 / 400.0;
            assert!(s.x.abs() <= 10.0 * left + 1e-4 && s.y.abs() <= 10.0 * left + 1e-4);
            assert_eq!(s, sc.sample(t).camera.shake, "same time, same shake");
            any |= s != Vec2::ZERO;
        }
        assert!(any, "the shake never moved the camera");
    }

    #[test]
    fn subtitle_window_is_half_open_and_latest_start_wins() {
        let sc = fixture();
        assert!(sc.sample(199).subtitle.is_none());
        let s = sc.sample(200).subtitle.unwrap();
        assert_eq!(
            (s.speaker, s.speaker_name, s.text),
            ("hero", "The Hero", "Hello.")
        );
        assert_eq!((s.elapsed_ms, s.duration_ms), (0, 1000));
        assert_eq!(s.voice, Some("v/hello.wav"));
        // The radio starts at 700 and wins while both are up.
        let s = sc.sample(800).subtitle.unwrap();
        assert_eq!((s.speaker, s.speaker_name), ("radio", "radio"));
        assert_eq!(s.delivery, Delivery::VoiceOver);
        // Hero's first line ends at exactly 1200, where his next one begins.
        assert_eq!(sc.sample(1199).subtitle.unwrap().text, "Hello.");
        assert_eq!(sc.sample(1200).subtitle.unwrap().text, "Again.");
        assert!(sc.sample(1700).subtitle.is_none());
    }

    #[test]
    fn layers_follow_shot_parallax_and_z() {
        let sc = fixture();
        let s = sc.sample(3000); // shot "a", camera at (200, 100)
        assert_eq!(s.shot, Some("a"));
        let back: Vec<_> = s
            .back_layers
            .iter()
            .map(|l| l.layer.name.as_str())
            .collect();
        assert_eq!(back, ["sky", "ground"]);
        // parallax 0.5: world position = pos + cam * 0.5.
        assert_eq!(s.back_layers[0].position, Vec2::new(100.0, 50.0));
        // parallax 1: stays where it is.
        assert_eq!(s.back_layers[1].position, Vec2::ZERO);
        // parallax 2 (foreground), visible in every shot: pos - cam.
        assert_eq!(s.front_layers[0].layer.name, "mist");
        assert_eq!(s.front_layers[0].position, Vec2::new(-200.0, -100.0));
        // Shot "b" swaps the sky for "other".
        let s = sc.sample(4200);
        assert_eq!(s.shot, Some("b"));
        let back: Vec<_> = s
            .back_layers
            .iter()
            .map(|l| l.layer.name.as_str())
            .collect();
        assert_eq!(back, ["ground", "other"]);
    }

    #[test]
    fn fade_in_out_and_cut() {
        let sc = fixture();
        assert_eq!(sc.sample(0).fade.alpha, 1.0);
        assert!(near(sc.sample(500).fade.alpha, 0.5));
        assert_eq!(sc.sample(1000).fade.alpha, 0.0);
        assert_eq!(sc.sample(5999).fade.alpha, 0.0);
        let f = sc.sample(6250).fade;
        assert!(near(f.alpha, 0.5));
        assert_eq!(f.color, Color::rgb(0x10, 0x20, 0x30));
        assert_eq!(sc.sample(6500).fade.alpha, 1.0);
        assert_eq!(sc.sample(6999).fade.alpha, 1.0);
        assert_eq!(sc.sample(7000).fade.alpha, 0.0);
    }

    #[test]
    fn finished_at_the_scene_duration() {
        let sc = fixture();
        assert_eq!(sc.duration_ms(), 7000);
        assert!(!sc.sample(6999).finished);
        assert!(sc.sample(7000).finished);
    }

    fn sound_events(sc: &Scene) -> Vec<(usize, usize)> {
        sc.events_in_order()
            .into_iter()
            .filter(|e| e.kind.is_sound())
            .map(|e| (e.track, e.index))
            .collect()
    }

    #[test]
    fn sound_events_are_the_cues_and_voiced_lines() {
        let sc = fixture();
        // Music, 2 x Sfx, Ambience, Music stop, plus the one voiced line.
        assert_eq!(sound_events(&sc).len(), 6);
    }

    #[test]
    fn firing_interval_is_open_at_the_start_and_closed_at_the_end() {
        let sc = fixture();
        let ats = |prev, t| -> Vec<Ms> { sc.fired_between(prev, t).iter().map(|f| f.at).collect() };
        assert_eq!(ats(None, 0), [0]);
        assert_eq!(ats(Some(0), 199), Vec::<Ms>::new());
        assert_eq!(ats(Some(0), 299), [200]);
        assert_eq!(ats(Some(0), 300), [200, 300, 300]);
        assert_eq!(ats(Some(300), 300), Vec::<Ms>::new());
        // Going backwards fires nothing.
        assert_eq!(ats(Some(5000), 100), Vec::<Ms>::new());
        // The step helper carries the same list in its state.
        let st = sc.sample_step(Some(0), 300);
        assert_eq!(st.sounds.len(), 3);
        assert_eq!(st.time_ms, 300);
        assert!(sc.sample(300).sounds.is_empty());
    }

    #[test]
    fn same_time_events_fire_in_track_then_list_order() {
        let sc = fixture();
        let f = sc.fired_between(Some(250), 300);
        let notes: Vec<_> = f
            .iter()
            .map(|f| match f.kind {
                EventKind::Sfx { note, .. } => note.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(notes, ["click", "clack"]);
    }

    fn fire_all(sc: &Scene, times: &[Ms]) -> Vec<(usize, usize)> {
        let mut prev = None;
        let mut out = Vec::new();
        for &t in times {
            out.extend(
                sc.sample_step(prev, t)
                    .sounds
                    .iter()
                    .map(|f| (f.track, f.index)),
            );
            prev = Some(t);
        }
        out
    }

    #[test]
    fn every_sound_event_fires_exactly_once_for_fixed_step_sizes() {
        let sc = fixture();
        let want = sound_events(&sc);
        for step in [1, 7, 16, 33, 250, 999, 5000, 100_000] {
            let mut times: Vec<Ms> = (0..)
                .map(|k| k * step)
                .take_while(|&t| t < sc.duration_ms() + step)
                .collect();
            times.push(sc.duration_ms());
            times.dedup();
            assert_eq!(fire_all(&sc, &times), want, "step {step} ms");
        }
    }

    proptest! {
        #[test]
        fn every_sound_event_fires_exactly_once_for_any_step_sequence(
            steps in proptest::collection::vec(0i32..2500, 1..60)
        ) {
            let sc = fixture();
            let mut t = 0;
            let mut times = vec![t];
            for s in steps {
                t += s;
                times.push(t);
            }
            times.push(sc.duration_ms().max(t));
            let got = fire_all(&sc, &times);
            // Each event at most once, in play order, and none lost up to the
            // last sampled time.
            let last = *times.last().unwrap();
            let want: Vec<_> = sc
                .events_in_order()
                .into_iter()
                .filter(|e| e.kind.is_sound() && e.at <= last)
                .map(|e| (e.track, e.index))
                .collect();
            prop_assert_eq!(got, want);
        }

        #[test]
        fn sampling_is_pure(t in -100i32..9000) {
            let sc = fixture();
            prop_assert_eq!(sc.sample(t), sc.sample(t));
        }
    }

    #[test]
    fn chained_overlapping_segments_do_not_overflow_the_stack() {
        // 20 000 pose changes, each starting before the previous one ends.
        let mut sc = fixture();
        let events = &mut sc.timeline.tracks[0].events;
        events.truncate(1);
        for k in 0..20_000 {
            events.push(crate::model::Event {
                at: 1000 + k,
                kind: EventKind::SetPose {
                    character: "hero".into(),
                    pose: if k % 2 == 0 { "raise" } else { "lean" }.into(),
                    duration_ms: 5_000,
                    easing: Easing::Linear,
                    hint: None,
                },
            });
        }
        // The last change is still running, and so is every one before it.
        let s = sc.sample(21_500);
        assert_eq!(s.characters.len(), 1);
        assert!(part(&s, "arm").pose.angle.is_finite());
    }
}
