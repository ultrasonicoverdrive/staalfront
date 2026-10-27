//! Where a player may build (WP6.2, `docs/rules/buildings.md`, "Placement").
//!
//! One pure function over the world as it is when the command runs,
//! [`World::preview_placement`]. It answers with a [`PlacementPreview`]: a
//! verdict for the whole footprint, one for each of its cells and the exit
//! cell of a factory. [`Command::BuildStructure`] executes by asking it, and
//! the presentation asks it too (through the sim thread) for the ghost under
//! the cursor, so the picture and the rule cannot disagree: there is no second
//! copy of the rules.
//!
//! The rules, in the order they are reported (the first that fails is the
//! verdict):
//!
//! (1) the kind is a structure of the unit table with an entry in the building
//!    table that has a price (the anchor and the mission structures cannot be
//!    built);
//! (2) the player has a faction ([`World::with_player_factions`]) and the
//!    structure's `factions` name it;
//! (3) the player has every structure of `requires` standing (the anchor is not in
//!    that list: every structure needs one within reach, rule 7);
//! (4) every cell of the footprint is on the map and buildable terrain;
//! (5) no cell is occupied by a structure;
//! (6) no unit of the table (a mobile one) stands on a cell of the footprint;
//! (7) the footprint is within the `build_radius` of a standing anchor of the
//!    player (the gap between the two footprints, see [`gap`]);
//! (8) a factory has a free, passable exit cell ([`exit_cell`]).
//!
//! Only [`Command::BuildStructure`] is checked. [`Command::PlaceStructure`]
//! stays what it was: the placement of scenario setup and mission objects (a
//! pre-placed enemy decoy, the sandbox), with no faction, prerequisite or
//! proximity rule.

use core::fmt;

use crate::buildings::{BuildingFunction, Side};
use crate::command::Command;
use crate::entities::NO_DEF;
use crate::ids::{Kind, PlayerId, Tick};
use crate::terrain::{Cell, NO_CELL};
use crate::units::{Footprint, UnitClass};
use crate::world::{ConfigError, World};

/// Why a placement is refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlacementError {
    /// The world was given no building table, so nothing can be built.
    NoBuildingRules,
    /// The kind is not a structure of the unit table.
    UnknownKind,
    /// The structure cannot be built: no price, so no build time (the base
    /// anchor, the mission structures).
    CannotBeBuilt,
    /// The player has no faction.
    NoFaction,
    /// The structure's `factions` do not name the player's.
    WrongFaction,
    /// A structure of `requires` does not stand (the first missing kind).
    MissingPrerequisite(Kind),
    /// A cell of the footprint is outside the map.
    OffMap,
    /// A cell of the footprint is terrain that cannot be built on.
    BadTerrain,
    /// A cell of the footprint is occupied by a structure.
    Occupied,
    /// A unit stands on a cell of the footprint.
    UnitInTheWay,
    /// No anchor of the player is within build radius.
    TooFarFromBase,
    /// The exit cell of a factory is outside the map, built on or not passable.
    ExitBlocked,
    /// The entity limit is reached.
    EntityLimit,
}

impl fmt::Display for PlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlacementError::NoBuildingRules => f.write_str("the world has no building rules"),
            PlacementError::UnknownKind => f.write_str("not a structure"),
            PlacementError::CannotBeBuilt => f.write_str("cannot be built"),
            PlacementError::NoFaction => f.write_str("the player has no faction"),
            PlacementError::WrongFaction => f.write_str("not available to this faction"),
            PlacementError::MissingPrerequisite(k) => {
                write!(f, "needs structure {} first", k.0)
            }
            PlacementError::OffMap => f.write_str("outside the map"),
            PlacementError::BadTerrain => f.write_str("cannot build on this terrain"),
            PlacementError::Occupied => f.write_str("another structure is in the way"),
            PlacementError::UnitInTheWay => f.write_str("a unit is in the way"),
            PlacementError::TooFarFromBase => f.write_str("too far from your base"),
            PlacementError::ExitBlocked => f.write_str("the exit is blocked"),
            PlacementError::EntityLimit => f.write_str("too many entities"),
        }
    }
}

impl std::error::Error for PlacementError {}

/// What one cell of a footprint is like.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CellVerdict {
    /// Buildable terrain on the map, no structure, no unit.
    Free,
    /// Outside the map.
    OffMap,
    /// Terrain that cannot be built on.
    BadTerrain,
    /// A structure stands on it.
    Structure,
    /// A unit stands on it.
    Unit,
}

/// The exit cell of a factory and whether it is usable.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ExitCell {
    /// The cell units come out onto (on the map).
    pub cell: Cell,
    /// Free of structures and passable ground.
    pub usable: bool,
}

/// The answer of [`World::preview_placement`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementPreview {
    /// The footprint of the structure; `None` for a kind that is not one.
    pub footprint: Option<Footprint>,
    /// Every cell of the footprint, row by row from the anchor, with its own
    /// verdict.
    pub cells: Vec<(Cell, CellVerdict)>,
    /// The exit cell of a factory whose cell is on the map.
    pub exit: Option<ExitCell>,
    /// Whether the structure can be placed here, or the first reason it cannot.
    pub verdict: Result<(), PlacementError>,
}

/// A refused [`Command::BuildStructure`], kept until the next step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlacementRejection {
    /// The tick the command ran in.
    pub tick: Tick,
    /// Who sent it.
    pub player: PlayerId,
    /// What was to be built.
    pub kind: Kind,
    /// The anchor cell asked for.
    pub cell: Cell,
    /// Why not.
    pub reason: PlacementError,
}

/// The placement state of a world: which faction each player is, and the
/// rejections of the last tick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlacementState {
    /// `(player, faction bit)`, ascending by player. The bit is one of the
    /// faction bits of the data (`Faction::bit`: 1, 2 or 4).
    pub players: Vec<(PlayerId, u8)>,
    /// The refused builds of the last step; cleared at the start of the next.
    pub rejections: Vec<PlacementRejection>,
}

/// The number of whole empty cells between two footprints, the larger of the
/// two directions (the chessboard distance of the rectangles): `0` when they
/// touch or overlap. The anchor cell and size of each are given.
pub fn gap(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> i64 {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    let gx = (bx - (ax + aw)).max(ax - (bx + bw)).max(0);
    let gy = (by - (ay + ah)).max(ay - (by + bh)).max(0);
    gx.max(gy)
}

/// The cell units come out onto: the one next to the middle of the exit side
/// of the footprint (`(anchor.x + width / 2)` along a horizontal side, the
/// same for the height, rounded down). The structure is never rotated, so the
/// side is the one of the data; the cell may be outside the map (negative or
/// beyond), hence the signed result.
pub fn exit_cell(anchor: Cell, footprint: Footprint, side: Side) -> (i64, i64) {
    let (x, y) = (i64::from(anchor.x), i64::from(anchor.y));
    let (w, h) = (i64::from(footprint.width), i64::from(footprint.height));
    match side {
        Side::PosX => (x + w, y + h / 2),
        Side::NegX => (x - 1, y + h / 2),
        Side::PosY => (x + w / 2, y + h),
        Side::NegY => (x + w / 2, y - 1),
    }
}

impl World {
    /// The same world with the faction of each player (WP6.2): `(player, bit)`
    /// with the bit of the data's `Faction::bit` (1, 2 or 4; exactly one bit).
    /// [`Command::BuildStructure`] checks it against the structure's
    /// `factions`; a player without one builds nothing. Call it before the
    /// world starts ([`ConfigError::PlayersAfterStart`] otherwise). The
    /// factions are part of the checksum.
    pub fn with_player_factions(
        mut self,
        players: &[(PlayerId, u8)],
    ) -> Result<World, ConfigError> {
        if self.tick != Tick::ZERO || self.entities.len() != 0 || !self.queue.is_empty() {
            return Err(ConfigError::PlayersAfterStart);
        }
        let mut list: Vec<(PlayerId, u8)> = players.to_vec();
        list.sort();
        for pair in list.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(ConfigError::PlayerFaction(pair[0].0));
            }
        }
        if let Some(&(player, _)) = list.iter().find(|(_, bit)| !bit.is_power_of_two()) {
            return Err(ConfigError::PlayerFaction(player));
        }
        self.placement.players = list;
        Ok(self)
    }

    /// The faction bit of `player`, if it has one.
    pub fn faction_of(&self, player: PlayerId) -> Option<u8> {
        self.placement
            .players
            .binary_search_by_key(&player, |&(p, _)| p)
            .ok()
            .map(|i| self.placement.players[i].1)
    }

    /// The `(player, faction bit)` pairs, ascending by player.
    pub fn player_factions(&self) -> &[(PlayerId, u8)] {
        &self.placement.players
    }

    /// The builds refused in the last step, in the order they ran. Replaced
    /// by every step.
    pub fn placement_rejections(&self) -> &[PlacementRejection] {
        &self.placement.rejections
    }

    /// Whether `player` may build a `kind` with its footprint anchored on
    /// `cell` now. See the module documentation for the rules.
    pub fn check_placement(
        &self,
        kind: Kind,
        cell: Cell,
        player: PlayerId,
    ) -> Result<(), PlacementError> {
        self.preview_placement(kind, cell, player).verdict
    }

    /// The full answer behind [`World::check_placement`]: the verdict, what
    /// each cell of the footprint is like and the exit cell. What the ghost
    /// under the cursor shows.
    pub fn preview_placement(&self, kind: Kind, cell: Cell, player: PlayerId) -> PlacementPreview {
        let mut preview = PlacementPreview {
            footprint: None,
            cells: Vec::new(),
            exit: None,
            verdict: Ok(()),
        };
        let Some(stats) = self.units.get(kind) else {
            preview.verdict = Err(PlacementError::UnknownKind);
            return preview;
        };
        let Some(footprint) = stats
            .footprint
            .filter(|_| stats.class() == UnitClass::Structure)
        else {
            preview.verdict = Err(PlacementError::UnknownKind);
            return preview;
        };
        preview.footprint = Some(footprint);

        // The cells first: they are the same whatever the rules say about the
        // kind, so the ghost can show them for a structure that cannot be built.
        let (w, h) = (u32::from(footprint.width), u32::from(footprint.height));
        let (map_w, map_h) = self.map_size();
        let mut cell_error = None;
        for dy in 0..h {
            for dx in 0..w {
                let c = Cell::new(cell.x.saturating_add(dx), cell.y.saturating_add(dy));
                let verdict = if c.x >= map_w || c.y >= map_h {
                    CellVerdict::OffMap
                } else if !self.terrain.flags_at(c).is_some_and(|f| f.buildable) {
                    CellVerdict::BadTerrain
                } else if self.is_occupied(c) {
                    CellVerdict::Structure
                } else {
                    CellVerdict::Free
                };
                preview.cells.push((c, verdict));
            }
        }
        // Units: the cell of each mobile unit's position.
        for i in 0..self.entities.len() {
            if !self.entities.alive[i]
                || self.entities.cell[i] != NO_CELL
                || self
                    .stats_at(i)
                    .is_none_or(|s| s.class() != UnitClass::Mobile)
            {
                continue;
            }
            let Some(at) = self.cell_at(self.entities.pos[i]) else {
                continue;
            };
            if let Some(slot) = preview
                .cells
                .iter_mut()
                .find(|(c, v)| *c == at && *v == CellVerdict::Free)
            {
                slot.1 = CellVerdict::Unit;
            }
        }
        for (_, v) in &preview.cells {
            let error = match v {
                CellVerdict::Free => continue,
                CellVerdict::OffMap => PlacementError::OffMap,
                CellVerdict::BadTerrain => PlacementError::BadTerrain,
                CellVerdict::Structure => PlacementError::Occupied,
                CellVerdict::Unit => PlacementError::UnitInTheWay,
            };
            // The worst first: off the map, terrain, structure, unit.
            let rank = |e: PlacementError| match e {
                PlacementError::OffMap => 0,
                PlacementError::BadTerrain => 1,
                PlacementError::Occupied => 2,
                _ => 3,
            };
            if cell_error.is_none_or(|e| rank(error) < rank(e)) {
                cell_error = Some(error);
            }
        }

        // The exit of a factory.
        let building = self.buildings.get(kind);
        if let Some(b) = building
            && b.function.is_factory()
            && let Some(side) = b.exit_side
        {
            let (ex, ey) = exit_cell(cell, footprint, side);
            if let (Ok(x), Ok(y)) = (u32::try_from(ex), u32::try_from(ey))
                && x < map_w
                && y < map_h
            {
                let c = Cell::new(x, y);
                let ground = b.function == BuildingFunction::VesselFactory
                    || self.terrain.flags_at(c).is_some_and(|f| f.ground);
                preview.exit = Some(ExitCell {
                    cell: c,
                    usable: ground && !self.is_occupied(c),
                });
            }
        }

        preview.verdict =
            self.placement_verdict(kind, cell, player, footprint, &preview, cell_error);
        preview
    }

    /// The rules in order; the cells and the exit are already worked out.
    fn placement_verdict(
        &self,
        kind: Kind,
        cell: Cell,
        player: PlayerId,
        footprint: Footprint,
        preview: &PlacementPreview,
        cell_error: Option<PlacementError>,
    ) -> Result<(), PlacementError> {
        if self.buildings.is_empty() {
            return Err(PlacementError::NoBuildingRules);
        }
        let b = self
            .buildings
            .get(kind)
            .filter(|b| b.cost.is_some())
            .ok_or(PlacementError::CannotBeBuilt)?;
        let bit = self.faction_of(player).ok_or(PlacementError::NoFaction)?;
        if b.factions & bit == 0 {
            return Err(PlacementError::WrongFaction);
        }

        // What the player has standing: its kinds and its anchors.
        let mut standing: Vec<Kind> = Vec::new();
        let mut anchors: Vec<(i64, i64, i64, i64, i64)> = Vec::new();
        for i in 0..self.entities.len() {
            if !self.entities.alive[i]
                || self.entities.owner[i] != player
                || self.entities.cell[i] == NO_CELL
                || self.entities.def[i] == NO_DEF
                || !self.is_standing(i)
            {
                continue;
            }
            let kind_i = self.entities.kind[i];
            standing.push(kind_i);
            if let Some(a) = self.buildings.get(kind_i)
                && a.function == BuildingFunction::Anchor
            {
                let at = self.occupancy.cell(self.entities.cell[i]);
                let fp = self.footprint_at(i);
                anchors.push((
                    i64::from(at.x),
                    i64::from(at.y),
                    i64::from(fp.width),
                    i64::from(fp.height),
                    i64::from(a.build_radius),
                ));
            }
        }
        if let Some(&missing) = b.requires.iter().find(|r| !standing.contains(r)) {
            return Err(PlacementError::MissingPrerequisite(missing));
        }

        if let Some(e) = cell_error {
            return Err(e);
        }

        let me = (
            i64::from(cell.x),
            i64::from(cell.y),
            i64::from(footprint.width),
            i64::from(footprint.height),
        );
        let near = anchors
            .iter()
            .any(|&(x, y, w, h, radius)| gap((x, y, w, h), me) <= radius);
        if !near {
            return Err(PlacementError::TooFarFromBase);
        }

        if b.function.is_factory() && !preview.exit.is_some_and(|e| e.usable) {
            return Err(PlacementError::ExitBlocked);
        }
        Ok(())
    }

    /// Whether the structure in slot `i` stands: it has hit points left, or
    /// cannot be damaged at all (zero in the table).
    fn is_standing(&self, i: usize) -> bool {
        self.entities.hp[i] > 0 || self.stats_at(i).is_some_and(|s| s.hp == 0)
    }

    /// Runs one command of `player`. `BuildStructure` is the only command whose
    /// rules need to know who sent it; everything else is [`World::apply`].
    pub(crate) fn apply_from(&mut self, player: PlayerId, command: Command) -> bool {
        let Command::BuildStructure { kind, cell } = command else {
            return self.apply(command);
        };
        let verdict = self.check_placement(kind, cell, player);
        let reason = match verdict {
            Ok(()) => match self.place_structure(kind, cell, player) {
                Some(_) => return true,
                None => PlacementError::EntityLimit,
            },
            Err(reason) => reason,
        };
        self.placement.rejections.push(PlacementRejection {
            tick: self.tick,
            player,
            kind,
            cell,
            reason,
        });
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings::{BuildingStats, BuildingTable};
    use crate::units::{UnitStats, UnitTable};
    use crate::world::WorldConfig;
    use sf_math::Fx;

    const ANCHOR: Kind = Kind(1);
    const WALL_ANCHOR: Kind = Kind(2);
    const HUT: Kind = Kind(3);

    fn world() -> World {
        let fp = Footprint {
            width: 2,
            height: 2,
        };
        let unit = |kind: Kind, hp| UnitStats {
            kind,
            hp,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(fp),
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        };
        let building = |kind: Kind, function, cost, build_radius| BuildingStats {
            kind,
            footprint: fp,
            function,
            exit_side: None,
            build_ticks: if cost { 10 } else { 0 },
            cost: cost.then_some(10),
            power: 0,
            factions: 1,
            requires: Vec::new(),
            build_radius,
        };
        World::new(WorldConfig::new(1).with_map_size(32, 32))
            .unwrap()
            .with_units(
                UnitTable::new(vec![unit(ANCHOR, 100), unit(WALL_ANCHOR, 0), unit(HUT, 50)])
                    .unwrap(),
            )
            .unwrap()
            .with_buildings(
                BuildingTable::new(vec![
                    building(ANCHOR, BuildingFunction::Anchor, false, 5),
                    building(WALL_ANCHOR, BuildingFunction::Anchor, false, 5),
                    building(HUT, BuildingFunction::None, true, 0),
                ])
                .unwrap(),
            )
            .unwrap()
            .with_player_factions(&[(PlayerId(0), 1)])
            .unwrap()
    }

    fn put(w: &mut World, kind: Kind, x: u32, y: u32) {
        let tick = w.tick();
        w.schedule(
            tick,
            PlayerId(0),
            Command::PlaceStructure {
                kind,
                cell: Cell::new(x, y),
                owner: PlayerId(0),
            },
        )
        .unwrap();
        w.step();
    }

    #[test]
    fn a_structure_at_zero_hit_points_does_not_stand_but_one_that_cannot_be_hurt_does() {
        let mut w = world();
        put(&mut w, ANCHOR, 10, 10);
        put(&mut w, WALL_ANCHOR, 20, 20);
        let near_first = Cell::new(13, 10);
        let near_second = Cell::new(23, 20);
        assert_eq!(w.check_placement(HUT, near_first, PlayerId(0)), Ok(()));
        assert_eq!(w.check_placement(HUT, near_second, PlayerId(0)), Ok(()));
        // Both fall to zero hit points: the one with hit points in the table
        // stops being a base, the one with none in the table stays one.
        for i in 0..w.entities.len() {
            w.entities.hp[i] = 0;
        }
        assert_eq!(
            w.check_placement(HUT, near_first, PlayerId(0)),
            Err(PlacementError::TooFarFromBase)
        );
        assert_eq!(w.check_placement(HUT, near_second, PlayerId(0)), Ok(()));
    }

    /// Destruction and placement (WP5.5 with WP6.2): a structure that destroyed is
    /// removed frees its cells for building, rubble on them blocks nothing (the
    /// destruction rules: a wreck blocks nothing, a building clears the rubble
    /// under it), and a destroyed anchor no longer gives build range.
    #[test]
    fn a_destroyed_structure_frees_its_cells_and_a_destroyed_anchor_gives_no_range() {
        let mut w = world()
            .with_destruction(crate::destruction::DestructionConfig::default())
            .unwrap();
        put(&mut w, ANCHOR, 10, 10);
        put(&mut w, HUT, 13, 10);
        put(&mut w, ANCHOR, 22, 22);
        let hut_cell = Cell::new(13, 10);
        let near_second = Cell::new(25, 22);
        assert_eq!(
            w.check_placement(HUT, hut_cell, PlayerId(0)),
            Err(PlacementError::Occupied)
        );
        assert_eq!(w.check_placement(HUT, near_second, PlayerId(0)), Ok(()));
        // The hut and the second anchor are shot to zero; the next tick removes them.
        for i in 0..w.entities.len() {
            let c = w.occupancy.cell(w.entities.cell[i]);
            if c == hut_cell || c == Cell::new(22, 22) {
                w.entities.hp[i] = 0;
            }
        }
        w.step();
        assert_eq!(w.wrecks().len(), 2, "both left rubble");
        // The hut's cells are free, rubble and all, and the first anchor still reaches them.
        assert_eq!(w.check_placement(HUT, hut_cell, PlayerId(0)), Ok(()));
        // The destroyed anchor gives no range any more.
        assert_eq!(
            w.check_placement(HUT, near_second, PlayerId(0)),
            Err(PlacementError::TooFarFromBase)
        );
    }
}
