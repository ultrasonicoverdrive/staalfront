//! The first commands from the player (WP3.4): right-click moves, shift
//! queues, C stops, X scatters, H holds. Orders are sim state
//! (`docs/rules/orders.md`); this module only turns input into [`Command`]s
//! and schedules them for the next tick. It never touches the world's state
//! (PLAN.md section 1.2 and quality rule 1): the only thing it does to a
//! [`World`] is [`World::schedule`].
//!
//! # Whom the orders go to
//!
//! To the units of [`Selection::orderable`](crate::selection::Selection::orderable):
//! the local player's selected mobile units (WP3.3). Selection is
//! presentation state in [`crate::selection`]; a unit that is only
//! *inspected* (an enemy or a neutral one) is never in that list, so no order
//! can reach it. The functions here take that list of ids and answer `None`
//! for an empty one.
//!
//! # Keys
//!
//! Every key and button is an action of `assets/config/input.ron`, so each can
//! be rebound there; the runner asks the actions, never a key. The defaults:
//!
//! | Action | Default | Command |
//! |---|---|---|
//! | `OrderMove` | right-click | the order the cursor shows ([`right_click`]): move, attack-move, attack or guard |
//! | `OrderQueue` | shift (held) | the order is queued behind the unit's orders |
//! | `OrderAttackMove` | F (held) | a right-click on the ground attack-moves |
//! | `OrderStop` | C | `Stop` |
//! | `OrderScatter` | X | `Order::Scatter` |
//! | `OrderHold` | H | `Order::HoldPosition` |
//!
//! The camera keeps WASD, the arrows and the screen edge; stop is not on S so
//! the two never share a key. Selecting is in [`crate::selection`].

use sf_math::Vec2;
use sf_sim::{Command, EntityId, Order, PlayerId, World};

use crate::cursor::{self, Hover, Modifiers};
use crate::sandbox::MERIDIAN;
use crate::selection::Selection;

/// The seat of the player at this keyboard: Meridian. (Seats come with the
/// skirmish setup of a later phase.)
pub const LOCAL_PLAYER: PlayerId = PlayerId(MERIDIAN);

/// What a right-click asks for: **exactly the order the cursor shows**
/// (WP3.5). The cursor is [`cursor::cursor_context`] and the order is
/// [`cursor::order_for`] of its answer, so there is one decision and the
/// picture and the click cannot disagree. `None` when the cursor promises no
/// order (Select, Inspect, NoGo, over the UI) or nothing can take one.
pub fn right_click(
    world: &World,
    selection: &Selection,
    hover: &Hover,
    modifiers: Modifiers,
) -> Option<Command> {
    let kind = cursor::cursor_context(world, selection, hover, modifiers);
    let order = cursor::order_for(kind, hover)?;
    let ids = selection.orderable(world);
    order_command(&ids, order, cursor::is_queued(kind, modifiers))
}

/// What a right-click on the ground at `point` (in cells) asks for; `None`
/// with nothing selected. `queued` is the shift key.
pub fn move_command(ids: &[EntityId], point: Vec2, queued: bool) -> Option<Command> {
    order_command(ids, Order::Move(point), queued)
}

/// What the scatter key asks for; `None` with nothing selected.
pub fn scatter_command(ids: &[EntityId], queued: bool) -> Option<Command> {
    order_command(ids, Order::Scatter, queued)
}

/// What the hold-position key asks for; `None` with nothing selected.
pub fn hold_command(ids: &[EntityId], queued: bool) -> Option<Command> {
    order_command(ids, Order::HoldPosition, queued)
}

/// What the stop key asks for; `None` with nothing selected.
pub fn stop_command(ids: &[EntityId]) -> Option<Command> {
    (!ids.is_empty()).then(|| Command::Stop { ids: ids.to_vec() })
}

fn order_command(ids: &[EntityId], order: Order, queued: bool) -> Option<Command> {
    (!ids.is_empty()).then(|| Command::Order {
        ids: ids.to_vec(),
        order,
        queued,
    })
}

/// Schedules `command` from the local player for the tick that runs next.
/// The simulation runs it at the start of that tick; nothing changes before.
/// Says what it asked for on the console, as the other game events do.
pub fn issue(world: &mut World, command: Command) {
    let tick = world.tick();
    match &command {
        Command::Order { ids, order, queued } => println!(
            "order for tick {tick}: {order:?} for {} units{}",
            ids.len(),
            if *queued { " (queued)" } else { "" }
        ),
        Command::Stop { ids } => println!("order for tick {tick}: Stop for {} units", ids.len()),
        _ => {}
    }
    if let Err(e) = world.schedule(tick, LOCAL_PLAYER, command) {
        // Cannot happen: the next tick is never in the past.
        eprintln!("sf_game: command refused: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;
    use sf_data::units::UnitDefs;
    use sf_sim::{Kind, WorldConfig};

    fn world_with(units: &[(&str, u8, i32, i32)]) -> (World, Vec<EntityId>) {
        let defs = UnitDefs::embedded();
        let table = sf_bridge::unit_table(&defs).unwrap();
        let mut world = World::new(WorldConfig::new(5).with_map_size(64, 64))
            .unwrap()
            .with_units(table)
            .unwrap();
        for &(role, owner, x, y) in units {
            let command = Command::SpawnUnit {
                kind: Kind(defs.kind_of(role).unwrap()),
                pos: Vec2::from_ints(x, y),
                owner: PlayerId(owner),
            };
            world
                .schedule(world.tick(), PlayerId(owner), command)
                .unwrap();
        }
        world.step();
        let ids = world.snapshot().iter().map(|v| v.id).collect();
        (world, ids)
    }

    fn pt(x: i32, y: i32) -> Vec2 {
        Vec2::from_ints(x, y)
    }

    /// Everything of the local player selected, as a drag over the whole map
    /// would.
    fn all_own(world: &World, ids: &[EntityId]) -> Vec<EntityId> {
        let mut selection = Selection::new(LOCAL_PLAYER);
        selection.drag(world, ids, false);
        selection.orderable(world)
    }

    #[test]
    fn right_click_issues_a_move_and_shift_queues_it() {
        let (mut world, ids) = world_with(&[("light_tank", 0, 10, 10), ("rifleman", 0, 12, 10)]);
        let selected = all_own(&world, &ids);
        assert_eq!(selected, ids);
        // The UI only builds and schedules commands: nothing has changed
        // until the tick runs them.
        let before = ids.iter().map(|&id| world.order_of(id)).collect::<Vec<_>>();
        issue(
            &mut world,
            move_command(&selected, pt(30, 10), false).unwrap(),
        );
        issue(
            &mut world,
            move_command(&selected, pt(30, 20), true).unwrap(),
        );
        assert_eq!(
            ids.iter().map(|&id| world.order_of(id)).collect::<Vec<_>>(),
            before
        );
        let report = world.step();
        assert_eq!(report.commands_executed, 2);
        for &id in &ids {
            assert_eq!(world.order_of(id), Some(Order::Move(pt(30, 10))));
            assert_eq!(
                world.queued_orders_of(id),
                Some(vec![Order::Move(pt(30, 20))])
            );
        }
    }

    #[test]
    fn stop_scatter_and_hold_follow_the_selection() {
        let (mut world, ids) = world_with(&[("light_tank", 0, 10, 10), ("scout", 0, 12, 10)]);
        assert!(stop_command(&[]).is_none());
        assert!(move_command(&[], pt(1, 1), false).is_none());
        assert!(scatter_command(&[], false).is_none());
        assert!(hold_command(&[], false).is_none());
        let selected = all_own(&world, &ids);
        issue(&mut world, scatter_command(&selected, false).unwrap());
        world.step();
        for &id in &ids {
            assert!(matches!(world.order_of(id), Some(Order::Move(_))), "{id}");
        }
        issue(&mut world, hold_command(&selected, false).unwrap());
        world.step();
        for &id in &ids {
            assert_eq!(world.order_of(id), Some(Order::HoldPosition));
        }
        issue(&mut world, stop_command(&selected).unwrap());
        world.step();
        for &id in &ids {
            assert_eq!(world.order_of(id), Some(Order::Idle));
        }
    }

    #[test]
    fn structures_and_other_sides_get_no_order_from_a_selection() {
        let (world, ids) = world_with(&[
            ("light_tank", 0, 10, 10),
            ("rifleman", 1, 12, 10),
            ("civilian", 2, 14, 10),
        ]);
        // A drag takes only own units; inspecting an enemy orders nothing.
        let mut selection = Selection::new(LOCAL_PLAYER);
        selection.drag(&world, &ids, false);
        assert_eq!(selection.orderable(&world), &ids[..1]);
        selection.click(&world, Some(ids[1]), false);
        assert!(selection.orderable(&world).is_empty());
        assert!(move_command(&selection.orderable(&world), pt(1, 1), false).is_none());
    }
}
