//! The outcome of a match (WP7.6, `docs/rules/match.md`): who is defeated and who wins.
//!
//! A world given match rules ([`World::with_match`]) knows which players take part. At the end of a
//! tick, after the dead have been removed ([`World::reap`]), it asks of each of them one question:
//! does this player still own **a standing structure** or **an armed unit**? A player for whom the
//! answer is no is **defeated**, at that tick, for good. When all but one of the players are defeated
//! the last one **wins**, at that tick, for good. When the last players fall in the same tick nobody
//! wins: the match is a **draw**. Once the result is decided nothing in this module changes any more,
//! whatever happens in the world afterwards (a defeated player's harvester may still be driving).
//!
//! **What counts.** A structure counts when it stands ([`World::stands`]: complete, with hit points
//! left): a building under construction is not yet something to lose. A mobile unit counts when its
//! kind carries a weapon in the weapon table and it has hit points left. A harvester does not count:
//! it cannot fight, and it cannot build, so a player with only harvesters can do nothing that changes
//! the match. (Building needs a standing structure of one's own, `docs/rules/construction.md`.) A
//! crate, an ore field and a neutral object count for nobody: only the players taking part are
//! looked at.
//!
//! **The match begins** at the first check at which every player owns something that counts. Until
//! then nobody can be defeated: the starting objects of a match enter the world through commands, and
//! a world whose second player appears at tick 5 must not end at tick 0. A world in which some player
//! never owns anything has no result.
//!
//! **Cost.** The question is asked only after something was removed from the world (the `dirty` flag,
//! set by [`World::despawn`], the one place an entity leaves) and, before the match began, at every
//! tick. The scan walks the slots in ascending order and stops as soon as every player still in the
//! match has been found once, so a tick without a removal costs one flag test. Only removal can change
//! the answer once the match began: a structure that stops standing and a unit that stops having hit
//! points are removed by the same tick (destruction is required of a world with match rules), and a
//! weapon table never changes.
//!
//! **Order and determinism.** Players are kept ascending. The scan reads the columns in slot order and
//! writes one flag per player; nothing depends on the order the entities were made in. Integers only.

use sf_math::hash::Fnv1a64;

use crate::entities::NO_DEF;
use crate::ids::{PlayerId, Tick};
use crate::units::UnitClass;
use crate::world::World;

/// The most players a match can have.
pub const MAX_MATCH_PLAYERS: usize = 8;

/// Written first into the hash of the section; bump when the layout changes.
const SECTION_TAG: &[u8] = b"sf_sim/outcome/v1";

/// Which players take part in the match.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchRules {
    players: Vec<PlayerId>,
}

/// Why match rules were refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MatchError {
    /// Fewer than two players, or more than [`MAX_MATCH_PLAYERS`].
    PlayerCount,
    /// A player is listed twice.
    Duplicate(PlayerId),
}

impl MatchRules {
    /// The players taking part, in any order; at least two and at most [`MAX_MATCH_PLAYERS`], each once.
    pub fn new(players: &[PlayerId]) -> Result<MatchRules, MatchError> {
        if !(2..=MAX_MATCH_PLAYERS).contains(&players.len()) {
            return Err(MatchError::PlayerCount);
        }
        let mut sorted = players.to_vec();
        sorted.sort_unstable();
        if let Some(pair) = sorted.windows(2).find(|w| w[0] == w[1]) {
            return Err(MatchError::Duplicate(pair[0]));
        }
        Ok(MatchRules { players: sorted })
    }

    /// The players taking part, ascending.
    pub fn players(&self) -> &[PlayerId] {
        &self.players
    }
}

/// How a decided match ended.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// All the other players were defeated; this one won.
    Won(PlayerId),
    /// The last players were defeated in the same tick: nobody won.
    Draw,
}

/// The decided result of a match.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct MatchResult {
    /// The tick in which the match was decided: the tick of the last defeat.
    pub tick: Tick,
    /// Who won, or a draw.
    pub verdict: Verdict,
}

/// The state of the outcome: the rules, whether the match began, the defeats and the result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Outcome {
    pub(crate) rules: MatchRules,
    /// Whether every player owned something that counts at some check.
    pub(crate) begun: bool,
    /// An entity left the world since the last check. Not state between steps (a step ends with
    /// the flag clear), so it is not hashed.
    pub(crate) dirty: bool,
    /// The defeats in the order they happened: ascending tick, ascending player within a tick.
    pub(crate) defeats: Vec<(PlayerId, Tick)>,
    pub(crate) result: Option<MatchResult>,
}

impl Outcome {
    pub(crate) fn new(rules: MatchRules) -> Outcome {
        Outcome {
            rules,
            begun: false,
            dirty: false,
            defeats: Vec::new(),
            result: None,
        }
    }

    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_bytes(SECTION_TAG);
        h.write_u64(self.rules.players.len() as u64);
        for p in &self.rules.players {
            h.write_u8(p.0);
        }
        h.write_u8(u8::from(self.begun));
        h.write_u64(self.defeats.len() as u64);
        for &(p, t) in &self.defeats {
            h.write_u8(p.0);
            h.write_u64(t.0);
        }
        match self.result {
            None => h.write_u8(0),
            Some(MatchResult {
                tick,
                verdict: Verdict::Won(p),
            }) => {
                h.write_u8(1);
                h.write_u64(tick.0);
                h.write_u8(p.0);
            }
            Some(MatchResult {
                tick,
                verdict: Verdict::Draw,
            }) => {
                h.write_u8(2);
                h.write_u64(tick.0);
            }
        }
    }
}

impl World {
    /// The same world with match rules (WP7.6): the players taking part, and with them the rule that
    /// decides who is defeated and who wins ([`crate::outcome`]). Give it last, after the unit table,
    /// the weapon table and the destruction settings (a world in which nothing is destroyed or
    /// nothing is armed has no match to decide): [`ConfigError::Match`] after the world has started,
    /// twice, without those, or for a player list [`MatchRules::new`] refuses.
    pub fn with_match(mut self, rules: MatchRules) -> Result<World, crate::world::ConfigError> {
        if self.tick != Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || self.outcome.is_some()
            || self.units.is_empty()
            || self.arsenal.is_empty()
            || self.destruction.config.is_none()
        {
            return Err(crate::world::ConfigError::Match);
        }
        self.outcome = Some(Box::new(Outcome::new(rules)));
        Ok(self)
    }

    /// The players taking part in the match; empty in a world without match rules.
    pub fn match_players(&self) -> &[PlayerId] {
        self.outcome.as_ref().map_or(&[], |o| o.rules.players())
    }

    /// Whether the match has begun: every player owned something that counts at some check.
    pub fn match_begun(&self) -> bool {
        self.outcome.as_ref().is_some_and(|o| o.begun)
    }

    /// The tick in which `player` was defeated; `None` if not (yet) or not taking part.
    pub fn defeat_tick(&self, player: PlayerId) -> Option<Tick> {
        self.outcome
            .as_ref()?
            .defeats
            .iter()
            .find(|d| d.0 == player)
            .map(|d| d.1)
    }

    /// The defeats so far, in the order they happened.
    pub fn defeats(&self) -> &[(PlayerId, Tick)] {
        self.outcome.as_ref().map_or(&[], |o| &o.defeats)
    }

    /// The result, once decided; `None` before, and in a world without match rules.
    pub fn match_result(&self) -> Option<MatchResult> {
        self.outcome.as_ref()?.result
    }

    /// Whether the entity in slot `i` is something that keeps its owner in the match: a standing
    /// structure, or an armed mobile unit with hit points left.
    fn keeps_in_match(&self, i: usize) -> bool {
        let Some(stats) = self.stats_at(i) else {
            return false;
        };
        match stats.class() {
            UnitClass::Structure => self.is_standing(i),
            UnitClass::Mobile => {
                self.arsenal.is_armed(self.entities.def[i]) && self.has_hit_points(i)
            }
            UnitClass::Object => false,
        }
    }

    /// The end of a tick (WP7.6): decides defeats and the win. Cheap when nothing was removed.
    pub(crate) fn evaluate_match(&mut self, tick: Tick) {
        let Some(mut outcome) = self.outcome.take() else {
            return;
        };
        if outcome.result.is_none() && (outcome.dirty || !outcome.begun) {
            outcome.dirty = false;
            self.decide(&mut outcome, tick);
        }
        outcome.dirty = false;
        self.outcome = Some(outcome);
    }

    fn decide(&self, outcome: &mut Outcome, tick: Tick) {
        let players = &outcome.rules.players;
        // Who still owns something that counts: a flag per player, set while walking the slots
        // in ascending order, which stops once every player in the match has been seen.
        let mut found = [false; MAX_MATCH_PLAYERS];
        let mut wanted = players.len();
        for p in 0..players.len() {
            if outcome.defeats.iter().any(|d| d.0 == players[p]) {
                // A defeated player is not looked for.
                found[p] = true;
                wanted -= 1;
            }
        }
        let mut seen = 0;
        if wanted > 0 {
            'slots: for i in 0..self.entities.len() {
                if !self.entities.alive[i] || self.entities.def[i] == NO_DEF {
                    continue;
                }
                let Ok(p) = players.binary_search(&self.entities.owner[i]) else {
                    continue;
                };
                if !found[p] && self.keeps_in_match(i) {
                    found[p] = true;
                    seen += 1;
                    if seen == wanted {
                        break 'slots;
                    }
                }
            }
        }
        if !outcome.begun {
            // The match begins when every player owns something that counts, all at once.
            if seen == wanted {
                outcome.begun = true;
            } else {
                return;
            }
        }
        let mut remaining = 0;
        let mut last = None;
        for (p, &player) in players.iter().enumerate() {
            let defeated_before = outcome.defeats.iter().any(|d| d.0 == player);
            if defeated_before {
                continue;
            }
            if found[p] {
                remaining += 1;
                last = Some(player);
            } else {
                outcome.defeats.push((player, tick));
            }
        }
        match (remaining, last) {
            (1, Some(winner)) => {
                outcome.result = Some(MatchResult {
                    tick,
                    verdict: Verdict::Won(winner),
                });
            }
            (0, _) => {
                outcome.result = Some(MatchResult {
                    tick,
                    verdict: Verdict::Draw,
                });
            }
            _ => {}
        }
    }
}
