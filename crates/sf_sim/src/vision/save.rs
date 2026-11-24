//! Section `V`: vision through save and restore (WP13.1c-3b, `docs/rules/save.md`).
//!
//! **Saved**: the stamp of every slot as the last pass left it (the length of the stamp column
//! too, and the stamps of slots that died after that pass), each with its `disc` flag; per player
//! whether it has a picture, its explored words and its version. **Rebuilt** from those: the
//! counts, the visible words, the disc counts, the running sums and the mixes, by adding the disc
//! of every stamp whose flag is set, exactly as the pass added it. The rebuild adds by integer
//! addition, which commutes, so it does not depend on the order the pass added in; the one thing
//! that did depend on order, which stamps the cap refused, is the saved flag. **Rules or derived
//! from rules**: the rules, the sight per table position and the discs (made in `Vision::new` for
//! every sight of the rules; the pass never makes another, see `restore`). `dirty` is false
//! between steps (`finish_pass` clears it at the end of every pass, and only the pass sets it).

use sf_math::bytes::{ByteReader, ByteWriter};

use super::{MAX_DISCS_PER_PLAYER, PlayerVision, Source, Stamp, Vision, bit, mix};
use crate::ids::{PlayerId, Tick};
use crate::save::{SaveResult, ensure, get_u64s, put_u64s};

impl Vision {
    /// Every saved field; destructured without `..`.
    pub(crate) fn save(&self, w: &mut ByteWriter) -> SaveResult<()> {
        let Vision {
            // Rules: the map size and the rules are in the rules digest; the sight per table
            // position follows from the rules and the unit table, the discs from the sights.
            width: _,
            height: _,
            rules: _,
            def_sight: _,
            masks: _,
            players,
            stamps,
        } = self;
        w.usize(stamps.len())?;
        for st in stamps {
            match st {
                None => w.u8(0),
                Some(Stamp { src, disc }) => {
                    let Source {
                        owner,
                        x,
                        y,
                        w: fw,
                        h: fh,
                        sight_milli,
                    } = *src;
                    w.u8(1);
                    w.u8(owner.0);
                    w.u32(x);
                    w.u32(y);
                    w.u8(fw);
                    w.u8(fh);
                    w.u32(sight_milli);
                    w.bool(*disc);
                }
            }
        }
        w.usize(players.len())?;
        for pv in players {
            match pv.as_deref() {
                None => w.u8(0),
                Some(PlayerVision {
                    // Derived: rebuilt from the stamps (see the module text).
                    count: _,
                    visible: _,
                    discs: _,
                    sum: _,
                    visible_cells: _,
                    visible_mix: _,
                    // Derived: from the explored words.
                    explored_cells: _,
                    explored_mix: _,
                    // False between steps.
                    dirty: _,
                    explored,
                    version,
                }) => {
                    w.u8(1);
                    w.u64(*version);
                    put_u64s(w, explored)?;
                }
            }
        }
        Ok(())
    }

    /// The vision of a world at tick `now` with `slots` entity slots, onto the fresh (empty)
    /// vision of the same rules. Refused when it is not one the pass leaves (the bounds, each with
    /// its writer, are in `docs/rules/save.md`).
    pub(crate) fn restore(
        &mut self,
        r: &mut ByteReader<'_>,
        slots: usize,
        now: Tick,
    ) -> SaveResult<()> {
        let n = r.count(1)?;
        // The pass syncs every slot, and `sync` grows the column to the slot it looks at; every slot
        // is made before the pass (`World::apply` runs the commands and `advance_production` hands
        // out units first in the step), and a slot is never taken away.
        ensure(n == slots, "the vision stamps are not one per slot")?;
        let mut stamps: Vec<Option<Stamp>> = Vec::with_capacity(n);
        for _ in 0..n {
            stamps.push(match r.u8()? {
                0 => None,
                1 => {
                    let src = Source {
                        owner: PlayerId(r.u8()?),
                        x: r.u32()?,
                        y: r.u32()?,
                        w: r.u8()?,
                        h: r.u8()?,
                        sight_milli: r.u32()?,
                    };
                    Some(Stamp {
                        src,
                        disc: r.bool()?,
                    })
                }
                _ => return Err(crate::save::SaveError::Corrupt("unknown vision stamp tag")),
            });
        }
        let words = self.cells().div_ceil(64);
        let np = r.count(1)?;
        // Players are indexed by `PlayerId.0`, and `player_mut` grows the list to the one it makes.
        ensure(np <= 256, "a vision player out of range")?;
        let mut players: Vec<Option<Box<PlayerVision>>> = Vec::with_capacity(np);
        for _ in 0..np {
            players.push(match r.u8()? {
                0 => None,
                1 => {
                    let version = r.u64()?;
                    let explored = get_u64s(r)?;
                    // A player is made by the pass that explores its first cell, which bumps its
                    // version; a pass bumps it at most once, and one pass runs per tick.
                    ensure(
                        version >= 1 && version <= now.0,
                        "a vision version the pass never reaches",
                    )?;
                    ensure(
                        explored.len() == words,
                        "explored words of the wrong length",
                    )?;
                    let mut pv = PlayerVision::new(self.cells());
                    pv.explored = explored.into_boxed_slice();
                    pv.version = version;
                    Some(Box::new(pv))
                }
                _ => return Err(crate::save::SaveError::Corrupt("unknown vision player tag")),
            });
        }
        ensure(
            players.last().is_none_or(Option::is_some),
            "the vision players end with a gap",
        )?;
        let cells = self.cells() as u32;
        for pv in players.iter_mut().flatten() {
            let tail = (words as u32 * 64).saturating_sub(cells);
            let off_map = (cells..cells + tail).any(|i| bit(&pv.explored, i));
            ensure(!off_map, "an explored cell off the map")?;
            let mut m = 0u64;
            let mut c = 0u32;
            for i in 0..cells {
                if bit(&pv.explored, i) {
                    m = m.wrapping_add(mix(i));
                    c += 1;
                }
            }
            pv.explored_cells = c;
            pv.explored_mix = m;
        }
        // The sights a stamp can have: what `vision_source` gives, the sight of a table position,
        // the legacy sight or nothing (a structure that does not stand). All of them above zero
        // have their disc from `Vision::new`, so the rebuild makes no new one.
        let sight_ok =
            |s: u32| s == 0 || s == self.rules.legacy_sight_milli || self.def_sight.contains(&s);
        let mut discs = vec![0u32; players.len()];
        for st in stamps.iter().flatten() {
            let s = st.src;
            ensure(
                s.w >= 1
                    && s.h >= 1
                    && u64::from(s.x) + u64::from(s.w) <= u64::from(self.width)
                    && u64::from(s.y) + u64::from(s.h) <= u64::from(self.height),
                "a vision stamp off the map",
            )?;
            ensure(
                sight_ok(s.sight_milli),
                "a vision stamp with a sight the rules do not give",
            )?;
            // `add_source` adds no disc for a sight of nothing.
            ensure(
                !st.disc || s.sight_milli > 0,
                "a vision disc without a sight",
            )?;
            let k = usize::from(s.owner.0);
            // `add_source` makes the player of every stamp it adds.
            ensure(
                players.get(k).is_some_and(Option::is_some),
                "a vision stamp of a player without a picture",
            )?;
            if st.disc {
                discs[k] += 1;
            }
        }
        // `add_source` refuses the disc past the cap.
        ensure(
            discs.iter().all(|&d| d <= MAX_DISCS_PER_PLAYER),
            "more vision discs than a count can hold",
        )?;
        self.players = players;
        self.stamps = stamps;
        // The rebuild: the footprint of every stamp explored and the disc of every flagged stamp
        // added, as `add_source` did. Both explore; a cell they explore that the saved words do
        // not hold was in sight (or a footprint) without being explored, which the pass never
        // leaves.
        let saved: Vec<Box<[u64]>> = self
            .players
            .iter()
            .map(|p| {
                p.as_ref()
                    .map_or_else(Default::default, |p| p.explored.clone())
            })
            .collect();
        for slot in 0..self.stamps.len() {
            let Some(st) = self.stamps[slot] else {
                continue;
            };
            self.explore_footprint(&st.src);
            if st.disc {
                self.player_mut(st.src.owner).discs += 1;
                self.apply_disc(&st.src, true);
            }
        }
        let same = self
            .players
            .iter()
            .zip(&saved)
            .all(|(p, s)| p.as_ref().is_none_or(|p| p.explored == *s));
        ensure(same, "a cell in sight that was never explored")?;
        for pv in self.players.iter_mut().flatten() {
            pv.dirty = false;
        }
        Ok(())
    }

    /// The coverage record of the section (`World::save_coverage`): `alive`, `building` and
    /// `selling` per slot.
    pub(crate) fn coverage(
        &self,
        alive: &[bool],
        selling: &[bool],
        building: &[bool],
        out: &mut Vec<&'static str>,
    ) {
        let mut add = |name: &'static str, on: bool| {
            if on && !out.contains(&name) {
                out.push(name);
            }
        };
        let c = self.contents(alive);
        add("vision.player", c.players > 0);
        add("vision.player.gap", c.player_gap);
        add("vision.stamp", c.stamps > 0);
        add("vision.stamp.dead", c.dead > 0);
        add("vision.stamp.blind", c.blind > 0);
        add("vision.stamp.refused", c.refused > 0);
        add("vision.fog", c.fog);
        add("vision.shared", c.shared_seen);
        let on = |k: &[bool], i: usize| k.get(i).copied().unwrap_or(false);
        let blind_at = |want: &[bool]| {
            self.stamps.iter().enumerate().any(|(i, st)| {
                st.is_some_and(|s| s.src.sight_milli == 0) && on(alive, i) && on(want, i)
            })
        };
        add("vision.stamp.selling", blind_at(selling));
        add("vision.stamp.building", blind_at(building));
    }

    /// What the vision holds, for the claims of the save tests.
    pub(crate) fn contents(&self, alive: &[bool]) -> VisionContents {
        let mut c = VisionContents::default();
        let present: Vec<bool> = self.players.iter().map(Option::is_some).collect();
        c.players = present.iter().filter(|&&p| p).count() as u32;
        c.player_gap = present.iter().any(|&p| !p);
        for (i, st) in self.stamps.iter().enumerate() {
            let Some(st) = st else { continue };
            c.stamps += 1;
            if !alive.get(i).copied().unwrap_or(false) {
                c.dead += 1;
            }
            if st.src.sight_milli == 0 {
                c.blind += 1;
            } else if !st.disc {
                c.refused += 1;
            }
        }
        c.fog = self.players.iter().flatten().any(|p| {
            p.explored
                .iter()
                .zip(p.visible.iter())
                .any(|(e, v)| e & !v != 0)
        });
        c.shared_seen = self.rules.shared.iter().any(|&(sharer, viewer)| {
            let (Some(a), b) = (self.player(sharer), self.player(viewer)) else {
                return false;
            };
            (0..self.cells() as u32)
                .any(|i| bit(&a.visible, i) && b.is_none_or(|b| !bit(&b.visible, i)))
        });
        c
    }
}

/// What a vision holds (`Vision::contents`).
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub(crate) struct VisionContents {
    /// Players with a picture.
    pub players: u32,
    /// A player index below the last without a picture.
    pub player_gap: bool,
    /// Slots with a stamp.
    pub stamps: u32,
    /// Stamps of slots that are not alive now (they died after the last pass).
    pub dead: u32,
    /// Stamps of a sight of nothing (a structure that does not stand, a unit without sight).
    pub blind: u32,
    /// Stamps with a sight whose disc the cap refused.
    pub refused: u32,
    /// A player has a cell explored and not visible.
    pub fog: bool,
    /// A viewer sees a cell only through a sharer.
    pub shared_seen: bool,
}
