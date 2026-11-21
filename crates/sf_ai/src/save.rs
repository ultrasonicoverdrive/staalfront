//! Saving and restoring an AI's state (WP13.1d, `docs/rules/save.md`, "The AI").
//!
//! The state is written with the byte writer of `sf_math` as one section of its own (tag [`AI_TAG`]) with its
//! own format version ([`AI_FORMAT_VERSION`]). The profile is rules: only its digest is written, and a restore
//! onto a fresh state with another profile is [`SaveError::Rules`]. The player and the start sites are what the
//! host hands over from the match, so they too must match the fresh state.
//!
//! Guard 1 of the design memo: [`AiState::save`] destructures the state without `..`, so a field added later
//! (a phase-9 work package) is a compile error here until it is saved and restored.
//!
//! Every value read back is checked against what the live AI can reach ([`SaveBounds`]: the world's tick, map,
//! entity slots and unit table, built by `observe.rs`, the one file that names the world). An entity id the AI
//! remembers may be dead or stale: the live AI keeps those and drops them at its next look, so a restore keeps
//! them too; it only refuses a slot index the world never had.

use std::collections::BTreeMap;

use sf_math::bytes::{ByteReader, ByteWriter};
use sf_math::{Rng, RngState};
use sf_sim::save::SaveError;
use sf_sim::{Cell, EntityId, Kind, PlayerId, Tick};

use crate::schedule::Manager;
use crate::state::{
    AiState, Directive, GROUPS_MAX, Group, GroupRole, GroupStage, HpSeen, Known, Posture,
    REFUSED_CAP, UnitSent,
};

/// The section tag of an AI's state.
pub const AI_TAG: u8 = b'A';

/// The format version of the AI section.
pub const AI_FORMAT_VERSION: u32 = 1;

/// What the world allows a restored AI state to hold. Built from the world the state is restored next to by
/// `Ai::restore` (`observe.rs`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveBounds {
    /// The world's tick: the next tick to run. Every think of the saved AI ran before it.
    pub tick: Tick,
    /// The map size in cells.
    pub map: (u32, u32),
    /// The world's entity slots (live plus free); a remembered id names one of them.
    pub slots: u32,
    /// The kinds of the world's unit table, ascending.
    pub kinds: Vec<Kind>,
}

type R<T> = Result<T, SaveError>;

fn bad(what: &'static str) -> SaveError {
    SaveError::Corrupt(what)
}

fn check(ok: bool, what: &'static str) -> R<()> {
    if ok { Ok(()) } else { Err(bad(what)) }
}

// --- writing -------------------------------------------------------------------------------------------------

fn put_id(w: &mut ByteWriter, id: EntityId) {
    w.u32(id.index());
    w.u32(id.generation());
}

fn put_cell(w: &mut ByteWriter, c: Cell) {
    w.u32(c.x);
    w.u32(c.y);
}

fn put_kind(w: &mut ByteWriter, k: Kind) {
    w.u16(k.0);
}

fn put_tick(w: &mut ByteWriter, t: Tick) {
    w.u64(t.0);
}

fn put_opt<T>(w: &mut ByteWriter, v: Option<T>, f: impl FnOnce(&mut ByteWriter, T)) {
    match v {
        Some(v) => {
            w.u8(1);
            f(w, v);
        }
        None => w.u8(0),
    }
}

fn put_hp_seen(w: &mut ByteWriter, map: &BTreeMap<EntityId, HpSeen>) -> R<()> {
    w.usize(map.len())?;
    for (id, (kind, cell, hp)) in map {
        put_id(w, *id);
        put_kind(w, *kind);
        put_cell(w, *cell);
        w.u32(*hp);
    }
    Ok(())
}

fn put_ids(w: &mut ByteWriter, ids: &[EntityId]) -> R<()> {
    w.usize(ids.len())?;
    for id in ids {
        put_id(w, *id);
    }
    Ok(())
}

fn put_until(w: &mut ByteWriter, map: &BTreeMap<EntityId, Tick>) -> R<()> {
    w.usize(map.len())?;
    for (id, t) in map {
        put_id(w, *id);
        put_tick(w, *t);
    }
    Ok(())
}

fn put_group(w: &mut ByteWriter, g: &Group) -> R<()> {
    let Group {
        id,
        role,
        members,
        goal,
        stage,
        since,
        launch_value,
        staging,
        merge_into,
        target_hp,
        recalled,
        closing,
    } = g;
    w.u32(*id);
    w.u8(*role as u8);
    put_ids(w, members)?;
    put_cell(w, *goal);
    w.u8(*stage as u8);
    put_tick(w, *since);
    w.u64(*launch_value);
    put_opt(w, *staging, put_cell);
    put_opt(w, *merge_into, |w, v| w.u32(v));
    put_opt(w, *target_hp, |w, (id, hp, t)| {
        put_id(w, id);
        w.u32(hp);
        put_tick(w, t);
    });
    put_ids(w, recalled)?;
    put_ids(w, closing)
}

impl AiState {
    /// Writes the state as one section (tag [`AI_TAG`]) onto `w`. The profile goes in as its digest only.
    pub fn save(&self, w: &mut ByteWriter) -> Result<(), SaveError> {
        w.section(AI_TAG, |w| self.save_body(w))
    }

    fn save_body(&self, w: &mut ByteWriter) -> R<()> {
        // Guard 1: no `..`. A new field is a compile error here until it is saved (or named as derived).
        let AiState {
            player,
            rng,
            profile,
            start_sites,
            thinks,
            last_think,
            // Derived: a function of `thinks` (one manager per think, in rotation). Checked on restore.
            manager_runs: _,
            known_structures,
            pending_build,
            blocked_until,
            refused,
            resume,
            previews,
            harvester_sent,
            full_turns,
            emergency,
            repair_backoff,
            stalled_turns,
            rescue_used,
            sales,
            rescue_after,
            gather,
            rally_backoff,
            units_sent,
            scout,
            scout_leg,
            scout_leg_sent,
            scouts_sent,
            scout_gone_at,
            hp_seen,
            threat,
            defend_since,
            directive,
            groups,
            next_group,
            last_attack_tick,
            attack_started,
            attacks_launched,
            jitter,
            sites_cleared,
            sent,
            intel,
            bounces,
            bounce_value,
            bounce_tick,
            harv_seen,
        } = self;
        w.u32(AI_FORMAT_VERSION);
        // Rules and match knowledge: compared with the fresh state, not restored.
        w.u64(profile.digest());
        w.u8(player.0);
        w.usize(start_sites.len())?;
        for (p, c) in start_sites {
            w.u8(p.0);
            put_cell(w, *c);
        }
        let RngState { state, inc } = rng.state();
        w.u64(state);
        w.u64(inc);
        w.u64(*thinks);
        put_opt(w, *last_think, put_tick);
        w.usize(known_structures.len())?;
        for (id, k) in known_structures {
            let Known {
                kind,
                owner,
                anchor,
                width,
                height,
                seen_tick,
            } = k;
            put_id(w, *id);
            put_kind(w, *kind);
            w.u8(owner.0);
            put_cell(w, *anchor);
            w.u8(*width);
            w.u8(*height);
            put_tick(w, *seen_tick);
        }
        put_opt(w, *pending_build, |w, (k, c, t)| {
            put_kind(w, k);
            put_cell(w, c);
            put_tick(w, t);
        });
        w.usize(blocked_until.len())?;
        for (k, t) in blocked_until {
            put_kind(w, *k);
            put_tick(w, *t);
        }
        w.usize(refused.len())?;
        for ((k, c), t) in refused {
            put_kind(w, *k);
            put_cell(w, *c);
            put_tick(w, *t);
        }
        w.usize(resume.len())?;
        for (k, n) in resume {
            put_kind(w, *k);
            w.u32(*n);
        }
        w.u64(*previews);
        put_opt(w, *harvester_sent, |w, (k, t)| {
            put_kind(w, k);
            put_tick(w, t);
        });
        w.u32(*full_turns);
        put_opt(w, *emergency, put_kind);
        put_until(w, repair_backoff)?;
        w.u32(*stalled_turns);
        w.bool(*rescue_used);
        w.usize(sales.len())?;
        for (id, t) in sales {
            put_id(w, *id);
            put_tick(w, *t);
        }
        put_tick(w, *rescue_after);
        put_opt(w, *gather, put_cell);
        put_until(w, rally_backoff)?;
        w.usize(units_sent.len())?;
        for (id, k, t, units, queued) in units_sent {
            put_id(w, *id);
            put_kind(w, *k);
            put_tick(w, *t);
            w.u32(*units);
            w.u32(*queued);
        }
        put_opt(w, *scout, put_id);
        w.u8(*scout_leg);
        put_opt(w, *scout_leg_sent, put_tick);
        w.u8(*scouts_sent);
        put_opt(w, *scout_gone_at, put_tick);
        put_hp_seen(w, hp_seen)?;
        put_opt(w, *threat, |w, (c, t)| {
            put_cell(w, c);
            put_tick(w, t);
        });
        put_opt(w, *defend_since, put_tick);
        let Directive { posture, target } = directive;
        w.u8(*posture as u8);
        put_opt(w, *target, put_cell);
        w.usize(groups.len())?;
        for g in groups {
            put_group(w, g)?;
        }
        w.u32(*next_group);
        put_tick(w, *last_attack_tick);
        put_tick(w, *attack_started);
        w.u32(*attacks_launched);
        put_opt(w, *jitter, |w, j| w.i32(j));
        w.usize(sites_cleared.len())?;
        for p in sites_cleared {
            w.u8(p.0);
        }
        w.usize(sent.len())?;
        for (id, (t, n)) in sent {
            put_id(w, *id);
            put_tick(w, *t);
            w.u8(*n);
        }
        w.usize(intel.len())?;
        for (k, (n, t)) in intel {
            put_kind(w, *k);
            w.u32(*n);
            put_tick(w, *t);
        }
        w.u32(*bounces);
        w.u64(*bounce_value);
        put_tick(w, *bounce_tick);
        put_hp_seen(w, harv_seen)
    }

    /// The state saved in the section at the front of `r`, restored onto `fresh`: the state the host made for
    /// the same player, profile and start sites exactly as for a new match (it must not have thought yet).
    /// The random stream comes from the bytes: the seed `fresh` was made with does not matter.
    ///
    /// Errors: [`SaveError::NotFresh`] when `fresh` has thought or holds anything a new state does not;
    /// [`SaveError::Version`] for another AI format; [`SaveError::Rules`] for another profile, player or start
    /// sites; [`SaveError::Corrupt`] for a value the live AI could not hold (see `docs/rules/save.md`);
    /// [`SaveError::Truncated`] when the bytes end early.
    pub fn restore(fresh: AiState, r: &mut ByteReader<'_>, bounds: &SaveBounds) -> R<AiState> {
        let blank = AiState::new(
            fresh.player,
            fresh.profile.clone(),
            0,
            fresh.start_sites.clone(),
        );
        let mut probe = fresh.clone();
        probe.rng = blank.rng.clone();
        if probe != blank {
            return Err(SaveError::NotFresh);
        }
        let mut body = r.section(AI_TAG)?;
        let s = Reader {
            r: &mut body,
            b: bounds,
        }
        .state(fresh)?;
        body.end()?;
        Ok(s)
    }
}

// --- reading -------------------------------------------------------------------------------------------------

struct Reader<'r, 'a> {
    r: &'r mut ByteReader<'a>,
    b: &'r SaveBounds,
}

impl Reader<'_, '_> {
    fn id(&mut self) -> R<EntityId> {
        let index = self.r.u32()?;
        let generation = self.r.u32()?;
        check(
            index < self.b.slots,
            "an entity id names a slot the world does not have",
        )?;
        Ok(EntityId::new(index, generation))
    }

    fn cell(&mut self) -> R<Cell> {
        let x = self.r.u32()?;
        let y = self.r.u32()?;
        check(
            x < self.b.map.0 && y < self.b.map.1,
            "a cell lies outside the map",
        )?;
        Ok(Cell::new(x, y))
    }

    fn kind(&mut self) -> R<Kind> {
        let k = Kind(self.r.u16()?);
        check(
            self.b.kinds.binary_search(&k).is_ok(),
            "a kind is not in the unit table",
        )?;
        Ok(k)
    }

    fn tick(&mut self) -> R<Tick> {
        Ok(Tick(self.r.u64()?))
    }

    fn flag(&mut self) -> R<bool> {
        Ok(self.r.bool()?)
    }

    fn opt<T>(&mut self, f: impl FnOnce(&mut Self) -> R<T>) -> R<Option<T>> {
        if self.flag()? {
            Ok(Some(f(self)?))
        } else {
            Ok(None)
        }
    }

    fn ids(&mut self) -> R<Vec<EntityId>> {
        let n = self.r.count(8)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.id()?);
        }
        check(
            v.windows(2).all(|p| p[0] < p[1]),
            "a list of entity ids is not strictly ascending",
        )?;
        Ok(v)
    }

    /// Entries of a map, keys strictly ascending (as a `BTreeMap` writes them): a key twice would merge.
    fn map<K: Ord + Copy, V>(
        &mut self,
        min: usize,
        mut entry: impl FnMut(&mut Self) -> R<(K, V)>,
        what: &'static str,
    ) -> R<BTreeMap<K, V>> {
        let n = self.r.count(min)?;
        let mut m = BTreeMap::new();
        let mut last: Option<K> = None;
        for _ in 0..n {
            let (k, v) = entry(self)?;
            check(last.is_none_or(|l| l < k), what)?;
            last = Some(k);
            m.insert(k, v);
        }
        Ok(m)
    }

    fn hp_seen(&mut self) -> R<BTreeMap<EntityId, HpSeen>> {
        self.map(
            22,
            |s| Ok((s.id()?, (s.kind()?, s.cell()?, s.r.u32()?))),
            "a list of structures seen is not strictly ascending",
        )
    }

    fn state(mut self, fresh: AiState) -> R<AiState> {
        let version = self.r.u32()?;
        if version != AI_FORMAT_VERSION {
            return Err(SaveError::Version(version));
        }
        if self.r.u64()? != fresh.profile.digest() {
            return Err(SaveError::Rules);
        }
        if self.r.u8()? != fresh.player.0 {
            return Err(SaveError::Rules);
        }
        let n = self.r.count(9)?;
        let mut sites = Vec::with_capacity(n);
        for _ in 0..n {
            sites.push((
                PlayerId(self.r.u8()?),
                Cell::new(self.r.u32()?, self.r.u32()?),
            ));
        }
        if sites != fresh.start_sites {
            return Err(SaveError::Rules);
        }
        let mut s = fresh;
        let profile = s.profile.clone();
        let (e, a) = (&profile.economy, &profile.army);
        let now = self.b.tick;

        let state = self.r.u64()?;
        let inc = self.r.u64()?;
        check(inc % 2 == 1, "the random stream has an even increment")?;
        s.rng = Rng::from_state(RngState { state, inc });

        s.thinks = self.r.u64()?;
        s.last_think = self.opt(|s| s.tick())?;
        check(
            s.thinks <= now.0,
            "more thinks than the world has run ticks",
        )?;
        check(
            (s.thinks == 0) == s.last_think.is_none(),
            "the last think and the number of thinks disagree",
        )?;
        check(
            s.last_think.is_none_or(|t| t < now),
            "the last think is not before the world's tick",
        )?;
        // The derived counts: one manager per think, in rotation.
        let all = Manager::ALL.len() as u64;
        for (m, runs) in s.manager_runs.iter_mut().enumerate() {
            let n = s.thinks / all + u64::from((m as u64) < s.thinks % all);
            *runs = u32::try_from(n).map_err(|_| bad("more thinks than a manager can count"))?;
        }
        // A thing the AI noted happened at a think: no later than its last one (0 before the first).
        let seen = s.last_think.unwrap_or(Tick(0));
        let past = |t: Tick, what| check(t <= seen, what);
        // A time a backoff runs out: a think plus `blocked_ticks`.
        let horizon = Tick(seen.0 + u64::from(e.blocked_ticks));
        let until = |t: Tick, what| check(t <= horizon, what);

        let me = s.player;
        s.known_structures = self.map(
            29,
            |r| {
                let id = r.id()?;
                let kind = r.kind()?;
                let owner = PlayerId(r.r.u8()?);
                let anchor = r.cell()?;
                let (width, height) = (r.r.u8()?, r.r.u8()?);
                let seen_tick = r.tick()?;
                check(owner != me, "a structure seen of the AI's own player")?;
                check(
                    width > 0
                        && height > 0
                        && anchor.x + u32::from(width) <= r.b.map.0
                        && anchor.y + u32::from(height) <= r.b.map.1,
                    "a structure seen does not fit the map",
                )?;
                Ok((
                    id,
                    Known {
                        kind,
                        owner,
                        anchor,
                        width,
                        height,
                        seen_tick,
                    },
                ))
            },
            "the structures seen are not strictly ascending",
        )?;
        for k in s.known_structures.values() {
            past(k.seen_tick, "a structure was seen after the last think")?;
        }
        s.pending_build = self.opt(|r| Ok((r.kind()?, r.cell()?, r.tick()?)))?;
        if let Some((_, _, t)) = s.pending_build {
            past(t, "a build was sent after the last think")?;
        }
        s.blocked_until = self.map(
            10,
            |r| Ok((r.kind()?, r.tick()?)),
            "the blocked kinds are not strictly ascending",
        )?;
        s.refused = self.map(
            18,
            |r| Ok(((r.kind()?, r.cell()?), r.tick()?)),
            "the refused places are not strictly ascending",
        )?;
        check(
            s.refused.len() <= REFUSED_CAP,
            "more refused places than the cap",
        )?;
        for t in s.blocked_until.values().chain(s.refused.values()) {
            until(*t, "a kind or place is blocked past what a think can set")?;
        }
        s.resume = self.map(
            6,
            |r| Ok((r.kind()?, r.r.u32()?)),
            "the resumptions are not strictly ascending",
        )?;
        s.previews = self.r.u64()?;
        s.harvester_sent = self.opt(|r| Ok((r.kind()?, r.tick()?)))?;
        if let Some((_, t)) = s.harvester_sent {
            past(t, "a harvester was sent after the last think")?;
        }
        s.full_turns = self.r.u32()?;
        s.emergency = self.opt(|r| r.kind())?;
        s.repair_backoff = self.map(
            16,
            |r| Ok((r.id()?, r.tick()?)),
            "the repair backoffs are not strictly ascending",
        )?;
        s.stalled_turns = self.r.u32()?;
        s.rescue_used = self.flag()?;
        let n = self.r.count(16)?;
        let mut sales = Vec::with_capacity(n);
        for _ in 0..n {
            let (id, t) = (self.id()?, self.tick()?);
            past(t, "a sale was sent after the last think")?;
            sales.push((id, t));
        }
        s.sales = sales;
        s.rescue_after = self.tick()?;
        until(s.rescue_after, "the rescue waits past what a think can set")?;
        s.gather = self.opt(|r| r.cell())?;
        s.rally_backoff = self.map(
            16,
            |r| Ok((r.id()?, r.tick()?)),
            "the rally backoffs are not strictly ascending",
        )?;
        for t in s.repair_backoff.values().chain(s.rally_backoff.values()) {
            until(*t, "a backoff runs past what a think can set")?;
        }
        let n = self.r.count(26)?;
        let mut units_sent: Vec<UnitSent> = Vec::with_capacity(n);
        for _ in 0..n {
            let u = (
                self.id()?,
                self.kind()?,
                self.tick()?,
                self.r.u32()?,
                self.r.u32()?,
            );
            past(u.2, "a unit order was sent after the last think")?;
            units_sent.push(u);
        }
        check(
            units_sent.windows(2).all(|p| p[0].0 < p[1].0),
            "the unit orders are not strictly ascending by factory",
        )?;
        s.units_sent = units_sent;
        s.scout = self.opt(|r| r.id())?;
        s.scout_leg = self.r.u8()?;
        // The legs of `army::waypoints`: the other start sites and four quarter points.
        let legs = s.start_sites.iter().filter(|(p, _)| *p != me).count() + 4;
        check(
            usize::from(s.scout_leg) <= legs,
            "the scout is past its last waypoint",
        )?;
        s.scout_leg_sent = self.opt(|r| r.tick())?;
        s.scouts_sent = self.r.u8()?;
        check(
            s.scouts_sent <= a.scouts_max.max(1),
            "more scouts sent than the profile allows",
        )?;
        s.scout_gone_at = self.opt(|r| r.tick())?;
        for t in [s.scout_leg_sent, s.scout_gone_at].into_iter().flatten() {
            past(t, "a scout time is after the last think")?;
        }
        s.hp_seen = self.hp_seen()?;
        s.threat = self.opt(|r| Ok((r.cell()?, r.tick()?)))?;
        s.defend_since = self.opt(|r| r.tick())?;
        for t in [s.threat.map(|t| t.1), s.defend_since]
            .into_iter()
            .flatten()
        {
            past(t, "a hit is after the last think")?;
        }
        let posture = match self.r.u8()? {
            0 => Posture::Gather,
            1 => Posture::Attack,
            2 => Posture::Defend,
            _ => return Err(bad("an unknown posture")),
        };
        let target = self.opt(|r| r.cell())?;
        s.directive = Directive { posture, target };

        let n = self.r.count(45)?;
        check(n <= GROUPS_MAX, "more groups than the most at one time")?;
        let mut groups = Vec::with_capacity(n);
        for _ in 0..n {
            groups.push(self.group()?);
        }
        s.next_group = self.r.u32()?;
        check(
            groups.windows(2).all(|p| p[0].id < p[1].id),
            "the groups are not strictly ascending by id",
        )?;
        check(
            groups.last().is_none_or(|g| g.id < s.next_group),
            "a group id is not below the next one",
        )?;
        for g in &groups {
            past(g.since, "a group entered its stage after the last think")?;
            if let Some((_, _, t)) = g.target_hp {
                past(t, "a group saw its target after the last think")?;
            }
            check(
                g.merge_into.is_none_or(|m| m < s.next_group && m != g.id),
                "a group merges into a group that never was",
            )?;
            // `army.rs` only adds to `recalled` and `closing` what it adds to (or already holds in) `members`
            // (lines 208, 1018-1021, 1141/1162-1190) and drops a unit from all three with the same `live` test
            // (lines 80-82), so both stay subsets of `members`.
            let member = |id: &EntityId| g.members.binary_search(id).is_ok();
            check(
                g.recalled.iter().all(member),
                "a group recalls a unit that is not its member",
            )?;
            check(
                g.closing.iter().all(member),
                "a group closes in with a unit that is not its member",
            )?;
        }
        let mut members: Vec<EntityId> = groups.iter().flat_map(|g| g.members.clone()).collect();
        members.sort();
        check(
            members.windows(2).all(|p| p[0] != p[1]),
            "a unit is in two groups",
        )?;
        s.groups = groups;
        s.last_attack_tick = self.tick()?;
        s.attack_started = self.tick()?;
        past(
            s.last_attack_tick,
            "an attack went out after the last think",
        )?;
        past(s.attack_started, "an attack started after the last think")?;
        s.attacks_launched = self.r.u32()?;
        s.jitter = self.opt(|r| Ok(r.r.i32()?))?;
        let spread = i32::from(a.timing_jitter_permille);
        check(
            s.jitter.is_none_or(|j| (-spread..=spread).contains(&j)),
            "the jitter is outside the profile's spread",
        )?;
        let n = self.r.count(1)?;
        let mut cleared = Vec::with_capacity(n);
        for _ in 0..n {
            cleared.push(PlayerId(self.r.u8()?));
        }
        check(
            cleared.windows(2).all(|p| p[0] < p[1]),
            "the cleared sites are not strictly ascending",
        )?;
        check(
            cleared
                .iter()
                .all(|p| *p != me && s.start_sites.iter().any(|(q, _)| q == p)),
            "a cleared site is not another player's start site",
        )?;
        s.sites_cleared = cleared;
        s.sent = self.map(
            17,
            |r| Ok((r.id()?, (r.tick()?, r.r.u8()?))),
            "the orders sent are not strictly ascending",
        )?;
        for (t, _) in s.sent.values() {
            past(*t, "a unit was ordered after the last think")?;
        }
        s.intel = self.map(
            14,
            |r| Ok((r.kind()?, (r.r.u32()?, r.tick()?))),
            "the tally is not strictly ascending",
        )?;
        for (_, t) in s.intel.values() {
            past(*t, "the tally saw a peak after the last think")?;
        }
        check(
            a.counter_on() || s.intel.is_empty(),
            "a tally with the counter mix off",
        )?;
        s.bounces = self.r.u32()?;
        s.bounce_value = self.r.u64()?;
        s.bounce_tick = self.tick()?;
        past(s.bounce_tick, "a strike failed after the last think")?;
        check(
            a.escalate_on() || (s.bounces, s.bounce_value, s.bounce_tick) == (0, 0, Tick(0)),
            "a bounce ledger with the escalation off",
        )?;
        s.harv_seen = self.hp_seen()?;
        check(
            a.harvester_threat || s.harv_seen.is_empty(),
            "harvesters seen with the harvester threat off",
        )?;
        Ok(s)
    }

    fn group(&mut self) -> R<Group> {
        let id = self.r.u32()?;
        let role = match self.r.u8()? {
            0 => GroupRole::Strike,
            _ => return Err(bad("an unknown group role")),
        };
        let members = self.ids()?;
        let goal = self.cell()?;
        let stage = match self.r.u8()? {
            0 => GroupStage::Staging,
            1 => GroupStage::Advancing,
            2 => GroupStage::Engaged,
            3 => GroupStage::Returning,
            _ => return Err(bad("an unknown group stage")),
        };
        let since = self.tick()?;
        let launch_value = self.r.u64()?;
        let staging = self.opt(|r| r.cell())?;
        let merge_into = self.opt(|r| Ok(r.r.u32()?))?;
        let target_hp = self.opt(|r| Ok((r.id()?, r.r.u32()?, r.tick()?)))?;
        let recalled = self.ids()?;
        let closing = self.ids()?;
        Ok(Group {
            id,
            role,
            members,
            goal,
            stage,
            since,
            launch_value,
            staging,
            merge_into,
            target_hp,
            recalled,
            closing,
        })
    }
}
