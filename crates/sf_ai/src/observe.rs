//! The fence: the one file of this crate that names the simulation's `World`.
//!
//! Everything the AI knows about the game comes through [`Observation`], built here, and the rest of the
//! crate takes only that. A source test (`tests/fence.rs`) fails when the type's name appears in any other
//! file, so reading the simulation anywhere else is a build-time-visible act, not a habit.
//!
//! What the observation holds, and why it is no cheat:
//!
//! * **Own things in full**: every entity of the player (id, kind, cell, hit points, order, whether it
//!   stands), the credits, the ore store and the power totals. A player knows its own.
//! * **Of others, only what is seen**: an entity of another player (or of nobody) is in the observation
//!   when its cell is `Visible` for this player (`World::visibility`, which includes shared vision), the
//!   same cell test the snapshot's `seen` uses (`docs/rules/vision.md`, "For the player's side"): a unit by
//!   the cell its position is in, a structure by **any** cell of its footprint. Nothing else about an
//!   unseen entity gets in: not its existence, not its hit points, not its order.
//! * **Gone**: a structure the AI remembers ([`crate::state::Known`]) whose cell is visible while it is not
//!   among the visible entities is gone. Decided from the picture alone: it never asks whether the entity is
//!   alive, which would tell the AI about structures it cannot see.
//! * A world without vision rules shows everything to everyone (as `World::visibility` does for a human).
//!
//! The second fenced type is the [`Lens`] (WP9.1b-1): the managers' window on what a player may know
//! that is not in the observation. The public rules (the building table, the cost of a unit, the kinds
//! that harvest), the terrain of a cell (a human sees the map: buildable ground, ore ground), whether a
//! cell is visible now, and the simulation's own placement rule, which it answers **only for a footprint
//! whose every cell and exit cell is visible now**: a preview on a cell in the fog or shroud could reveal
//! a hidden unit (`UnitInTheWay`) or structure, so it is not asked. The amount of ore on a cell is not
//! in the lens (only what is seen; not needed yet).

use std::collections::BTreeMap;

use sf_sim::{
    BuildingFunction, BuildingStats, Cell, Command, Construction, EntityId, FactoryStatus,
    Footprint, Kind, Order, PlacementError, PlayerId, PowerTotals, Tick, Visibility, World,
    exit_cell,
};

use crate::ai::Ai;
use crate::state::Known;

/// One entity of the player's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Own {
    /// Its handle.
    pub id: EntityId,
    /// What it is.
    pub kind: Kind,
    /// A structure: the corner cell of its footprint with the smallest x and y; a unit: the cell its
    /// position is in.
    pub cell: Cell,
    /// The cells of a structure; `None` for a unit.
    pub footprint: Option<Footprint>,
    /// Current hit points; zero for an entity without unit-table stats.
    pub hp: u32,
    /// What it is doing now.
    pub order: Order,
    /// A structure: complete and with hit points left. A unit: always true.
    pub standing: bool,
    /// Its full hit points (the unit table's); zero for an entity without unit-table stats.
    pub max_hp: u32,
    /// A structure under construction: the ticks left and in all. `None` when complete.
    pub building: Option<Construction>,
    /// Whether it is being repaired.
    pub repairing: bool,
    /// A factory (or a structure with a production record): its queue.
    pub factory: Option<OwnFactory>,
    /// Whether it is being sold (WP9.1b-2: the sale started and cannot be cancelled).
    pub selling: bool,
    /// WP9.1c-1: whether its kind carries a weapon (the public weapon table).
    pub armed: bool,
    /// WP9.1c-1: whether its kind moves (it has a movement class: not a structure, not an object).
    pub mobile: bool,
}

/// The production queue of one of the player's own structures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnFactory {
    /// The items, the one being built first: `(kind, free)`; a free one comes with a structure.
    pub queue: Vec<(Kind, bool)>,
    /// Credits paid for the front item so far.
    pub front_paid: u64,
    /// What it is doing.
    pub status: FactoryStatus,
    /// The rally point.
    pub rally: Option<Cell>,
}

/// One entity of another player (or of nobody) that is seen now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// Its handle.
    pub id: EntityId,
    /// What it is.
    pub kind: Kind,
    /// Who owns it.
    pub owner: PlayerId,
    /// A structure: the corner cell of its footprint with the smallest x and y; a unit: the cell its
    /// position is in.
    pub cell: Cell,
    /// The cells of a structure; `None` for a unit.
    pub footprint: Option<Footprint>,
    /// Its hit points (a player sees them); zero for an entity without unit-table stats.
    pub hp: u32,
    /// WP9.1c-1: whether its kind carries a weapon (the public weapon table: what a seen unit can do is
    /// no secret once it is seen).
    pub armed: bool,
}

/// What one player can know at one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    /// Ticks the game has run so far: the tick the next step runs.
    pub tick: Tick,
    /// The player looking.
    pub player: PlayerId,
    /// Map width and height in cells (public: a player sees the size of the map).
    pub map: (u32, u32),
    /// The player's credits.
    pub credits: u64,
    /// Ore in the player's stores.
    pub ore_stored: u64,
    /// What the player's stores can hold.
    pub storage_capacity: u64,
    /// Power made and used; `None` in a game without power rules.
    pub power: Option<PowerTotals>,
    /// Every entity of the player, ascending by id.
    pub own: Vec<Own>,
    /// Every entity of others that is seen, ascending by id.
    pub visible: Vec<Seen>,
    /// The remembered structures that are seen to be gone, ascending by id.
    pub gone: Vec<EntityId>,
    /// WP9.1c-2: the player is defeated or the match is decided (`World::defeat_tick`,
    /// `World::match_result`, both public: a player knows it has lost and that the match is over). An AI
    /// stops thinking then.
    pub out_of_play: bool,
}

/// The cells of a footprint at `anchor`, row by row.
fn cells_of(anchor: Cell, fp: Footprint) -> impl Iterator<Item = Cell> {
    (0..u32::from(fp.height)).flat_map(move |dy| {
        (0..u32::from(fp.width)).map(move |dx| Cell::new(anchor.x + dx, anchor.y + dy))
    })
}

impl Observation {
    /// What `player` can know of `world` now. `known` is the AI's memory of structures, needed to say
    /// which of them are seen to be gone.
    pub fn build(
        world: &World,
        player: PlayerId,
        known: &BTreeMap<EntityId, Known>,
    ) -> Observation {
        let sees = |cell: Cell| world.visibility(player, cell) == Visibility::Visible;
        let mut own = Vec::new();
        let mut visible = Vec::new();
        for view in world.snapshot().iter() {
            // A structure has a footprint and an anchor cell of its own; a unit has neither.
            let footprint = world.footprint_of(view.id);
            let cell = match footprint {
                Some(_) => world.cell_of(view.id),
                None => world.cell_at(view.pos),
            };
            let Some(cell) = cell else {
                continue;
            };
            if view.owner == player {
                own.push(Own {
                    id: view.id,
                    kind: view.kind,
                    cell,
                    footprint,
                    hp: view.hp,
                    order: world.order_of(view.id).unwrap_or(Order::Idle),
                    standing: footprint.is_none() || world.stands(view.id),
                    max_hp: world.stats_of(view.id).map_or(0, |s| s.hp),
                    building: world.construction_of(view.id),
                    repairing: world.is_repairing(view.id),
                    selling: world.sale_of(view.id).is_some(),
                    armed: !world.weapon_table().loadout(view.kind).is_empty(),
                    mobile: world
                        .stats_of(view.id)
                        .is_some_and(|s| s.movement_class.is_some()),
                    factory: world.production_of(view.id).map(|f| OwnFactory {
                        queue: f.queue.iter().map(|q| (q.kind, q.free)).collect(),
                        front_paid: f.front_paid,
                        status: f.status,
                        rally: f.rally,
                    }),
                });
                continue;
            }
            let seen = match footprint {
                Some(fp) => cells_of(cell, fp).any(sees),
                None => sees(cell),
            };
            if seen {
                visible.push(Seen {
                    id: view.id,
                    kind: view.kind,
                    owner: view.owner,
                    cell,
                    footprint,
                    hp: view.hp,
                    armed: !world.weapon_table().loadout(view.kind).is_empty(),
                });
            }
        }
        let gone = known
            .iter()
            .filter(|(id, k)| {
                visible.binary_search_by_key(*id, |s| s.id).is_err()
                    && cells_of(
                        k.anchor,
                        Footprint {
                            width: k.width,
                            height: k.height,
                        },
                    )
                    .any(sees)
            })
            .map(|(id, _)| *id)
            .collect();
        Observation {
            tick: world.tick(),
            player,
            map: world.map_size(),
            credits: world.credits(player),
            ore_stored: world.ore_stored(player),
            storage_capacity: world.storage_capacity(player),
            power: world.power_of(player),
            own,
            visible,
            gone,
            out_of_play: world.defeat_tick(player).is_some() || world.match_result().is_some(),
        }
    }
}

/// The managers' fenced window on the public rules, the terrain, what is visible now, and the
/// simulation's placement rule for visible ground (module documentation). Made per think by
/// [`Ai::think`]; it counts the placement questions it passes on to the simulation.
pub struct Lens<'w> {
    world: &'w World,
    player: PlayerId,
    previews: u32,
}

impl<'w> Lens<'w> {
    /// The lens of `player` on `world`.
    pub fn new(world: &'w World, player: PlayerId) -> Lens<'w> {
        Lens {
            world,
            player,
            previews: 0,
        }
    }

    /// The building table entry of `kind` (public rules).
    pub fn building(&self, kind: Kind) -> Option<&'w BuildingStats> {
        self.world.building_table().get(kind)
    }

    /// The kinds that harvest, ascending (public rules).
    pub fn harvester_kinds(&self) -> &'w [Kind] {
        self.world.harvester_kinds()
    }

    /// The price of the unit `kind` in a factory queue; `None` for a kind no factory makes.
    pub fn unit_cost(&self, kind: Kind) -> Option<u32> {
        self.world
            .production_table()
            .and_then(|t| t.get(kind))
            .map(|u| u.cost)
    }

    /// WP9.2b-1: how well the unit `role` hurts the unit `enemy`: the largest [`sf_sim::weapons::weapon_score`] over the
    /// weapons of the role's loadout, each against the armour the enemy kind wears (public tables: the weapon
    /// table, the armour table). 0 for a role with no weapon. A raw score, to be compared **per credit** by the
    /// caller (`counter::best_role`), never between roles of different cost. A kind the armour table does not list,
    /// or a world without an armour table, takes a hundred percent, as the simulation's own targeting reads it.
    pub fn counter_score(&self, role: Kind, enemy: Kind) -> u64 {
        let table = self.world.weapon_table();
        let armor = table.armor();
        let armor_type = armor.and_then(|a| a.armor_of(enemy));
        table
            .loadout(role)
            .iter()
            .filter_map(|&w| table.weapon(w))
            .map(|w| {
                let percent = match (armor, armor_type) {
                    (Some(a), Some(t)) => a.percent(w.damage_type, t).unwrap_or(100),
                    _ => 100,
                };
                sf_sim::weapons::weapon_score(w, percent)
            })
            .max()
            .unwrap_or(0)
    }

    /// What one standing structure of `kind` stores (public rules); zero for a kind that stores nothing.
    pub fn storage_of(&self, kind: Kind) -> u32 {
        self.world.storage_of_kind(kind)
    }

    /// The building table, ascending by kind (public rules).
    pub fn buildings(&self) -> impl Iterator<Item = &'w BuildingStats> {
        self.world.building_table().iter()
    }

    /// The faction bit of the player (public: a player knows what it plays); `None` in a game without
    /// factions.
    pub fn faction_bit(&self) -> Option<u8> {
        self.world.faction_of(self.player)
    }

    /// The function of the factory that makes the unit `kind`, when the player's faction may have it;
    /// `None` for a kind no factory makes or that is not the player's (public rules).
    pub fn unit_factory(&self, kind: Kind) -> Option<BuildingFunction> {
        let build = self.world.production_table()?.get(kind)?;
        let open = self
            .faction_bit()
            .is_none_or(|bit| build.factions & bit != 0);
        open.then_some(build.factory)
    }

    /// Whether a structure may stand on the terrain of `cell` (the map; false off the map).
    pub fn buildable(&self, cell: Cell) -> bool {
        self.world
            .terrain()
            .flags_at(cell)
            .is_some_and(|f| f.buildable)
    }

    /// Whether the terrain of `cell` is open ground every ground unit can enter (false off the map).
    pub fn ground(&self, cell: Cell) -> bool {
        self.world
            .terrain()
            .flags_at(cell)
            .is_some_and(|f| f.ground)
    }

    /// Whether the terrain of `cell` is ore ground (the map, not the amount: false off the map).
    pub fn harvestable(&self, cell: Cell) -> bool {
        self.world
            .terrain()
            .flags_at(cell)
            .is_some_and(|f| f.harvestable)
    }

    /// Whether any cell of the map is ore ground: the terrain of the whole map, which a human sees (decision 2
    /// of the WP9.1b memo: ore field locations are map knowledge, the amounts are not).
    pub fn has_ore_ground(&self) -> bool {
        let (w, h) = self.world.map_size();
        (0..h).any(|y| (0..w).any(|x| self.harvestable(Cell::new(x, y))))
    }

    /// What selling a structure of `kind` with `hp` of `max_hp` hit points pays when the sale ends (the
    /// simulation's own rule, `sale_refund`, with the price of the building table); `None` in a world without
    /// repair and sale rules and for a kind with no price (public rules).
    pub fn sale_refund(&self, kind: Kind, hp: u32, max_hp: u32) -> Option<u64> {
        let rules = self.world.repair_rules()?;
        let price = self.building(kind)?.cost?;
        Some(sf_sim::sale_refund(
            price,
            rules.refund_permille,
            hp,
            max_hp,
        ))
    }

    /// Whether the player sees `cell` now (false off the map).
    pub fn visible(&self, cell: Cell) -> bool {
        let (w, h) = self.world.map_size();
        cell.x < w && cell.y < h && self.world.visibility(self.player, cell) == Visibility::Visible
    }

    /// The simulation's verdict on building `kind` anchored on `cell` now ([`World::check_placement`]),
    /// asked **only** when every cell of the footprint and the exit cell (of a structure with an exit
    /// side, when it is on the map) is visible to the player; `None` without asking otherwise, and for a
    /// kind that is not in the building table or a footprint that leaves the map.
    pub fn placement(&mut self, kind: Kind, cell: Cell) -> Option<Result<(), PlacementError>> {
        let b = self.building(kind)?;
        let fp = b.footprint;
        let (map_w, map_h) = self.world.map_size();
        let (x1, y1) = (
            u64::from(cell.x) + u64::from(fp.width),
            u64::from(cell.y) + u64::from(fp.height),
        );
        if x1 > u64::from(map_w) || y1 > u64::from(map_h) {
            return None;
        }
        if !cells_of(cell, fp).all(|c| self.visible(c)) {
            return None;
        }
        if let Some(side) = b.exit_side {
            let (ex, ey) = exit_cell(cell, fp, side);
            if let (Ok(x), Ok(y)) = (u32::try_from(ex), u32::try_from(ey))
                && x < map_w
                && y < map_h
                && !self.visible(Cell::new(x, y))
            {
                return None;
            }
        }
        self.previews += 1;
        Some(self.world.check_placement(kind, cell, self.player))
    }

    /// How many placement questions this lens has passed on to the simulation.
    pub fn previews(&self) -> u32 {
        self.previews
    }
}

impl Ai {
    /// One think: observe `world` through the fence, update the memory, run the manager whose turn it is,
    /// and push the commands it wants to send onto `out`. The host calls it when [`Ai::due`] says so,
    /// immediately before the world's step, and schedules `out` for the tick about to run under this AI's
    /// player id.
    pub fn think(&mut self, world: &World, out: &mut Vec<Command>) {
        let obs = Observation::build(world, self.player(), &self.state().known_structures);
        let mut lens = Lens::new(world, self.player());
        self.think_with(&obs, &mut lens, out);
    }
}

/// An open world of `width` by `height` cells with no rules: the lens of a unit test that needs one.
#[cfg(test)]
pub(crate) fn open_world(width: u32, height: u32) -> World {
    World::new(sf_sim::WorldConfig::new(1).with_map_size(width, height)).unwrap()
}
