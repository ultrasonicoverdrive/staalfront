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
/// (threat, timing, target) and turned into orders by `army::operations` together with the groups.
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

/// What a group of the army is for. The numbers are hashed, so they are permanent. WP9.2a has the strike only;
/// the raid of WP9.3b is the next number.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum GroupRole {
    /// A wave that goes for the target of the attack (rule A5).
    #[default]
    Strike = 0,
}

/// Where a group is in its operation. The numbers are hashed, so they are permanent.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum GroupStage {
    /// Walking to, or waiting at, the staging cell.
    #[default]
    Staging = 0,
    /// Marching on its goal.
    Advancing = 1,
    /// A member is within `arrive_radius` of the goal.
    Engaged = 2,
    /// Called back by a hit on the construction centre: the members defend, and the group is dissolved when
    /// the threat is over.
    Returning = 3,
}

/// A group of army units on an operation (WP9.2a, `docs/rules/ai.md`, "Groups"). The army units that are in
/// no group are the home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The number of the group: from 0, ascending in the order the groups were made, never reused.
    pub id: u32,
    /// What it is for.
    pub role: GroupRole,
    /// Its units, ascending by id; pruned to the live ones every army turn.
    pub members: Vec<EntityId>,
    /// The target cell of its operation (rule A5), worked out again every army turn; for a reinforcement the
    /// cell it is marching to meet the strike at.
    pub goal: Cell,
    /// Where it is.
    pub stage: GroupStage,
    /// The tick it entered `stage`.
    pub since: Tick,
    /// What its members were worth when it went out (unit costs).
    pub launch_value: u64,
    /// The staging cell, fixed when the group went out; `None` without staging.
    pub staging: Option<Cell>,
    /// The group this one is a reinforcement for, until it merges into it.
    pub merge_into: Option<u32>,
    /// The structure the members at the target finish, its hit points when last seen to drop and that tick;
    /// the operation ends when they have not dropped for `attack_patience`.
    pub target_hp: Option<(EntityId, u32, Tick)>,
    /// Members called back by a hit on the construction centre that have not had the defence order yet,
    /// ascending; they get it whatever their order is.
    pub recalled: Vec<EntityId>,
    /// Members at the target that saw no enemy structure and got the one `AttackMove` onto the middle of the
    /// target structure (or start site) of this operation, ascending.
    pub closing: Vec<EntityId>,
}

/// The most groups at one time.
pub const GROUPS_MAX: usize = 4;

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
    /// WP9.2a: the groups out, ascending by id, at most [`GROUPS_MAX`] (replaces the single wave of WP9.1c-2).
    pub groups: Vec<Group>,
    /// The id the next group gets.
    pub next_group: u32,
    /// The tick the last attack went out (0 before the first).
    pub last_attack_tick: Tick,
    /// The tick the strike went out, or the last army turn a member of it stood within `arrive_radius` of its
    /// target: `attack_patience` counts from here (fix round 2 of WP9.1c-2).
    pub attack_started: Tick,
    /// How many attacks went out in the match.
    pub attacks_launched: u32,
    /// WP9.2a, the timing jitter: the permille drawn (minus to plus `timing_jitter_permille`) for the launch
    /// that comes next; `None` until the first army turn that needs it, and again after every launch.
    pub jitter: Option<i32>,
    /// The other players whose start site the AI has seen empty (no construction centre on its cell),
    /// ascending.
    pub sites_cleared: Vec<PlayerId>,
    /// Per army unit: the tick of the last order (gather, attack or defence) and how many gather orders it got
    /// without arriving; pruned to the live units every army turn.
    pub sent: BTreeMap<EntityId, (Tick, u8)>,
    /// WP9.2b-1, the tally of the counter mix (`crate::counter`): per enemy kind the peak count of armed units seen at
    /// one think and the tick of it. Empty, and not hashed, while `counter_permille` is zero.
    pub intel: BTreeMap<Kind, (u32, Tick)>,
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
            groups: Vec::new(),
            next_group: 0,
            last_attack_tick: Tick(0),
            attack_started: Tick(0),
            attacks_launched: 0,
            jitter: None,
            sites_cleared: Vec::new(),
            sent: BTreeMap::new(),
            intel: BTreeMap::new(),
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
        // The wave of WP9.1c-2, in its old layout, as the groups show it: a profile with no number of WP9.2a
        // set has at most one group and hashes as before.
        let attackers = self.attackers();
        h.write_u32(attackers.len() as u32);
        for id in &attackers {
            h.write_u32(id.index());
            h.write_u32(id.generation());
        }
        let strike = self.strike();
        write_cell(&mut h, strike.map(|g| g.goal));
        h.write_u64(self.last_attack_tick.0);
        h.write_u64(self.attack_started.0);
        h.write_u32(self.attacks_launched);
        match strike.and_then(|g| g.target_hp) {
            Some((id, hp, t)) => {
                h.write_u8(1);
                h.write_u32(id.index());
                h.write_u32(id.generation());
                h.write_u32(hp);
                h.write_u64(t.0);
            }
            None => h.write_u8(0),
        }
        let recalled = self.union_of(|g| &g.recalled, true);
        h.write_u32(recalled.len() as u32);
        for id in &recalled {
            h.write_u32(id.index());
            h.write_u32(id.generation());
        }
        let closing = self.union_of(|g| &g.closing, false);
        h.write_u32(closing.len() as u32);
        for id in &closing {
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
        // WP9.2a: the whole of every group and the jitter, only for a profile with a number of WP9.2a set.
        if self.profile.army.groups_on() {
            h.write_u8(0x92);
            h.write_u32(self.next_group);
            match self.jitter {
                Some(j) => {
                    h.write_u8(1);
                    h.write_i32(j);
                }
                None => h.write_u8(0),
            }
            h.write_u32(self.groups.len() as u32);
            for g in &self.groups {
                h.write_u32(g.id);
                h.write_u8(g.role as u8);
                h.write_u32(g.members.len() as u32);
                for id in &g.members {
                    h.write_u32(id.index());
                    h.write_u32(id.generation());
                }
                h.write_u32(g.goal.x);
                h.write_u32(g.goal.y);
                h.write_u8(g.stage as u8);
                h.write_u64(g.since.0);
                h.write_u64(g.launch_value);
                write_cell(&mut h, g.staging);
                match g.merge_into {
                    Some(id) => {
                        h.write_u8(1);
                        h.write_u32(id);
                    }
                    None => h.write_u8(0),
                }
                match g.target_hp {
                    Some((id, hp, t)) => {
                        h.write_u8(1);
                        h.write_u32(id.index());
                        h.write_u32(id.generation());
                        h.write_u32(hp);
                        h.write_u64(t.0);
                    }
                    None => h.write_u8(0),
                }
                for list in [&g.recalled, &g.closing] {
                    h.write_u32(list.len() as u32);
                    for id in list {
                        h.write_u32(id.index());
                        h.write_u32(id.generation());
                    }
                }
            }
        }
        // WP9.2b-1: the tally, only with the counter mix on.
        if self.profile.army.counter_on() {
            h.write_u8(0x94);
            h.write_u32(self.intel.len() as u32);
            for (kind, (n, tick)) in &self.intel {
                h.write_u32(u32::from(kind.0));
                h.write_u32(*n);
                h.write_u64(tick.0);
            }
        }
        h.finish()
    }

    /// The units of the groups out, ascending: those of every group that is not [`GroupStage::Returning`].
    /// What `attackers` was before WP9.2a.
    pub fn attackers(&self) -> Vec<EntityId> {
        let mut all: Vec<EntityId> = self
            .groups
            .iter()
            .filter(|g| g.stage != GroupStage::Returning)
            .flat_map(|g| g.members.iter().copied())
            .collect();
        all.sort();
        all.dedup();
        all
    }

    /// Whether `id` is in a group that is out (not [`GroupStage::Returning`]): such a unit is not at home.
    pub fn in_field(&self, id: EntityId) -> bool {
        self.groups
            .iter()
            .any(|g| g.stage != GroupStage::Returning && g.members.binary_search(&id).is_ok())
    }

    /// The strike: the first group that is out and is no reinforcement.
    pub fn strike(&self) -> Option<&Group> {
        self.groups
            .iter()
            .find(|g| g.stage != GroupStage::Returning && g.merge_into.is_none())
    }

    /// Makes a group of `members` (ascending) with the next id and returns the id. The recall and closing lists
    /// start empty. The one place a group is made, for the army rules and for tests.
    #[allow(clippy::too_many_arguments)]
    pub fn add_group(
        &mut self,
        members: Vec<EntityId>,
        goal: Cell,
        stage: GroupStage,
        now: Tick,
        launch_value: u64,
        staging: Option<Cell>,
        merge_into: Option<u32>,
    ) -> u32 {
        let id = self.next_group;
        self.next_group += 1;
        self.groups.push(Group {
            id,
            role: GroupRole::Strike,
            members,
            goal,
            stage,
            since: now,
            launch_value,
            staging,
            merge_into,
            target_hp: None,
            recalled: Vec::new(),
            closing: Vec::new(),
        });
        id
    }

    /// The target cell of the attack going on: the goal of the strike. What `attack_target` was before WP9.2a.
    pub fn attack_target(&self) -> Option<Cell> {
        self.strike().map(|g| g.goal)
    }

    /// The units on `list` of every group when `returning` is set, or of the groups that are not Returning only when it
    /// is not, ascending.
    fn union_of(&self, list: impl Fn(&Group) -> &Vec<EntityId>, returning: bool) -> Vec<EntityId> {
        let mut all: Vec<EntityId> = self
            .groups
            .iter()
            .filter(|g| returning || g.stage != GroupStage::Returning)
            .flat_map(|g| list(g).iter().copied())
            .collect();
        all.sort();
        all.dedup();
        all
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
        s.groups.push(Group {
            id: 3,
            role: GroupRole::Strike,
            members: vec![EntityId::new(20, 0), EntityId::new(21, 1)],
            goal: Cell::new(71, 16),
            stage: GroupStage::Advancing,
            since: Tick(1310),
            launch_value: 2100,
            staging: Some(Cell::new(60, 30)),
            merge_into: None,
            target_hp: Some((EntityId::new(11, 0), 1500, Tick(1450))),
            recalled: vec![EntityId::new(22, 0), EntityId::new(23, 1)],
            closing: vec![EntityId::new(24, 0), EntityId::new(25, 1)],
        });
        s.groups.push(Group {
            id: 4,
            role: GroupRole::Strike,
            members: vec![EntityId::new(30, 0), EntityId::new(31, 0)],
            goal: Cell::new(60, 30),
            stage: GroupStage::Advancing,
            since: Tick(1320),
            launch_value: 1000,
            staging: None,
            merge_into: Some(3),
            target_hp: None,
            recalled: Vec::new(),
            closing: Vec::new(),
        });
        s.next_group = 5;
        s.jitter = Some(-40);
        s.last_attack_tick = Tick(1200);
        s.attack_started = Tick(1300);
        s.attacks_launched = 1;
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
        // 0xaa333a1fd36cb5a9, the goal list, `attack_value` and `max_wait`; WP9.2a: -> 0x8383_758e_5b5e_90e6, the numbers of the groups in the shipped profile and the group state; with them off and one group the old value comes back, `the_digest_of_the_base_state_with_the_numbers_of_wp9_2a_off_is_the_old_pin`; WP9.2a-2: 0x8383_758e_5b5e_90e6 -> 0x4d0a_8207_c799_2fad, `gather_refinery_gap` 2 and `gather_everyone` true in the shipped profile; with both off, 0x8383_758e_5b5e_90e6 comes back, `the_digest_of_the_base_state_with_the_numbers_of_wp9_2a_off_is_the_old_pin` covers the older pin).
        assert_eq!(base_state().digest(), 0x4d0a_8207_c799_2fad);
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
            |s| s.groups[0].closing.reverse(),
            |s| s.groups[0].closing.clear(),
            |s| s.groups[0].target_hp = None,
            |s| s.groups[0].target_hp = Some((EntityId::new(11, 1), 1500, Tick(1450))),
            |s| s.groups[0].target_hp = Some((EntityId::new(11, 0), 1499, Tick(1450))),
            |s| s.groups[0].target_hp = Some((EntityId::new(11, 0), 1500, Tick(1451))),
            |s| s.groups[0].recalled.reverse(),
            |s| s.groups[0].recalled.clear(),
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
            |s| s.groups.clear(),
            |s| s.groups[0].members.reverse(),
            |s| s.groups[0].members[1] = EntityId::new(21, 2),
            |s| s.groups[0].goal = Cell::new(71, 15),
            |s| s.groups[0].goal = Cell::new(71, 17),
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
    fn the_digest_of_the_base_state_with_the_numbers_of_wp9_2a_off_is_the_old_pin() {
        // The cause of the move of the pin above, as a test: the shipped profile with the numbers of WP9.2a
        // (guard, staging, reinforcement, partial defence, jitter) set to 0, one group and no jitter drawn hashes
        // to the pin WP9.1c-3 left, so the new layout adds nothing for a profile that does not use it.
        let mut s = base_state();
        let a = &mut s.profile.army;
        a.home_guard_permille = 0;
        a.stage_distance = 0;
        a.reinforce_value = 0;
        a.defend_ratio_permille = 0;
        a.timing_jitter_permille = 0;
        a.gather_refinery_gap = 0;
        a.gather_everyone = false;
        s.groups.truncate(1);
        s.next_group = 0;
        s.jitter = None;
        assert_eq!(s.digest(), 0xaa33_3a1f_d36c_b5a9);
    }

    #[test]
    fn the_digest_of_the_base_state_with_the_gather_numbers_off_is_the_wp9_2a_pin() {
        // The cause of the move of the pin above, as a test (WP9.2a-2): the shipped profile with only the two
        // gather numbers set to 0 hashes to the pin WP9.2a left.
        let mut s = base_state();
        s.profile.army.gather_refinery_gap = 0;
        s.profile.army.gather_everyone = false;
        assert_eq!(s.digest(), 0x8383_758e_5b5e_90e6);
    }

    #[test]
    fn every_part_of_the_groups_moves_the_digest() {
        let d = base_state().digest();
        let changes: [fn(&mut AiState); 24] = [
            |s| s.groups[0].id = 2,
            |s| s.groups[0].stage = GroupStage::Engaged,
            |s| s.groups[0].stage = GroupStage::Returning,
            |s| s.groups[0].since = Tick(1311),
            |s| s.groups[0].launch_value = 2101,
            |s| s.groups[0].staging = None,
            |s| s.groups[0].staging = Some(Cell::new(60, 31)),
            |s| s.groups[0].merge_into = Some(4),
            |s| s.groups[1].id = 5,
            |s| {
                s.groups[1].members.pop();
            },
            |s| s.groups[1].goal = Cell::new(60, 31),
            |s| s.groups[1].stage = GroupStage::Staging,
            |s| s.groups[1].since = Tick(1321),
            |s| s.groups[1].launch_value = 1001,
            |s| s.groups[1].staging = Some(Cell::new(1, 1)),
            |s| s.groups[1].merge_into = None,
            |s| s.groups[1].merge_into = Some(2),
            |s| s.groups[1].recalled.push(EntityId::new(30, 0)),
            |s| s.groups[1].closing.push(EntityId::new(31, 0)),
            |s| s.groups.reverse(),
            |s| s.next_group = 6,
            |s| s.jitter = None,
            |s| s.jitter = Some(-41),
            |s| s.groups.truncate(1),
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut s = base_state();
            change(&mut s);
            assert_ne!(s.digest(), d, "change {i}");
        }
        // The numbers are hashed: the permanent values.
        assert_eq!(GroupRole::Strike as u8, 0);
        assert_eq!(
            (
                GroupStage::Staging as u8,
                GroupStage::Advancing as u8,
                GroupStage::Engaged as u8,
                GroupStage::Returning as u8
            ),
            (0, 1, 2, 3)
        );
        assert_eq!(GROUPS_MAX, 4);
    }

    #[test]
    fn the_legacy_view_of_the_groups_is_what_the_single_wave_was() {
        let s = base_state();
        // Both groups are out: the units of both, the target of the strike (the first group).
        assert_eq!(
            s.attackers(),
            [(20, 0), (21, 1), (30, 0), (31, 0)].map(|(i, g)| EntityId::new(i, g))
        );
        assert_eq!(s.strike().map(|g| g.id), Some(3));
        let mut t = s.clone();
        t.groups[0].stage = GroupStage::Returning;
        // A group called back is no longer out: the second group is the strike now, and the first one's units are
        // home (only its `recalled` list is left in the legacy view).
        assert_eq!(t.attackers().len(), 2);
        assert!(!t.in_field(EntityId::new(20, 0)));
        assert!(t.in_field(EntityId::new(30, 0)));
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
