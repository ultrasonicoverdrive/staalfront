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
//! * WP9.1b-1: [`observe::Lens`] (the second fenced type: public rules, terrain, visibility and the
//!   simulation's placement rule for visible ground), [`budget`] (free credits), [`base`] (the construction
//!   manager: goal list, power rule, money rule) and [`placement`] (where to build).
//! * WP9.1b-2: [`economy`] (harvesters, the ore silo, repair, the rescue sale). The commands it sends are
//!   `BuildStructure`, `QueueUnit`, `Repair` and `Sell`.
//! * WP9.1c-1: [`production`] (which units, the rally point, the money rule) and [`army`] (the gather cell, the
//!   scout, gathering; no attacks yet). New commands: `SetRally` and `Order { Move }`.

pub mod ai;
pub mod army;
pub mod base;
pub mod budget;
pub mod economy;
pub mod memory;
pub mod observe;
pub mod placement;
pub mod production;
pub mod profile;
pub mod schedule;
pub mod sites;
pub mod state;

pub use ai::Ai;
pub use observe::{Lens, Observation, Own, OwnFactory, Seen};
pub use profile::Profile;
pub use schedule::{Manager, due};
pub use sites::{make_ai, start_sites};
pub use state::{AI_STREAM, AiState, Known, REFUSED_CAP};
