//! The AI itself: its state, the think schedule and one think.
//!
//! The AI is a deterministic player outside the simulation: a pure function of (the observation at tick
//! `t`, its own state, its seed). It acts only through the commands a human can send, which the host
//! schedules, so they are in the command log and the world checksum like any other. It follows the
//! simulation's determinism rules (no floats, no hash collections, no clock, no I/O, randomness from
//! `sf_math::Rng` only), enforced by `scripts/check-sim-determinism.sh`, `clippy.toml` and the float lints
//! of `Cargo.toml`.
//!
//! WP9.1a is the frame: the AI observes, remembers and keeps its schedule, and sends no command yet.

use sf_sim::{Cell, Command, PlayerId, Tick};

use crate::memory;
use crate::observe::Observation;
use crate::profile::Profile;
use crate::schedule::{self, Manager};
use crate::state::AiState;

/// One computer opponent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ai {
    state: AiState,
}

impl Ai {
    /// An AI for `player` with `profile`, its random stream seeded by `seed`, told where each player
    /// starts (`start_sites`, see [`crate::sites::start_sites`]).
    pub fn new(
        player: PlayerId,
        profile: Profile,
        seed: u64,
        start_sites: Vec<(PlayerId, Cell)>,
    ) -> Ai {
        Ai {
            state: AiState::new(player, profile, seed, start_sites),
        }
    }

    /// The player it plays.
    pub fn player(&self) -> PlayerId {
        self.state.player
    }

    /// Its state, read-only.
    pub fn state(&self) -> &AiState {
        &self.state
    }

    /// Whether it thinks at `tick` (the number of ticks the game has run so far).
    pub fn due(&self, tick: Tick) -> bool {
        schedule::due(
            tick,
            self.state.player,
            self.state.profile.think_interval,
            self.state.profile.stagger,
        )
    }

    /// A hash of its whole state; see [`AiState::digest`].
    pub fn digest(&self) -> u64 {
        self.state.digest()
    }

    /// One think from an observation: update the memory, run the manager whose turn it is, count the think.
    /// The commands go onto `out`; the managers send none yet.
    pub fn think_with(&mut self, obs: &Observation, out: &mut Vec<Command>) {
        memory::update(&mut self.state.known_structures, obs);
        let manager = Manager::of_think(self.state.thinks);
        self.state.manager_runs[manager as usize] += 1;
        run_manager(manager, obs, out);
        self.state.thinks += 1;
        self.state.last_think = Some(obs.tick);
    }
}

/// Runs one manager. The managers are the later parts: economy and construction in WP9.1b, production and
/// army in WP9.1c. Until then a manager's turn only counts.
fn run_manager(_manager: Manager, _obs: &Observation, _out: &mut Vec<Command>) {}
