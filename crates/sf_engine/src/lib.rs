//! Main loop, window, input, time and assets; no game rules.
//!
//! - [`time`]: pure fixed-step accumulator (20 Hz sim) and frame-time stats.
//! - [`input`]: raw input -> [`input::InputState`] -> named actions via RON bindings.
//! - [`assets`]: asset root resolution and debug-only RON hot-reload.
//! - [`app`]: the winit window and loop driving the [`App`] trait.

pub mod app;
pub mod assets;
pub mod input;
pub mod time;

pub use app::{App, Context, EngineConfig, Resized, ScreenMode, ScreenReport, run};
pub use assets::AssetRoot;
pub use input::{Action, ActionState, Bindings, InputState};
pub use time::{FixedStep, FrameStats, FrameSummary, SIM_HZ, SIM_STEP};
pub use winit;
