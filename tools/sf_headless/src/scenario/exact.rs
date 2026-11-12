//! Commands with their exact values, and the file of a recorded game (WP1.16a).
//!
//! A scenario's own `commands` are written the way a person writes them: a target is a cell, an id
//! may be a range. A command the game sent has fixed-point positions (a click on the ground) that are
//! not a cell middle. So that a recorded game replays to the same checksum, this module has a second
//! way to write any [`Command`]: [`ExactCommand`] holds every field, positions as the raw fixed-point
//! number (`Fx::raw`, 16 fractional bits) and ids as `(index, generation)`. [`ExactCommand::from_command`]
//! and [`ExactCommand::to_command`] are exhaustive over [`Command`], so a new command in `sf_sim`
//! does not compile here until it is covered (and the test `every_command_round_trips` lists it).
//!
//! A [`RecordedCommand`] adds when it entered the world: `arrived` is the number of ticks the world
//! had run when the sim thread handed the command to it (`tick` is the tick it runs at). The world's
//! checksum includes the commands that are waiting for their tick, so a replay that gives the same
//! checksum at every tick has to put each command in at the same moment. `docs/rules/hitch_log.md`
//! ("The command record") has the whole format; [`render_recording`] writes it.

use serde::{Deserialize, Serialize};
use sf_math::{Fx, Vec2};
use sf_sim::{Area, Cell, Command, EntityId, GuardTarget, Kind, Order, PlayerId, Tick};

/// A position in cells as raw fixed-point numbers `(x, y)`.
pub type RawPos = (i64, i64);

fn pos(v: Vec2) -> RawPos {
    (v.x.raw(), v.y.raw())
}

fn vec2(p: RawPos) -> Vec2 {
    Vec2::new(Fx::from_raw(p.0), Fx::from_raw(p.1))
}

fn id(e: EntityId) -> (u32, u32) {
    (e.index(), e.generation())
}

fn entity(p: (u32, u32)) -> EntityId {
    EntityId::new(p.0, p.1)
}

fn cell(c: Cell) -> (u32, u32) {
    (c.x, c.y)
}

fn to_cell(p: (u32, u32)) -> Cell {
    Cell::new(p.0, p.1)
}

fn ids(list: &[EntityId]) -> Vec<(u32, u32)> {
    list.iter().map(|&e| id(e)).collect()
}

fn entities(list: &[(u32, u32)]) -> Vec<EntityId> {
    list.iter().map(|&p| entity(p)).collect()
}

/// An [`Order`] with exact values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ExactOrder {
    Idle,
    HoldPosition,
    Move(RawPos),
    Attack((u32, u32)),
    AttackMove(RawPos),
    GuardPoint(RawPos),
    GuardEntity((u32, u32)),
    Scatter,
    Pursue((u32, u32)),
}

impl ExactOrder {
    /// Every field of `order`.
    pub fn from_order(order: Order) -> ExactOrder {
        match order {
            Order::Idle => ExactOrder::Idle,
            Order::HoldPosition => ExactOrder::HoldPosition,
            Order::Move(p) => ExactOrder::Move(pos(p)),
            Order::Attack(e) => ExactOrder::Attack(id(e)),
            Order::AttackMove(p) => ExactOrder::AttackMove(pos(p)),
            Order::Guard(GuardTarget::Point(p)) => ExactOrder::GuardPoint(pos(p)),
            Order::Guard(GuardTarget::Entity(e)) => ExactOrder::GuardEntity(id(e)),
            Order::Scatter => ExactOrder::Scatter,
            Order::Pursue(e) => ExactOrder::Pursue(id(e)),
        }
    }

    /// The order this describes.
    pub fn to_order(&self) -> Order {
        match *self {
            ExactOrder::Idle => Order::Idle,
            ExactOrder::HoldPosition => Order::HoldPosition,
            ExactOrder::Move(p) => Order::Move(vec2(p)),
            ExactOrder::Attack(e) => Order::Attack(entity(e)),
            ExactOrder::AttackMove(p) => Order::AttackMove(vec2(p)),
            ExactOrder::GuardPoint(p) => Order::Guard(GuardTarget::Point(vec2(p))),
            ExactOrder::GuardEntity(e) => Order::Guard(GuardTarget::Entity(entity(e))),
            ExactOrder::Scatter => Order::Scatter,
            ExactOrder::Pursue(e) => Order::Pursue(entity(e)),
        }
    }
}

/// A [`Command`] with exact values: one variant per `sf_sim` command, with the same fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ExactCommand {
    Spawn {
        kind: u16,
        pos: RawPos,
        owner: u8,
    },
    Place {
        kind: u16,
        cell: (u32, u32),
        owner: u8,
    },
    SpawnUnit {
        kind: u16,
        pos: RawPos,
        owner: u8,
    },
    PlaceStructure {
        kind: u16,
        cell: (u32, u32),
        owner: u8,
    },
    BuildStructure {
        kind: u16,
        cell: (u32, u32),
    },
    QueueUnit {
        factory: (u32, u32),
        kind: u16,
    },
    CancelUnit {
        factory: (u32, u32),
        index: u8,
    },
    SetRally {
        factory: (u32, u32),
        cell: Option<(u32, u32)>,
    },
    SetRepeat {
        factory: (u32, u32),
        on: bool,
    },
    Repair {
        structure: (u32, u32),
        on: bool,
    },
    Sell {
        structure: (u32, u32),
    },
    Order {
        ids: Vec<(u32, u32)>,
        order: ExactOrder,
        queued: bool,
    },
    Stop {
        ids: Vec<(u32, u32)>,
    },
    AimTurret {
        ids: Vec<(u32, u32)>,
        target: Option<(u32, u32)>,
    },
    TakeOre {
        cell: (u32, u32),
        amount: u16,
    },
    Despawn {
        id: (u32, u32),
    },
    SetWanderArea {
        ids: Vec<(u32, u32)>,
        min: RawPos,
        max: RawPos,
    },
    MoveTo {
        ids: Vec<(u32, u32)>,
        target: RawPos,
    },
}

impl ExactCommand {
    /// Every field of `command`. Exhaustive: a new `sf_sim` command stops this from compiling.
    pub fn from_command(command: &Command) -> ExactCommand {
        match command {
            Command::Spawn {
                kind,
                pos: p,
                owner,
            } => ExactCommand::Spawn {
                kind: kind.0,
                pos: pos(*p),
                owner: owner.0,
            },
            Command::Place {
                kind,
                cell: c,
                owner,
            } => ExactCommand::Place {
                kind: kind.0,
                cell: cell(*c),
                owner: owner.0,
            },
            Command::SpawnUnit {
                kind,
                pos: p,
                owner,
            } => ExactCommand::SpawnUnit {
                kind: kind.0,
                pos: pos(*p),
                owner: owner.0,
            },
            Command::PlaceStructure {
                kind,
                cell: c,
                owner,
            } => ExactCommand::PlaceStructure {
                kind: kind.0,
                cell: cell(*c),
                owner: owner.0,
            },
            Command::BuildStructure { kind, cell: c } => ExactCommand::BuildStructure {
                kind: kind.0,
                cell: cell(*c),
            },
            Command::QueueUnit { factory, kind } => ExactCommand::QueueUnit {
                factory: id(*factory),
                kind: kind.0,
            },
            Command::CancelUnit { factory, index } => ExactCommand::CancelUnit {
                factory: id(*factory),
                index: *index,
            },
            Command::SetRally { factory, cell: c } => ExactCommand::SetRally {
                factory: id(*factory),
                cell: c.map(cell),
            },
            Command::SetRepeat { factory, on } => ExactCommand::SetRepeat {
                factory: id(*factory),
                on: *on,
            },
            Command::Repair { structure, on } => ExactCommand::Repair {
                structure: id(*structure),
                on: *on,
            },
            Command::Sell { structure } => ExactCommand::Sell {
                structure: id(*structure),
            },
            Command::Order {
                ids: list,
                order,
                queued,
            } => ExactCommand::Order {
                ids: ids(list),
                order: ExactOrder::from_order(*order),
                queued: *queued,
            },
            Command::Stop { ids: list } => ExactCommand::Stop { ids: ids(list) },
            Command::AimTurret { ids: list, target } => ExactCommand::AimTurret {
                ids: ids(list),
                target: target.map(id),
            },
            Command::TakeOre { cell: c, amount } => ExactCommand::TakeOre {
                cell: cell(*c),
                amount: *amount,
            },
            Command::Despawn { id: e } => ExactCommand::Despawn { id: id(*e) },
            Command::SetWanderArea { ids: list, area } => ExactCommand::SetWanderArea {
                ids: ids(list),
                min: pos(area.min),
                max: pos(area.max),
            },
            Command::MoveTo { ids: list, target } => ExactCommand::MoveTo {
                ids: ids(list),
                target: pos(*target),
            },
        }
    }

    /// The command this describes.
    pub fn to_command(&self) -> Command {
        match self {
            ExactCommand::Spawn { kind, pos, owner } => Command::Spawn {
                kind: Kind(*kind),
                pos: vec2(*pos),
                owner: PlayerId(*owner),
            },
            ExactCommand::Place { kind, cell, owner } => Command::Place {
                kind: Kind(*kind),
                cell: to_cell(*cell),
                owner: PlayerId(*owner),
            },
            ExactCommand::SpawnUnit { kind, pos, owner } => Command::SpawnUnit {
                kind: Kind(*kind),
                pos: vec2(*pos),
                owner: PlayerId(*owner),
            },
            ExactCommand::PlaceStructure { kind, cell, owner } => Command::PlaceStructure {
                kind: Kind(*kind),
                cell: to_cell(*cell),
                owner: PlayerId(*owner),
            },
            ExactCommand::BuildStructure { kind, cell } => Command::BuildStructure {
                kind: Kind(*kind),
                cell: to_cell(*cell),
            },
            ExactCommand::QueueUnit { factory, kind } => Command::QueueUnit {
                factory: entity(*factory),
                kind: Kind(*kind),
            },
            ExactCommand::CancelUnit { factory, index } => Command::CancelUnit {
                factory: entity(*factory),
                index: *index,
            },
            ExactCommand::SetRally { factory, cell } => Command::SetRally {
                factory: entity(*factory),
                cell: cell.map(to_cell),
            },
            ExactCommand::SetRepeat { factory, on } => Command::SetRepeat {
                factory: entity(*factory),
                on: *on,
            },
            ExactCommand::Repair { structure, on } => Command::Repair {
                structure: entity(*structure),
                on: *on,
            },
            ExactCommand::Sell { structure } => Command::Sell {
                structure: entity(*structure),
            },
            ExactCommand::Order { ids, order, queued } => Command::Order {
                ids: entities(ids),
                order: order.to_order(),
                queued: *queued,
            },
            ExactCommand::Stop { ids } => Command::Stop { ids: entities(ids) },
            ExactCommand::AimTurret { ids, target } => Command::AimTurret {
                ids: entities(ids),
                target: target.map(entity),
            },
            ExactCommand::TakeOre { cell, amount } => Command::TakeOre {
                cell: to_cell(*cell),
                amount: *amount,
            },
            ExactCommand::Despawn { id } => Command::Despawn { id: entity(*id) },
            ExactCommand::SetWanderArea { ids, min, max } => Command::SetWanderArea {
                ids: entities(ids),
                area: Area::new(vec2(*min), vec2(*max)),
            },
            ExactCommand::MoveTo { ids, target } => Command::MoveTo {
                ids: entities(ids),
                target: vec2(*target),
            },
        }
    }
}

/// One command of a recorded game: what it is, who sent it, when the world got it and when it ran.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedCommand {
    /// Ticks the world had run when the command was handed to it. The replay schedules it at this
    /// moment: pending commands are part of the checksum.
    pub arrived: u64,
    /// The tick the command runs at (at least `arrived`).
    pub tick: u64,
    /// Who sent it.
    pub player: u8,
    /// What it asks.
    pub command: ExactCommand,
}

impl RecordedCommand {
    /// A recorded command from the parts a sim thread logs.
    pub fn new(arrived: Tick, effective: Tick, player: PlayerId, command: &Command) -> Self {
        RecordedCommand {
            arrived: arrived.0,
            tick: effective.0,
            player: player.0,
            command: ExactCommand::from_command(command),
        }
    }
}

/// The text of a recorded game as a scenario file: the `game` block (what the world is made of), the
/// seed, the map size, the number of ticks the record covers and the commands, one per line in the
/// order they were handed to the world.
pub fn render_recording(
    name: &str,
    seed: u64,
    map: (u32, u32),
    game: &super::GameSpec,
    ticks: u64,
    commands: &[RecordedCommand],
) -> Result<String, ron::Error> {
    let mut out = String::new();
    out.push_str(
        "// A recorded game (sf_game --match --record, WP1.16a): replay it with\n\
         //   sf_headless THIS_FILE\n\
         // It is made for the build that wrote it (the rules are the ones compiled into that build, see\n\
         // docs/rules/hitch_log.md, \"The command record\").\n",
    );
    out.push_str("Scenario(\n");
    out.push_str(&format!("    name: {},\n", ron::to_string(name)?));
    out.push_str(&format!("    seed: {seed},\n"));
    out.push_str(&format!(
        "    map: (width: {}, height: {}),\n",
        map.0, map.1
    ));
    out.push_str(&format!("    game: Some({}),\n", ron::to_string(game)?));
    out.push_str(&format!("    ticks: {ticks},\n"));
    out.push_str("    recorded: [\n");
    for c in commands {
        out.push_str(&format!("        {},\n", ron::to_string(c)?));
    }
    out.push_str("    ],\n)\n");
    Ok(out)
}
