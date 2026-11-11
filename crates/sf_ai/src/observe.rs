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
//! Left for later parts (documented in `docs/rules/ai.md`): the public tables (unit, building and
//! production data, terrain) and the ore on seen or explored cells; WP9.1b asks for them when the build
//! rules need them.

use std::collections::BTreeMap;

use sf_sim::{
    Cell, Command, EntityId, Footprint, Kind, Order, PlayerId, PowerTotals, Tick, Visibility, World,
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
        }
    }
}

impl Ai {
    /// One think: observe `world` through the fence, update the memory, run the manager whose turn it is,
    /// and push the commands it wants to send onto `out`. The host calls it when [`Ai::due`] says so,
    /// immediately before the world's step, and schedules `out` for the tick about to run under this AI's
    /// player id.
    pub fn think(&mut self, world: &World, out: &mut Vec<Command>) {
        let obs = Observation::build(world, self.player(), &self.state().known_structures);
        self.think_with(&obs, out);
    }
}
