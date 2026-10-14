//! Sound, voices and music; no game rules.
//!
//! - [`AudioSystem`]: the entry point. Opens the default output device via
//!   `kira`; without one (CI, containers) it runs **silently**: every call is
//!   accepted and does nothing. A missing sound file is logged once and skipped.
//! - [`bank`]: the sound bank (`assets/audio/bank.ron`): named sounds with
//!   file, base volume, pitch variation, voice limit and category.
//! - [`spatial`]: pure volume/pan maths for positional sound relative to the
//!   camera, including "quieter when zoomed out".
//! - [`voices`]: voice limiting.
//!
//! Music crossfades between tracks; positional sounds use `play_at` with a
//! [`Listener`] derived from the camera each frame.

pub mod bank;
mod live;
pub mod spatial;
mod system;
pub mod voices;

pub use bank::{Bank, BankError, Category, SoundDef};
pub use spatial::{Listener, Mix, SpatialParams};
pub use system::{AudioSystem, BANK_FILE, DEFAULT_CROSSFADE, Stats};
