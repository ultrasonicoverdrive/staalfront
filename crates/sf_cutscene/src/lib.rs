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
//! The player that draws a [`SceneState`] is WP12.2. The writer's and artist's
//! description of the format is `docs/cutscene-format.md`.
//!
//! This is presentation code, not simulation: it uses floats for geometry, but
//! all time is integer milliseconds so that tests are reproducible. `sf_sim`
//! does not depend on this crate.

pub mod fountain;
pub mod load;
pub mod math;
pub mod model;
pub mod sample;
pub mod span;
pub mod validate;

pub use load::{LoadedScene, SceneError, load_file, load_str, parse_str};
pub use math::{Affine, Color, Easing, Ms, Vec2};
pub use model::{
    Art, Character, Delivery, Event, EventKind, Facing, Layer, LoopAction, MapMarker, Part,
    PartPose, Pose, Scene, Stage, Timeline, Track, TransitionKind,
};
pub use sample::{
    CameraState, CharacterState, Fade, FiredEvent, LayerState, MapOverlayState, PartState,
    PartXform, SceneState, Subtitle,
};
pub use validate::{Issue, MissingAsset, Validation, missing_assets, validate};
