//! The repair and sell modes of the sidebar (WP6.6, `docs/rules/repair_sell.md`, `sidebar.md`,
//! `cursor.md`). Presentation only: the modes are what the player has switched on in the sidebar; a
//! left click on one of the player's structures in a mode turns into [`Command::Repair`] or
//! [`Command::Sell`], and the simulation checks it again at its tick. Nothing here decides a rule:
//! the choices below (a structure at full health is not offered a repair, a structure being sold
//! gets nothing) only keep the cursor and the click from promising what the simulation would refuse.
//!
//! One source of truth, as for the cursor of `cursor.md`: [`target`] says what a click on an entity
//! would do, [`cursor`] is the picture that goes with it and [`click`] the command, all from that.

use sf_sim::{Command, EntityId, PlayerId};

use crate::cursor::CursorKind;
use crate::sim_snapshot::SimSnapshot;

/// A mode of the sidebar: what a left click on one of the player's structures does.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolMode {
    /// Start or stop repairing the structure.
    Repair,
    /// Sell the structure.
    Sell,
}

impl ToolMode {
    /// Both modes, in the order of the sidebar's buttons.
    pub const ALL: [ToolMode; 2] = [ToolMode::Repair, ToolMode::Sell];

    /// The place of the button in [`ToolMode::ALL`].
    pub const fn index(self) -> usize {
        match self {
            ToolMode::Repair => 0,
            ToolMode::Sell => 1,
        }
    }

    /// The cursor picture of the mode (`cursor.md`): the spanner and the coin of the placeholder set.
    pub const fn cursor(self) -> CursorKind {
        match self {
            ToolMode::Repair => CursorKind::Repair,
            ToolMode::Sell => CursorKind::Sell,
        }
    }
}

/// What a left click on a structure does in a mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Not a structure of the player, or one the mode cannot act on now.
    Nothing,
    /// Start repairing it.
    StartRepair,
    /// Stop the repair that is going on.
    StopRepair,
    /// Sell it.
    Sell,
}

/// What a click on `entity` does in `mode` for `player`, read from the snapshot. Anything that is not
/// a structure of the player's, a structure under construction, one that is being sold and one at 0
/// hit points is [`Target::Nothing`]; a repair is started on a structure that is hurt and stopped on
/// one that is repairing; a sale is offered on every other structure.
pub fn target(
    snap: &SimSnapshot,
    mode: ToolMode,
    entity: Option<EntityId>,
    player: PlayerId,
) -> Target {
    let Some(id) = entity else {
        return Target::Nothing;
    };
    let Some(view) = snap.get(id) else {
        return Target::Nothing;
    };
    let is_structure = snap.stats_of(id).is_some_and(|s| s.footprint.is_some());
    if view.owner != player
        || !is_structure
        || snap.construction_of(id).is_some()
        || snap.sale_of(id).is_some()
    {
        return Target::Nothing;
    }
    let max = snap.max_hp(&view);
    // A structure at 0 hit points is a wreck in a world without destruction: nothing to act on
    // (the simulation refuses it too). One that cannot be damaged has `max` 0 and stands.
    if view.hp == 0 && max > 0 {
        return Target::Nothing;
    }
    match mode {
        ToolMode::Sell => Target::Sell,
        ToolMode::Repair => {
            if snap.repair_of(id).is_some() {
                Target::StopRepair
            } else if max > 0 && view.hp < max {
                Target::StartRepair
            } else {
                Target::Nothing
            }
        }
    }
}

/// The cursor in `mode` over `entity`: the mode's picture over a structure it can act on, the
/// forbidden sign everywhere else (the click would do nothing).
pub fn cursor(
    snap: &SimSnapshot,
    mode: ToolMode,
    entity: Option<EntityId>,
    player: PlayerId,
) -> CursorKind {
    match target(snap, mode, entity, player) {
        Target::Nothing => CursorKind::NoGo,
        _ => mode.cursor(),
    }
}

/// The command a left click on `entity` sends in `mode`; `None` when the click does nothing.
pub fn click(
    snap: &SimSnapshot,
    mode: ToolMode,
    entity: Option<EntityId>,
    player: PlayerId,
) -> Option<Command> {
    let structure = entity?;
    match target(snap, mode, entity, player) {
        Target::Nothing => None,
        Target::StartRepair => Some(Command::Repair {
            structure,
            on: true,
        }),
        Target::StopRepair => Some(Command::Repair {
            structure,
            on: false,
        }),
        Target::Sell => Some(Command::Sell { structure }),
    }
}

#[cfg(test)]
mod tests;
