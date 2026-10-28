//! Sound, voices and music; no game rules.
//!
//! - [`AudioSystem`]: the entry point. Opens the default output device via
//!   `kira`; without one (CI, containers) it runs **silently**: every call is
//!   accepted and does nothing. A missing sound file is logged once and skipped.
//! - [`bank`]: the sound bank (`assets/audio/bank.ron`): named sounds with
//!   file, base volume, pitch variation, voice limit and category.
//! - [`spatial`]: pure volume/pan maths for positional sound relative to the
//!   camera, including "quieter when zoomed out".
//! - [`config`]: `assets/config/audio.ron`, every tunable of the audio.
//! - [`positional`]: which in-game sounds play (merge, space, limit).
//! - [`voices`]: voice limiting.
//!
//! - [`ra`]: optional original Red Alert audio (WP11.7): MIX/AUD readers,
//!   the event mapping and the sound-browser catalog, loaded on a
//!   background thread by [`AudioSystem::enable_original_audio`].
//!
//! Music crossfades between tracks; positional sounds use `play_at` with a
//! [`Listener`] derived from the camera each frame.

pub mod bank;
pub mod config;
mod live;
pub mod positional;
pub mod ra;
pub mod spatial;
mod system;
pub mod voices;

pub use bank::{Bank, BankError, Category, SoundDef};
pub use config::AudioConfig;
pub use positional::{Cue, PlanStats, Planner, Request};
pub use spatial::{Listener, Mix, SpatialParams};
pub use system::{AudioSystem, BANK_FILE, DEFAULT_CROSSFADE, PlayTrace, RaStatus, Stats};
