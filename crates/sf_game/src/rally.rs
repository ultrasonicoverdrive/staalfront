//! Rally points by right-click (WP6.9): with own factories selected, a right-click on the ground
//! sends `SetRally` for each of them, a right-click on one of the selected factories clears its
//! rally point. Rules in `docs/rules/production.md` ("Rally by right-click") and
//! `docs/rules/cursor.md` (rows R1 and R2).
//!
//! Like [`crate::cursor`] this is presentation: it reads the snapshot and builds commands; the sim
//! checks them again at its tick (`SetRally` is refused for a factory that is not the player's, does
//! not stand or is being sold, and for a cell off the map). **One source of truth:** the cursor
//! decides ([`cursor`] for a selection of factories only, the unit cursors for a mixed one) and
//! [`plan`] maps the cursor kind to what the factories do, the way `cursor::order_for` maps it to an
//! order for the units, so the picture and the click cannot disagree.

use sf_sim::{BuildingFunction, Cell, Command, EntityId};

use crate::cursor::{CursorKind, Hover};
use crate::selection::{Selection, facts};
use crate::sidebar::stands_when_built;
use crate::sim_snapshot::SimSnapshot;

/// The function of `id` when it is an own factory that stands (finished, not being sold, hit points
/// left) and so takes a rally point; `None` for anything else.
fn standing_factory(
    snap: &SimSnapshot,
    selection: &Selection,
    id: EntityId,
) -> Option<BuildingFunction> {
    let view = snap.get(id)?;
    if view.owner != selection.local() {
        return None;
    }
    let function = snap.menu().factory_function(view.kind)?;
    (snap.construction_of(id).is_none() && stands_when_built(snap, &view)).then_some(function)
}

/// The selected own factories that stand, with what they build, in the selection's order (ascending
/// by id): the buildings a rally right-click is for. Allocates nothing.
pub fn selected_factories<'a>(
    snap: &'a SimSnapshot,
    selection: &'a Selection,
) -> impl Iterator<Item = (EntityId, BuildingFunction)> + 'a {
    selection
        .ids()
        .iter()
        .filter_map(|&id| standing_factory(snap, selection, id).map(|f| (id, f)))
}

/// The cursor for a selection **of factories only** (no selected unit can take an order): over one of
/// the selected factories **RallyClear** (a right-click clears its rally point), over an enemy or
/// neutral thing nothing here (the caller says Inspect), with no ground point nothing here (Select),
/// off the map **NoGo**, else **Rally**. `None` when no selected factory stands.
pub fn cursor(snap: &SimSnapshot, selection: &Selection, hover: &Hover) -> Option<CursorKind> {
    let (mut any, mut over_selected) = (false, false);
    for (id, _) in selected_factories(snap, selection) {
        any = true;
        over_selected |= hover.entity == Some(id);
    }
    if !any {
        return None;
    }
    if over_selected {
        return Some(CursorKind::RallyClear);
    }
    if let Some(f) = hover.entity.and_then(|id| facts(snap, id))
        && f.owner != selection.local()
    {
        return None;
    }
    let point = hover.point?;
    Some(if snap.cell_at(point).is_none() {
        CursorKind::NoGo
    } else {
        CursorKind::Rally
    })
}

/// What a right-click does to the selected factories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RallyPlan {
    /// Nothing.
    Nothing,
    /// Every one of `factories` gets the rally point `cell`.
    Set {
        factories: Vec<EntityId>,
        cell: Cell,
    },
    /// `factory` loses its rally point.
    Clear { factory: EntityId },
}

/// What a right-click does to the selected factories when the cursor is `kind` (the answer of
/// [`crate::cursor::cursor_context`]); the only place a cursor becomes a rally command.
///
/// * **Rally** (factories only): the selected factories rally to the cell under the mouse.
/// * **RallyClear**: the factory under the mouse loses its rally point.
/// * A mixed selection (units and factories): the units get the order of their cursor, and the
///   factories follow it: **Move**, **Queue** and **AttackMove** over ground set the rally point to
///   the same cell; **Guard** over a selected factory clears its rally point, over anything else sets
///   it to that cell; **Attack** (an enemy), **NoGo**, Select, Inspect and the scroll cursors give the
///   factories nothing.
///
/// A cell in shroud or fog is allowed, like a move order; a cell off the map is not (the cursor is
/// NoGo there, and the sim refuses it anyway).
pub fn plan(
    kind: CursorKind,
    snap: &SimSnapshot,
    selection: &Selection,
    hover: &Hover,
) -> RallyPlan {
    let over_selected = hover
        .entity
        .filter(|&id| selected_factories(snap, selection).any(|(f, _)| f == id));
    let set = || {
        let factories: Vec<EntityId> = selected_factories(snap, selection)
            .map(|(id, _)| id)
            .collect();
        match hover.point.and_then(|p| snap.cell_at(p)) {
            Some(cell) if !factories.is_empty() => RallyPlan::Set { factories, cell },
            _ => RallyPlan::Nothing,
        }
    };
    match kind {
        CursorKind::RallyClear | CursorKind::Guard if over_selected.is_some() => {
            over_selected.map_or(RallyPlan::Nothing, |factory| RallyPlan::Clear { factory })
        }
        CursorKind::Rally
        | CursorKind::Move
        | CursorKind::Queue
        | CursorKind::AttackMove
        | CursorKind::Guard => set(),
        _ => RallyPlan::Nothing,
    }
}

/// The commands of a plan: one `SetRally` per factory.
pub fn commands(plan: &RallyPlan) -> Vec<Command> {
    match plan {
        RallyPlan::Nothing => Vec::new(),
        RallyPlan::Set { factories, cell } => factories
            .iter()
            .map(|&factory| Command::SetRally {
                factory,
                cell: Some(*cell),
            })
            .collect(),
        RallyPlan::Clear { factory } => vec![Command::SetRally {
            factory: *factory,
            cell: None,
        }],
    }
}

/// The cell centre `(x, y)` in cells where the units of the factory `id` appear: the cell next to the
/// middle of the exit side of its footprint (`sf_sim::exit_cell`, `buildings.md`), which may lie off
/// the map for a structure on the edge. `None` for anything that is not a factory of this snapshot.
/// The footprint is centred on the entity (`construction_view::front_edge`), so the anchor is the
/// position less half the footprint.
pub fn exit_cell_centre(snap: &SimSnapshot, id: EntityId) -> Option<(f32, f32)> {
    let view = snap.get(id)?;
    let side = snap.menu().exit_side(view.kind)?;
    let footprint = snap.stats_of(id)?.footprint?;
    let half = |n: u8| sf_math::Fx::from_ratio(i64::from(n), 2);
    let ax = view.pos.x.saturating_sub(half(footprint.width)).round_int();
    let ay = view
        .pos
        .y
        .saturating_sub(half(footprint.height))
        .round_int();
    let anchor = Cell::new(u32::try_from(ax).ok()?, u32::try_from(ay).ok()?);
    let (x, y) = sf_sim::exit_cell(anchor, footprint, side);
    Some((x as f32 + 0.5, y as f32 + 0.5))
}

#[cfg(test)]
mod tests;
