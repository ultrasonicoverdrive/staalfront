//! Shroud and fog of war (WP8.1, `docs/rules/vision.md`): what each player can see, per cell.
//!
//! The pass works out, for every player, which cells are *visible* (inside the sight of one of its
//! objects now), *explored* (visible at some time; never shrinks) and which are neither (*shroud*).
//! A cell that is explored but not visible is *fog*. [`World::visibility`] is what the rest of the
//! simulation asks. **Only one thing reads it** (WP8.2a): the targeting gate, when the rules say
//! `acquire_needs_sight` (off in everything that ships); targeting, defences, orders and production
//! otherwise behave exactly as before. The rules may also list `shared` pairs (a viewer sees what a
//! sharer sees); `visibility` is the union.
//!
//! **Sight.** Sight is kept in thousandths of a cell. A cell at offset `(dx, dy)` from the cell of an
//! object is inside its disc when `(1000 dx)^2 + (1000 dy)^2 <= sight^2`: the centre of the cell
//! counts, the edge of the disc is inside. Discs are precomputed once per distinct sight as integer
//! row spans ([`Mask`]); there is no square root of a float anywhere. A sight of 0 reveals nothing.
//!
//! **Who stamps what.**
//! * A mobile unit stamps the disc round the cell its position lies in. Outside the map: nothing.
//! * A structure stamps the disc **grown by its footprint**: a cell is in when its distance to the
//!   nearest cell of the footprint satisfies the disc test, so a 4 by 4 structure of sight 10 sees ten
//!   cells beyond each side, not ten cells from its anchor.
//! * A structure sees only while it **stands** ([`World::is_standing`](crate::World): complete, hit
//!   points left, not being sold). Under construction (and while sold) it sees nothing; that is the
//!   rule power and production use for "this structure counts".
//! * Every object of a player that has a cell marks **its own cell or footprint as explored**, even
//!   with a sight of 0 or while under construction: a player knows where its own things are.
//! * An entity made without a unit table entry (a placeholder wanderer, a legacy `Place` structure)
//!   has the rules' `legacy_sight_milli`. A legacy structure has no hit points and so never stands:
//!   it only marks its cell explored.
//! * A dead entity stamps nothing from the first pass after it was reaped. A slot freed and reused
//!   in between is handled the same way: the pass compares what the slot stamped with what the slot
//!   is now.
//!
//! **Counts.** Per player a `u16` per cell counts how many of its discs cover the cell; visible is
//! count above zero. Per slot the pass remembers the stamp it made (owner, cell, footprint, sight).
//! A pass walks the slots in ascending order, works out the stamp each *should* have and, only where
//! it differs from the remembered one, removes the old disc and adds the new. A disc is added and
//! removed with integer addition, which commutes, so the arrays do not depend on slot order (only the
//! cap below can, and only past 65,535 discs of one player).
//!
//! **The cap.** `u16` counts cannot overflow if one player has at most [`MAX_DISCS_PER_PLAYER`]
//! (65,535) live discs. A stamp that would be the 65,536th is **refused** (the object sees nothing,
//! only its own cells are explored) and retried each pass. Nothing ships near this; it keeps the
//! arithmetic honest.
//!
//! **The hash** (section `v`, only in a world given vision rules) covers the rules and, per player in
//! ascending order, a running sum `sum(index + 1) * count`, the number of visible cells with a mixed
//! sum of their indices, and the number of explored cells with the same mixed sum, all kept up to
//! date in constant time per change. It does not walk the arrays (a 256 by 256 map with 8 players
//! would cost a megabyte of hashing per tick); the full recompute lives in
//! [`World::vision_audit_current`], which the tests run every tick.
//!
//! **Where in the tick.** One pass right after the crowd pass and before targeting, so the gate
//! reads this tick's positions. Deaths of a tick are picked up by the next pass. A world without
//! vision rules has no pass and no section and hashes exactly as before.

use sf_math::hash::Fnv1a64;
use sf_math::{Fx, Vec2};

use crate::entities::NO_DEF;
use crate::ids::{Kind, PlayerId};
use crate::terrain::{Cell, NO_CELL};
use crate::world::{ConfigError, World};

/// The largest sight, in thousandths of a cell: 64 cells, the bound `unit_defs.ron` is checked to.
pub const MAX_SIGHT_MILLI: u32 = 64_000;

/// The most discs one player may have stamped at once, so that a `u16` count cannot overflow.
pub const MAX_DISCS_PER_PLAYER: u32 = u16::MAX as u32;

/// What a player knows about a cell.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Visibility {
    /// Never seen. (Ordered: shroud < fog < visible.)
    Shroud,
    /// Seen before, not now.
    Fog,
    /// Inside the sight of one of its objects now.
    Visible,
}

/// The sight of every kind, in thousandths of a cell (`sight` of `unit_defs.ron`, handed over by
/// `sf_bridge::vision_rules`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisionRules {
    /// The sight of an entity that has no unit table entry (placeholder wanderers, legacy
    /// structures), at most [`MAX_SIGHT_MILLI`].
    pub legacy_sight_milli: u32,
    /// `(kind, sight)`, strictly ascending by kind, each kind in the unit table. A kind of the table
    /// that is not listed has a sight of nothing.
    pub sights: Vec<(Kind, u32)>,
    /// WP8.2a, the targeting gate: when set, automatic acquisition (a unit's scan, a defence
    /// structure's look round) only picks candidates that are [`Visibility::Visible`] to the
    /// scanner's owner. Off by default; retaliation and explicit orders are never gated.
    pub acquire_needs_sight: bool,
    /// WP8.2a, shared vision: `(sharer, viewer)` pairs; the viewer also sees (and has explored)
    /// everything the sharer does. One way: the viewer's objects are not seen by the sharer unless
    /// the reverse pair is listed too. Strictly ascending, no pair with itself. Empty by default.
    pub shared: Vec<(PlayerId, PlayerId)>,
}

impl VisionRules {
    /// Rules with the sights only: the gate off and nothing shared (what WP8.1 had).
    pub fn new(legacy_sight_milli: u32, sights: Vec<(Kind, u32)>) -> VisionRules {
        VisionRules {
            legacy_sight_milli,
            sights,
            acquire_needs_sight: false,
            shared: Vec::new(),
        }
    }

    /// The same rules with the targeting gate set.
    pub fn with_acquire_needs_sight(mut self, on: bool) -> VisionRules {
        self.acquire_needs_sight = on;
        self
    }

    /// The same rules with these `(sharer, viewer)` pairs (must be strictly ascending).
    pub fn with_shared(mut self, shared: Vec<(PlayerId, PlayerId)>) -> VisionRules {
        self.shared = shared;
        self
    }

    /// Whether the numbers are inside their bounds, the kinds ascend without repeats and the
    /// shared pairs ascend without repeats or a player sharing with itself.
    pub fn is_valid(&self) -> bool {
        self.legacy_sight_milli <= MAX_SIGHT_MILLI
            && self.sights.iter().all(|&(_, s)| s <= MAX_SIGHT_MILLI)
            && self.sights.windows(2).all(|w| w[0].0 < w[1].0)
            && self.shared.iter().all(|&(a, b)| a != b)
            && self.shared.windows(2).all(|w| w[0] < w[1])
    }
}

/// The disc of one sight as integer row spans: `half[dy]` is the largest `dx` inside on a row `dy`
/// cells from the centre (the disc is symmetric in both axes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    half: Vec<u16>,
}

impl Mask {
    /// The disc for `sight_milli` thousandths of a cell: `(1000 dx)^2 + (1000 dy)^2 <= sight^2`. A
    /// sight of 0 has no cells at all (it reveals nothing); any sight above 0 has at least the centre.
    pub fn new(sight_milli: u32) -> Mask {
        let mut half = Vec::new();
        if sight_milli == 0 {
            return Mask { half };
        }
        let s2 = u64::from(sight_milli) * u64::from(sight_milli);
        let mut dy: u64 = 0;
        loop {
            let used = dy * dy * 1_000_000;
            if used > s2 {
                break;
            }
            let dx2 = (s2 - used) / 1_000_000;
            half.push(dx2.isqrt() as u16);
            dy += 1;
        }
        Mask { half }
    }

    /// Rows on one side of the centre row, the centre row not counted: the disc reaches this many
    /// cells up and down; nothing for a sight of nothing.
    pub fn radius(&self) -> u32 {
        self.half.len().saturating_sub(1) as u32
    }

    /// How many cells the disc has.
    pub fn cells(&self) -> u32 {
        let mut n = 0;
        for (dy, &h) in self.half.iter().enumerate() {
            let row = 2 * u32::from(h) + 1;
            n += if dy == 0 { row } else { 2 * row };
        }
        n
    }

    /// Whether the offset `(dx, dy)` is inside.
    pub fn contains(&self, dx: i32, dy: i32) -> bool {
        self.half
            .get(dy.unsigned_abs() as usize)
            .is_some_and(|&h| dx.unsigned_abs() <= u32::from(h))
    }

    /// The largest `dx` inside on row `dy` from the centre; `None` beyond the disc.
    pub fn half_width(&self, dy: u32) -> Option<u32> {
        self.half.get(dy as usize).map(|&h| u32::from(h))
    }
}

/// What one slot stamps: the cell (anchor) and footprint it stamps from and the sight, zero for an
/// object that sees nothing but knows where it stands.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Source {
    pub owner: PlayerId,
    pub x: u32,
    pub y: u32,
    pub w: u8,
    pub h: u8,
    pub sight_milli: u32,
}

/// A stamp a slot made: what it was made from and whether a disc was actually added (a refused
/// stamp, see the cap, has none).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Stamp {
    src: Source,
    disc: bool,
}

/// What one player knows. See the module text for the sums.
#[derive(Clone, PartialEq, Eq, Debug)]
struct PlayerVision {
    count: Box<[u16]>,
    /// Bit `i % 64` of word `i / 64`: `count[i] > 0`, kept on every crossing of zero.
    visible: Box<[u64]>,
    /// Same layout; bits are only ever set.
    explored: Box<[u64]>,
    discs: u32,
    sum: u64,
    visible_cells: u32,
    visible_mix: u64,
    explored_cells: u32,
    explored_mix: u64,
    /// Bumped once by every pass that changed the visible or explored set of this player.
    version: u64,
    dirty: bool,
}

/// A fixed mixing of a cell index, so that sums of it tell sets apart (splitmix64 finaliser).
fn mix(index: u32) -> u64 {
    let mut z = u64::from(index).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// `sum(i + 1)` for `i` in `a..=b`.
fn row_sum(a: u32, b: u32) -> u64 {
    let len = u64::from(b - a) + 1;
    len * (u64::from(a) + u64::from(b) + 2) / 2
}

fn bit(words: &[u64], i: u32) -> bool {
    words[(i / 64) as usize] >> (i % 64) & 1 == 1
}

impl PlayerVision {
    fn new(cells: usize) -> PlayerVision {
        let words = cells.div_ceil(64);
        PlayerVision {
            count: vec![0; cells].into_boxed_slice(),
            visible: vec![0; words].into_boxed_slice(),
            explored: vec![0; words].into_boxed_slice(),
            discs: 0,
            sum: 0,
            visible_cells: 0,
            visible_mix: 0,
            explored_cells: 0,
            explored_mix: 0,
            version: 0,
            dirty: false,
        }
    }

    /// Marks cell `i` explored.
    fn explore(&mut self, i: u32) {
        let (w, b) = ((i / 64) as usize, 1u64 << (i % 64));
        if self.explored[w] & b == 0 {
            self.explored[w] |= b;
            self.explored_cells += 1;
            self.explored_mix = self.explored_mix.wrapping_add(mix(i));
            self.dirty = true;
        }
    }

    /// Adds one to the cells `a..=b` (one row).
    fn add_row(&mut self, a: u32, b: u32) {
        for i in a..=b {
            let c = &mut self.count[i as usize];
            if *c == 0 {
                self.visible[(i / 64) as usize] |= 1u64 << (i % 64);
                self.visible_cells += 1;
                self.visible_mix = self.visible_mix.wrapping_add(mix(i));
                self.dirty = true;
                // A cell that becomes visible is explored.
                let (w, bt) = ((i / 64) as usize, 1u64 << (i % 64));
                if self.explored[w] & bt == 0 {
                    self.explored[w] |= bt;
                    self.explored_cells += 1;
                    self.explored_mix = self.explored_mix.wrapping_add(mix(i));
                }
            }
            *c += 1;
        }
        self.sum = self.sum.wrapping_add(row_sum(a, b));
    }

    /// Takes one from the cells `a..=b` (one row).
    fn remove_row(&mut self, a: u32, b: u32) {
        for i in a..=b {
            let c = &mut self.count[i as usize];
            *c -= 1;
            if *c == 0 {
                self.visible[(i / 64) as usize] &= !(1u64 << (i % 64));
                self.visible_cells -= 1;
                self.visible_mix = self.visible_mix.wrapping_sub(mix(i));
                self.dirty = true;
            }
        }
        self.sum = self.sum.wrapping_sub(row_sum(a, b));
    }
}

/// The state of vision: the rules, the discs, what every player knows and what every slot stamped.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Vision {
    width: u32,
    height: u32,
    rules: VisionRules,
    /// Sight per unit table position.
    def_sight: Vec<u32>,
    /// The discs of the distinct sights seen so far, ascending by sight.
    masks: Vec<(u32, Mask)>,
    /// Indexed by `PlayerId.0`, made the first time a player has an object.
    players: Vec<Option<Box<PlayerVision>>>,
    /// Indexed by slot.
    stamps: Vec<Option<Stamp>>,
}

impl Vision {
    pub(crate) fn new(width: u32, height: u32, rules: VisionRules, def_sight: Vec<u32>) -> Vision {
        let mut v = Vision {
            width,
            height,
            rules,
            def_sight,
            masks: Vec::new(),
            players: Vec::new(),
            stamps: Vec::new(),
        };
        // The discs of the shipped sights, made once; others are made the first time they are used.
        let mut sights: Vec<u32> = v.def_sight.clone();
        sights.push(v.rules.legacy_sight_milli);
        sights.sort_unstable();
        sights.dedup();
        for s in sights.into_iter().filter(|&s| s > 0) {
            v.mask_index(s);
        }
        v
    }

    pub(crate) fn legacy_sight(&self) -> u32 {
        self.rules.legacy_sight_milli
    }

    pub(crate) fn sight_of_def(&self, def: usize) -> u32 {
        self.def_sight.get(def).copied().unwrap_or(0)
    }

    pub(crate) fn rules(&self) -> &VisionRules {
        &self.rules
    }

    /// The disc of `sight_milli` (above zero), made if it is new.
    fn mask_index(&mut self, sight_milli: u32) -> usize {
        match self.masks.binary_search_by_key(&sight_milli, |&(s, _)| s) {
            Ok(k) => k,
            Err(k) => {
                self.masks.insert(k, (sight_milli, Mask::new(sight_milli)));
                k
            }
        }
    }

    fn cells(&self) -> usize {
        self.width as usize * self.height as usize
    }

    fn player_mut(&mut self, p: PlayerId) -> &mut PlayerVision {
        let k = usize::from(p.0);
        if self.players.len() <= k {
            self.players.resize_with(k + 1, || None);
        }
        let cells = self.cells();
        self.players[k].get_or_insert_with(|| Box::new(PlayerVision::new(cells)))
    }

    fn player(&self, p: PlayerId) -> Option<&PlayerVision> {
        self.players.get(usize::from(p.0))?.as_deref()
    }

    /// Adds (`add`) or removes the disc of `src`, clipped to the map.
    fn apply_disc(&mut self, src: &Source, add: bool) {
        let m = self.mask_index(src.sight_milli);
        let (w, h) = (i64::from(self.width), i64::from(self.height));
        let k = usize::from(src.owner.0);
        let mask = &self.masks[m].1;
        let Some(Some(pv)) = self.players.get_mut(k) else {
            return;
        };
        let r = i64::from(mask.radius());
        let (x0, y0) = (i64::from(src.x), i64::from(src.y));
        let (fw, fh) = (i64::from(src.w), i64::from(src.h));
        let (y_first, y_last) = (y0 - r, y0 + fh - 1 + r);
        for y in y_first.max(0)..=y_last.min(h - 1) {
            let ddy = if y < y0 {
                y0 - y
            } else if y > y0 + fh - 1 {
                y - (y0 + fh - 1)
            } else {
                0
            };
            let half = i64::from(mask.half[ddy as usize]);
            let xa = (x0 - half).max(0);
            let xb = (x0 + fw - 1 + half).min(w - 1);
            if xa > xb {
                continue;
            }
            let (a, b) = ((y * w + xa) as u32, (y * w + xb) as u32);
            if add {
                pv.add_row(a, b);
            } else {
                pv.remove_row(a, b);
            }
        }
    }

    /// Marks the footprint of `src` explored.
    fn explore_footprint(&mut self, src: &Source) {
        let w = self.width;
        let pv = self.player_mut(src.owner);
        for dy in 0..u32::from(src.h) {
            for dx in 0..u32::from(src.w) {
                pv.explore((src.y + dy) * w + src.x + dx);
            }
        }
    }

    /// Adds the stamp of `src`: its disc if it has a sight and the player has room, and its own
    /// cells explored. Returns whether a disc was added.
    fn add_source(&mut self, src: &Source) -> bool {
        self.explore_footprint(src);
        if src.sight_milli == 0 {
            return false;
        }
        let pv = self.player_mut(src.owner);
        if pv.discs >= MAX_DISCS_PER_PLAYER {
            return false;
        }
        pv.discs += 1;
        self.apply_disc(src, true);
        true
    }

    fn remove_source(&mut self, src: &Source) {
        self.apply_disc(src, false);
        self.player_mut(src.owner).discs -= 1;
    }

    /// Brings the stamp of `slot` in line with `src`, touching nothing if they already agree.
    pub(crate) fn sync(&mut self, slot: usize, src: Option<Source>) {
        if self.stamps.len() <= slot {
            self.stamps.resize(slot + 1, None);
        }
        let cur = self.stamps[slot];
        let unchanged = match (cur, src) {
            (None, None) => true,
            // A refused disc (the cap) is tried again each pass.
            (Some(c), Some(s)) => c.src == s && (c.disc || s.sight_milli == 0),
            _ => false,
        };
        if unchanged {
            return;
        }
        if let Some(c) = cur
            && c.disc
        {
            self.remove_source(&c.src);
        }
        self.stamps[slot] = src.map(|s| Stamp {
            src: s,
            disc: self.add_source(&s),
        });
    }

    /// Ends a pass: every player whose sets changed gets a new version.
    pub(crate) fn finish_pass(&mut self) {
        for pv in self.players.iter_mut().flatten() {
            if pv.dirty {
                pv.dirty = false;
                pv.version += 1;
            }
        }
    }

    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_u32(self.rules.legacy_sight_milli);
        h.write_u64(self.rules.sights.len() as u64);
        for &(kind, s) in &self.rules.sights {
            h.write_bytes(&kind.0.to_le_bytes());
            h.write_u32(s);
        }
        // WP8.2a: only when something is set, so rules without the gate and without sharing hash
        // exactly as they did in WP8.1.
        if self.rules.acquire_needs_sight || !self.rules.shared.is_empty() {
            h.write_u8(b'g');
            h.write_u8(u8::from(self.rules.acquire_needs_sight));
            h.write_u64(self.rules.shared.len() as u64);
            for &(a, b) in &self.rules.shared {
                h.write_u8(a.0);
                h.write_u8(b.0);
            }
        }
        let n = self.players.iter().flatten().count();
        h.write_u64(n as u64);
        for (id, pv) in self.players.iter().enumerate() {
            let Some(pv) = pv else { continue };
            h.write_u8(id as u8);
            h.write_u32(pv.discs);
            h.write_u64(pv.sum);
            h.write_u32(pv.visible_cells);
            h.write_u64(pv.visible_mix);
            h.write_u32(pv.explored_cells);
            h.write_u64(pv.explored_mix);
        }
    }

    /// What `player` knows about the cell at `index`: its own picture united with that of every
    /// player that shares vision with it (`rules.shared`).
    pub(crate) fn visibility(&self, player: PlayerId, index: u32) -> Visibility {
        let mut best = self.own_visibility(player, index);
        for &(sharer, viewer) in &self.rules.shared {
            if viewer == player && best != Visibility::Visible {
                best = best.max(self.own_visibility(sharer, index));
            }
        }
        best
    }

    /// What `player` itself knows about the cell, shared vision not counted.
    fn own_visibility(&self, player: PlayerId, index: u32) -> Visibility {
        let Some(pv) = self.player(player) else {
            return Visibility::Shroud;
        };
        if bit(&pv.visible, index) {
            Visibility::Visible
        } else if bit(&pv.explored, index) {
            Visibility::Fog
        } else {
            Visibility::Shroud
        }
    }

    /// Whether the targeting gate is on (`rules.acquire_needs_sight`).
    pub(crate) fn gate_on(&self) -> bool {
        self.rules.acquire_needs_sight
    }

    /// Whether any cell of the `w` by `h` rectangle at `(x, y)` is visible to `player` (cells
    /// outside the map do not count).
    pub(crate) fn sees_rect(&self, player: PlayerId, x: u32, y: u32, w: u32, h: u32) -> bool {
        for cy in y..(y + h).min(self.height) {
            for cx in x..(x + w).min(self.width) {
                if self.visibility(player, cy * self.width + cx) == Visibility::Visible {
                    return true;
                }
            }
        }
        false
    }

    /// Whether `player` sees one cell now (a cell outside the map is never seen).
    fn sees_cell(&self, player: PlayerId, x: u32, y: u32) -> bool {
        x < self.width
            && y < self.height
            && self.visibility(player, y * self.width + x) == Visibility::Visible
    }

    /// Whether `player` sees a structure whose footprint is the `w` by `h` rectangle at `(x, y)`,
    /// **measured to its edge** (WP7.7): any cell of the footprint, or a cell that shares a side with
    /// it, is visible. The cell around a footprint reaches the edge of the structure the way a weapon
    /// reaches the edge of its target: the visible cell is half a cell wide on each side of its
    /// centre, so a visible cell that touches the footprint has the structure's edge inside the
    /// sight. Cells that touch only at a corner do not count (a weapon measures to the inscribed
    /// circle, not to the corner).
    pub(crate) fn sees_footprint_edge(
        &self,
        player: PlayerId,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> bool {
        self.sees_rect(player, x, y, w, h)
            || (x > 0 && self.sees_rect(player, x - 1, y, 1, h))
            || self.sees_rect(player, x + w, y, 1, h)
            || (y > 0 && self.sees_rect(player, x, y - 1, w, 1))
            || self.sees_rect(player, x, y + h, w, 1)
    }

    /// Whether `player` sees a unit whose body is the disc of radius `radius` round `pos`, **measured
    /// to its edge** (WP7.7): the cell its position lies in, or a neighbour cell (including a diagonal
    /// one) that the disc reaches, is visible. A cell counts when the distance from `pos` to the
    /// square of the cell is at most `radius`, squared on both sides, the test `in_reach` of the
    /// weapons makes (`<=`, edge included). A unit that is nowhere near the border of its cell is
    /// seen through its own cell only.
    pub(crate) fn sees_body(&self, player: PlayerId, pos: Vec2, radius: Fx) -> bool {
        let (cx, cy) = pos.cell();
        let (Ok(x), Ok(y)) = (u32::try_from(cx), u32::try_from(cy)) else {
            return false;
        };
        if self.sees_cell(player, x, y) {
            return true;
        }
        if radius <= Fx::ZERO {
            return false;
        }
        // How far `pos` is from the left and top border of its cell (from zero up to, not including,
        // one) and from the right and bottom one.
        let left = pos.x - Fx::from_int(cx);
        let top = pos.y - Fx::from_int(cy);
        let reach = radius.square();
        for (dy, gap_y) in [(-1i32, top), (0, Fx::ZERO), (1, Fx::ONE - top)] {
            for (dx, gap_x) in [(-1i32, left), (0, Fx::ZERO), (1, Fx::ONE - left)] {
                if (dx, dy) == (0, 0) || gap_x.square() + gap_y.square() > reach {
                    continue;
                }
                let (Ok(nx), Ok(ny)) = (u32::try_from(cx + dx), u32::try_from(cy + dy)) else {
                    continue;
                };
                if self.sees_cell(player, nx, ny) {
                    return true;
                }
            }
        }
        false
    }

    /// The number of discs `player` has stamped.
    pub(crate) fn discs(&self, player: PlayerId) -> u32 {
        self.player(player).map_or(0, |p| p.discs)
    }

    /// Checks that the arrays, bitsets and sums of every player are what the stamps in `self` make
    /// of them (valid at any time, the stamps being what the slots last did).
    pub(crate) fn audit(&self) -> Result<(), String> {
        let mut fresh = Vision::new(
            self.width,
            self.height,
            self.rules.clone(),
            self.def_sight.clone(),
        );
        fresh.stamps.resize(self.stamps.len(), None);
        for (slot, st) in self.stamps.iter().enumerate() {
            let Some(st) = st else { continue };
            // A disc the cap refused is not in the live arrays either.
            let mut src = st.src;
            if !st.disc {
                src.sight_milli = 0;
            }
            fresh.sync(slot, Some(src));
        }
        fresh.finish_pass();
        self.compare(&fresh)
    }

    /// Compares with a vision built from scratch from the same stamps or entities. The counts,
    /// visible set and sums must be equal; the explored set of `self` may hold more than `fresh`
    /// (a history that `fresh` cannot know) but never less, and must agree with its own sums.
    pub(crate) fn compare(&self, fresh: &Vision) -> Result<(), String> {
        let n = self.players.len().max(fresh.players.len());
        for id in 0..n {
            let a = self.players.get(id).and_then(|p| p.as_deref());
            let b = fresh.players.get(id).and_then(|p| p.as_deref());
            let (a, b) = match (a, b) {
                (Some(a), Some(b)) => (a, b),
                (None, None) => continue,
                (Some(a), None) if a.visible_cells == 0 && a.discs == 0 => continue,
                _ => return Err(format!("player {id}: present in one and not the other")),
            };
            if a.count != b.count {
                let i = a.count.iter().zip(b.count.iter()).position(|(x, y)| x != y);
                return Err(format!("player {id}: counts differ, first at cell {i:?}"));
            }
            if a.visible != b.visible
                || a.sum != b.sum
                || a.visible_cells != b.visible_cells
                || a.visible_mix != b.visible_mix
                || a.discs != b.discs
            {
                return Err(format!("player {id}: visible set or its sums differ"));
            }
            if a.explored
                .iter()
                .zip(b.explored.iter())
                .any(|(x, y)| y & !x != 0)
            {
                return Err(format!(
                    "player {id}: explored lacks a cell that is in sight"
                ));
            }
            // The explored sums are what the bits say, whatever the history.
            let bits: u32 = a.explored.iter().map(|w| w.count_ones()).sum();
            if bits != a.explored_cells {
                return Err(format!(
                    "player {id}: explored count {} but {bits} bits",
                    a.explored_cells
                ));
            }
            let mut m = 0u64;
            for i in 0..a.count.len() as u32 {
                if bit(&a.explored, i) {
                    m = m.wrapping_add(mix(i));
                }
                if bit(&a.visible, i) && !bit(&a.explored, i) {
                    return Err(format!("player {id}: cell {i} visible and not explored"));
                }
            }
            if m != a.explored_mix {
                return Err(format!("player {id}: explored mix differs from its bits"));
            }
        }
        Ok(())
    }
}

impl World {
    /// The same world with vision (WP8.1, `docs/rules/vision.md`): each player works out which cells
    /// it sees and has seen. Call it after the unit table and before the world starts. Refused with
    /// [`ConfigError::Vision`] after the start, twice, with a kind the unit table does not have, with
    /// kinds out of order or a sight above 64 cells; a unit table given after it is refused too (the
    /// sight of each table position was frozen). A world without a unit table may be given rules
    /// with no kinds: every entity then has the legacy sight. A world without it has no
    /// pass and no section and hashes as before; [`World::visibility`] then says everything is
    /// visible.
    pub fn with_vision(mut self, rules: VisionRules) -> Result<World, ConfigError> {
        if self.tick != crate::ids::Tick::ZERO
            || self.entities.len() != 0
            || !self.queue.is_empty()
            || self.vision.is_some()
            || !rules.is_valid()
            || rules
                .sights
                .iter()
                .any(|&(kind, _)| self.units.index_of(kind).is_none())
        {
            return Err(ConfigError::Vision);
        }
        let mut def_sight = vec![0u32; self.units.len()];
        for &(kind, s) in &rules.sights {
            if let Some(def) = self.units.index_of(kind) {
                def_sight[def] = s;
            }
        }
        self.vision = Some(Box::new(Vision::new(
            self.config.width,
            self.config.height,
            rules,
            def_sight,
        )));
        Ok(self)
    }

    /// The vision rules; `None` for a world without them.
    pub fn vision_rules(&self) -> Option<&VisionRules> {
        self.vision.as_deref().map(Vision::rules)
    }

    /// What `player` knows about `cell` as of the last pass, united with what every player that
    /// shares vision with it knows (WP8.2a). The targeting gate reads it when the rules ask. In a world without vision rules everything is [`Visibility::Visible`]; in one with
    /// them a cell outside the map, and any cell of a player that has no object yet, is
    /// [`Visibility::Shroud`].
    pub fn visibility(&self, player: PlayerId, cell: Cell) -> Visibility {
        let Some(v) = self.vision.as_deref() else {
            return Visibility::Visible;
        };
        if cell.x >= v.width || cell.y >= v.height {
            return Visibility::Shroud;
        }
        v.visibility(player, cell.y * v.width + cell.x)
    }

    /// The version of what `player` sees: bumped by every pass that changed its visible or explored
    /// cells. `None` without vision rules; zero for a player that has seen nothing. The snapshot
    /// copies the bitsets only when this changes.
    pub fn fog_version(&self, player: PlayerId) -> Option<u64> {
        let v = self.vision.as_deref()?;
        Some(v.player(player).map_or(0, |p| p.version))
    }

    /// The visible and the explored bitsets of `player`: bit `i % 64` of word `i / 64` is the cell
    /// at row-major index `i = y * width + x`. `None` without vision rules or for a player that has
    /// not had an object yet (everything is shroud).
    pub fn fog_words(&self, player: PlayerId) -> Option<(&[u64], &[u64])> {
        let pv = self.vision.as_deref()?.player(player)?;
        Some((&pv.visible, &pv.explored))
    }

    /// The number of discs `player` has stamped (zero without vision rules).
    pub fn vision_discs(&self, player: PlayerId) -> u32 {
        self.vision.as_deref().map_or(0, |v| v.discs(player))
    }

    /// A hash of the vision section alone (what the checksum adds in a world with vision rules);
    /// `None` without them. For tests and tools.
    pub fn vision_digest(&self) -> Option<u64> {
        let v = self.vision.as_deref()?;
        let mut h = Fnv1a64::new();
        v.write_hash(&mut h);
        Some(h.finish())
    }

    /// Checks that the counts, bitsets and sums of every player are what the stamps the slots made
    /// give, by building them again from scratch. Valid at any time. For tests.
    #[doc(hidden)]
    pub fn vision_audit(&self) -> Result<(), String> {
        match self.vision.as_deref() {
            Some(v) => v.audit(),
            None => Ok(()),
        }
    }

    /// Checks that the vision is what the entities in the world **now** make of it: a vision built
    /// from scratch from the present entities has the same counts, visible set and sums, and the
    /// explored set only lacks cells that no longer matter. It runs the pass on **a clone** first
    /// (the deaths of the weapon stage are only picked up by the next pass), so it never changes
    /// this world: there is no public way to run the pass outside a step. For tests.
    #[doc(hidden)]
    pub fn vision_audit_current(&self) -> Result<(), String> {
        let mut c = self.clone();
        c.update_vision();
        c.audit_current_after_pass()
    }

    fn audit_current_after_pass(&self) -> Result<(), String> {
        let Some(v) = self.vision.as_deref() else {
            return Ok(());
        };
        let mut fresh = Vision::new(v.width, v.height, v.rules.clone(), v.def_sight.clone());
        for i in 0..self.entities.len() {
            fresh.sync(i, self.vision_source(&fresh, i));
        }
        fresh.finish_pass();
        v.compare(&fresh)?;
        // Every slot's remembered stamp is the one the entity gives now.
        for i in 0..self.entities.len() {
            let want = self.vision_source(v, i);
            let have = v.stamps.get(i).copied().flatten().map(|s| s.src);
            if want != have {
                return Err(format!("slot {i}: stamped {have:?}, entity gives {want:?}"));
            }
        }
        Ok(())
    }

    /// What the entity in slot `i` stamps now; `None` if it stamps nothing.
    fn vision_source(&self, v: &Vision, i: usize) -> Option<Source> {
        let e = &self.entities;
        if !e.alive[i] {
            return None;
        }
        let owner = e.owner[i];
        let sight = if e.def[i] == NO_DEF {
            v.legacy_sight()
        } else {
            v.sight_of_def(e.def[i] as usize)
        };
        if e.cell[i] != NO_CELL {
            // A structure: from its footprint, and only while it stands.
            let anchor = self.occupancy.cell(e.cell[i]);
            let fp = self.footprint_at(i);
            return Some(Source {
                owner,
                x: anchor.x,
                y: anchor.y,
                w: fp.width,
                h: fp.height,
                sight_milli: if self.is_standing(i) { sight } else { 0 },
            });
        }
        let cell = self.cell_at(e.pos[i])?;
        Some(Source {
            owner,
            x: cell.x,
            y: cell.y,
            w: 1,
            h: 1,
            sight_milli: sight,
        })
    }

    /// The vision pass: every slot's stamp is brought in line with its entity, in ascending slot
    /// order, only where it changed. After the crowd pass and before targeting.
    pub(crate) fn update_vision(&mut self) {
        let Some(mut v) = self.vision.take() else {
            return;
        };
        for i in 0..self.entities.len() {
            let src = self.vision_source(&v, i);
            v.sync(i, src);
        }
        v.finish_pass();
        self.vision = Some(v);
    }
}

#[cfg(test)]
mod tests;
