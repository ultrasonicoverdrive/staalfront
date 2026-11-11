//! Computer opponent: reads the game through a fog-filtered observation and acts only through commands.
//!
//! A deterministic player outside `sf_sim` (design memo 10-11-2026, `docs/rules/ai.md`):
//!
//! * [`observe`] is the **fence**: the only file that names the simulation's world. It builds an
//!   [`Observation`] with the player's own things in full and of others only what is visible, and the rest
//!   of the crate takes only that. `tests/fence.rs` fails when another file names the world.
//! * [`memory`] keeps the structures the AI has seen (`AiState::known_structures`) and drops the ones it
//!   sees gone.
//! * [`state`] is the AI's plain-data state with a [`digest`](AiState::digest), [`schedule`] says when it
//!   thinks (every `think_interval` ticks, staggered per player, one manager per think), [`profile`] is the
//!   build order and numbers of `assets/ai/<profile>.ron`, [`sites`] the start sites the host hands over.
//! * [`Ai`] ties them together: `Ai::due(tick)` and `Ai::think(&world, &mut commands)`, called by the host
//!   (`sf_headless` today, the game's sim thread in WP9.1d) right before the world's step, which schedules
//!   the commands under the AI's player id.
//!
//! WP9.1a is the frame only: the AI observes and remembers, and sends no command.

pub mod ai;
pub mod memory;
pub mod observe;
pub mod profile;
pub mod schedule;
pub mod sites;
pub mod state;

pub use ai::Ai;
pub use observe::{Observation, Own, Seen};
pub use profile::Profile;
pub use schedule::{Manager, due};
pub use sites::start_sites;
pub use state::{AI_STREAM, AiState, Known};
