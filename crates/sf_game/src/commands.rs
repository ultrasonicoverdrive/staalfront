//! The first commands from the player (WP3.4): right-click moves, shift
//! queues, S stops, X scatters. Orders are sim state (`docs/rules/orders.md`);
//! this module only turns input into [`Command`]s and schedules them for the
//! next tick. It never touches the world's state (PLAN.md section 1.2 and
//! quality rule 1): the only thing it does to a [`World`] is
//! [`World::schedule`].
//!
//! # Temporary selection
//!
//! Real selection (click, drag, shift-add, groups) is WP3.3. Until then
//! [`DebugSelection`] holds whatever **F2** picked: every mobile unit of the
//! local player that is in view. It is a debug aid and goes when WP3.3 lands;
//! nothing else depends on how it picks.
//!
//! # Keys
//!
//! Every key and button is an action of `assets/config/input.ron`, so each can
//! be rebound there; the runner asks the actions, never a key. The defaults:
//!
//! | Action | Default | Command |
//! |---|---|---|
//! | `DebugSelectAll` | F2 | (temporary) select all own units in view |
//! | `OrderMove` | right-click on the ground | `Order::Move` to the point |
//! | `OrderQueue` | shift (held) | the move, scatter or hold is queued behind the unit's orders |
//! | `OrderStop` | C | `Stop` |
//! | `OrderScatter` | X | `Order::Scatter` |
//! | `OrderHold` | H | `Order::HoldPosition` |
//!
//! The camera keeps WASD, the arrows and the screen edge; stop is not on S so
//! the two never share a key.

use sf_math::Vec2;
use sf_sim::{Command, EntityId, Order, PlayerId, World};

use crate::sandbox::MERIDIAN;

/// The seat of the player at this keyboard: Meridian. (Seats come with the
/// skirmish setup of a later phase.)
pub const LOCAL_PLAYER: PlayerId = PlayerId(MERIDIAN);

/// The units F2 selected. **Temporary** (WP3.3 replaces it with real
/// selection).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugSelection {
    ids: Vec<EntityId>,
}

impl DebugSelection {
    /// The selected units, in slot order.
    pub fn ids(&self) -> &[EntityId] {
        &self.ids
    }

    /// How many are selected.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// True if nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Selects every mobile unit of `owner` whose position (in cells) passes
    /// `in_view`, replacing the selection. Only roles of the unit table count
    /// (a unit that can move: not a structure, not an object, not a legacy
    /// wanderer).
    pub fn select_own_in_view(
        &mut self,
        world: &World,
        owner: PlayerId,
        in_view: impl Fn(Vec2) -> bool,
    ) {
        self.ids = world
            .snapshot()
            .iter()
            .filter(|v| v.owner == owner && v.has_stats && in_view(v.pos))
            .filter(|v| {
                world
                    .stats_of(v.id)
                    .is_some_and(|s| s.movement_class.is_some())
            })
            .map(|v| v.id)
            .collect();
    }

    /// Drops the units that are gone.
    pub fn forget_the_dead(&mut self, world: &World) {
        self.ids.retain(|&id| world.is_alive(id));
    }
}

/// What a right-click on the ground at `point` (in cells) asks for; `None`
/// with nothing selected. `queued` is the shift key.
pub fn move_command(selection: &DebugSelection, point: Vec2, queued: bool) -> Option<Command> {
    order_command(selection, Order::Move(point), queued)
}

/// What the scatter key asks for; `None` with nothing selected.
pub fn scatter_command(selection: &DebugSelection, queued: bool) -> Option<Command> {
    order_command(selection, Order::Scatter, queued)
}

/// What the hold-position key asks for; `None` with nothing selected.
pub fn hold_command(selection: &DebugSelection, queued: bool) -> Option<Command> {
    order_command(selection, Order::HoldPosition, queued)
}

/// What the stop key asks for; `None` with nothing selected.
pub fn stop_command(selection: &DebugSelection) -> Option<Command> {
    (!selection.is_empty()).then(|| Command::Stop {
        ids: selection.ids().to_vec(),
    })
}

fn order_command(selection: &DebugSelection, order: Order, queued: bool) -> Option<Command> {
    (!selection.is_empty()).then(|| Command::Order {
        ids: selection.ids().to_vec(),
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
    use sf_data::units::UnitDefs;
    use sf_math::Fx;
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

    #[test]
    fn f2_selects_own_mobile_units_in_view_only() {
        let (world, ids) = world_with(&[
            ("light_tank", 0, 10, 10),
            ("rifleman", 0, 12, 10),
            ("rifleman", 1, 14, 10),   // the other side
            ("light_tank", 0, 50, 50), // out of view
        ]);
        let mut selection = DebugSelection::default();
        selection.select_own_in_view(&world, LOCAL_PLAYER, |p| p.x < Fx::from_int(32));
        assert_eq!(selection.ids(), &ids[..2]);
        // Nothing in view, nothing selected.
        selection.select_own_in_view(&world, LOCAL_PLAYER, |_| false);
        assert!(selection.is_empty());
    }

    #[test]
    fn structures_and_wanderers_are_never_selected() {
        let defs = UnitDefs::embedded();
        let table = sf_bridge::unit_table(&defs).unwrap();
        let mut world = World::new(WorldConfig::new(5).with_map_size(64, 64))
            .unwrap()
            .with_units(table)
            .unwrap();
        let schedule = |world: &mut World, command| {
            world.schedule(world.tick(), PlayerId(0), command).unwrap();
        };
        schedule(
            &mut world,
            Command::PlaceStructure {
                kind: Kind(defs.kind_of("refinery").unwrap()),
                cell: sf_sim::Cell::new(10, 10),
                owner: PlayerId(0),
            },
        );
        schedule(
            &mut world,
            Command::Spawn {
                kind: Kind(1),
                pos: pt(5, 5),
                owner: PlayerId(0),
            },
        );
        world.step();
        let mut selection = DebugSelection::default();
        selection.select_own_in_view(&world, LOCAL_PLAYER, |_| true);
        assert!(selection.is_empty());
    }

    #[test]
    fn right_click_issues_a_move_and_shift_queues_it() {
        let (mut world, ids) = world_with(&[("light_tank", 0, 10, 10), ("rifleman", 0, 12, 10)]);
        let mut selection = DebugSelection::default();
        selection.select_own_in_view(&world, LOCAL_PLAYER, |_| true);
        // The UI only builds and schedules commands: nothing has changed
        // until the tick runs them.
        let before = ids.iter().map(|&id| world.order_of(id)).collect::<Vec<_>>();
        issue(
            &mut world,
            move_command(&selection, pt(30, 10), false).unwrap(),
        );
        issue(
            &mut world,
            move_command(&selection, pt(30, 20), true).unwrap(),
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
        let mut selection = DebugSelection::default();
        assert!(stop_command(&selection).is_none());
        assert!(move_command(&selection, pt(1, 1), false).is_none());
        assert!(scatter_command(&selection, false).is_none());
        assert!(hold_command(&selection, false).is_none());
        selection.select_own_in_view(&world, LOCAL_PLAYER, |_| true);
        issue(&mut world, scatter_command(&selection, false).unwrap());
        world.step();
        for &id in &ids {
            assert!(matches!(world.order_of(id), Some(Order::Move(_))), "{id}");
        }
        issue(&mut world, hold_command(&selection, false).unwrap());
        world.step();
        for &id in &ids {
            assert_eq!(world.order_of(id), Some(Order::HoldPosition));
        }
        issue(&mut world, stop_command(&selection).unwrap());
        world.step();
        for &id in &ids {
            assert_eq!(world.order_of(id), Some(Order::Idle));
        }
    }

    #[test]
    fn the_dead_leave_the_selection() {
        let (mut world, ids) = world_with(&[("light_tank", 0, 10, 10), ("scout", 0, 12, 10)]);
        let mut selection = DebugSelection::default();
        selection.select_own_in_view(&world, LOCAL_PLAYER, |_| true);
        issue(&mut world, Command::Despawn { id: ids[0] });
        world.step();
        selection.forget_the_dead(&world);
        assert_eq!(selection.ids(), &ids[1..]);
    }
}
