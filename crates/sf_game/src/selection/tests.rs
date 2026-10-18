//! The selection logic without a window: clicks, rectangles, shift, double
//! clicks, groups and the dead (WP3.3). Which entity a click or a rectangle
//! touches on the screen is decided by `sf_render` (`Instancer::pick`,
//! `Instancer::pick_rect`, tested there for models and icons); here that
//! answer is simply handed in.

use super::*;
use crate::sandbox::{MERIDIAN, NEUTRAL, VEYRAN};
use sf_data::units::UnitDefs;
use sf_math::Vec2;
use sf_sim::{Cell, Command, Order, WorldConfig};

const ME: PlayerId = PlayerId(MERIDIAN);

struct Scene {
    world: World,
    defs: UnitDefs,
}

impl Scene {
    fn new() -> Self {
        let defs = UnitDefs::embedded();
        let table = sf_bridge::unit_table(&defs).unwrap();
        let world = World::new(WorldConfig::new(3).with_map_size(64, 64))
            .unwrap()
            .with_units(table)
            .unwrap();
        Scene { world, defs }
    }

    fn new_ids(&mut self, command: Command, owner: u8) -> EntityId {
        let before: Vec<EntityId> = self.world.snapshot().iter().map(|v| v.id).collect();
        self.world
            .schedule(self.world.tick(), PlayerId(owner), command)
            .unwrap();
        self.world.step();
        self.world
            .snapshot()
            .iter()
            .map(|v| v.id)
            .find(|id| !before.contains(id))
            .expect("the command made an entity")
    }

    fn unit(&mut self, role: &str, owner: u8, x: i32, y: i32) -> EntityId {
        let kind = Kind(self.defs.kind_of(role).unwrap());
        let pos = Vec2::from_ints(x, y);
        self.new_ids(
            Command::SpawnUnit {
                kind,
                pos,
                owner: PlayerId(owner),
            },
            owner,
        )
    }

    fn structure(&mut self, role: &str, owner: u8, x: u32, y: u32) -> EntityId {
        let kind = Kind(self.defs.kind_of(role).unwrap());
        let cell = Cell::new(x, y);
        self.new_ids(
            Command::PlaceStructure {
                kind,
                cell,
                owner: PlayerId(owner),
            },
            owner,
        )
    }

    fn kill(&mut self, id: EntityId) {
        self.world
            .schedule(self.world.tick(), ME, Command::Despawn { id })
            .unwrap();
        self.world.step();
        assert!(!self.world.is_alive(id));
    }
}

fn selection() -> Selection {
    Selection::new(ME)
}

#[test]
fn a_click_selects_one_own_unit_and_replaces_the_selection() {
    let mut s = Scene::new();
    let (a, b) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
    );
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    assert_eq!(sel.ids(), [a]);
    sel.click(&s.world, Some(b), false);
    assert_eq!(sel.ids(), [b]);
}

#[test]
fn a_click_on_bare_ground_deselects_and_a_shift_click_on_it_does_not() {
    let mut s = Scene::new();
    let a = s.unit("light_tank", 0, 10, 10);
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    sel.click(&s.world, None, true);
    assert_eq!(
        sel.ids(),
        [a],
        "a shift-click that missed keeps the selection"
    );
    sel.click(&s.world, None, false);
    assert!(sel.is_empty());
    assert!(sel.inspected().is_none());
}

#[test]
fn shift_toggles_a_unit_in_the_selection() {
    let mut s = Scene::new();
    let (a, b, c) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
        s.unit("scout", 0, 14, 10),
    );
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    sel.click(&s.world, Some(b), true);
    sel.click(&s.world, Some(c), true);
    assert_eq!(sel.ids(), [a, b, c]);
    sel.click(&s.world, Some(b), true);
    assert_eq!(sel.ids(), [a, c], "a second shift-click takes it out again");
    sel.click(&s.world, Some(a), true);
    sel.click(&s.world, Some(c), true);
    assert!(sel.is_empty());
}

#[test]
fn the_selection_is_ordered_by_id_however_it_was_built() {
    let mut s = Scene::new();
    let (a, b) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
    );
    let (mut one, mut two) = (selection(), selection());
    one.click(&s.world, Some(a), false);
    one.click(&s.world, Some(b), true);
    two.click(&s.world, Some(b), false);
    two.click(&s.world, Some(a), true);
    assert_eq!(one.ids(), two.ids());
}

#[test]
fn an_enemy_or_neutral_unit_can_be_inspected_but_never_ordered() {
    let mut s = Scene::new();
    let own = s.unit("light_tank", 0, 10, 10);
    let enemy = s.unit("light_tank", VEYRAN, 30, 10);
    let neutral = s.unit("civilian", NEUTRAL, 20, 20);
    let mut sel = selection();
    sel.click(&s.world, Some(own), false);

    sel.click(&s.world, Some(enemy), false);
    assert_eq!(sel.inspected(), Some(enemy));
    assert!(sel.is_empty(), "inspecting replaces the selection");
    assert!(sel.is_marked(enemy));
    assert!(
        sel.orderable(&s.world).is_empty(),
        "no order ever goes to it"
    );
    assert!(
        crate::commands::move_command(&sel.orderable(&s.world), Vec2::from_ints(1, 1), false)
            .is_none()
    );

    sel.click(&s.world, Some(neutral), false);
    assert_eq!(sel.inspected(), Some(neutral));
    assert!(!sel.is_marked(enemy));
    assert!(sel.orderable(&s.world).is_empty());

    // Shift cannot add it to a selection, and the own unit is back with a click.
    sel.click(&s.world, Some(own), false);
    sel.click(&s.world, Some(enemy), true);
    assert_eq!(sel.ids(), [own]);
    assert!(sel.inspected().is_none());
    // A shift-click on an own unit while inspecting leaves the inspection.
    sel.click(&s.world, Some(enemy), false);
    sel.click(&s.world, Some(own), true);
    assert_eq!(sel.ids(), [own]);
    assert!(sel.inspected().is_none());
}

#[test]
fn a_structure_is_selected_by_a_click_but_takes_no_order() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", 0, 10, 10);
    let barracks = s.structure("barracks", 0, 20, 20);
    let mut sel = selection();
    sel.click(&s.world, Some(barracks), false);
    assert_eq!(sel.ids(), [barracks]);
    assert!(sel.orderable(&s.world).is_empty());
    sel.click(&s.world, Some(tank), true);
    assert_eq!(sel.ids().len(), 2);
    assert_eq!(
        sel.orderable(&s.world),
        [tank],
        "orders go to the unit only"
    );
}

#[test]
fn a_drag_takes_own_mobile_units_and_leaves_structures_and_enemies() {
    let mut s = Scene::new();
    let (a, b) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
    );
    let barracks = s.structure("barracks", 0, 20, 20);
    let enemy = s.unit("light_tank", VEYRAN, 14, 10);
    let neutral = s.unit("civilian", NEUTRAL, 16, 10);
    let mut sel = selection();
    sel.drag(&s.world, &[a, b, barracks, enemy, neutral], false);
    assert_eq!(sel.ids(), [a, b]);
    assert!(sel.inspected().is_none());
}

#[test]
fn a_shift_drag_adds_and_a_plain_drag_replaces() {
    let mut s = Scene::new();
    let (a, b, c) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
        s.unit("scout", 0, 14, 10),
    );
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    sel.drag(&s.world, &[b, c], true);
    assert_eq!(sel.ids(), [a, b, c]);
    sel.drag(&s.world, &[a, b], true);
    assert_eq!(sel.ids(), [a, b, c], "a shift drag never takes a unit out");
    sel.drag(&s.world, &[c], false);
    assert_eq!(sel.ids(), [c]);
}

#[test]
fn a_drag_that_finds_nothing_deselects_unless_shift_is_held() {
    let mut s = Scene::new();
    let a = s.unit("light_tank", 0, 10, 10);
    let barracks = s.structure("barracks", 0, 20, 20);
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    sel.drag(&s.world, &[], true);
    assert_eq!(sel.ids(), [a]);
    sel.drag(&s.world, &[barracks], true);
    assert_eq!(sel.ids(), [a], "a structure in the rectangle is not taken");
    sel.drag(&s.world, &[barracks], false);
    assert!(sel.is_empty());
}

#[test]
fn a_drag_ends_an_inspection() {
    let mut s = Scene::new();
    let (a, enemy) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", VEYRAN, 40, 10),
    );
    let mut sel = selection();
    sel.click(&s.world, Some(enemy), false);
    sel.drag(&s.world, &[a], false);
    assert_eq!(sel.ids(), [a]);
    assert!(sel.inspected().is_none());
}

#[test]
fn a_double_click_takes_own_units_of_that_kind_in_view() {
    let mut s = Scene::new();
    let t1 = s.unit("light_tank", 0, 10, 10);
    let t2 = s.unit("light_tank", 0, 12, 10);
    let t_far = s.unit("light_tank", 0, 60, 60); // not in view
    let rifle = s.unit("rifleman", 0, 14, 10);
    let enemy_tank = s.unit("light_tank", VEYRAN, 16, 10);
    let in_view = [t1, t2, rifle, enemy_tank];
    let _ = t_far;
    let mut sel = selection();
    sel.select_same_kind(&s.world, Some(t1), &in_view, false);
    assert_eq!(sel.ids(), [t1, t2]);
    // The clicked unit counts even if the view list missed it.
    let mut sel = selection();
    sel.select_same_kind(&s.world, Some(rifle), &[], false);
    assert_eq!(sel.ids(), [rifle]);
}

#[test]
fn a_shift_double_click_adds_the_kind_to_the_selection() {
    let mut s = Scene::new();
    let (t1, t2) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("light_tank", 0, 12, 10),
    );
    let rifle = s.unit("rifleman", 0, 14, 10);
    let mut sel = selection();
    sel.click(&s.world, Some(rifle), false);
    sel.select_same_kind(&s.world, Some(t1), &[t1, t2, rifle], true);
    assert_eq!(sel.ids(), [t1, t2, rifle]);
}

#[test]
fn a_double_click_on_a_structure_or_an_enemy_is_a_single_click() {
    let mut s = Scene::new();
    let (b1, b2) = (
        s.structure("barracks", 0, 20, 20),
        s.structure("barracks", 0, 30, 20),
    );
    let enemy = s.unit("light_tank", VEYRAN, 16, 10);
    let enemy2 = s.unit("light_tank", VEYRAN, 18, 10);
    let mut sel = selection();
    sel.select_same_kind(&s.world, Some(b1), &[b1, b2], false);
    assert_eq!(
        sel.ids(),
        [b1],
        "structures are only selected one at a time"
    );
    sel.select_same_kind(&s.world, Some(enemy), &[enemy, enemy2], false);
    assert_eq!(sel.inspected(), Some(enemy));
    assert!(sel.is_empty());
    sel.select_same_kind(&s.world, None, &[], false);
    assert!(sel.inspected().is_none());
}

#[test]
fn groups_are_assigned_and_recalled() {
    let mut s = Scene::new();
    let (a, b, c) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
        s.unit("scout", 0, 14, 10),
    );
    let mut sel = selection();
    sel.drag(&s.world, &[a, b], false);
    sel.assign_group(0);
    sel.click(&s.world, Some(c), false);
    sel.assign_group(1);
    assert_eq!(sel.group(0), [a, b]);
    assert_eq!(sel.group(1), [c]);

    sel.clear();
    assert_eq!(sel.recall_group(&s.world, 0, 1_000), Recall::Selected);
    assert_eq!(sel.ids(), [a, b]);
    assert_eq!(sel.recall_group(&s.world, 1, 5_000), Recall::Selected);
    assert_eq!(sel.ids(), [c]);
}

#[test]
fn a_unit_is_in_one_group_at_a_time() {
    let mut s = Scene::new();
    let (a, b) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
    );
    let mut sel = selection();
    sel.drag(&s.world, &[a, b], false);
    sel.assign_group(0);
    sel.click(&s.world, Some(b), false);
    sel.assign_group(2);
    assert_eq!(sel.group(0), [a], "b left group 1 for group 3");
    assert_eq!(sel.group(2), [b]);
    // Assigning again replaces the group's members.
    sel.click(&s.world, Some(a), false);
    sel.assign_group(2);
    assert_eq!(sel.group(2), [a]);
    assert!(sel.group(0).is_empty());
    // Assigning an empty selection empties the group.
    sel.clear();
    sel.assign_group(2);
    assert!(sel.group(2).is_empty());
}

#[test]
fn a_double_tap_centres_on_the_group_and_a_slow_one_does_not() {
    let mut s = Scene::new();
    let (a, b) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 20, 30),
    );
    let mut sel = selection();
    sel.drag(&s.world, &[a, b], false);
    sel.assign_group(3);
    sel.clear();

    assert_eq!(sel.recall_group(&s.world, 3, 10_000), Recall::Selected);
    let Recall::Centre([x, y]) = sel.recall_group(&s.world, 3, 10_000 + DOUBLE_TAP_MS) else {
        panic!("the second tap in time centres the camera");
    };
    // The middle of (10, 10) and (20, 30), in cells (units stand on a cell middle or its corner).
    assert!((x - 15.0).abs() < 1.0 && (y - 20.0).abs() < 1.0, "{x} {y}");
    assert_eq!(sel.ids(), [a, b]);
    // A third tap is a new first tap.
    assert_eq!(
        sel.recall_group(&s.world, 3, 10_100 + DOUBLE_TAP_MS),
        Recall::Selected
    );

    // Too slow, or another number in between.
    assert_eq!(sel.recall_group(&s.world, 3, 20_000), Recall::Selected);
    assert_eq!(
        sel.recall_group(&s.world, 3, 20_000 + DOUBLE_TAP_MS + 1),
        Recall::Selected
    );
    sel.click(&s.world, Some(a), false);
    sel.assign_group(4);
    assert_eq!(sel.recall_group(&s.world, 3, 30_000), Recall::Selected);
    assert_eq!(sel.recall_group(&s.world, 4, 30_100), Recall::Selected);
    assert_eq!(sel.recall_group(&s.world, 3, 30_200), Recall::Selected);
}

#[test]
fn an_empty_group_leaves_the_selection_alone() {
    let mut s = Scene::new();
    let a = s.unit("light_tank", 0, 10, 10);
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    assert_eq!(sel.recall_group(&s.world, 5, 0), Recall::Empty);
    assert_eq!(sel.ids(), [a]);
    assert_eq!(sel.recall_group(&s.world, GROUPS, 0), Recall::Empty);
    assert!(sel.group(GROUPS).is_empty());
}

#[test]
fn recalling_a_group_ends_an_inspection() {
    let mut s = Scene::new();
    let (a, enemy) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("light_tank", VEYRAN, 40, 10),
    );
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    sel.assign_group(0);
    sel.click(&s.world, Some(enemy), false);
    sel.recall_group(&s.world, 0, 0);
    assert_eq!(sel.ids(), [a]);
    assert!(sel.inspected().is_none());
}

#[test]
fn the_dead_leave_the_selection_the_groups_and_the_inspection() {
    let mut s = Scene::new();
    let (a, b) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
    );
    let enemy = s.unit("light_tank", VEYRAN, 40, 10);
    let mut sel = selection();
    sel.drag(&s.world, &[a, b], false);
    sel.assign_group(0);
    s.kill(a);
    sel.forget_the_dead(&s.world);
    assert_eq!(sel.ids(), [b]);
    assert_eq!(sel.group(0), [b]);

    sel.click(&s.world, Some(enemy), false);
    s.kill(enemy);
    sel.forget_the_dead(&s.world);
    assert!(sel.inspected().is_none());
    assert!(!sel.is_marked(enemy));
}

#[test]
fn a_new_unit_in_a_dead_unit_slot_is_not_selected() {
    let mut s = Scene::new();
    let a = s.unit("light_tank", 0, 10, 10);
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    sel.assign_group(0);
    s.kill(a);
    // The freed slot is reused by the next spawn, under a new generation.
    let reborn = s.unit("light_tank", 0, 11, 11);
    assert_eq!(reborn.index(), a.index(), "the slot was reused");
    assert_ne!(reborn.generation(), a.generation());
    assert!(!sel.is_marked(reborn));
    sel.forget_the_dead(&s.world);
    assert!(
        sel.is_empty(),
        "the old id is stale, whatever lives in its slot now"
    );
    assert!(sel.group(0).is_empty());
    assert_eq!(sel.recall_group(&s.world, 0, 0), Recall::Empty);
}

#[test]
fn a_selection_never_changes_the_world_and_its_orders_are_commands() {
    use crate::commands;
    let mut s = Scene::new();
    let (a, b) = (
        s.unit("light_tank", 0, 10, 10),
        s.unit("rifleman", 0, 12, 10),
    );
    let enemy = s.unit("light_tank", VEYRAN, 40, 10);
    let checksum = s.world.checksum_history().to_vec();
    let mut sel = selection();
    sel.click(&s.world, Some(a), false);
    sel.click(&s.world, Some(b), true);
    sel.assign_group(0);
    sel.click(&s.world, Some(enemy), false);
    sel.recall_group(&s.world, 0, 0);
    assert_eq!(
        s.world.checksum_history(),
        checksum,
        "selecting is not a sim event"
    );
    assert!(s.world.pending_commands().is_empty(), "nor a command");

    let ids = sel.orderable(&s.world);
    commands::issue(
        &mut s.world,
        commands::move_command(&ids, Vec2::from_ints(30, 30), false).unwrap(),
    );
    assert_eq!(
        s.world.order_of(a),
        Some(Order::Idle),
        "nothing changes before the tick"
    );
    s.world.step();
    assert_eq!(
        s.world.order_of(a),
        Some(Order::Move(Vec2::from_ints(30, 30)))
    );
    assert_eq!(
        s.world.order_of(b),
        Some(Order::Move(Vec2::from_ints(30, 30)))
    );
    assert_eq!(
        s.world.order_of(enemy),
        Some(Order::Idle),
        "the enemy got nothing"
    );
}

#[test]
fn placeholder_wanderers_cannot_be_selected() {
    let mut s = Scene::new();
    let id = s.new_ids(
        Command::Spawn {
            kind: Kind(1),
            pos: Vec2::from_ints(5, 5),
            owner: ME,
        },
        0,
    );
    let mut sel = selection();
    sel.click(&s.world, Some(id), false);
    assert!(sel.is_empty());
    sel.drag(&s.world, &[id], false);
    assert!(sel.is_empty());
}

// ---- the pointer ----

fn at(x: f32, y: f32) -> [f32; 2] {
    [x, y]
}

#[test]
fn a_press_and_release_in_place_is_a_click() {
    let mut p = Pointer::default();
    p.press(at(100.0, 100.0));
    assert!(p.is_down());
    assert!(p.drag_rect().is_none());
    p.moved(at(102.0, 101.0));
    assert!(p.drag_rect().is_none(), "a small wobble is not a drag");
    assert_eq!(
        p.release(at(102.0, 101.0), 0),
        Some(Gesture::Click { double: false })
    );
    assert!(!p.is_down());
}

#[test]
fn moving_far_enough_makes_a_drag_with_the_rectangle_between_the_corners() {
    let mut p = Pointer::default();
    p.press(at(200.0, 150.0));
    p.moved(at(120.0, 190.0));
    assert_eq!(
        p.drag_rect(),
        Some(ScreenRect::from_corners(at(120.0, 150.0), at(200.0, 190.0)))
    );
    let g = p.release(at(100.0, 220.0), 0).unwrap();
    assert_eq!(
        g,
        Gesture::Drag(ScreenRect::from_corners(at(100.0, 150.0), at(200.0, 220.0)))
    );
    if let Gesture::Drag(r) = g {
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (100.0, 150.0, 200.0, 220.0));
    }
}

#[test]
fn a_drag_stays_a_drag_if_the_cursor_comes_back() {
    let mut p = Pointer::default();
    p.press(at(50.0, 50.0));
    p.moved(at(90.0, 50.0));
    p.moved(at(51.0, 50.0));
    assert!(matches!(
        p.release(at(51.0, 50.0), 0),
        Some(Gesture::Drag(_))
    ));
}

#[test]
fn two_quick_close_clicks_are_a_double_click_and_a_third_starts_over() {
    let mut p = Pointer::default();
    let click = |p: &mut Pointer, x: f32, t: u64| {
        p.press(at(x, 80.0));
        p.release(at(x, 80.0), t)
    };
    assert_eq!(
        click(&mut p, 100.0, 1_000),
        Some(Gesture::Click { double: false })
    );
    assert_eq!(
        click(&mut p, 103.0, 1_000 + DOUBLE_CLICK_MS),
        Some(Gesture::Click { double: true })
    );
    assert_eq!(
        click(&mut p, 103.0, 1_100 + DOUBLE_CLICK_MS),
        Some(Gesture::Click { double: false })
    );
}

#[test]
fn slow_or_distant_clicks_are_not_a_double_click() {
    let mut p = Pointer::default();
    let click = |p: &mut Pointer, x: f32, t: u64| {
        p.press(at(x, 80.0));
        p.release(at(x, 80.0), t)
    };
    click(&mut p, 100.0, 0);
    assert_eq!(
        click(&mut p, 100.0, DOUBLE_CLICK_MS + 1),
        Some(Gesture::Click { double: false })
    );
    assert_eq!(
        click(&mut p, 100.0 + DOUBLE_CLICK_PX + 1.0, DOUBLE_CLICK_MS + 50),
        Some(Gesture::Click { double: false })
    );
}

#[test]
fn a_drag_between_two_clicks_breaks_the_double_click() {
    let mut p = Pointer::default();
    p.press(at(100.0, 80.0));
    p.release(at(100.0, 80.0), 0);
    p.press(at(100.0, 80.0));
    p.moved(at(160.0, 120.0));
    assert!(matches!(
        p.release(at(160.0, 120.0), 100),
        Some(Gesture::Drag(_))
    ));
    p.press(at(100.0, 80.0));
    assert_eq!(
        p.release(at(100.0, 80.0), 200),
        Some(Gesture::Click { double: false })
    );
}

#[test]
fn a_release_without_a_tracked_press_is_nothing() {
    let mut p = Pointer::default();
    assert_eq!(p.release(at(1.0, 1.0), 0), None);
    p.press(at(1.0, 1.0));
    p.cancel();
    assert_eq!(p.release(at(1.0, 1.0), 0), None);
}
