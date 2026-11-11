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

#[cfg(test)]
mod tests {
    use super::*;
    use sf_data::ai_profile::AiProfile;
    use sf_data::units::{Faction, UnitDefs};

    fn ai(player: u8) -> Ai {
        let profile = Profile::resolve(
            &AiProfile::embedded(),
            Faction::Meridian,
            &UnitDefs::embedded(),
        )
        .unwrap();
        Ai::new(
            PlayerId(player),
            profile,
            5,
            vec![(PlayerId(0), Cell::new(8, 8))],
        )
    }

    fn obs(tick: u64) -> Observation {
        Observation {
            tick: Tick(tick),
            player: PlayerId(0),
            map: (32, 32),
            credits: 0,
            ore_stored: 0,
            storage_capacity: 0,
            power: None,
            own: Vec::new(),
            visible: Vec::new(),
            gone: Vec::new(),
        }
    }

    #[test]
    fn due_follows_the_profile_not_a_constant() {
        let mut a = ai(2);
        // (tick + 3 * 2) % 10 == 0 gives the ticks 4, 14 and 24 here.
        let due: Vec<u64> = (0..30).filter(|&t| a.due(Tick(t))).collect();
        assert_eq!(due, [4, 14, 24]);
        a.state.profile.think_interval = 5;
        a.state.profile.stagger = 1;
        let due: Vec<u64> = (0..16).filter(|&t| a.due(Tick(t))).collect();
        assert_eq!(due, [3, 8, 13]);
    }

    #[test]
    fn a_think_counts_and_rotates_the_managers_and_sends_nothing() {
        let mut a = ai(0);
        let mut out = Vec::new();
        for i in 0..9u64 {
            a.think_with(&obs(i * 10), &mut out);
        }
        assert!(out.is_empty(), "WP9.1a sends no command: {out:?}");
        let s = a.state();
        assert_eq!((s.thinks, s.last_think), (9, Some(Tick(80))));
        // Thinks 0 to 8: Economy 3 times, the others twice.
        assert_eq!(s.manager_runs, [3, 2, 2, 2]);
    }

    #[test]
    fn the_digest_moves_with_every_part_of_the_state() {
        let mut a = ai(0);
        let first = a.digest();
        assert_eq!(first, ai(0).digest());
        // Another player: another digest.
        assert_ne!(first, ai(1).digest());
        let mut out = Vec::new();
        a.think_with(&obs(10), &mut out);
        let one = a.digest();
        assert_ne!(first, one);
        a.think_with(&obs(20), &mut out);
        assert_ne!(one, a.digest());
        // The random stream is part of it.
        let mut b = a.clone();
        b.state.rng.next_u32();
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn two_ais_with_the_same_input_stay_equal() {
        let (mut a, mut b) = (ai(0), ai(0));
        let mut out = Vec::new();
        for i in 0..20u64 {
            a.think_with(&obs(i * 10), &mut out);
            b.think_with(&obs(i * 10), &mut out);
        }
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
    }
}
