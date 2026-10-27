//! Ore on the map (WP7.1): an amount per cell and a slow, fixed regrowth.
//!
//! The terrain says where ore *is* (a cell whose type is `harvestable`, the type
//! `ore` as shipped); this module says **how much** of it is there now. The
//! terrain type of a cell never changes, so a cell that has been emptied is
//! still an `ore` cell with an amount of zero: it stays unbuildable, keeps its
//! movement cost, and is one of the cells that grow back. The picture and the
//! minimap show the amount, not the type (`docs/rules/economy.md`).
//!
//! # State
//!
//! A world has ore state only if it was given [`OreRules`]
//! ([`World::with_ore`](crate::World::with_ore)); every world without them is
//! untouched, including its checksum. The state is one `u16` per cell
//! (zero outside the field cells) and the sorted list of the **field cells**:
//! the harvestable cells of the terrain at the moment the world was made.
//!
//! # Regrowth
//!
//! Every [`OreRules::regrow_period`] ticks each field cell below
//! [`OreRules::max`] gains [`OreRules::regrow_amount`], never going above the
//! maximum. The cells are spread over the period by their position in the field
//! list: in tick `t` the cells at list positions `t % period`, `t % period +
//! period`, ... are the ones that grow. So a tick touches `cells / period` cells,
//! not the map, and a given cell grows in the same ticks every game. No random
//! numbers are used.
//!
//! # Checksum
//!
//! The state is hashed through a running sum of one mixed term per field cell
//! (`mix(cell, amount)`), changed in constant time whenever an amount changes, so
//! a tick costs eight bytes of hashing however large the map is
//! ([`Ore::fingerprint`]). A test recomputes it from scratch.

use crate::terrain::{Cell, Terrain};
use crate::world::mix64;

/// The numbers of the ore rules. Loaded by `sf_data` from `assets/config/ore.ron`
/// and handed over by `sf_bridge::ore_rules`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct OreRules {
    /// What every field cell holds when the world is made.
    pub start: u16,
    /// The most a cell can hold; regrowth stops here.
    pub max: u16,
    /// What a cell below the maximum gains each time its turn comes.
    pub regrow_amount: u16,
    /// Ticks between two turns of the same cell, at least one.
    pub regrow_period: u32,
}

impl OreRules {
    /// The most ticks between two turns of a cell (an hour of game time): a bound against a
    /// typing mistake.
    pub const MAX_PERIOD: u32 = 72_000;

    /// Whether the numbers fit: a maximum of at least 1, a start that is not above it, and a
    /// period between 1 and [`OreRules::MAX_PERIOD`].
    pub fn is_valid(&self) -> bool {
        self.max >= 1
            && self.start <= self.max
            && (1..=Self::MAX_PERIOD).contains(&self.regrow_period)
    }
}

/// The amount of ore in every cell. See the module documentation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ore {
    rules: OreRules,
    width: u32,
    height: u32,
    /// `width * height` amounts, row-major; zero outside the field cells.
    amount: Box<[u16]>,
    /// Row-major indices of the field cells, ascending.
    fields: Box<[u32]>,
    /// Running sum of [`term`] over the field cells.
    sum: u64,
    /// Counts the changes of any amount; the presentation compares it.
    version: u64,
}

/// One cell's share of the fingerprint.
fn term(index: u32, amount: u16) -> u64 {
    mix64((u64::from(index) << 16) | u64::from(amount))
}

impl Ore {
    /// Ore on every harvestable cell of `terrain`, each holding `rules.start`.
    ///
    /// `rules` must be valid ([`OreRules::is_valid`]); the world checks it.
    pub(crate) fn new(rules: OreRules, terrain: &Terrain) -> Ore {
        let (width, height) = (terrain.width(), terrain.height());
        let mut amount = vec![0_u16; width as usize * height as usize];
        let mut fields = Vec::new();
        for (i, &id) in terrain.ids().iter().enumerate() {
            let harvestable = terrain
                .flags_of(crate::terrain::TerrainId(id))
                .is_some_and(|f| f.harvestable);
            if harvestable && terrain.has_layer() {
                amount[i] = rules.start;
                fields.push(i as u32);
            }
        }
        let mut ore = Ore {
            rules,
            width,
            height,
            amount: amount.into_boxed_slice(),
            fields: fields.into_boxed_slice(),
            sum: 0,
            version: 0,
        };
        ore.sum = ore.recompute_sum();
        ore
    }

    /// The numbers the state runs on.
    pub fn rules(&self) -> &OreRules {
        &self.rules
    }

    fn index(&self, cell: Cell) -> Option<usize> {
        (cell.x < self.width && cell.y < self.height)
            .then(|| cell.y as usize * self.width as usize + cell.x as usize)
    }

    /// What `cell` holds; zero outside the map and on a cell that is no field cell.
    pub fn amount_at(&self, cell: Cell) -> u16 {
        self.index(cell).map_or(0, |i| self.amount[i])
    }

    /// Whether `cell` is a field cell (it holds ore, or did and may grow back).
    pub fn is_field(&self, cell: Cell) -> bool {
        self.index(cell)
            .is_some_and(|i| self.fields.binary_search(&(i as u32)).is_ok())
    }

    /// How many field cells there are.
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// The field cells with what they hold, in ascending (row-major) order.
    pub fn cells(&self) -> impl ExactSizeIterator<Item = (Cell, u16)> + Clone + '_ {
        self.fields.iter().map(|&i| {
            (
                Cell::new(i % self.width, i / self.width),
                self.amount[i as usize],
            )
        })
    }

    /// All the ore on the map.
    pub fn total(&self) -> u64 {
        self.fields
            .iter()
            .map(|&i| u64::from(self.amount[i as usize]))
            .sum()
    }

    /// Counts the changes of any amount (a take, a growth). Equal versions mean equal amounts.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Takes up to `want` from `cell`; returns what it gave (zero outside a field and on
    /// an empty cell). The only way an amount goes down; the harvester (WP7.2) calls it
    /// inside the tick, and so does `Command::TakeOre`.
    pub(crate) fn take(&mut self, cell: Cell, want: u16) -> u16 {
        let Some(i) = self.index(cell) else {
            return 0;
        };
        let old = self.amount[i];
        let got = old.min(want);
        if got > 0 {
            self.set(i, old - got);
        }
        got
    }

    fn set(&mut self, i: usize, new: u16) {
        let old = self.amount[i];
        self.sum = self
            .sum
            .wrapping_sub(term(i as u32, old))
            .wrapping_add(term(i as u32, new));
        self.amount[i] = new;
        self.version += 1;
    }

    /// The regrowth of tick `tick`: the cells whose turn it is gain
    /// [`OreRules::regrow_amount`]. Touches `field_count / regrow_period` cells.
    pub(crate) fn regrow(&mut self, tick: u64) {
        let rules = self.rules;
        if rules.regrow_amount == 0 {
            return;
        }
        let period = rules.regrow_period as usize;
        let start = (tick % u64::from(rules.regrow_period)) as usize;
        let mut at = start;
        while at < self.fields.len() {
            let i = self.fields[at] as usize;
            let old = self.amount[i];
            if old < rules.max {
                self.set(i, old.saturating_add(rules.regrow_amount).min(rules.max));
            }
            at += period;
        }
    }

    /// The nearest cell with ore from `from`, within `max_radius` cells, that `accept`
    /// takes: the smallest squared distance between cell indices, ties broken by the smaller
    /// row, then the smaller column. `None` if there is none (or `from` is off the map).
    ///
    /// The search walks square rings outward and stops as soon as no ring can beat what it
    /// has, so it costs about the area of a disc up to the answer, not the map.
    pub fn nearest_where(
        &self,
        from: Cell,
        max_radius: u32,
        mut accept: impl FnMut(Cell) -> bool,
    ) -> Option<Cell> {
        self.index(from)?;
        // Best so far as (squared distance, row, column).
        let mut best: Option<(u64, u32, u32)> = None;
        let reach = from
            .x
            .max(self.width - 1 - from.x)
            .max(from.y)
            .max(self.height - 1 - from.y);
        let last = max_radius.min(reach);
        let mut consider = |x: u32, y: u32, best: &mut Option<(u64, u32, u32)>| {
            let i = y as usize * self.width as usize + x as usize;
            if self.amount[i] == 0 || !accept(Cell::new(x, y)) {
                return;
            }
            let dx = u64::from(x.abs_diff(from.x));
            let dy = u64::from(y.abs_diff(from.y));
            let candidate = (dx * dx + dy * dy, y, x);
            if best.is_none_or(|b| candidate < b) {
                *best = Some(candidate);
            }
        };
        for r in 0..=last {
            // Every cell of ring r is at least r away: when that is already farther than
            // the best, nothing later can win (equal distance can still win on the tie).
            if let Some((d2, _, _)) = best
                && u64::from(r) * u64::from(r) > d2
            {
                break;
            }
            let y0 = from.y.saturating_sub(r);
            let y1 = (from.y + r).min(self.height - 1);
            let x0 = from.x.saturating_sub(r);
            let x1 = (from.x + r).min(self.width - 1);
            for y in y0..=y1 {
                let edge_row = y.abs_diff(from.y) == r;
                if edge_row {
                    for x in x0..=x1 {
                        consider(x, y, &mut best);
                    }
                } else {
                    // Only the two columns of the ring, when they are on the map.
                    if from.x >= r {
                        consider(from.x - r, y, &mut best);
                    }
                    if r > 0 && from.x + r < self.width {
                        consider(from.x + r, y, &mut best);
                    }
                }
            }
        }
        best.map(|(_, y, x)| Cell::new(x, y))
    }

    /// The fingerprint hashed into the checksum: the rules, the number of field cells and the
    /// running sum.
    pub(crate) fn fingerprint(&self) -> [u64; 3] {
        [
            (u64::from(self.rules.start) << 48)
                | (u64::from(self.rules.max) << 32)
                | (u64::from(self.rules.regrow_amount) << 16),
            (u64::from(self.rules.regrow_period) << 32) | self.fields.len() as u64,
            self.sum,
        ]
    }

    /// The sum recomputed from the amounts (what the running sum must equal).
    pub fn recompute_sum(&self) -> u64 {
        self.fields.iter().fold(0_u64, |acc, &i| {
            acc.wrapping_add(term(i, self.amount[i as usize]))
        })
    }

    /// The running sum, for the invariant test.
    pub fn running_sum(&self) -> u64 {
        self.sum
    }
}
