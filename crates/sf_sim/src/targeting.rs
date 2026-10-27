//! Target selection and automatic return fire (WP5.4, `docs/rules/targeting.md`).
//!
//! Until now a unit fired only at the victim of an `Attack` order. A world that is given a
//! [`TargetingConfig`] ([`World::with_targeting`](crate::World::with_targeting)) lets the armed
//! units of its players choose a target on their own, and fire back at whoever shoots them. Without
//! one nothing here runs, nothing is hashed and every earlier checksum is what it was.
//!
//! # A chosen target is an order
//!
//! A unit that picks a target does not get a new column to fire from: it takes an
//! [`Order::Pursue`] on it, which is an `Attack` the unit gave itself, and keeps the order it had in
//! `Engage::resume`. Everything an attack already does then works as it is: the unit turns, its
//! turret aims, it fires when its weapon is ready and the target is in reach, it walks if it may. When the
//! engagement ends (the target died, left its reach, was despawned) the old order is begun again:
//! a guard walks back to its post, an attack-move marches on, a hold holds, an idle unit stays where it
//! is (and starts the first order that was queued behind it, if a player queued one meanwhile). **A
//! player's order always wins**: any command that gives the unit an order, or stops it, replaces the
//! engagement (`orders::begin` and `orders::clear` forget it), so an explicit `Attack` overrides what
//! the unit would have chosen.
//!
//! | The unit was | It scans (picks a target in reach) | It follows an attacker beyond its reach |
//! |---|---|---|
//! | idle | yes | yes, within the pursuit distance |
//! | on `HoldPosition` | yes, from where it stands | never (it never moves) |
//! | on `Guard` | yes | yes, within the pursuit distance |
//! | on `AttackMove` | yes | no (it has somewhere to be) |
//! | on `Move`, `Attack`, or already engaged | no | no |
//!
//! A unit on an explicit `Move` order does **not** stop to fire back: a move is a decision to go
//! somewhere, not to fight on the way (that is what attack-move is for). Structures will take part when
//! they can hold a target without an order, in the defence work package (WP6.7); in this one only mobile
//! units choose targets.
//!
//! # The tick
//!
//! Between the crowd pass and the turrets, `Targeting::update` runs: every engaged unit checks its
//! target (gone, at zero hit points, out of reach for a unit that may not walk, strayed too far for a
//! unit that is following), and every unit that may scan and whose cooldown has run out looks for one.
//! After the weapons, `Targeting::return_fire` turns the units that were hurt this tick on their
//! attackers. Both read the positions and hit points of the end of the movement stage and the end of
//! the weapon stage, and write only the engagement of the unit they are looking at, so the order
//! the units are visited in cannot matter.
//!
//! # What a unit picks
//!
//! Candidates are the units and structures of another player that can be hurt (hit points above
//! zero) and that some weapon of the unit hurts (the armor table gives it more than zero percent: a
//! sniper does not pick a tank) and could fire at now (in range, outside the minimum range, both to the edge
//! of the candidate, as a shot is checked). Each is scored by its best weapon: [`weapon_score`](crate::weapons::weapon_score),
//! the weapon's damage times the armor table's percentage, per second of reloads. So a rifle prefers
//! infantry, a rocket or a cannon prefers vehicles, a shell prefers buildings: the preference is the
//! armor table, which is data. The key is the score (higher first), then the squared distance (nearer
//! first), then the position (y, then x) and only last the slot, so two candidates that tie on score
//! and distance are told apart by where they stand, not by when they were spawned. The key has a total order, so the
//! choice is the same whatever order the candidates are found in.
//!
//! A scan finds its candidates through the spatial index of the crowd pass (`crowd::Index`), built once
//! in a tick that has a scanner, and costs a few buckets. A unit scans again only after
//! [`TargetingConfig::scan_cooldown`] ticks (staggering the scans of many units would need the slot or
//! some other order-dependent number; they scan on the same tick instead, which costs one index build
//! and a few buckets each). A unit that has a target keeps it while it is valid; there is no
//! switching to a better one.
//!
//! # Return fire
//!
//! A unit that is hurt by an enemy's shot or splash (zero damage does not count: an immune armor
//! shrugs it off) turns on its attacker at once, not on the cooldown, if it may (the table above). An
//! attacker in reach is engaged where the unit stands. One beyond its reach but not beyond reach
//! plus [`TargetingConfig::pursuit`] is followed, as far as `pursuit` cells from the spot the unit
//! stood on when it started, and dropped when it gets away. When several enemies hurt a unit in one
//! tick it takes the one whose key is best, in reach before out of it. A unit that already has a
//! target keeps it.
//!
//! # Zero hit points
//!
//! Nothing is removed when a unit reaches zero until WP5.5. Meanwhile a unit at zero is not a
//! candidate, picks nothing and fires nothing, and with targeting on **it loses its orders and stays
//! where it fell** (`HoldPosition`, which nobody pushes), so a wreck does not march on to its
//! destination. A unit that is only at zero because its kind cannot be damaged (an ore field, hit
//! points zero in the table) is not a unit at all here: it is not mobile.

use core::cmp::Reverse;

use sf_math::hash::Fnv1a64;
use sf_math::{Fx, Vec2};

use crate::behavior::Behavior;
use crate::crowd::Index;
use crate::entities::{Entities, NO_DEF};
use crate::ids::{EntityId, Tick};
use crate::orders::{self, GuardTarget, Order, live_slot, radius_of};
use crate::units::{UnitClass, UnitTable};
use crate::weapons::Arsenal;

/// Ticks between two scans of a unit unless the settings say otherwise: half a second.
pub const DEFAULT_SCAN_COOLDOWN: u16 = 10;

/// Cells a unit follows an attacker beyond its reach unless the settings say otherwise.
pub const DEFAULT_PURSUIT_CELLS: u8 = 6;

/// Ticks an idle unit must be back at rest before a new chase measures from a new spot, unless
/// the settings say otherwise: five seconds.
pub const DEFAULT_PURSUIT_RESET_TICKS: u16 = 100;

/// The most ticks `pursuit_reset_ticks` may be.
pub const MAX_PURSUIT_RESET_TICKS: u16 = 2000;

/// The most ticks a scan cooldown may be (fifty seconds).
pub const MAX_SCAN_COOLDOWN: u16 = 1000;

/// The most cells a pursuit may be.
pub const MAX_PURSUIT_CELLS: u8 = 32;

/// Written first into the hash of the settings; bump when the layout changes.
const SETTINGS_TAG: &[u8] = b"sf_sim/targeting/v2";

/// The settings of target selection (`sim.ron`, `docs/rules/targeting.md`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TargetingConfig {
    /// Ticks between two looks round for a target of one unit; 1 to [`MAX_SCAN_COOLDOWN`].
    pub scan_cooldown: u16,
    /// How far beyond its weapon's reach, in cells, a unit that is shot at follows its attacker,
    /// and the farthest it strays from where it stood when it began. Zero: it never follows.
    /// At most [`MAX_PURSUIT_CELLS`].
    pub pursuit: Fx,
    /// Ticks an idle unit must have been back at rest (no target, not hurt) before its next chase
    /// measures the pursuit from where it then stands; until then it measures from where its first
    /// chase began. 1 to [`MAX_PURSUIT_RESET_TICKS`].
    pub pursuit_reset: u16,
}

impl TargetingConfig {
    /// The numbers of the shipped `sim.ron`, which a test keeps equal.
    pub const DEFAULT: TargetingConfig =
        TargetingConfig::from_cells(DEFAULT_SCAN_COOLDOWN, DEFAULT_PURSUIT_CELLS);

    /// Settings with a pursuit given in whole cells, as `sim.ron` gives it.
    pub const fn from_cells(scan_cooldown: u16, pursuit_cells: u8) -> TargetingConfig {
        TargetingConfig {
            scan_cooldown,
            pursuit: Fx::from_int(pursuit_cells as i32),
            pursuit_reset: DEFAULT_PURSUIT_RESET_TICKS,
        }
    }

    /// The same settings with another `pursuit_reset`.
    pub const fn with_pursuit_reset(self, ticks: u16) -> TargetingConfig {
        TargetingConfig {
            pursuit_reset: ticks,
            ..self
        }
    }

    /// Whether the numbers are in their ranges.
    pub fn is_valid(&self) -> bool {
        (1..=MAX_SCAN_COOLDOWN).contains(&self.scan_cooldown)
            && !self.pursuit.is_negative()
            && self.pursuit <= Fx::from_int(i32::from(MAX_PURSUIT_CELLS))
            && (1..=MAX_PURSUIT_RESET_TICKS).contains(&self.pursuit_reset)
    }

    fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_bytes(SETTINGS_TAG);
        h.write_bytes(&self.scan_cooldown.to_le_bytes());
        h.write_fx(self.pursuit);
        h.write_bytes(&self.pursuit_reset.to_le_bytes());
    }
}

impl Default for TargetingConfig {
    fn default() -> TargetingConfig {
        TargetingConfig::DEFAULT
    }
}

/// What a unit remembers about choosing targets. At rest (the value of [`Engage::REST`]) for a unit that
/// has not.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Engage {
    /// The order the unit had when it picked its target, while its current order is
    /// [`Order::Pursue`]; `None` otherwise. `Some(Order::Idle)` for a unit that was idle.
    pub resume: Option<Order>,
    /// Whether the unit may walk after the target (an attacker beyond its reach) as opposed to
    /// firing from where it stands.
    pub chase: bool,
    /// What a following unit measures its pursuit from: the guard post for a guard, otherwise
    /// where the unit stood when its first chase began (kept across chases that follow straight
    /// after each other, see `held`).
    pub anchor: Vec2,
    /// Whether `anchor` still binds: set by every engagement, kept when it ends, and dropped once
    /// the unit has been at rest for `pursuit_reset` ticks or is given an order by its player.
    pub held: bool,
    /// Ticks the unit has been at rest (no `Pursue`, not hurt) while `held`.
    pub rest: u16,
    /// Where the target was at the end of the movement stage of the last tick. The unit walks and
    /// faces toward this, not toward the live position of the target, which another unit's step may
    /// already have changed this tick (so the slot order cannot matter).
    pub aim: Vec2,
    /// The range of the weapon that chose the target: how near the unit walks to it.
    pub reach: Fx,
    /// Ticks until the unit may scan again.
    pub cool: u16,
}

impl Engage {
    /// A unit that has not chosen a target and may scan at once.
    pub(crate) const REST: Engage = Engage {
        resume: None,
        chase: false,
        anchor: Vec2::ZERO,
        held: false,
        rest: 0,
        aim: Vec2::ZERO,
        reach: Fx::ZERO,
        cool: 0,
    };

    /// Forgets an engagement, keeping the cooldown (a new order does not make a unit look round
    /// sooner).
    pub(crate) fn forget(&mut self) {
        *self = Engage {
            cool: self.cool,
            ..Engage::REST
        };
    }
}

/// What a unit that has chosen a target is doing, for the presentation and the tests
/// ([`World::engagement_of`](crate::World::engagement_of)).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Engagement {
    /// The target.
    pub target: EntityId,
    /// Whether the unit may walk after it (it is following an attacker beyond its reach).
    pub following: bool,
    /// The order the unit goes back to when the engagement ends; [`Order::Idle`] for a unit that
    /// was idle.
    pub resumes: Order,
}

/// The scratch memory of the scans. Not state: two worlds are equal whatever it holds.
#[derive(Clone, Default)]
struct Scratch {
    index: Index,
    map: Option<(u32, u32)>,
    scanners: Vec<usize>,
}

impl PartialEq for Scratch {
    fn eq(&self, _: &Scratch) -> bool {
        true
    }
}

impl Eq for Scratch {}

/// The world's part: the settings (none: a world that does not choose targets) and the scratch.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Targeting {
    pub config: Option<TargetingConfig>,
    scratch: Scratch,
}

/// What [`Targeting`] reads besides the entities.
pub(crate) struct Context<'a> {
    pub units: &'a UnitTable,
    pub arsenal: &'a Arsenal,
    pub config: TargetingConfig,
    pub tick: Tick,
    /// The smallest and largest valid position (for beginning an order).
    pub bounds: (Vec2, Vec2),
    pub map: (u32, u32),
}

/// What the stance of a unit lets it do, from its current order.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Stance {
    Idle,
    Hold,
    Guard,
    Advance,
}

impl Stance {
    /// Whether a unit in this stance walks after an attacker that is out of reach.
    fn follows(self) -> bool {
        matches!(self, Stance::Idle | Stance::Guard)
    }
}

/// The stance of unit `i`; `None` for a unit that does not choose targets: one that walks a
/// move (or a legacy walk), has an attack order, or has chosen already.
fn stance_of(e: &Entities, i: usize) -> Option<Stance> {
    match e.order[i] {
        Order::Idle if e.behavior[i] == Behavior::Idle => Some(Stance::Idle),
        Order::HoldPosition => Some(Stance::Hold),
        Order::Guard(_) => Some(Stance::Guard),
        Order::AttackMove(_) => Some(Stance::Advance),
        _ => None,
    }
}

/// The key by which candidates are ranked, best first: a mode (0 in reach, 1 to be followed), the
/// score (more first), the squared distance, the position, the slot. A total order.
type Key = (u8, Reverse<u64>, i64, i64, i64, u32, u32);

fn key_of(e: &Entities, j: usize, mode: u8, score: u64, d2: Fx) -> Key {
    (
        mode,
        Reverse(score),
        d2.raw(),
        e.pos[j].y.raw(),
        e.pos[j].x.raw(),
        j as u32,
        e.generation[j],
    )
}

/// Whether unit `j` is something to shoot at: a unit or structure of the table that can be hurt
/// and has hit points left.
fn targetable(e: &Entities, j: usize) -> bool {
    e.alive[j] && e.def[j] != NO_DEF && e.hp[j] > 0
}

/// What unit `i` would measure a chase from if it began one now: a guard from its post, an idle unit
/// from where its first chase began while that still binds (so an attacker that hits and steps back
/// cannot lure it on chase by chase), otherwise from where it stands.
fn anchor_of(e: &Entities, i: usize) -> Vec2 {
    match e.order[i] {
        Order::Guard(GuardTarget::Point(p)) => p,
        Order::Guard(GuardTarget::Entity(id)) => live_slot(e, id).map_or(e.pos[i], |k| e.pos[k]),
        _ if e.eng[i].held => e.eng[i].anchor,
        _ => e.pos[i],
    }
}

/// Puts unit `i` on a `Pursue` order toward `j`, remembering the order it had.
fn engage(e: &mut Entities, i: usize, j: usize, chase: bool, reach: Fx, cx: &Context<'_>) {
    let resume = e.order[i];
    let anchor = anchor_of(e, i);
    let victim = EntityId::new(j as u32, e.generation[j]);
    orders::begin(e, i, Order::Pursue(victim), cx.tick, cx.bounds);
    e.eng[i] = Engage {
        resume: Some(resume),
        chase,
        anchor,
        held: true,
        rest: 0,
        aim: e.pos[j],
        reach,
        cool: cx.config.scan_cooldown,
    };
}

/// The engagement of unit `i` is over: it goes back to the order it had (or, if it was idle, to
/// the first order queued behind it, or to rest).
pub(crate) fn end_engagement(e: &mut Entities, i: usize, tick: Tick, bounds: (Vec2, Vec2)) {
    let Engage { resume, anchor, .. } = e.eng[i];
    e.eng[i].forget();
    match resume {
        Some(order) if order != Order::Idle => orders::begin(e, i, order, tick, bounds),
        _ => orders::finish(e, i, tick, bounds),
    }
    // The anchor outlives the engagement until the unit has been at rest a while.
    e.eng[i].anchor = anchor;
    e.eng[i].held = true;
    e.eng[i].rest = 0;
}

impl Targeting {
    /// Checks the engagements and lets the units that may look round do so. Runs after the crowd
    /// pass and before the turrets.
    pub(crate) fn update(&mut self, e: &mut Entities, cx: &Context<'_>) {
        let mut scanners = std::mem::take(&mut self.scratch.scanners);
        scanners.clear();
        for i in 0..e.len() {
            if !e.alive[i] || e.def[i] == NO_DEF {
                continue;
            }
            let stats = cx.units.at(e.def[i] as usize);
            if stats.class() != UnitClass::Mobile {
                continue;
            }
            e.eng[i].cool = e.eng[i].cool.saturating_sub(1);
            if stats.hp > 0 && e.hp[i] == 0 {
                fall(e, i, cx);
                continue;
            }
            if !cx.arsenal.is_armed(e.def[i]) {
                continue;
            }
            if let Order::Pursue(victim) = e.order[i] {
                check(e, i, victim, cx);
            } else if e.eng[i].held {
                let eng = &mut e.eng[i];
                eng.rest += 1;
                if eng.rest >= cx.config.pursuit_reset {
                    eng.held = false;
                    eng.rest = 0;
                    eng.anchor = Vec2::ZERO;
                }
            }
            if e.eng[i].cool == 0 && stance_of(e, i).is_some() {
                scanners.push(i);
            }
        }
        if !scanners.is_empty() {
            self.build_index(e, cx);
            for &i in &scanners {
                self.scan(e, i, cx);
            }
        }
        self.scratch.scanners = scanners;
    }

    /// The index of everything that can be shot at, in slot order.
    fn build_index(&mut self, e: &Entities, cx: &Context<'_>) {
        let s = &mut self.scratch;
        if s.map != Some(cx.map) {
            s.index.configure(INDEX_BUCKET, cx.map.0, cx.map.1);
            s.map = Some(cx.map);
        }
        s.index.build_with(e, e.len(), |j| {
            targetable(e, j).then(|| {
                (
                    false,
                    orders::radius_of_stats(cx.units.at(e.def[j] as usize)),
                )
            })
        });
    }

    /// Unit `i` looks for the best target in reach.
    fn scan(&mut self, e: &mut Entities, i: usize, cx: &Context<'_>) {
        e.eng[i].cool = cx.config.scan_cooldown;
        let def = e.def[i];
        let farthest = cx
            .arsenal
            .weapons_of(def)
            .map(|w| w.range)
            .max()
            .unwrap_or(Fx::ZERO);
        let around = Vec2::splat(farthest + cx.arsenal.widest());
        let mut best: Option<(Key, usize, Fx)> = None;
        for stretch in self
            .scratch
            .index
            .in_rect(e.pos[i] - around, e.pos[i] + around)
        {
            for near in stretch {
                let j = near.id as usize;
                if j == i || e.owner[j] == e.owner[i] {
                    continue;
                }
                let d2 = (near.at - e.pos[i]).length_squared();
                let Some(pick) = cx
                    .arsenal
                    .best_against(def, e.def[j], d2, near.radius, true)
                else {
                    continue;
                };
                let key = key_of(e, j, 0, pick.score, d2);
                if best.is_none_or(|(b, _, _)| key < b) {
                    best = Some((key, j, pick.reach));
                }
            }
        }
        if let Some((_, j, reach)) = best {
            engage(e, i, j, false, reach, cx);
        }
    }

    /// Turns the units that were hurt this tick on their attackers (`attacks` is who hurt whom,
    /// from the weapon stage). Runs after the weapons.
    pub(crate) fn return_fire(
        &mut self,
        e: &mut Entities,
        cx: &Context<'_>,
        attacks: &mut Vec<(u32, EntityId)>,
    ) {
        attacks.sort_unstable_by_key(|&(v, a)| (v, a.index(), a.generation()));
        attacks.dedup();
        let mut from = 0;
        while from < attacks.len() {
            let i = attacks[from].0 as usize;
            let n = attacks[from..]
                .iter()
                .take_while(|a| a.0 as usize == i)
                .count();
            let group = &attacks[from..from + n];
            from += n;
            if !e.alive[i] || e.def[i] == NO_DEF {
                continue;
            }
            let stats = cx.units.at(e.def[i] as usize);
            if stats.class() != UnitClass::Mobile {
                continue;
            }
            // Shot to zero on this tick: it stands where it fell from now on, not one step later.
            if stats.hp > 0 && e.hp[i] == 0 {
                fall(e, i, cx);
                continue;
            }
            if !cx.arsenal.is_armed(e.def[i]) {
                continue;
            }
            // Being hurt is not rest: a unit at the end of its leash that keeps being hit (and does
            // not follow) keeps measuring from where its first chase began.
            e.eng[i].rest = 0;
            let Some(stance) = stance_of(e, i) else {
                continue;
            };
            // A unit already at the end of its leash does not set out again (it would creep a step
            // further with every hit).
            let follow = stance.follows()
                && !cx.config.pursuit.is_zero()
                && (e.pos[i] - anchor_of(e, i)).length_squared() < cx.config.pursuit.square();
            let def = e.def[i];
            let mut best: Option<(Key, usize, bool, Fx)> = None;
            for &(_, attacker) in group {
                let Some(j) = live_slot(e, attacker) else {
                    continue;
                };
                if !targetable(e, j) || e.owner[j] == e.owner[i] {
                    continue;
                }
                let edge = radius_of(e, j, cx.units);
                let d2 = (e.pos[j] - e.pos[i]).length_squared();
                let (mode, pick) =
                    if let Some(pick) = cx.arsenal.best_against(def, e.def[j], d2, edge, true) {
                        (0, pick)
                    } else if follow
                        && let Some(pick) = cx.arsenal.best_against(def, e.def[j], d2, edge, false)
                        && d2 > (pick.reach + edge).square()
                        && d2 <= (pick.reach + edge + cx.config.pursuit).square()
                    {
                        (1, pick)
                    } else {
                        continue;
                    };
                let key = key_of(e, j, mode, pick.score, d2);
                if best.is_none_or(|(b, _, _, _)| key < b) {
                    best = Some((key, j, mode == 1, pick.reach));
                }
            }
            if let Some((_, j, chase, reach)) = best {
                engage(e, i, j, chase, reach, cx);
            }
        }
    }
}

/// Cells to a bucket of the index the scans look their candidates up in.
const INDEX_BUCKET: i64 = 4;

/// Unit `i` has chosen a target; checks that it still holds. A target that is gone, at zero hit
/// points, or (for a unit that does not follow) out of reach ends the engagement; a unit that
/// follows keeps going until the target is beyond its reach plus the pursuit distance, or the unit
/// has strayed that far from where it started. Otherwise the unit notes where the target is.
fn check(e: &mut Entities, i: usize, victim: EntityId, cx: &Context<'_>) {
    let Some(j) = live_slot(e, victim).filter(|&j| targetable(e, j)) else {
        end_engagement(e, i, cx.tick, cx.bounds);
        return;
    };
    let eng = e.eng[i];
    let edge = radius_of(e, j, cx.units);
    let d2 = (e.pos[j] - e.pos[i]).length_squared();
    let keep = if eng.chase {
        let strayed = (e.pos[i] - eng.anchor).length_squared() > cx.config.pursuit.square();
        let escaped = d2 > (eng.reach + edge + cx.config.pursuit).square();
        // In reach but no weapon can fire (inside its minimum range): walking closer will not
        // help, so it lets go instead of standing there for ever.
        let too_close = d2 <= (eng.reach + edge).square()
            && cx
                .arsenal
                .best_against(e.def[i], e.def[j], d2, edge, true)
                .is_none();
        !strayed && !escaped && !too_close
    } else {
        cx.arsenal
            .best_against(e.def[i], e.def[j], d2, edge, true)
            .is_some()
    };
    if keep {
        e.eng[i].aim = e.pos[j];
    } else {
        end_engagement(e, i, cx.tick, cx.bounds);
    }
}

/// A unit at zero hit points loses its orders and stays where it fell (`HoldPosition`, which
/// nobody pushes). Only in a world without destruction settings: with them (WP5.5) a unit at zero is
/// removed in the same tick, before this runs again.
fn fall(e: &mut Entities, i: usize, cx: &Context<'_>) {
    if e.order[i] != Order::HoldPosition || !e.queue[i].is_empty() || e.eng[i].resume.is_some() {
        orders::give(e, i, Order::HoldPosition, false, cx.tick, cx.bounds);
        e.turret_target[i] = None;
    }
    e.eng[i].forget();
}

/// Writes the targeting section of the checksum: the settings, then every live unit whose
/// engagement is not at rest.
pub(crate) fn write_hash(h: &mut Fnv1a64, config: &TargetingConfig, e: &Entities) {
    config.write_hash(h);
    let busy = |i: usize| e.alive[i] && e.eng[i] != Engage::REST;
    h.write_u64(u64::try_from((0..e.len()).filter(|&i| busy(i)).count()).unwrap_or(u64::MAX));
    for i in (0..e.len()).filter(|&i| busy(i)) {
        let eng = &e.eng[i];
        h.write_u32(i as u32);
        h.write_bytes(&eng.cool.to_le_bytes());
        h.write_u8(u8::from(eng.chase));
        match eng.resume {
            Some(order) => {
                h.write_u8(1);
                order.write_hash(h);
            }
            None => h.write_u8(0),
        }
        h.write_vec2(eng.anchor);
        h.write_u8(u8::from(eng.held));
        h.write_bytes(&eng.rest.to_le_bytes());
        h.write_vec2(eng.aim);
        h.write_fx(eng.reach);
    }
}

#[cfg(test)]
mod tests;
