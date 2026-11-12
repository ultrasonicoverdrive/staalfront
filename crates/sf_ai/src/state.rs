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

/// What the army manager wants the army to do (`docs/rules/ai.md`, "The army"): written by the army rules
/// (threat, timing, target) and turned into orders by `army::operations_minimal`, the seam that WP9.2 replaces.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct Directive {
    /// What the army does.
    pub posture: Posture,
    /// Where it does it: the attack's target cell, the cell beside the structure that was hit; `None` while
    /// gathering.
    pub target: Option<Cell>,
}

/// The army's posture. The numbers are hashed, so they are permanent.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Posture {
    /// Units gather at the gather cell and wait.
    #[default]
    Gather = 0,
    /// WP9.1c-2: the attackers march on the target (the units at home keep gathering).
    Attack = 1,
    /// WP9.1c-2: a structure was hit; the units at home go there.
    Defend = 2,
}

/// An own structure as the threat rule last saw it: `(kind, anchor cell, hit points)`.
pub type HpSeen = (Kind, Cell, u32);

/// A unit order the production turn sent and has not yet seen take effect: `(factory, kind, tick, how many
/// units of the kind the player had before the order, how many items of the kind that factory had queued
/// before it)`.
pub type UnitSent = (EntityId, Kind, Tick, u32, u32);

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
    /// Per kind, how many candidates the next search skips: the ones the last search asked about without
    /// a yes (`docs/rules/ai.md`, "Placement", the resumption).
    pub resume: BTreeMap<Kind, u32>,
    /// Placement questions passed on to the simulation in all its thinks.
    pub previews: u64,
    /// WP9.1b-2, the economy: the harvester sent at the last economy turn, `(kind, tick)`, until the next
    /// turn sees it in a queue (`docs/rules/ai.md`, "The economy turn").
    pub harvester_sent: Option<(Kind, Tick)>,
    /// How many economy turns in a row the ore store was at least `silo_percent` full.
    pub full_turns: u32,
    /// A structure the economy turn wants built before the goal list: the ore silo.
    pub emergency: Option<Kind>,
    /// A structure a repair was sent for, and the tick until which it is not asked again unless it is
    /// seen repairing; entries go when the structure is repaired, gone, repairing or the time is up.
    pub repair_backoff: BTreeMap<EntityId, Tick>,
    /// WP9.1b-2, the rescue (decision of 12-11-2026): how many economy turns in a row an emergency build has
    /// been stalled for lack of money with no income possible.
    pub stalled_turns: u32,
    /// Whether the one rescue of the match (selling structures to pay a stalled emergency build) is spent.
    pub rescue_used: bool,
    /// The sales sent at the last economy turn and the tick they were sent, until the next turn sees them
    /// start (or be refused).
    pub sales: Vec<(EntityId, Tick)>,
    /// After a refused rescue: the tick from which it may be tried again.
    pub rescue_after: Tick,
    /// WP9.1c-1, the gather cell (`docs/rules/ai.md`, "The gather cell"): computed once, again only when
    /// it is `None`, its cell stops being open ground or it lies under a structure the AI knows.
    pub gather: Option<Cell>,
    /// A factory a rally point was sent for, and the tick until which it is not asked again unless the
    /// observation shows another rally point; entries go when the rally point is right, the factory is gone
    /// or the time is up.
    pub rally_backoff: BTreeMap<EntityId, Tick>,
    /// The unit orders sent at the last production turn, ascending by factory id, until the next turn sees
    /// whether the player has more of the kind (`docs/rules/ai.md`, "Production").
    pub units_sent: Vec<UnitSent>,
    /// The scout, once a unit of the scout kind has been seen.
    pub scout: Option<EntityId>,
    /// Which waypoint the scout is on (an index into the legs, see `army::waypoints`).
    pub scout_leg: u8,
    /// The tick the scout was sent to its current leg; `None` until it is.
    pub scout_leg_sent: Option<Tick>,
    /// How many scouts were ordered in the match (a refused order is taken back).
    pub scouts_sent: u8,
    /// The tick the AI saw that its scout was gone (dead, or lost in a queue); `None` while it has one.
    pub scout_gone_at: Option<Tick>,
    /// WP9.1c-2, the threat rule (`docs/rules/ai.md`, "Threat"): every own structure that is not being sold,
    /// as the last think saw it; refreshed at every think.
    pub hp_seen: BTreeMap<EntityId, HpSeen>,
    /// The cell to defend (beside the structure hit last) and the tick of the last hit; `None` when there is
    /// nothing to defend.
    pub threat: Option<(Cell, Tick)>,
    /// The tick of the first hit of the defence going on; a unit not ordered since is ordered once.
    pub defend_since: Option<Tick>,
    /// What the army does now (above).
    pub directive: Directive,
    /// The units sent on the attack going on, ascending by id; pruned to the live ones every army turn.
    pub attackers: Vec<EntityId>,
    /// The target cell of the attack going on.
    pub attack_target: Option<Cell>,
    /// The tick the last attack went out (0 before the first).
    pub last_attack_tick: Tick,
    /// The tick the attack going on went out, or the last army turn an attacker stood within `arrive_radius`
    /// of its target: `attack_patience` counts from here.
    pub attack_started: Tick,
    /// How many attacks went out in the match.
    pub attacks_launched: u32,
    /// Fix round 1: the structure the attackers at the target finish, its hit points when last seen to drop
    /// and that tick; the attack ends when they have not dropped for `attack_patience`.
    pub target_hp: Option<(EntityId, u32, Tick)>,
    /// Fix round 1: former attackers called back by a hit on the construction centre that have not had the
    /// defence order yet, ascending; they get it whatever their order is.
    pub recalled: Vec<EntityId>,
    /// Fix round 2: attackers at the target that saw no enemy structure and got the one `AttackMove` onto the
    /// middle of the target structure (or start site) of this attack, ascending; cleared when the attack ends.
    pub closing: Vec<EntityId>,
    /// The other players whose start site the AI has seen empty (no construction centre on its cell),
    /// ascending.
    pub sites_cleared: Vec<PlayerId>,
    /// Per army unit: the tick of the last order (gather, attack or defence) and how many gather orders it got
    /// without arriving; pruned to the live units every army turn.
    pub sent: BTreeMap<EntityId, (Tick, u8)>,
}

fn write_cell(h: &mut Fnv1a64, c: Option<Cell>) {
    match c {
        Some(c) => {
            h.write_u8(1);
            h.write_u32(c.x);
            h.write_u32(c.y);
        }
        None => h.write_u8(0),
    }
}

fn write_tick(h: &mut Fnv1a64, t: Option<Tick>) {
    match t {
        Some(t) => {
            h.write_u8(1);
            h.write_u64(t.0);
        }
        None => h.write_u8(0),
    }
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
            resume: BTreeMap::new(),
            previews: 0,
            harvester_sent: None,
            full_turns: 0,
            emergency: None,
            repair_backoff: BTreeMap::new(),
            stalled_turns: 0,
            rescue_used: false,
            sales: Vec::new(),
            rescue_after: Tick(0),
            gather: None,
            rally_backoff: BTreeMap::new(),
            units_sent: Vec::new(),
            scout: None,
            scout_leg: 0,
            scout_leg_sent: None,
            scouts_sent: 0,
            scout_gone_at: None,
            hp_seen: BTreeMap::new(),
            threat: None,
            defend_since: None,
            directive: Directive::default(),
            attackers: Vec::new(),
            attack_target: None,
            last_attack_tick: Tick(0),
            attack_started: Tick(0),
            attacks_launched: 0,
            target_hp: None,
            recalled: Vec::new(),
            closing: Vec::new(),
            sites_cleared: Vec::new(),
            sent: BTreeMap::new(),
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
        h.write_u32(self.resume.len() as u32);
        for (kind, n) in &self.resume {
            h.write_u32(u32::from(kind.0));
            h.write_u32(*n);
        }
        h.write_u64(self.previews);
        // WP9.1b-2: the economy.
        match self.harvester_sent {
            Some((kind, tick)) => {
                h.write_u8(1);
                h.write_u32(u32::from(kind.0));
                h.write_u64(tick.0);
            }
            None => h.write_u8(0),
        }
        h.write_u32(self.full_turns);
        match self.emergency {
            Some(kind) => {
                h.write_u8(1);
                h.write_u32(u32::from(kind.0));
            }
            None => h.write_u8(0),
        }
        h.write_u32(self.repair_backoff.len() as u32);
        for (id, until) in &self.repair_backoff {
            h.write_u32(id.index());
            h.write_u32(id.generation());
            h.write_u64(until.0);
        }
        h.write_u32(self.stalled_turns);
        h.write_u8(u8::from(self.rescue_used));
        h.write_u32(self.sales.len() as u32);
        for (id, tick) in &self.sales {
            h.write_u32(id.index());
            h.write_u32(id.generation());
            h.write_u64(tick.0);
        }
        h.write_u64(self.rescue_after.0);
        // WP9.1c-1: production, the scout and the army.
        match self.gather {
            Some(c) => {
                h.write_u8(1);
                h.write_u32(c.x);
                h.write_u32(c.y);
            }
            None => h.write_u8(0),
        }
        h.write_u32(self.rally_backoff.len() as u32);
        for (id, until) in &self.rally_backoff {
            h.write_u32(id.index());
            h.write_u32(id.generation());
            h.write_u64(until.0);
        }
        h.write_u32(self.units_sent.len() as u32);
        for (id, kind, tick, units, queued) in &self.units_sent {
            h.write_u32(id.index());
            h.write_u32(id.generation());
            h.write_u32(u32::from(kind.0));
            h.write_u64(tick.0);
            h.write_u32(*units);
            h.write_u32(*queued);
        }
        match self.scout {
            Some(id) => {
                h.write_u8(1);
                h.write_u32(id.index());
                h.write_u32(id.generation());
            }
            None => h.write_u8(0),
        }
        h.write_u8(self.scout_leg);
        match self.scout_leg_sent {
            Some(t) => {
                h.write_u8(1);
                h.write_u64(t.0);
            }
            None => h.write_u8(0),
        }
        h.write_u8(self.scouts_sent);
        match self.scout_gone_at {
            Some(t) => {
                h.write_u8(1);
                h.write_u64(t.0);
            }
            None => h.write_u8(0),
        }
        // WP9.1c-2: the threat, the posture and the attack.
        h.write_u32(self.hp_seen.len() as u32);
        for (id, (kind, cell, hp)) in &self.hp_seen {
            h.write_u32(id.index());
            h.write_u32(id.generation());
            h.write_u32(u32::from(kind.0));
            h.write_u32(cell.x);
            h.write_u32(cell.y);
            h.write_u32(*hp);
        }
        match self.threat {
            Some((c, t)) => {
                h.write_u8(1);
                h.write_u32(c.x);
                h.write_u32(c.y);
                h.write_u64(t.0);
            }
            None => h.write_u8(0),
        }
        write_tick(&mut h, self.defend_since);
        h.write_u8(self.directive.posture as u8);
        write_cell(&mut h, self.directive.target);
        h.write_u32(self.attackers.len() as u32);
        for id in &self.attackers {
            h.write_u32(id.index());
            h.write_u32(id.generation());
        }
        write_cell(&mut h, self.attack_target);
        h.write_u64(self.last_attack_tick.0);
        h.write_u64(self.attack_started.0);
        h.write_u32(self.attacks_launched);
        match self.target_hp {
            Some((id, hp, t)) => {
                h.write_u8(1);
                h.write_u32(id.index());
                h.write_u32(id.generation());
                h.write_u32(hp);
                h.write_u64(t.0);
            }
            None => h.write_u8(0),
        }
        h.write_u32(self.recalled.len() as u32);
        for id in &self.recalled {
            h.write_u32(id.index());
            h.write_u32(id.generation());
        }
        h.write_u32(self.closing.len() as u32);
        for id in &self.closing {
            h.write_u32(id.index());
            h.write_u32(id.generation());
        }
        h.write_u32(self.sites_cleared.len() as u32);
        for p in &self.sites_cleared {
            h.write_u8(p.0);
        }
        h.write_u32(self.sent.len() as u32);
        for (id, (tick, tries)) in &self.sent {
            h.write_u32(id.index());
            h.write_u32(id.generation());
            h.write_u64(tick.0);
            h.write_u8(*tries);
        }
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
        s.resume.insert(Kind(2), 32);
        s.resume.insert(Kind(4), 64);
        s.previews = 7;
        s.harvester_sent = Some((Kind(11), Tick(240)));
        s.full_turns = 3;
        s.emergency = Some(Kind(43));
        s.repair_backoff.insert(EntityId::new(7, 0), Tick(900));
        s.repair_backoff.insert(EntityId::new(9, 1), Tick(950));
        s.stalled_turns = 2;
        s.rescue_used = true;
        s.sales.push((EntityId::new(4, 0), Tick(300)));
        s.sales.push((EntityId::new(6, 1), Tick(300)));
        s.rescue_after = Tick(1200);
        s.gather = Some(Cell::new(24, 64));
        s.rally_backoff.insert(EntityId::new(5, 0), Tick(1500));
        s.rally_backoff.insert(EntityId::new(8, 1), Tick(1600));
        s.units_sent
            .push((EntityId::new(5, 0), Kind(12), Tick(1400), 3, 1));
        s.units_sent
            .push((EntityId::new(8, 1), Kind(19), Tick(1400), 1, 0));
        s.scout = Some(EntityId::new(11, 0));
        s.scout_leg = 2;
        s.scout_leg_sent = Some(Tick(1390));
        s.scouts_sent = 1;
        s.scout_gone_at = Some(Tick(1380));
        s.hp_seen
            .insert(EntityId::new(0, 0), (Kind(0), Cell::new(12, 68), 2000));
        s.hp_seen
            .insert(EntityId::new(3, 1), (Kind(1), Cell::new(18, 60), 700));
        s.threat = Some((Cell::new(17, 60), Tick(1490)));
        s.defend_since = Some(Tick(1450));
        s.directive.posture = Posture::Defend;
        s.directive.target = Some(Cell::new(60, 20));
        s.attackers.push(EntityId::new(20, 0));
        s.attackers.push(EntityId::new(21, 1));
        s.attack_target = Some(Cell::new(71, 16));
        s.last_attack_tick = Tick(1200);
        s.attack_started = Tick(1300);
        s.attacks_launched = 1;
        s.target_hp = Some((EntityId::new(11, 0), 1500, Tick(1450)));
        s.recalled.push(EntityId::new(22, 0));
        s.recalled.push(EntityId::new(23, 1));
        s.closing.push(EntityId::new(24, 0));
        s.closing.push(EntityId::new(25, 1));
        s.sites_cleared.push(PlayerId(1));
        s.sites_cleared.push(PlayerId(2));
        s.sent.insert(EntityId::new(12, 0), (Tick(1500), 1));
        s.sent.insert(EntityId::new(14, 2), (Tick(1560), 2));
        s
    }

    #[test]
    fn the_digest_of_the_base_state_is_pinned() {
        // A literal: a change of the layout, or an iteration of a map in another order, fails here. It hashes
        // the shipped profile too, so a change of assets/ai/normal.ron moves it as well (WP9.1c-3: 0x392b1f3256aa3c19 ->
        // 0x0dbafeb8f1c44e5c, the goal list and `max_wait`).
        assert_eq!(base_state().digest(), 0x0dba_feb8_f1c4_4e5c);
    }

    #[test]
    fn every_new_part_of_the_state_moves_the_digest() {
        let d = base_state().digest();
        let changes: [fn(&mut AiState); 24] = [
            |s| s.pending_build = None,
            |s| s.pending_build = Some((Kind(2), Cell::new(17, 65), Tick(250))),
            |s| {
                s.blocked_until.insert(Kind(4), Tick(901));
            },
            |s| {
                s.refused.remove(&(Kind(1), Cell::new(9, 72)));
            },
            |s| {
                s.resume.remove(&Kind(2));
            },
            |s| {
                s.resume.insert(Kind(4), 65);
            },
            |s| s.previews += 1,
            |s| s.harvester_sent = None,
            |s| s.harvester_sent = Some((Kind(12), Tick(240))),
            |s| s.harvester_sent = Some((Kind(11), Tick(241))),
            |s| s.full_turns += 1,
            |s| s.emergency = None,
            |s| s.emergency = Some(Kind(44)),
            |s| {
                s.repair_backoff.remove(&EntityId::new(7, 0));
            },
            |s| {
                s.repair_backoff.insert(EntityId::new(9, 1), Tick(951));
            },
            |s| {
                s.repair_backoff.insert(EntityId::new(9, 2), Tick(950));
            },
            |s| s.stalled_turns += 1,
            |s| s.rescue_used = false,
            |s| s.sales.clear(),
            |s| s.sales[0].1 = Tick(301),
            |s| s.sales[1].0 = EntityId::new(6, 2),
            |s| s.sales.reverse(),
            |s| s.sales.push((EntityId::new(7, 0), Tick(300))),
            |s| s.rescue_after = Tick(1201),
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut s = base_state();
            change(&mut s);
            assert_ne!(s.digest(), d, "change {i}");
        }
    }

    #[test]
    fn every_part_of_the_production_and_army_state_moves_the_digest() {
        let d = base_state().digest();
        let changes: [fn(&mut AiState); 25] = [
            |s| s.gather = None,
            |s| s.gather = Some(Cell::new(24, 65)),
            |s| s.gather = Some(Cell::new(25, 64)),
            |s| {
                s.rally_backoff.remove(&EntityId::new(5, 0));
            },
            |s| {
                s.rally_backoff.insert(EntityId::new(8, 1), Tick(1601));
            },
            |s| {
                s.rally_backoff.insert(EntityId::new(8, 2), Tick(1600));
            },
            |s| s.units_sent.clear(),
            |s| s.units_sent.reverse(),
            |s| s.units_sent[0].1 = Kind(13),
            |s| s.units_sent[0].2 = Tick(1401),
            |s| s.units_sent[1].3 = 2,
            |s| s.units_sent[1].4 = 1,
            |s| s.scout = None,
            |s| s.scout = Some(EntityId::new(11, 1)),
            |s| s.scout_leg = 3,
            |s| s.scout_leg_sent = None,
            |s| s.scout_leg_sent = Some(Tick(1391)),
            |s| s.scouts_sent = 2,
            |s| s.scout_gone_at = None,
            |s| s.scout_gone_at = Some(Tick(1381)),
            |s| s.directive.target = None,
            |s| s.directive.target = Some(Cell::new(60, 21)),
            |s| {
                s.sent.remove(&EntityId::new(12, 0));
            },
            |s| {
                s.sent.insert(EntityId::new(14, 2), (Tick(1560), 3));
            },
            |s| {
                s.sent.insert(EntityId::new(14, 2), (Tick(1561), 2));
            },
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut s = base_state();
            change(&mut s);
            assert_ne!(s.digest(), d, "change {i}");
        }
        // The posture's number is hashed: the permanent value.
        assert_eq!(crate::state::Posture::Gather as u8, 0);
    }

    #[test]
    fn every_part_of_the_threat_and_attack_state_moves_the_digest() {
        let d = base_state().digest();
        let changes: [fn(&mut AiState); 32] = [
            |s| s.closing.reverse(),
            |s| s.closing.clear(),
            |s| s.target_hp = None,
            |s| s.target_hp = Some((EntityId::new(11, 1), 1500, Tick(1450))),
            |s| s.target_hp = Some((EntityId::new(11, 0), 1499, Tick(1450))),
            |s| s.target_hp = Some((EntityId::new(11, 0), 1500, Tick(1451))),
            |s| s.recalled.reverse(),
            |s| s.recalled.clear(),
            |s| {
                s.hp_seen.remove(&EntityId::new(0, 0));
            },
            |s| {
                s.hp_seen
                    .insert(EntityId::new(3, 1), (Kind(1), Cell::new(18, 60), 699));
            },
            |s| {
                s.hp_seen
                    .insert(EntityId::new(3, 1), (Kind(2), Cell::new(18, 60), 700));
            },
            |s| {
                s.hp_seen
                    .insert(EntityId::new(3, 1), (Kind(1), Cell::new(18, 61), 700));
            },
            |s| s.threat = None,
            |s| s.threat = Some((Cell::new(17, 61), Tick(1490))),
            |s| s.threat = Some((Cell::new(17, 60), Tick(1491))),
            |s| s.defend_since = None,
            |s| s.defend_since = Some(Tick(1451)),
            |s| s.directive.posture = Posture::Attack,
            |s| s.directive.posture = Posture::Gather,
            |s| s.attackers.clear(),
            |s| s.attackers.reverse(),
            |s| s.attackers[1] = EntityId::new(21, 2),
            |s| s.attack_target = None,
            |s| s.attack_target = Some(Cell::new(71, 17)),
            |s| s.last_attack_tick = Tick(1201),
            |s| s.attack_started = Tick(1301),
            |s| s.attacks_launched = 2,
            |s| s.sites_cleared.clear(),
            |s| s.sites_cleared.reverse(),
            |s| s.sites_cleared[1] = PlayerId(3),
            |s| {
                s.sent.insert(EntityId::new(12, 0), (Tick(1500), 2));
            },
            |s| s.directive.target = Some(Cell::new(61, 20)),
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut s = base_state();
            change(&mut s);
            assert_ne!(s.digest(), d, "change {i}");
        }
        // The postures' numbers are hashed: the permanent values.
        assert_eq!(
            (
                Posture::Gather as u8,
                Posture::Attack as u8,
                Posture::Defend as u8
            ),
            (0, 1, 2)
        );
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
