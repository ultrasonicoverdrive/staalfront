//! The state of one AI: plain data (integers, vectors, ordered maps), so that it can be hashed
//! ([`AiState::digest`]) and, later, saved with the game (WP13.1). No closures, no trait objects, no hash
//! collections.

use std::collections::BTreeMap;

use sf_math::Rng;
use sf_math::hash::Fnv1a64;
use sf_sim::{Cell, EntityId, Kind, PlayerId, Tick};

use crate::profile::Profile;
use crate::schedule::Manager;

/// The stream of the AI's random numbers: the world seed (or the seed of the scenario's `ai` entry) and
/// the player number pick the sequence, so two AIs never share one.
pub const AI_STREAM: u64 = 0xA1_0000;

/// A structure of another player the AI has seen and remembers: where it stood, and when it was last seen
/// standing there. Written at every think for each such structure that is visible; dropped when its cells
/// are visible and it is not there (the ghost rule of `docs/rules/fog_look.md`, on the AI's side).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Known {
    /// What it is.
    pub kind: Kind,
    /// Who owns it.
    pub owner: PlayerId,
    /// The corner cell of its footprint with the smallest x and y.
    pub anchor: Cell,
    /// Cells along x.
    pub width: u8,
    /// Cells along y.
    pub height: u8,
    /// The tick of the think that last saw it.
    pub seen_tick: Tick,
}

/// Everything an AI remembers between thinks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiState {
    /// The player it plays.
    pub player: PlayerId,
    /// Its random numbers (`sf_math::Rng`, stream [`AI_STREAM`] plus the player).
    pub rng: Rng,
    /// What it builds and makes, and the numbers of its rules.
    pub profile: Profile,
    /// The cell of every player's construction centre at the start: map knowledge the host hands over.
    pub start_sites: Vec<(PlayerId, Cell)>,
    /// How many times it has thought.
    pub thinks: u64,
    /// The tick of its last think.
    pub last_think: Option<Tick>,
    /// How many times each manager has run, indexed by `Manager as usize`.
    pub manager_runs: [u32; Manager::ALL.len()],
    /// The structures of others it has seen and not seen gone.
    pub known_structures: BTreeMap<EntityId, Known>,
    /// WP9.1b-1, the base: the build it sent at the last construction turn, `(kind, anchor cell, tick)`,
    /// until the next turn checks that it stands there (`docs/rules/ai.md`, "The construction turn").
    pub pending_build: Option<(Kind, Cell, Tick)>,
    /// A kind for which no place was found, and the tick from which it is tried again.
    pub blocked_until: BTreeMap<Kind, Tick>,
    /// `(kind, anchor cell)` the simulation refused (a preview that said no, or a sent build that does
    /// not stand), and the tick from which it may be tried again; at most [`REFUSED_CAP`] entries.
    pub refused: BTreeMap<(Kind, Cell), Tick>,
    /// The ore ground near the base (terrain: map knowledge), ascending by (y, x); listed at the first
    /// construction turn with an anchor, `None` before.
    pub ore_cells: Option<Vec<Cell>>,
    /// Placement questions passed on to the simulation in all its thinks.
    pub previews: u64,
}

/// The most entries of [`AiState::refused`]; the one that expires first goes when it is full.
pub const REFUSED_CAP: usize = 64;

impl AiState {
    /// A fresh state for `player` with the random stream of `seed`.
    pub fn new(
        player: PlayerId,
        profile: Profile,
        seed: u64,
        start_sites: Vec<(PlayerId, Cell)>,
    ) -> AiState {
        AiState {
            player,
            rng: Rng::with_stream(seed, AI_STREAM + u64::from(player.0)),
            profile,
            start_sites,
            thinks: 0,
            last_think: None,
            manager_runs: [0; Manager::ALL.len()],
            known_structures: BTreeMap::new(),
            pending_build: None,
            blocked_until: BTreeMap::new(),
            refused: BTreeMap::new(),
            ore_cells: None,
            previews: 0,
        }
    }

    /// A hash of the whole state, in a fixed order. The AI is not part of the world checksum (it lives
    /// outside the simulation), so `sf_headless` prints this beside it: an AI that diverges shows up here.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv1a64::new();
        h.write_u8(self.player.0);
        h.write_u64(self.rng.state().state);
        h.write_u64(self.rng.state().inc);
        h.write_u64(self.profile.digest());
        h.write_u32(self.start_sites.len() as u32);
        for (p, c) in &self.start_sites {
            h.write_u8(p.0);
            h.write_u32(c.x);
            h.write_u32(c.y);
        }
        h.write_u64(self.thinks);
        match self.last_think {
            Some(t) => {
                h.write_u8(1);
                h.write_u64(t.0);
            }
            None => h.write_u8(0),
        }
        for n in self.manager_runs {
            h.write_u32(n);
        }
        h.write_u32(self.known_structures.len() as u32);
        for (id, k) in &self.known_structures {
            h.write_u32(id.index());
            h.write_u32(id.generation());
            h.write_u32(u32::from(k.kind.0));
            h.write_u8(k.owner.0);
            h.write_u32(k.anchor.x);
            h.write_u32(k.anchor.y);
            h.write_u8(k.width);
            h.write_u8(k.height);
            h.write_u64(k.seen_tick.0);
        }
        // WP9.1b-1: the base.
        match self.pending_build {
            Some((kind, cell, tick)) => {
                h.write_u8(1);
                h.write_u32(u32::from(kind.0));
                h.write_u32(cell.x);
                h.write_u32(cell.y);
                h.write_u64(tick.0);
            }
            None => h.write_u8(0),
        }
        h.write_u32(self.blocked_until.len() as u32);
        for (kind, until) in &self.blocked_until {
            h.write_u32(u32::from(kind.0));
            h.write_u64(until.0);
        }
        h.write_u32(self.refused.len() as u32);
        for ((kind, cell), until) in &self.refused {
            h.write_u32(u32::from(kind.0));
            h.write_u32(cell.x);
            h.write_u32(cell.y);
            h.write_u64(until.0);
        }
        match &self.ore_cells {
            Some(cells) => {
                h.write_u8(1);
                h.write_u32(cells.len() as u32);
                for c in cells {
                    h.write_u32(c.x);
                    h.write_u32(c.y);
                }
            }
            None => h.write_u8(0),
        }
        h.write_u64(self.previews);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::refuse;
    use sf_data::ai_profile::AiProfile;
    use sf_data::units::{Faction, UnitDefs};

    fn state() -> AiState {
        let profile = Profile::resolve(
            &AiProfile::embedded(),
            Faction::Meridian,
            &UnitDefs::embedded(),
        )
        .unwrap();
        AiState::new(
            PlayerId(0),
            profile,
            5,
            vec![(PlayerId(1), Cell::new(70, 10))],
        )
    }

    /// A state with two entries in each map of WP9.1b-1 and the other new parts set.
    fn base_state() -> AiState {
        let mut s = state();
        s.pending_build = Some((Kind(2), Cell::new(17, 64), Tick(250)));
        s.blocked_until.insert(Kind(2), Tick(800));
        s.blocked_until.insert(Kind(4), Tick(900));
        s.refused.insert((Kind(1), Cell::new(9, 68)), Tick(610));
        s.refused.insert((Kind(1), Cell::new(9, 72)), Tick(650));
        s.ore_cells = Some(vec![Cell::new(24, 66), Cell::new(25, 66)]);
        s.previews = 7;
        s
    }

    #[test]
    fn the_digest_of_the_base_state_is_pinned() {
        // A literal: a change of the layout, or an iteration of a map in another order, fails here. It hashes
        // the shipped profile too, so a change of assets/ai/normal.ron moves it as well.
        assert_eq!(base_state().digest(), 0x188f_d194_100a_7ff4);
    }

    #[test]
    fn every_new_part_of_the_state_moves_the_digest() {
        let d = base_state().digest();
        let changes: [fn(&mut AiState); 7] = [
            |s| s.pending_build = None,
            |s| s.pending_build = Some((Kind(2), Cell::new(17, 65), Tick(250))),
            |s| {
                s.blocked_until.insert(Kind(4), Tick(901));
            },
            |s| {
                s.refused.remove(&(Kind(1), Cell::new(9, 72)));
            },
            |s| s.ore_cells = None,
            |s| s.ore_cells = Some(vec![Cell::new(24, 66)]),
            |s| s.previews += 1,
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut s = base_state();
            change(&mut s);
            assert_ne!(s.digest(), d, "change {i}");
        }
    }

    #[test]
    fn the_refusals_are_capped_and_the_first_to_expire_goes() {
        let mut s = state();
        for i in 0..REFUSED_CAP as u32 {
            refuse(&mut s, Kind(1), Cell::new(i, 0), Tick(1000 + u64::from(i)));
        }
        assert_eq!(s.refused.len(), REFUSED_CAP);
        refuse(&mut s, Kind(1), Cell::new(99, 0), Tick(5000));
        assert_eq!(s.refused.len(), REFUSED_CAP);
        // The one at tick 1000 is gone, the new one is in.
        assert!(!s.refused.contains_key(&(Kind(1), Cell::new(0, 0))));
        assert!(s.refused.contains_key(&(Kind(1), Cell::new(99, 0))));
        assert_eq!(REFUSED_CAP, 64);
    }
}
