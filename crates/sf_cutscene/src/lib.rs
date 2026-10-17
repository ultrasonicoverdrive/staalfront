//! Cartoon cutscene script format (WP12.1).
//!
//! A scene is a RON file describing a cutout cartoon: background layers with
//! parallax, characters made of named parts and poses, and a timeline of
//! tracks with events at integer millisecond times (movement, pose changes,
//! dialogue, sound, camera, transitions, the briefing map). This crate defines
//! and checks that format, and evaluates it **without drawing anything**:
//!
//! * [`model`]: the serde types ([`Scene`] and what it contains).
//! * [`load`]: [`load_str`] / [`load_file`], with line-numbered errors.
//! * [`validate`]: reference, time and overlap checks; the list of missing
//!   asset files (placeholders are allowed, so that list is not an error).
//! * [`sample`]: [`Scene::sample`] gives the frame at a time (poses blended and
//!   eased, camera, subtitle, fade) and [`Scene::sample_step`] adds the sound
//!   events that start in `(prev, t]`, exactly once each.
//! * [`fountain`]: turns a Fountain briefing (`content/briefings/*.fountain`)
//!   into a skeleton scene; the `fountain2scene` binary wraps it.
//!
//! WP12.2 adds the player:
//!
//! * [`player`]: [`Player`], a state machine driven by the caller's wall-clock
//!   milliseconds (play, pause, seek, skip, next shot); sound goes out through
//!   the [`SoundSink`] trait, restored correctly after a seek.
//! * [`controls`]: [`Key`], [`Action`] and [`Bindings`] (Esc/Space/click skip,
//!   a configurable next-shot key).
//! * [`render`]: [`render::CutsceneRenderer`], the wgpu renderer for a
//!   [`SceneState`], built on the public API of `sf_render` and `sf_ui`.
//! * the `play_scene` binary: a window to preview a scene or a Fountain file.
//!
//! WP12.3 adds lip-sync:
//!
//! * [`lipsync`]: the nine mouth shapes, readers for Rhubarb's JSON and TSV
//!   output into an integer-millisecond series ([`MouthCues`]), and the "flap"
//!   fallback made from the text. [`CharacterState::mouth`] reports the shape
//!   per character; a rig's `mouth` part draws the matching art variant.
//! * [`rhubarb`]: finding and running the external `rhubarb` executable, for
//!   the `lipsync` binary.
//!
//! WP12.6 adds the live preview (`play_scene --watch`):
//!
//! * [`watch`]: polling for changed files with a debounce ([`watch::SceneWatcher`]).
//! * [`preview`]: the reload state machine ([`preview::Preview`]: last good
//!   scene, error list, playhead kept) and the writer's HUD text.
//!
//! The writer's and artist's description of the format and of the player is
//! `docs/cutscene-format.md`.
//!
//! This is presentation code, not simulation: it uses floats for geometry, but
//! all time is integer milliseconds so that tests are reproducible. `sf_sim`
//! does not depend on this crate.

pub mod controls;
pub mod fountain;
pub mod lipsync;
pub mod load;
pub mod math;
pub mod model;
pub mod player;
pub mod preview;
pub mod render;
pub mod rhubarb;
pub mod sample;
pub mod span;
pub mod validate;
pub mod watch;

pub use controls::{Action, Bindings, Key};
pub use lipsync::{MouthCue, MouthCueSet, MouthCues, MouthShape};
pub use load::{LoadedScene, SceneError, load_file, load_str, parse_str};
pub use math::{Affine, Color, Easing, Ms, Vec2};
pub use model::{
    Art, Character, Delivery, Event, EventKind, Facing, Layer, LoopAction, MapMarker, Part,
    PartPose, Pose, Scene, Stage, Timeline, Track, TransitionKind,
};
pub use player::{Bed, MAX_STEP_MS, Mode, NullSink, Player, RecordingSink, SoundEvent, SoundSink};
pub use sample::{
    CameraState, CharacterState, Fade, FiredEvent, LayerState, LoopState, MapOverlayState,
    PartState, PartXform, RunningVoice, SceneState, Subtitle,
};
pub use validate::{Issue, MissingAsset, Validation, missing_assets, validate};
