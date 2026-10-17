//! The player: a state machine that plays a [`Scene`] against a clock the
//! caller supplies.
//!
//! The player has **no clock of its own**. The caller (the viewer, the game's
//! main loop, a test) calls [`Player::update`] once per frame with the current
//! wall-clock time in milliseconds, from any monotonic source. Everything the
//! player does is a function of those numbers, so it is testable without
//! waiting.
//!
//! # Controls
//!
//! [`Player::play`], [`Player::pause`], [`Player::seek`], [`Player::skip_to_end`]
//! and [`Player::skip_to_next_shot`] change the state; [`Player::sample`] gives the
//! frame to draw (the pure [`Scene::sample`] at the current time) and
//! [`Player::update`] advances time and hands the sound events to a
//! [`SoundSink`]. [`crate::controls`] maps keys and clicks to these calls.
//!
//! # Sound without depending on a sound crate
//!
//! The player never plays anything. It tells a [`SoundSink`] what to do, in
//! order, with owned [`SoundEvent`]s. `sf_audio` (or a test) implements the
//! trait; this crate does not depend on it.
//!
//! * Each sound event of the scene (`Sfx`, `Music`, `Ambience`, a `Dialogue`
//!   with a voice file) is delivered **once** as time passes over it. Pausing
//!   delivers [`SoundEvent::Pause`] and [`SoundEvent::Resume`] and nothing in
//!   between; time stands still, so nothing can fire twice or be lost.
//! * A **seek** (or a skip to the next shot) puts the sound back the way it
//!   would be had the scene played normally up to the new time: the sink gets
//!   [`SoundEvent::StopAll`], then the music and the ambience that are active
//!   there ([`SceneState::music`], [`SceneState::ambience`]), then the voice
//!   line that is under way, resumed at the right offset
//!   ([`SoundEvent::Voice::offset_ms`]). One-shot sound effects between the
//!   old and the new time are not played. An event exactly at the new time
//!   fires normally right after the restore.
//! * A seek made while **paused** is remembered and carried out when playback
//!   resumes, so nothing starts sounding in a paused scene.
//! * **Skipping to the end** (or seeking to the very end) sends
//!   [`SoundEvent::StopAll`] and no restore: the
//!   scene is over and whoever called the player decides what comes next. A
//!   scene that simply plays to its end sends nothing at the end.

use crate::controls::Action;
use crate::math::Ms;
use crate::model::{EventKind, LoopAction, Scene};
use crate::sample::SceneState;

/// The longest stretch of scene time one [`Player::update`] advances. A frame
/// that took longer (a window being dragged, a debugger, a loading hitch)
/// continues from where it was instead of skipping dialogue.
pub const MAX_STEP_MS: u64 = 500;

/// Where the player is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Time advances with the caller's clock.
    Playing,
    /// Time stands still; the frame at [`Player::time_ms`] stays on screen.
    Paused,
    /// The end was reached (played or skipped); the last frame stays.
    Finished,
}

/// The two looping beds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bed {
    Music,
    Ambience,
}

/// What the player asks the sound system to do.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundEvent {
    /// A one-shot effect. `sound` may be absent while the recording does not
    /// exist; `note` says what the writer wanted.
    Sfx {
        sound: Option<String>,
        note: String,
        volume: f32,
    },
    /// Play a recorded line. `offset_ms` is 0 for a line that starts now and
    /// more when playback is resumed in the middle of it after a seek.
    Voice {
        speaker: String,
        file: String,
        offset_ms: Ms,
    },
    /// Start or stop the music or the ambient bed. After a seek the start
    /// comes with `fade_ms: 0`.
    Bed {
        bed: Bed,
        action: LoopAction,
        file: Option<String>,
        note: String,
        fade_ms: Ms,
    },
    /// Silence everything this scene started: effects, voices, music and
    /// ambience, including sounds that were paused (the paused state is
    /// forgotten).
    StopAll,
    /// Pause every sound of this scene.
    Pause,
    /// Continue what [`SoundEvent::Pause`] paused.
    Resume,
}

/// Receives the player's sound events. Implemented by whatever plays sound;
/// closures `FnMut(&SoundEvent)` work too.
pub trait SoundSink {
    fn event(&mut self, event: &SoundEvent);
}

impl<F: FnMut(&SoundEvent)> SoundSink for F {
    fn event(&mut self, event: &SoundEvent) {
        self(event);
    }
}

/// Throws every event away (a silent viewer).
#[derive(Clone, Copy, Debug, Default)]
pub struct NullSink;

impl SoundSink for NullSink {
    fn event(&mut self, _: &SoundEvent) {}
}

/// Remembers every event, for tests and for showing what would play.
#[derive(Clone, Debug, Default)]
pub struct RecordingSink(pub Vec<SoundEvent>);

impl SoundSink for RecordingSink {
    fn event(&mut self, event: &SoundEvent) {
        self.0.push(event.clone());
    }
}

/// A scene being played.
#[derive(Clone, Debug)]
pub struct Player {
    scene: Scene,
    duration: Ms,
    /// Start times of the shots (`CameraCut` events that name a shot), sorted
    /// and without duplicates.
    shot_starts: Vec<Ms>,
    mode: Mode,
    /// Scene time, `0..=duration`.
    time: Ms,
    /// Sound events with `at <= fired_to` have been delivered (or deliberately
    /// skipped by a seek). `-1` = none yet; events at time 0 are still due.
    fired_to: Ms,
    last_wall: Option<u64>,
    /// The sound has to be put back to match `time` before the next advance.
    resync: bool,
    outbox: Vec<SoundEvent>,
}

impl Player {
    /// A player at the start of `scene`, playing. The first
    /// [`Player::update`] delivers the events at time 0.
    pub fn new(scene: Scene) -> Player {
        let duration = scene.duration_ms();
        let mut shot_starts: Vec<Ms> = scene
            .events_in_order()
            .into_iter()
            .filter(|e| matches!(e.kind, EventKind::CameraCut { shot: Some(_), .. }))
            .map(|e| e.at)
            .collect();
        shot_starts.dedup();
        Player {
            scene,
            duration,
            shot_starts,
            mode: Mode::Playing,
            time: 0,
            fired_to: -1,
            last_wall: None,
            resync: false,
            outbox: Vec::new(),
        }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn is_playing(&self) -> bool {
        self.mode == Mode::Playing
    }

    pub fn is_paused(&self) -> bool {
        self.mode == Mode::Paused
    }

    pub fn is_finished(&self) -> bool {
        self.mode == Mode::Finished
    }

    /// Scene time in milliseconds, `0..=duration_ms()`.
    pub fn time_ms(&self) -> Ms {
        self.time
    }

    pub fn duration_ms(&self) -> Ms {
        self.duration
    }

    /// When each shot starts, in order.
    pub fn shot_starts(&self) -> &[Ms] {
        &self.shot_starts
    }

    /// The frame to draw now.
    pub fn sample(&self) -> SceneState<'_> {
        self.scene.sample(self.time)
    }

    /// Sound events not yet handed to a sink (they go out with the next
    /// [`Player::update`]); for inspection in tests.
    pub fn pending_sound(&self) -> &[SoundEvent] {
        &self.outbox
    }

    // -- controls ------------------------------------------------------

    /// Continue after a pause. At the end this plays the scene again from the
    /// start.
    pub fn play(&mut self) {
        match self.mode {
            Mode::Playing => {}
            Mode::Paused => {
                self.mode = Mode::Playing;
                if !self.resync {
                    self.outbox.push(SoundEvent::Resume);
                }
            }
            Mode::Finished => {
                self.seek(0);
                self.mode = Mode::Playing;
            }
        }
    }

    /// Freeze the scene where it is. No effect when not playing.
    pub fn pause(&mut self) {
        if self.mode == Mode::Playing {
            self.mode = Mode::Paused;
            // Always told, even when a seek is waiting: until the restore
            // runs, the sink may still be playing the old position's sound.
            self.outbox.push(SoundEvent::Pause);
        }
    }

    pub fn toggle_pause(&mut self) {
        match self.mode {
            Mode::Playing => self.pause(),
            Mode::Paused | Mode::Finished => self.play(),
        }
    }

    /// Jump to `ms` (clamped to the scene). Playing, paused or finished stay
    /// as they are, except that seeking away from the end of a finished scene
    /// plays it. The sound is put back as the module docs describe.
    pub fn seek(&mut self, ms: Ms) {
        let to = ms.clamp(0, self.duration);
        if to >= self.duration {
            // The end: the same as skipping there (sound stops, no restore).
            self.skip_to_end();
            return;
        }
        self.time = to;
        // Sound events up to `to - 1` count as done; the ones at `to` fire
        // after the restore. At the very start nothing has been done.
        self.fired_to = to - 1;
        self.resync = true;
        if self.mode == Mode::Finished {
            self.mode = Mode::Playing;
        }
    }

    /// Jump by `delta_ms` (negative = back).
    pub fn seek_by(&mut self, delta_ms: Ms) {
        self.seek(self.time.saturating_add(delta_ms));
    }

    /// Go back to the start and play.
    pub fn restart(&mut self) {
        self.seek(0);
        self.mode = Mode::Playing;
    }

    /// Jump to the end: the last frame, the sound stopped, [`Mode::Finished`].
    pub fn skip_to_end(&mut self) {
        if self.mode == Mode::Finished {
            return;
        }
        self.time = self.duration;
        self.fired_to = self.duration;
        self.resync = false;
        self.mode = Mode::Finished;
        self.outbox.push(SoundEvent::StopAll);
    }

    /// Jump to the start of the next shot; at the last shot, to the end.
    pub fn skip_to_next_shot(&mut self) {
        let next = self.shot_starts.iter().copied().find(|&s| s > self.time);
        match next {
            Some(s) if s < self.duration => self.seek(s),
            _ => self.skip_to_end(),
        }
    }

    /// Carry out an [`Action`] from the key bindings. `seek_step_ms` is the
    /// size of [`Action::SeekBack`] and [`Action::SeekForward`].
    pub fn apply(&mut self, action: Action, seek_step_ms: Ms) {
        match action {
            Action::TogglePause => self.toggle_pause(),
            Action::SkipToEnd => self.skip_to_end(),
            Action::NextShot => self.skip_to_next_shot(),
            Action::Restart => self.restart(),
            Action::SeekBack => self.seek_by(-seek_step_ms),
            Action::SeekForward => self.seek_by(seek_step_ms),
        }
    }

    // -- time ------------------------------------------------------------

    /// Advance to the caller's clock `now_ms` (any monotonic millisecond
    /// counter; only the difference between calls matters) and hand all sound
    /// events that came up, in order, to `sink`.
    ///
    /// Call it once per frame, also while paused (the clock must keep
    /// running so that resuming does not jump).
    pub fn update(&mut self, now_ms: u64, sink: &mut dyn SoundSink) {
        let dt = self
            .last_wall
            .map_or(0, |last| now_ms.saturating_sub(last))
            .min(MAX_STEP_MS);
        self.last_wall = Some(now_ms);
        if self.mode == Mode::Playing {
            if self.resync {
                self.restore_sound();
            }
            let target = (i64::from(self.time) + dt as i64).min(i64::from(self.duration)) as Ms;
            self.advance_to(target);
        }
        for e in self.outbox.drain(..) {
            sink.event(&e);
        }
    }

    /// Deliver the events in `(fired_to, target]` and move time to `target`.
    fn advance_to(&mut self, target: Ms) {
        for e in self.scene.fired_between(Some(self.fired_to), target) {
            if let Some(ev) = sound_event(e.kind) {
                self.outbox.push(ev);
            }
        }
        self.fired_to = self.fired_to.max(target);
        self.time = target;
        if target >= self.duration {
            self.mode = Mode::Finished;
        }
    }

    /// Stop everything and start what should be sounding at `time`.
    fn restore_sound(&mut self) {
        self.resync = false;
        self.outbox.push(SoundEvent::StopAll);
        if self.time <= 0 {
            return;
        }
        // The state just before `time`: events at `time` itself are still to
        // come and fire normally.
        let before = self.scene.sample(self.time - 1);
        for (bed, state) in [(Bed::Music, before.music), (Bed::Ambience, before.ambience)] {
            if let Some(l) = state {
                self.outbox.push(SoundEvent::Bed {
                    bed,
                    action: LoopAction::Play,
                    file: l.file.map(str::to_owned),
                    note: l.note.to_owned(),
                    fade_ms: 0,
                });
            }
        }
        for v in self.scene.voices_running_at(self.time) {
            self.outbox.push(SoundEvent::Voice {
                speaker: v.speaker.to_owned(),
                file: v.file.to_owned(),
                offset_ms: v.elapsed_ms,
            });
        }
    }
}

/// The sound event for a timeline event, if it is one.
fn sound_event(kind: &EventKind) -> Option<SoundEvent> {
    match kind {
        EventKind::Sfx {
            sound,
            note,
            volume,
        } => Some(SoundEvent::Sfx {
            sound: sound.clone(),
            note: note.clone(),
            volume: *volume,
        }),
        EventKind::Music {
            action,
            file,
            note,
            fade_ms,
        } => Some(SoundEvent::Bed {
            bed: Bed::Music,
            action: *action,
            file: file.clone(),
            note: note.clone(),
            fade_ms: *fade_ms,
        }),
        EventKind::Ambience {
            action,
            file,
            note,
            fade_ms,
        } => Some(SoundEvent::Bed {
            bed: Bed::Ambience,
            action: *action,
            file: file.clone(),
            note: note.clone(),
            fade_ms: *fade_ms,
        }),
        EventKind::Dialogue {
            speaker,
            voice: Some(file),
            ..
        } => Some(SoundEvent::Voice {
            speaker: speaker.clone(),
            file: file.clone(),
            offset_ms: 0,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::load_str;
    use proptest::prelude::*;

    /// 6 s long. Shots start at 0, 2500 and 5000.
    const SCENE: &str = r##"
#![enable(implicit_some)]
Scene(
    name: "player",
    characters: [(id: "a", parts: [(name: "body", art: Rect(w: 10, h: 10, color: "#ffffff"))])],
    timeline: (tracks: [
        (name: "stage", events: [
            (at: 0, kind: Enter(character: "a", pos: (0, 0))),
            (at: 0, kind: Wait(duration_ms: 6000)),
        ]),
        (name: "camera", events: [
            (at: 0, kind: CameraCut(shot: "one")),
            (at: 2500, kind: CameraCut(shot: "two")),
            (at: 5000, kind: CameraCut(shot: "three")),
            (at: 5500, kind: CameraCut(zoom: 2)),
        ]),
        (name: "audio", events: [
            (at: 0, kind: Music(note: "drums", fade_ms: 400)),
            (at: 500, kind: Ambience(note: "rain")),
            (at: 1000, kind: Sfx(note: "click")),
            (at: 2500, kind: Sfx(note: "at the shot")),
            (at: 3000, kind: Sfx(note: "clack")),
            (at: 4000, kind: Music(action: Stop)),
        ]),
        (name: "dialogue", events: [
            (at: 2000, kind: Dialogue(speaker: "a", text: "Hello.", duration_ms: 2000, voice: "v/hello.wav")),
        ]),
    ]),
)
"##;

    fn player() -> Player {
        Player::new(load_str(SCENE, None).expect("scene").scene)
    }

    /// Run `p.update` at `now` and return what the sink got.
    fn step(p: &mut Player, now: u64) -> Vec<SoundEvent> {
        let mut sink = RecordingSink::default();
        p.update(now, &mut sink);
        sink.0
    }

    fn sfx(note: &str) -> SoundEvent {
        SoundEvent::Sfx {
            sound: None,
            note: note.into(),
            volume: 1.0,
        }
    }

    fn notes(events: &[SoundEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                SoundEvent::Sfx { note, .. } => Some(note.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn plays_in_the_callers_time_and_fires_events_once() {
        let mut p = player();
        // The first update fires what is at time 0.
        let first = step(&mut p, 10_000);
        assert_eq!(p.time_ms(), 0);
        assert!(matches!(
            first.as_slice(),
            [SoundEvent::Bed {
                bed: Bed::Music,
                action: LoopAction::Play,
                fade_ms: 400,
                ..
            }]
        ));
        // 700 ms later: the ambience at 500.
        let e = step(&mut p, 10_500);
        assert_eq!(p.time_ms(), 500);
        assert!(matches!(
            e.as_slice(),
            [SoundEvent::Bed {
                bed: Bed::Ambience,
                ..
            }]
        ));
        // The click at 1000 fires once, however the frames fall.
        let mut all = Vec::new();
        for now in [10_900, 11_000, 11_001, 11_001, 11_400] {
            all.extend(step(&mut p, now));
        }
        assert_eq!(all, vec![sfx("click")]);
        assert_eq!(p.time_ms(), 1400);
    }

    #[test]
    fn a_long_frame_does_not_skip_the_scene() {
        let mut p = player();
        step(&mut p, 0);
        let e = step(&mut p, 60_000);
        assert_eq!(p.time_ms(), MAX_STEP_MS as Ms);
        // Everything up to 500 still fired in order, nothing was jumped over.
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn pause_freezes_time_and_resume_continues_without_repeats() {
        let mut p = player();
        step(&mut p, 0);
        step(&mut p, 400);
        p.pause();
        assert!(p.is_paused());
        let during = [step(&mut p, 900), step(&mut p, 5_000), step(&mut p, 5_100)];
        assert_eq!(p.time_ms(), 400);
        // The only thing the sink hears is the pause itself, once.
        assert_eq!(during[0], vec![SoundEvent::Pause]);
        assert!(during[1].is_empty() && during[2].is_empty());
        p.play();
        // The clock jumped while paused; playing continues from 400.
        let mut resumed = step(&mut p, 5_300);
        assert_eq!(resumed.remove(0), SoundEvent::Resume);
        assert_eq!(p.time_ms(), 600);
        // The ambience at 500 fires exactly once, now.
        assert_eq!(resumed.len(), 1);
        let again = step(&mut p, 5_400);
        assert!(again.is_empty());
    }

    #[test]
    fn pausing_twice_or_playing_twice_is_harmless() {
        let mut p = player();
        step(&mut p, 0);
        p.pause();
        p.pause();
        p.play();
        p.play();
        assert_eq!(
            step(&mut p, 10),
            vec![SoundEvent::Pause, SoundEvent::Resume]
        );
    }

    #[test]
    fn seek_forward_restores_music_ambience_and_the_voice_line() {
        let mut p = player();
        step(&mut p, 0);
        step(&mut p, 300);
        // The player is at 300; jump into the middle of the spoken line.
        p.seek(3200);
        assert_eq!(p.time_ms(), 3200);
        let e = step(&mut p, 300);
        assert_eq!(e[0], SoundEvent::StopAll);
        assert_eq!(
            e[1..],
            [
                SoundEvent::Bed {
                    bed: Bed::Music,
                    action: LoopAction::Play,
                    file: None,
                    note: "drums".into(),
                    fade_ms: 0
                },
                SoundEvent::Bed {
                    bed: Bed::Ambience,
                    action: LoopAction::Play,
                    file: None,
                    note: "rain".into(),
                    fade_ms: 0
                },
                SoundEvent::Voice {
                    speaker: "a".into(),
                    file: "v/hello.wav".into(),
                    offset_ms: 1200
                },
            ],
            "the sound events skipped over (click, clack) are not played"
        );
        // From here on events fire normally and once: the music stops at 4000.
        let mut later = Vec::new();
        for now in [600, 900, 1_200] {
            later.extend(step(&mut p, now));
        }
        assert_eq!(later.len(), 1);
        assert!(matches!(
            later[0],
            SoundEvent::Bed {
                bed: Bed::Music,
                action: LoopAction::Stop,
                ..
            }
        ));
    }

    #[test]
    fn after_the_music_stopped_a_seek_does_not_bring_it_back() {
        let mut p = player();
        step(&mut p, 0);
        p.seek(4500);
        let e = step(&mut p, 0);
        assert!(
            !e.iter().any(|e| matches!(
                e,
                SoundEvent::Bed {
                    bed: Bed::Music,
                    ..
                }
            )),
            "{e:?}"
        );
        assert!(
            e.iter().any(|e| matches!(
                e,
                SoundEvent::Bed {
                    bed: Bed::Ambience,
                    ..
                }
            )),
            "the ambience carries on: {e:?}"
        );
    }

    #[test]
    fn seek_back_replays_events_once_each() {
        let mut p = player();
        step(&mut p, 0);
        let mut heard = Vec::new();
        for now in (1..=11).map(|k| k * 100) {
            heard.extend(notes(&step(&mut p, now)));
        }
        assert_eq!(heard, ["click"]);
        p.seek(900);
        let mut again = Vec::new();
        for now in (12..=20).map(|k| k * 100) {
            again.extend(notes(&step(&mut p, now)));
        }
        assert_eq!(
            again,
            ["click"],
            "back before the click: it plays again once"
        );
    }

    #[test]
    fn an_event_exactly_at_the_seek_target_fires_after_the_restore() {
        let mut p = player();
        p.seek(2500);
        let e = step(&mut p, 0);
        assert_eq!(e[0], SoundEvent::StopAll);
        assert_eq!(notes(&e), ["at the shot"]);
        // And only once.
        assert!(notes(&step(&mut p, 100)).is_empty());
    }

    #[test]
    fn seek_to_the_start_restarts_everything() {
        let mut p = player();
        step(&mut p, 0);
        step(&mut p, 500);
        p.seek(0);
        let e = step(&mut p, 500);
        assert_eq!(e[0], SoundEvent::StopAll);
        assert!(
            matches!(
                e[1],
                SoundEvent::Bed {
                    bed: Bed::Music,
                    fade_ms: 400,
                    ..
                }
            ),
            "the events at 0 fire again: {e:?}"
        );
    }

    #[test]
    fn a_seek_made_while_paused_waits_for_the_resume() {
        let mut p = player();
        step(&mut p, 0);
        step(&mut p, 100);
        p.pause();
        assert_eq!(step(&mut p, 200), vec![SoundEvent::Pause]);
        p.seek(3200);
        assert_eq!(p.time_ms(), 3200);
        assert_eq!(p.sample().time_ms, 3200, "the frame follows at once");
        assert!(step(&mut p, 300).is_empty(), "no sound while paused");
        p.play();
        let e = step(&mut p, 400);
        assert_eq!(e[0], SoundEvent::StopAll);
        assert!(
            !e.contains(&SoundEvent::Resume),
            "StopAll already cleared the pause: {e:?}"
        );
        assert!(e.iter().any(|e| matches!(e, SoundEvent::Voice { .. })));
    }

    #[test]
    fn pausing_right_after_a_seek_still_tells_the_sink() {
        let mut p = player();
        step(&mut p, 0);
        step(&mut p, 300);
        p.seek(3000);
        p.pause();
        // The sink may still be playing the old position: it is paused now,
        // and only a resume puts the new position's sound in place.
        assert_eq!(step(&mut p, 400), vec![SoundEvent::Pause]);
        p.play();
        assert_eq!(step(&mut p, 500)[0], SoundEvent::StopAll);
    }

    #[test]
    fn seeking_to_the_end_stops_the_sound_like_a_skip() {
        let mut p = player();
        step(&mut p, 0);
        p.seek(6000);
        assert!(p.is_finished());
        assert_eq!(step(&mut p, 10), vec![SoundEvent::StopAll]);
    }

    #[test]
    fn seek_is_clamped_and_the_end_finishes() {
        let mut p = player();
        p.seek(-50);
        assert_eq!(p.time_ms(), 0);
        p.seek(99_999);
        assert_eq!(p.time_ms(), 6000);
        assert!(p.is_finished());
        // Seeking away from the end plays again.
        p.seek(1000);
        assert!(p.is_playing());
        p.seek_by(-5000);
        assert_eq!(p.time_ms(), 0);
        p.seek_by(1_000_000);
        assert!(p.is_finished());
    }

    #[test]
    fn skip_to_end_stops_the_sound_once_and_shows_the_last_frame() {
        let mut p = player();
        step(&mut p, 0);
        step(&mut p, 300);
        p.skip_to_end();
        assert!(p.is_finished());
        assert_eq!(p.time_ms(), 6000);
        assert!(p.sample().finished);
        assert_eq!(step(&mut p, 400), vec![SoundEvent::StopAll]);
        // Nothing more, whatever happens to the clock; skipping again is a no-op.
        p.skip_to_end();
        assert!(step(&mut p, 5_000).is_empty());
    }

    #[test]
    fn playing_to_the_end_finishes_without_a_stop() {
        let mut p = player();
        let mut all = Vec::new();
        for now in (0..=130).map(|k| k * 100) {
            all.extend(step(&mut p, now));
        }
        assert!(p.is_finished());
        assert_eq!(p.time_ms(), 6000);
        assert!(!all.contains(&SoundEvent::StopAll));
        // Every sound event of the scene came out exactly once.
        let want = p
            .scene()
            .events_in_order()
            .iter()
            .filter(|e| e.kind.is_sound())
            .count();
        assert_eq!(all.len(), want);
    }

    #[test]
    fn next_shot_jumps_shot_by_shot_then_to_the_end() {
        let mut p = player();
        assert_eq!(p.shot_starts(), &[0, 2500, 5000]);
        step(&mut p, 0);
        p.skip_to_next_shot();
        assert_eq!(p.time_ms(), 2500);
        assert_eq!(p.sample().shot, Some("two"));
        p.skip_to_next_shot();
        assert_eq!(p.time_ms(), 5000);
        assert_eq!(p.sample().shot, Some("three"));
        p.skip_to_next_shot();
        assert!(p.is_finished(), "no shot left: the end");
        assert_eq!(p.time_ms(), 6000);
    }

    #[test]
    fn play_after_the_end_starts_over() {
        let mut p = player();
        p.skip_to_end();
        step(&mut p, 0);
        p.play();
        assert!(p.is_playing());
        assert_eq!(p.time_ms(), 0);
        let e = step(&mut p, 50);
        assert_eq!(e[0], SoundEvent::StopAll);
    }

    #[test]
    fn a_scene_without_events_finishes_at_once() {
        let scene = load_str("Scene(name: \"empty\")", None).unwrap().scene;
        let mut p = Player::new(scene);
        assert_eq!(p.duration_ms(), 0);
        assert!(step(&mut p, 0).is_empty());
        assert!(p.is_finished());
        p.skip_to_next_shot();
        p.seek(10);
        assert_eq!(p.time_ms(), 0);
    }

    #[test]
    fn closures_are_sinks() {
        let mut p = player();
        let mut count = 0;
        p.update(0, &mut |_: &SoundEvent| count += 1);
        assert_eq!(count, 1);
    }

    #[derive(Clone, Debug)]
    enum Op {
        Tick(u64),
        Pause,
        Play,
        Seek(Ms),
        SkipShot,
        SkipEnd,
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            4 => (0u64..800).prop_map(Op::Tick),
            1 => Just(Op::Pause),
            1 => Just(Op::Play),
            1 => (-500i32..6500).prop_map(Op::Seek),
            1 => Just(Op::SkipShot),
            1 => Just(Op::SkipEnd),
        ]
    }

    proptest! {
        /// Between seeks, no one-shot effect is delivered twice, whatever mix of
        /// frames, pauses and resumes happens; and time stays inside the scene.
        #[test]
        fn no_effect_twice_between_seeks(ops in proptest::collection::vec(op(), 1..80)) {
            let mut p = player();
            let mut now = 0u64;
            let mut heard: Vec<String> = Vec::new();
            let mut sink = RecordingSink::default();
            p.update(now, &mut sink);
            for o in ops {
                let jumps = matches!(o, Op::Seek(_) | Op::SkipShot | Op::SkipEnd)
                    || (matches!(o, Op::Play) && p.is_finished());
                match o {
                    Op::Tick(dt) => {
                        now += dt;
                        let before = sink.0.len();
                        p.update(now, &mut sink);
                        for note in notes(&sink.0[before..]) {
                            prop_assert!(!heard.contains(&note), "{note} twice");
                            heard.push(note);
                        }
                    }
                    Op::Pause => p.pause(),
                    Op::Play => p.play(),
                    Op::Seek(ms) => p.seek(ms),
                    Op::SkipShot => p.skip_to_next_shot(),
                    Op::SkipEnd => p.skip_to_end(),
                }
                if jumps {
                    heard.clear();
                }
                prop_assert!(p.time_ms() >= 0 && p.time_ms() <= p.duration_ms());
                if p.is_finished() {
                    prop_assert_eq!(p.time_ms(), p.duration_ms());
                }
            }
        }
    }
}
