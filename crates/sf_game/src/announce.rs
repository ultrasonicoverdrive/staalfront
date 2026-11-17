//! Announcements (WP11.2c): short status lines about the player's own base, "Unit ready", "Base under
//! attack". Rules in `docs/rules/announcements.md`.
//!
//! Presentation only: nothing here reaches `sf_sim`, nothing is hashed. The moments come from what the
//! simulation already publishes:
//!
//! * [`Facts::read`] takes the few things of the **local player** out of a [`SimSnapshot`] (its
//!   constructions, factories, units, hit points, power, storage, the sidebar's options, the match result)
//!   and [`changes`] compares two of them: an announcement is an edge, not a state, so a state that stays
//!   (low power) is said once when it starts.
//! * [`deaths`] reads the shown effects events: the own deaths.
//! * The refusal of a placement comes from the runner (`handle_build`), from the simulation's own rejections
//!   of the local player.
//!
//! [`Announcer`] holds the brake: one at a time, a minimum gap, a longer gap per event, the same event not
//! queued twice, a short queue ordered by priority, and nothing said before the game has settled. It does
//! not talk over a unit voice (it waits, up to `max_wait_ms`).

use std::collections::BTreeSet;

use sf_audio::{AnnounceConfig, AudioSystem};
use sf_sim::{EntityId, FactoryStatus, Kind, PlayerId, Verdict};

use crate::fx_events::{FxEvent, FxKind};
use crate::sidebar::{Catalog, Situation, available};
use crate::sim_snapshot::SimSnapshot;
use crate::unit_voice::UnitVoice;

/// What is announced. Each is a sound event of `bank.ron` (and `ra_mapping.ron`): see [`Announcement::event`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Announcement {
    /// A structure of the player went up.
    StructureReady,
    /// A unit left a factory.
    UnitReady,
    /// A structure was placed and goes up.
    Building,
    /// A unit was queued.
    Training,
    /// A finished unit waits for room, or a unit's structure is gone: the queue holds.
    OnHold,
    /// A queued unit was taken out again.
    Cancelled,
    /// A structure of the player lost hit points.
    BaseUnderAttack,
    /// A harvester of the player lost hit points.
    HarvesterUnderAttack,
    /// The player's power does not cover its use.
    LowPower,
    /// A factory or a structure going up waits for money.
    InsufficientFunds,
    /// The ore store of the player is full.
    StorageFull,
    /// A unit of the player was destroyed.
    UnitLost,
    /// A structure of the player was destroyed.
    StructureLost,
    /// The sidebar offers something it never offered in this game.
    NewOptions,
    /// The simulation refused a placement of the player.
    CannotDeploy,
    /// The match is won.
    Victory,
    /// The match is lost.
    Defeat,
    /// A sale of the player started.
    StructureSold,
    /// A repair of the player started.
    Repairing,
}

impl Announcement {
    /// The place of this event in [`Announcement::ALL`] (an exhaustive match: a new event must be given one).
    pub fn index(self) -> usize {
        match self {
            Announcement::StructureReady => 0,
            Announcement::UnitReady => 1,
            Announcement::Building => 2,
            Announcement::Training => 3,
            Announcement::OnHold => 4,
            Announcement::Cancelled => 5,
            Announcement::BaseUnderAttack => 6,
            Announcement::HarvesterUnderAttack => 7,
            Announcement::LowPower => 8,
            Announcement::InsufficientFunds => 9,
            Announcement::StorageFull => 10,
            Announcement::UnitLost => 11,
            Announcement::StructureLost => 12,
            Announcement::NewOptions => 13,
            Announcement::CannotDeploy => 14,
            Announcement::Victory => 15,
            Announcement::Defeat => 16,
            Announcement::StructureSold => 17,
            Announcement::Repairing => 18,
        }
    }

    pub const ALL: [Announcement; 19] = [
        Announcement::StructureReady,
        Announcement::UnitReady,
        Announcement::Building,
        Announcement::Training,
        Announcement::OnHold,
        Announcement::Cancelled,
        Announcement::BaseUnderAttack,
        Announcement::HarvesterUnderAttack,
        Announcement::LowPower,
        Announcement::InsufficientFunds,
        Announcement::StorageFull,
        Announcement::UnitLost,
        Announcement::StructureLost,
        Announcement::NewOptions,
        Announcement::CannotDeploy,
        Announcement::Victory,
        Announcement::Defeat,
        Announcement::StructureSold,
        Announcement::Repairing,
    ];

    /// The sound event of the bank and of `ra_mapping.ron`.
    pub fn event(self) -> &'static str {
        match self {
            Announcement::StructureReady => "announce.structure_ready",
            Announcement::UnitReady => "announce.unit_ready",
            Announcement::Building => "announce.building",
            Announcement::Training => "announce.training",
            Announcement::OnHold => "announce.on_hold",
            Announcement::Cancelled => "announce.cancelled",
            Announcement::BaseUnderAttack => "announce.base_under_attack",
            Announcement::HarvesterUnderAttack => "announce.harvester_under_attack",
            Announcement::LowPower => "announce.low_power",
            Announcement::InsufficientFunds => "announce.insufficient_funds",
            Announcement::StorageFull => "announce.storage_full",
            Announcement::UnitLost => "announce.unit_lost",
            Announcement::StructureLost => "announce.structure_lost",
            Announcement::NewOptions => "announce.new_options",
            Announcement::CannotDeploy => "announce.cannot_deploy",
            Announcement::Victory => "announce.victory",
            Announcement::Defeat => "announce.defeat",
            Announcement::StructureSold => "announce.structure_sold",
            Announcement::Repairing => "announce.repairing",
        }
    }
}

/// How the match stands for the local player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Won,
    Lost,
    /// The last players fell in the same tick: nobody won, nothing is announced.
    Draw,
}

/// One entity of the local player and its hit points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Health {
    pub id: EntityId,
    pub hp: u32,
    pub structure: bool,
    pub harvester: bool,
}

/// One standing factory of the local player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactoryFact {
    pub id: EntityId,
    /// Items in its queue that are not free (a free item is the harvester a refinery brings).
    pub items: usize,
    /// The queue holds: a finished unit finds no room, or a structure it needs is gone.
    pub holding: bool,
    /// The owner cannot pay for the next tick.
    pub waiting_for_money: bool,
}

/// What the announcements read of one snapshot: only things of the local player. Plain data, so a test can
/// state a situation without a world; [`Facts::read`] fills it from a [`SimSnapshot`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub tick: u64,
    /// Every entity of the player that has stats, ascending by slot.
    pub health: Vec<Health>,
    /// The mobile units, ascending.
    pub units: Vec<EntityId>,
    /// The structures going up, ascending.
    pub constructions: Vec<EntityId>,
    /// The structures being sold, being repaired, ascending.
    pub sales: Vec<EntityId>,
    pub repairs: Vec<EntityId>,
    /// The standing factories, ascending.
    pub factories: Vec<FactoryFact>,
    /// A structure going up waits for money.
    pub construction_waits_for_money: bool,
    /// The player's power is below its use.
    pub low_power: bool,
    /// The ore store is full.
    pub storage_full: bool,
    /// The kinds the sidebar offers, ascending (empty without a catalog).
    pub options: Vec<Kind>,
    /// The decided result of the match, if any.
    pub standing: Option<Standing>,
}

impl Facts {
    /// What `local` has in `snap`. `catalog` is the sidebar's: without it no option is read.
    pub fn read(snap: &SimSnapshot, local: PlayerId, catalog: Option<&Catalog>) -> Facts {
        let mut f = Facts {
            tick: snap.tick().0,
            construction_waits_for_money: snap.waits_for_money(local),
            low_power: snap.power_of(local).is_some() && snap.power_speed(local) < 1000,
            storage_full: snap
                .storage_of(local)
                .is_some_and(|(stored, capacity)| capacity > 0 && stored >= capacity),
            standing: snap.match_result().map(|r| match r.verdict {
                Verdict::Won(p) if p == local => Standing::Won,
                Verdict::Won(_) => Standing::Lost,
                Verdict::Draw => Standing::Draw,
            }),
            ..Facts::default()
        };
        for v in snap.views() {
            if v.owner != local || !v.has_stats {
                continue;
            }
            let structure = snap.stats_for(v).is_some_and(|s| s.footprint.is_some());
            f.health.push(Health {
                id: v.id,
                hp: v.hp,
                structure,
                harvester: snap.cargo_of(v.id).is_some(),
            });
            if snap.sale_of(v.id).is_some() {
                f.sales.push(v.id);
            }
            if snap.repair_of(v.id).is_some() {
                f.repairs.push(v.id);
            }
            if !structure {
                f.units.push(v.id);
            } else if snap.construction_of(v.id).is_some() {
                f.constructions.push(v.id);
            } else if snap.menu().factory_function(v.kind).is_some()
                && crate::sidebar::stands_when_built(snap, v)
            {
                let view = snap.factory_of(v.id);
                f.factories.push(FactoryFact {
                    id: v.id,
                    items: view.map_or(0, |q| q.queue.iter().filter(|i| !i.free).count()),
                    holding: view.is_some_and(|q| {
                        matches!(
                            q.status,
                            FactoryStatus::WaitingForExit | FactoryStatus::MissingPrerequisite(_)
                        )
                    }),
                    waiting_for_money: view
                        .is_some_and(|q| q.status == FactoryStatus::WaitingForMoney),
                });
            }
        }
        if let Some(catalog) = catalog {
            let sit = Situation::read(snap, local, catalog, &[]);
            f.options = catalog
                .entries()
                .iter()
                .filter(|e| available(e, &sit))
                .map(|e| e.kind)
                .collect();
            f.options.sort();
            f.options.dedup();
        }
        f
    }

    /// The health of `id`. `health` is ascending by id (the snapshot's slot order), so this is a binary search.
    fn hp_of(&self, id: EntityId) -> Option<&Health> {
        self.health
            .binary_search_by_key(&id, |h| h.id)
            .ok()
            .map(|i| &self.health[i])
    }

    fn short_of_money(&self) -> bool {
        self.construction_waits_for_money || self.factories.iter().any(|f| f.waiting_for_money)
    }
}

/// Whether `id` is in `list`, which is ascending (every id list of [`Facts`] is: the snapshot walks the slots in order).
fn has(list: &[EntityId], id: &EntityId) -> bool {
    list.binary_search(id).is_ok()
}

/// What happened between two snapshots, in a fixed order. Pure. Every id list of both is ascending, so each
/// lookup is a binary search: the cost is linear in the entities of the player times a logarithm, not quadratic. The first one of a game
/// and the one at the end of the settling time are not compared (see [`Announcer::observe`]).
///
/// * `Building`: a structure of the player is going up that was not before. `StructureReady`: one that was
///   going up stands now (it is still there and is not being sold).
/// * `UnitReady`: a mobile unit of the player that was not there before. `Training`: a factory has more
///   (paid) items in its queue than before. `Cancelled`: the factories lost more items than units came out.
///   (A factory that repeats its item refills at once, so a cancel of that one cannot be seen.)
/// * `OnHold`: a factory starts to hold. `InsufficientFunds`: something starts to wait for money.
/// * `LowPower`, `StorageFull`: the state starts.
/// * `BaseUnderAttack` / `HarvesterUnderAttack`: a structure / harvester of the player that was there before
///   has fewer hit points than before (a structure being sold is left out).
/// * `StructureSold`, `Repairing`: a sale / repair of the player started.
/// * `Victory` / `Defeat`: the match was decided for / against the player.
pub fn changes(prev: &Facts, now: &Facts) -> Vec<Announcement> {
    let mut out = Vec::new();
    let new_constructions = now
        .constructions
        .iter()
        .any(|id| !has(&prev.constructions, id));
    let finished = prev
        .constructions
        .iter()
        .any(|id| !has(&now.constructions, id) && now.hp_of(*id).is_some() && !has(&now.sales, id));
    let new_units = now.units.iter().filter(|id| !has(&prev.units, id)).count();
    let queued = now.factories.iter().any(|f| {
        f.items
            > prev
                .factories
                .iter()
                .find(|p| p.id == f.id)
                .map_or(0, |p| p.items)
    });
    let taken_out: usize = now
        .factories
        .iter()
        .filter_map(|f| {
            let before = prev.factories.iter().find(|p| p.id == f.id)?;
            before.items.checked_sub(f.items)
        })
        .sum();
    let holds = now.factories.iter().any(|f| {
        f.holding
            && !prev
                .factories
                .iter()
                .find(|p| p.id == f.id)
                .is_some_and(|p| p.holding)
    });
    let hit = |harvester: bool, structure: bool| {
        now.health.iter().any(|h| {
            (h.structure == structure && h.harvester == harvester)
                && !has(&now.sales, &h.id)
                && prev.hp_of(h.id).is_some_and(|p| h.hp < p.hp)
        })
    };
    let checks = [
        (Announcement::StructureReady, finished),
        (Announcement::UnitReady, new_units > 0),
        (Announcement::Building, new_constructions),
        (Announcement::Training, queued),
        (Announcement::OnHold, holds),
        (Announcement::Cancelled, taken_out > new_units),
        (Announcement::BaseUnderAttack, hit(false, true)),
        (Announcement::HarvesterUnderAttack, hit(true, false)),
        (Announcement::LowPower, now.low_power && !prev.low_power),
        (
            Announcement::InsufficientFunds,
            now.short_of_money() && !prev.short_of_money(),
        ),
        (
            Announcement::StorageFull,
            now.storage_full && !prev.storage_full,
        ),
        (
            Announcement::StructureSold,
            now.sales.iter().any(|id| !has(&prev.sales, id)),
        ),
        (
            Announcement::Repairing,
            now.repairs.iter().any(|id| !has(&prev.repairs, id)),
        ),
        (
            Announcement::Victory,
            prev.standing.is_none() && now.standing == Some(Standing::Won),
        ),
        (
            Announcement::Defeat,
            prev.standing.is_none() && now.standing == Some(Standing::Lost),
        ),
    ];
    out.extend(checks.iter().filter(|(_, on)| *on).map(|(a, _)| *a));
    out
}

/// The own losses among the shown effects events: `UnitLost` and `StructureLost`, each once at most. Only the
/// deaths of `local`'s own units and structures count (an event carries the owner of what died).
pub fn deaths(events: &[FxEvent], local: PlayerId) -> Vec<Announcement> {
    let mut unit = false;
    let mut structure = false;
    for e in events {
        if e.player != local.0 {
            continue;
        }
        if let FxKind::Death { structure: s, .. } = e.kind {
            if s {
                structure = true;
            } else {
                unit = true;
            }
        }
    }
    let mut out = Vec::new();
    if unit {
        out.push(Announcement::UnitLost);
    }
    if structure {
        out.push(Announcement::StructureLost);
    }
    out
}

/// `CannotDeploy` when the simulation refused a placement of `local` among `rejections` (the refusals of
/// the last steps, as `SimHandle::rejected_builds` keeps them): only the local player's own refusals count.
pub fn refusal(rejections: &[sf_sim::PlacementRejection], local: PlayerId) -> Option<Announcement> {
    rejections
        .iter()
        .any(|r| r.player == local)
        .then_some(Announcement::CannotDeploy)
}

/// What the announcer needs from the audio: [`AudioSystem`] in the game, a stand-in in tests.
pub trait AnnounceOut {
    /// The event has a sound to play (the bank, or a mapped Red Alert sound). Never warns.
    fn has_sound(&self, event: &str) -> bool;
    /// A unit voice or an announcement is still sounding.
    fn busy(&self) -> bool;
    fn play(&mut self, event: &str);
}

/// The events of every voice that shares the one channel: the three unit voices and the announcements.
/// What [`AudioSystem::is_playing`] is asked, by the unit voices and by the announcer.
pub fn voice_channel() -> Vec<&'static str> {
    UnitVoice::ALL
        .iter()
        .map(|v| v.event())
        .chain(Announcement::ALL.iter().map(|a| a.event()))
        .collect()
}

impl AnnounceOut for AudioSystem {
    fn has_sound(&self, event: &str) -> bool {
        AudioSystem::has_sound(self, event)
    }

    fn busy(&self) -> bool {
        self.is_playing(&voice_channel())
    }

    fn play(&mut self, event: &str) {
        // The voice volume of the settings, like the unit voices.
        AudioSystem::play_as(self, event, sf_audio::Category::Voice);
    }
}

/// Counts of what the brake did, for the log and the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnnounceStats {
    pub raised: u64,
    pub played: u64,
    /// Raised again inside the event's repeat gap, or already waiting.
    pub suppressed: u64,
    /// Waited longer than `max_wait_ms`, or pushed out of a full queue.
    pub dropped: u64,
}

/// The brake, the queue and the baseline of the announcements.
pub struct Announcer {
    config: AnnounceConfig,
    /// The raised announcements that wait to be said, with the time they were raised.
    pending: Vec<(Announcement, u64)>,
    /// When the last announcement started, and when each event last did (by [`Announcement::ALL`] index).
    last_start: Option<u64>,
    last_by_event: [Option<u64>; 19],
    first_tick: Option<u64>,
    /// The settling time is over (set by [`Announcer::observe`]).
    settled: bool,
    prev: Option<Facts>,
    /// Every option the sidebar has offered in this game.
    seen_options: BTreeSet<Kind>,
    stats: AnnounceStats,
}

impl Announcer {
    pub fn new(config: AnnounceConfig) -> Self {
        Announcer {
            config,
            pending: Vec::new(),
            last_start: None,
            last_by_event: [None; 19],
            first_tick: None,
            settled: false,
            prev: None,
            seen_options: BTreeSet::new(),
            stats: AnnounceStats::default(),
        }
    }

    /// Whether `tick` is a snapshot not looked at yet (the facts are read once per tick, not once per frame).
    pub fn is_new_tick(&self, tick: u64) -> bool {
        self.prev.as_ref().is_none_or(|p| p.tick != tick)
    }

    /// Forget everything (a new game).
    pub fn reset(&mut self) {
        *self = Announcer::new(self.config.clone());
    }

    pub fn stats(&self) -> AnnounceStats {
        self.stats
    }

    /// The announcements waiting to be said, in the order raised.
    pub fn pending(&self) -> Vec<Announcement> {
        self.pending.iter().map(|(a, _)| *a).collect()
    }

    /// The facts of a new snapshot at `now_ms`. The first of a game, and every one in the first
    /// `settle_ticks` after it, only set the baseline: the starting objects of the game are not news. After
    /// that each is compared with the one before ([`changes`]) and the sidebar's new options are looked for.
    pub fn observe(&mut self, facts: Facts, now_ms: u64) {
        let first = *self.first_tick.get_or_insert(facts.tick);
        let settled = facts.tick >= first + u64::from(self.config.settle_ticks);
        self.settled = settled;
        let new_options = facts
            .options
            .iter()
            .filter(|k| self.seen_options.insert(**k))
            .count()
            > 0;
        if settled {
            if let Some(prev) = &self.prev {
                for a in changes(prev, &facts) {
                    self.raise(a, now_ms);
                }
            }
            if new_options {
                self.raise(Announcement::NewOptions, now_ms);
            }
        }
        self.prev = Some(facts);
    }

    /// [`Announcer::raise`] for what the runner learns outside [`Announcer::observe`] (the shown deaths, the
    /// refusals of a placement): nothing is raised while the game settles, so the grace of `settle_ticks`
    /// covers every path. Returns whether it was queued.
    pub fn raise_live(&mut self, a: Announcement, now_ms: u64) -> bool {
        if !self.settled {
            return false;
        }
        self.raise(a, now_ms)
    }

    /// Raise `a` at `now_ms`: it waits to be said unless the same one waits already or was said inside its
    /// repeat gap. Returns whether it was queued.
    pub fn raise(&mut self, a: Announcement, now_ms: u64) -> bool {
        self.stats.raised += 1;
        let index = a.index();
        let gap = u64::from(self.config.rule(a.event()).repeat_gap_ms);
        let recent = self.last_by_event[index].is_some_and(|at| now_ms.saturating_sub(at) < gap);
        if recent || self.pending.iter().any(|(p, _)| *p == a) {
            self.stats.suppressed += 1;
            return false;
        }
        self.pending.push((a, now_ms));
        let cap = self.config.queue_len as usize;
        while self.pending.len() > cap {
            // The lowest priority goes, the oldest of those; the one just raised is the newest.
            let worst = (0..self.pending.len())
                .min_by_key(|&i| {
                    (
                        self.config.rule(self.pending[i].0.event()).priority,
                        self.pending[i].1,
                    )
                })
                .unwrap_or(0);
            self.pending.remove(worst);
            self.stats.dropped += 1;
        }
        true
    }

    /// Say the most important waiting announcement if it may be heard now: the voices are quiet
    /// (`out.busy()`), `min_gap_ms` has passed since the last one started, it has a sound. Old ones are
    /// dropped first and one without a sound is dropped silently (it uses up no gap). At most one per call.
    /// Returns what was said.
    pub fn speak(&mut self, now_ms: u64, out: &mut dyn AnnounceOut) -> Option<Announcement> {
        let max_wait = u64::from(self.config.max_wait_ms);
        let before = self.pending.len();
        self.pending
            .retain(|(_, at)| now_ms.saturating_sub(*at) <= max_wait);
        self.stats.dropped += (before - self.pending.len()) as u64;
        // A silent event (no sound for it) is dropped without a word and uses up no gap; it did not
        // wait too long, so it is not counted as dropped.
        self.pending.retain(|(a, _)| out.has_sound(a.event()));
        if self.pending.is_empty() || out.busy() {
            return None;
        }
        if self
            .last_start
            .is_some_and(|at| now_ms.saturating_sub(at) < u64::from(self.config.min_gap_ms))
        {
            return None;
        }
        // The highest priority; of equal ones the first raised (`max_by_key` would take the last of equal
        // maxima, and everything raised in one `observe` has the same time), so the index breaks the tie.
        let best = (0..self.pending.len()).max_by_key(|&i| {
            (
                self.config.rule(self.pending[i].0.event()).priority,
                std::cmp::Reverse(i),
            )
        })?;
        let (a, _) = self.pending.remove(best);
        out.play(a.event());
        self.last_start = Some(now_ms);
        let index = a.index();
        self.last_by_event[index] = Some(now_ms);
        self.stats.played += 1;
        Some(a)
    }
}

#[cfg(test)]
mod tests;
