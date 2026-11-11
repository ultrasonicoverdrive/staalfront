//! Rally points by right-click and the sidebar that follows the selection, without a window (WP6.9):
//! which command each right-click sends (ground, one of the selected factories, an enemy, a neutral
//! thing, off the map, a mixed selection), that the cursor shows exactly that, and when the sidebar
//! changes its tab. The sim carries the commands out here, so a rally point that the cursor promised
//! is really set (or cleared).

use super::*;
use crate::commands::{self, LOCAL_PLAYER};
use crate::cursor::{CursorKind, Modifiers, cursor_context, order_for};
use crate::playfield;
use crate::sandbox::{MERIDIAN, NEUTRAL, VEYRAN};
use crate::sidebar::{Tab, TabFollow};
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::UnitDefs;
use sf_sim::{Kind, Order, PlayerId, Terrain, World, WorldConfig};

const W: u32 = 64;

/// A cell on grass, one in the river (columns 20 to 29).
const LAND: Cell = Cell::new(10, 10);
const RIVER: Cell = Cell::new(25, 10);
const WATER: u8 = 2;
const GRASS: u8 = 0;

struct Scene {
    world: World,
    defs: UnitDefs,
}

impl Scene {
    fn new(repair: bool) -> Self {
        let defs = UnitDefs::embedded();
        let types = TerrainTypes::embedded();
        let costs = MovementCosts::embedded();
        let mut ids = vec![GRASS; (W * W) as usize];
        for y in 0..W {
            for x in 20..=29 {
                ids[(y * W + x) as usize] = WATER;
            }
        }
        let terrain = Terrain::from_layer(W, W, &ids, &sf_bridge::terrain_defs(&types, &costs))
            .expect("the layer is valid");
        let world = World::with_terrain(WorldConfig::new(7).with_map_size(W, W), terrain)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let world = playfield::with_buildings(playfield::with_movement(world, &defs), &defs);
        let mut world = playfield::with_production(world, &defs);
        if repair {
            world = playfield::with_repair(world);
        }
        Scene { world, defs }
    }

    fn snap(&self) -> SimSnapshot {
        SimSnapshot::capture(&self.world)
    }

    fn made(&mut self, command: Command, owner: u8) -> EntityId {
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
        let pos = sf_math::Vec2::from_ints(x, y);
        let command = Command::SpawnUnit {
            kind,
            pos,
            owner: PlayerId(owner),
        };
        self.made(command, owner)
    }

    fn structure(&mut self, role: &str, owner: u8, x: u32, y: u32) -> EntityId {
        let kind = Kind(self.defs.kind_of(role).unwrap());
        let command = Command::PlaceStructure {
            kind,
            cell: Cell::new(x, y),
            owner: PlayerId(owner),
        };
        self.made(command, owner)
    }

    /// The local player's selection of `ids`, each added by a shift-click (so a structure can be in
    /// it).
    fn select(&self, ids: &[EntityId]) -> Selection {
        let snap = self.snap();
        let mut sel = Selection::new(LOCAL_PLAYER);
        for &id in ids {
            sel.click(&snap, Some(id), true);
        }
        assert_eq!(sel.ids().len(), ids.len(), "everything is selectable");
        sel
    }

    fn cursor(&self, sel: &Selection, hover: &Hover) -> CursorKind {
        self.with(sel, hover, Modifiers::default())
    }

    fn with(&self, sel: &Selection, hover: &Hover, mods: Modifiers) -> CursorKind {
        cursor_context(&self.snap(), sel, hover, mods)
    }

    /// The factory commands of a plain right-click.
    fn rally(&self, sel: &Selection, hover: &Hover) -> Vec<Command> {
        commands::rally_clicks(&self.snap(), sel, hover, Modifiers::default())
    }

    /// Runs `commands` as the sim thread would: scheduled for the next tick, then the tick.
    fn run(&mut self, commands: Vec<Command>) {
        for command in commands {
            self.world
                .schedule(self.world.tick(), LOCAL_PLAYER, command)
                .unwrap();
        }
        self.world.step();
    }

    fn rally_of(&self, factory: EntityId) -> Option<Cell> {
        self.world.production_of(factory).and_then(|v| v.rally)
    }
}

fn set(factory: EntityId, cell: Cell) -> Command {
    Command::SetRally {
        factory,
        cell: Some(cell),
    }
}

fn clear(factory: EntityId) -> Command {
    Command::SetRally {
        factory,
        cell: None,
    }
}

fn at(cell: Cell) -> Hover {
    Hover::ground(cell)
}

/// Two own factories (a barracks and a vehicle factory), an own power plant and tank, an enemy
/// rifleman and barracks, a neutral civilian.
struct Cast {
    s: Scene,
    barracks: EntityId,
    workshop: EntityId,
    plant: EntityId,
    tank: EntityId,
    enemy: EntityId,
    enemy_barracks: EntityId,
    civilian: EntityId,
}

fn cast() -> Cast {
    let mut s = Scene::new(false);
    let barracks = s.structure("barracks", MERIDIAN, 5, 30);
    let workshop = s.structure("vehicle_factory", MERIDIAN, 12, 40);
    let plant = s.structure("power_plant", MERIDIAN, 5, 50);
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let enemy_barracks = s.structure("barracks", VEYRAN, 50, 30);
    let civilian = s.unit("civilian", NEUTRAL, 50, 12);
    Cast {
        s,
        barracks,
        workshop,
        plant,
        tank,
        enemy,
        enemy_barracks,
        civilian,
    }
}

#[test]
fn a_right_click_on_the_ground_sets_the_rally_point_of_every_selected_factory() {
    let c = cast();
    let mut s = c.s;
    let sel = s.select(&[c.barracks, c.workshop]);
    let hover = at(LAND);
    assert_eq!(s.cursor(&sel, &hover), CursorKind::Rally);
    // No unit takes part: the right-click gives no unit order.
    assert_eq!(
        commands::right_click(&s.snap(), &sel, &hover, Modifiers::default()),
        None
    );
    let sent = s.rally(&sel, &hover);
    assert_eq!(sent, vec![set(c.barracks, LAND), set(c.workshop, LAND)]);
    assert_eq!(s.rally_of(c.barracks), None);
    s.run(sent);
    // The sim took both.
    assert_eq!(s.rally_of(c.barracks), Some(LAND));
    assert_eq!(s.rally_of(c.workshop), Some(LAND));
}

#[test]
fn only_the_selected_factories_are_rallied() {
    let c = cast();
    let mut s = c.s;
    let sel = s.select(&[c.workshop]);
    let sent = s.rally(&sel, &at(LAND));
    assert_eq!(sent, vec![set(c.workshop, LAND)]);
    s.run(sent);
    assert_eq!(s.rally_of(c.workshop), Some(LAND));
    assert_eq!(
        s.rally_of(c.barracks),
        None,
        "the barracks was not selected"
    );
}

#[test]
fn no_factory_selected_means_no_rally_command() {
    let c = cast();
    let s = c.s;
    let nobody = Selection::new(LOCAL_PLAYER);
    let hovers = [at(LAND), at(RIVER), Hover::default()];
    for sel in [nobody, s.select(&[c.plant]), s.select(&[c.tank])] {
        for hover in &hovers {
            let kind = s.cursor(&sel, hover);
            assert!(!kind.is_rally(), "{kind:?}");
            assert_eq!(s.rally(&sel, hover), vec![], "{kind:?} for {hover:?}");
        }
    }
    // Factories that stand but are not selected are not rallied by a click meant for the tank.
    let sel = s.select(&[c.tank]);
    assert_eq!(s.cursor(&sel, &at(LAND)), CursorKind::Move);
    assert_eq!(s.rally(&sel, &at(LAND)), vec![]);
}

#[test]
fn a_right_click_on_a_selected_factory_clears_its_rally_point_and_only_its() {
    let c = cast();
    let mut s = c.s;
    let both = s.select(&[c.barracks, c.workshop]);
    s.run(s.rally(&both, &at(LAND)));
    assert_eq!(s.rally_of(c.barracks), Some(LAND));
    let on_barracks = Hover::over(c.barracks, Cell::new(5, 30));
    assert_eq!(s.cursor(&both, &on_barracks), CursorKind::RallyClear);
    let sent = s.rally(&both, &on_barracks);
    assert_eq!(sent, vec![clear(c.barracks)]);
    s.run(sent);
    assert_eq!(s.rally_of(c.barracks), None, "cleared");
    assert_eq!(s.rally_of(c.workshop), Some(LAND), "the other one is kept");
    // A factory that is not selected, under the mouse, is ground: the selected ones rally to it.
    let only_workshop = s.select(&[c.workshop]);
    let hover = Hover::over(c.barracks, Cell::new(5, 30));
    assert_eq!(s.cursor(&only_workshop, &hover), CursorKind::Rally);
    assert_eq!(
        s.rally(&only_workshop, &hover),
        vec![set(c.workshop, Cell::new(5, 30))]
    );
}

#[test]
fn an_enemy_a_neutral_thing_the_sky_and_the_map_edge_give_a_factory_selection_nothing() {
    let c = cast();
    let s = c.s;
    let sel = s.select(&[c.barracks]);
    for (hover, kind) in [
        (Hover::over(c.enemy, Cell::new(50, 10)), CursorKind::Inspect),
        (
            Hover::over(c.enemy_barracks, Cell::new(50, 30)),
            CursorKind::Inspect,
        ),
        (
            Hover::over(c.civilian, Cell::new(50, 12)),
            CursorKind::Inspect,
        ),
        // No ground point (the sky, the UI): select.
        (Hover::default(), CursorKind::Select),
    ] {
        assert_eq!(s.cursor(&sel, &hover), kind, "{hover:?}");
        assert_eq!(s.rally(&sel, &hover), vec![], "{hover:?}");
    }
    // Off the map: the forbidden sign, and no command (the sim would refuse it, `OffMap`).
    for point in [
        sf_math::Vec2::from_ints(-3, 10),
        sf_math::Vec2::from_ints(10, 64),
        sf_math::Vec2::from_ints(70, 70),
    ] {
        let hover = Hover {
            point: Some(point),
            ..Hover::default()
        };
        assert_eq!(s.cursor(&sel, &hover), CursorKind::NoGo, "{point:?}");
        assert_eq!(s.rally(&sel, &hover), vec![], "{point:?}");
    }
    // The last cell of the map is on the map.
    let hover = at(Cell::new(W - 1, W - 1));
    assert_eq!(s.cursor(&sel, &hover), CursorKind::Rally);
}

#[test]
fn a_cell_nobody_can_walk_to_is_still_a_rally_cell() {
    // The sim's only check is the map (`SetRally` refuses `OffMap`), as for a move order a unit that
    // may enter water could get; the cursor says Rally, not NoGo.
    let c = cast();
    let mut s = c.s;
    let sel = s.select(&[c.barracks]);
    assert_eq!(s.cursor(&sel, &at(RIVER)), CursorKind::Rally);
    s.run(s.rally(&sel, &at(RIVER)));
    assert_eq!(s.rally_of(c.barracks), Some(RIVER));
}

#[test]
fn a_mixed_selection_orders_the_units_and_rallies_the_factories_to_the_same_cell() {
    let c = cast();
    let mut s = c.s;
    let sel = s.select(&[c.tank, c.barracks, c.workshop]);
    let hover = at(LAND);
    // The units' cursor is the picture; the factories follow it.
    assert_eq!(s.cursor(&sel, &hover), CursorKind::Move);
    assert_eq!(
        commands::right_click(&s.snap(), &sel, &hover, Modifiers::default()),
        Some(Command::Order {
            ids: vec![c.tank],
            order: order_for(CursorKind::Move, &hover).unwrap(),
            queued: false,
        })
    );
    let sent = s.rally(&sel, &hover);
    assert_eq!(sent, vec![set(c.barracks, LAND), set(c.workshop, LAND)]);
    s.run(sent);
    assert_eq!(s.rally_of(c.barracks), Some(LAND));
    // Shift queues the units' move; the rally point is not queued, it is set.
    let queue = Modifiers {
        queue: true,
        attack_move: false,
    };
    assert_eq!(s.with(&sel, &hover, queue), CursorKind::Queue);
    assert_eq!(
        commands::rally_clicks(&s.snap(), &sel, &hover, queue),
        vec![set(c.barracks, LAND), set(c.workshop, LAND)]
    );
    // The same with attack-move held.
    let fight = Modifiers {
        queue: false,
        attack_move: true,
    };
    assert_eq!(s.with(&sel, &hover, fight), CursorKind::AttackMove);
    assert_eq!(
        commands::rally_clicks(&s.snap(), &sel, &hover, fight).len(),
        2
    );
}

#[test]
fn a_mixed_selection_over_an_enemy_attacks_and_rallies_nothing() {
    let c = cast();
    let s = c.s;
    let sel = s.select(&[c.tank, c.barracks]);
    let hover = Hover::over(c.enemy, Cell::new(50, 10));
    assert_eq!(s.cursor(&sel, &hover), CursorKind::Attack);
    assert!(
        commands::right_click(&s.snap(), &sel, &hover, Modifiers::default()).is_some(),
        "the tank attacks"
    );
    assert_eq!(s.rally(&sel, &hover), vec![]);
    // Water the tank cannot enter: the units get nothing (NoGo), and so do the factories, so the
    // picture is true.
    assert_eq!(s.cursor(&sel, &at(RIVER)), CursorKind::NoGo);
    assert_eq!(s.rally(&sel, &at(RIVER)), vec![]);
}

#[test]
fn a_mixed_selection_over_a_selected_factory_guards_it_and_keeps_its_rally_point() {
    let c = cast();
    let mut s = c.s;
    let sel = s.select(&[c.tank, c.barracks]);
    s.run(s.rally(&sel, &at(LAND)));
    assert_eq!(s.rally_of(c.barracks), Some(LAND));
    let hover = Hover::over(c.barracks, Cell::new(5, 30));
    assert_eq!(s.cursor(&sel, &hover), CursorKind::Guard);
    assert!(commands::right_click(&s.snap(), &sel, &hover, Modifiers::default()).is_some());
    // The cursor says Guard, nothing about the rally: the rally point stays.
    assert_eq!(s.rally(&sel, &hover), vec![]);
    assert_eq!(s.rally_of(c.barracks), Some(LAND));
    // Over an own unit that is not a selected factory, the barracks rallies to its cell.
    let hover = Hover::over(c.tank, Cell::new(12, 10));
    assert_eq!(
        s.rally(&sel, &hover),
        vec![set(c.barracks, Cell::new(12, 10))]
    );
}

#[test]
fn a_factory_being_sold_takes_no_rally_point() {
    let mut s = Scene::new(true);
    let barracks = s.structure("barracks", MERIDIAN, 5, 30);
    let sel = s.select(&[barracks]);
    assert_eq!(s.cursor(&sel, &at(LAND)), CursorKind::Rally);
    s.run(vec![Command::Sell {
        structure: barracks,
    }]);
    assert!(s.snap().sale_of(barracks).is_some(), "the sale began");
    // It stands no more: no cursor, no command, whether over the ground or over itself.
    assert_eq!(selected_factories(&s.snap(), &sel).count(), 0);
    assert_eq!(s.cursor(&sel, &at(LAND)), CursorKind::Select);
    assert_eq!(s.rally(&sel, &at(LAND)), vec![]);
    let on_it = Hover::over(barracks, Cell::new(5, 30));
    assert_eq!(s.rally(&sel, &on_it), vec![]);
}

/// Every selection, hover and modifier combination: the cursor and the commands agree.
#[test]
fn the_cursor_and_the_commands_of_a_right_click_never_disagree() {
    let c = cast();
    let s = c.s;
    let selections = [
        Selection::new(LOCAL_PLAYER),
        s.select(&[c.tank]),
        s.select(&[c.plant]),
        s.select(&[c.barracks]),
        s.select(&[c.barracks, c.workshop]),
        s.select(&[c.tank, c.barracks]),
        s.select(&[c.tank, c.barracks, c.workshop]),
        s.select(&[c.tank, c.plant]),
    ];
    let hovers = [
        Hover::default(),
        at(LAND),
        at(RIVER),
        Hover::over(c.barracks, Cell::new(5, 30)),
        Hover::over(c.workshop, Cell::new(12, 40)),
        Hover::over(c.plant, Cell::new(5, 50)),
        Hover::over(c.tank, Cell::new(12, 10)),
        Hover::over(c.enemy, Cell::new(50, 10)),
        Hover::over(c.enemy_barracks, Cell::new(50, 30)),
        Hover::over(c.civilian, Cell::new(50, 12)),
        Hover {
            point: Some(sf_math::Vec2::from_ints(-3, 10)),
            ..Hover::default()
        },
        Hover {
            edges: sf_engine::Edges {
                left: true,
                ..sf_engine::Edges::default()
            },
            ..at(LAND)
        },
    ];
    let snap = s.snap();
    let (mut sets, mut clears, mut mixed) = (0, 0, 0);
    for sel in &selections {
        let factories = selected_factories(&snap, sel).count();
        let units = sel.orderable(&snap).len();
        for hover in &hovers {
            for queue in [false, true] {
                for attack_move in [false, true] {
                    let mods = Modifiers { queue, attack_move };
                    let kind = s.with(sel, hover, mods);
                    let unit = commands::right_click(&snap, sel, hover, mods);
                    let sent = commands::rally_clicks(&snap, sel, hover, mods);
                    let ctx = format!("{kind:?} for {hover:?} {mods:?}: {sent:?}");
                    // Every command is a SetRally of a selected factory.
                    for command in &sent {
                        let Command::SetRally { factory, .. } = command else {
                            panic!("not a rally command: {ctx}");
                        };
                        assert!(
                            selected_factories(&snap, sel).any(|(f, _)| f == *factory),
                            "{ctx}"
                        );
                    }
                    match kind {
                        CursorKind::Rally => {
                            assert_eq!((units, unit), (0, None), "{ctx}");
                            assert_eq!(sent.len(), factories, "{ctx}");
                            assert!(
                                sent.iter()
                                    .all(|c| matches!(c, Command::SetRally { cell: Some(_), .. }))
                            );
                            sets += 1;
                        }
                        CursorKind::RallyClear => {
                            assert_eq!((units, unit), (0, None), "{ctx}");
                            assert_eq!(sent.len(), 1, "{ctx}");
                            assert!(matches!(sent[0], Command::SetRally { cell: None, .. }));
                            clears += 1;
                        }
                        CursorKind::Move | CursorKind::Queue | CursorKind::AttackMove => {
                            assert_eq!(sent.len(), factories, "{ctx}");
                            // The factories rally to the cell the units are sent to.
                            if factories > 0 {
                                let Some(Command::Order {
                                    order: Order::Move(p) | Order::AttackMove(p),
                                    ..
                                }) = unit
                                else {
                                    panic!("no move order for the units: {ctx} {unit:?}");
                                };
                                let target = snap.cell_at(p);
                                assert!(target.is_some(), "{ctx}");
                                for command in &sent {
                                    assert!(
                                        matches!(command, Command::SetRally { cell, .. } if *cell == target),
                                        "{ctx} {unit:?}"
                                    );
                                }
                            }
                            mixed += usize::from(factories > 0);
                        }
                        CursorKind::Guard => {
                            let over_selected = hover.entity.is_some_and(|id| {
                                selected_factories(&snap, sel).any(|(f, _)| f == id)
                            });
                            // Over a selected factory the units guard it and its rally stays.
                            assert_eq!(
                                sent.len(),
                                if over_selected { 0 } else { factories },
                                "{ctx}"
                            );
                        }
                        // The cursor promises the factories nothing.
                        _ => assert_eq!(sent, vec![], "{ctx}"),
                    }
                    // Without a factory selected a rally cursor and rally commands never happen.
                    if factories == 0 {
                        assert!(!kind.is_rally() && sent.is_empty(), "{ctx}");
                    }
                }
            }
        }
    }
    assert!(
        sets > 20 && clears > 5 && mixed > 20,
        "{sets} {clears} {mixed}"
    );
}

#[test]
fn the_rally_cursors_give_no_unit_order_and_have_names_and_pictures() {
    assert_eq!(CursorKind::Rally.name(), "rally");
    assert_eq!(CursorKind::RallyClear.name(), "rally_clear");
    assert!(CursorKind::Rally.is_rally() && CursorKind::RallyClear.is_rally());
    assert!(!CursorKind::Rally.is_placeholder() && !CursorKind::RallyClear.is_scroll());
    for kind in [CursorKind::Rally, CursorKind::RallyClear] {
        assert_eq!(order_for(kind, &at(LAND)), None);
        assert!(!crate::cursor::is_queued(
            kind,
            Modifiers {
                queue: true,
                attack_move: false
            }
        ));
    }
}

/// The line starts where the units come out: the sim's own exit cell.
#[test]
fn the_exit_cell_of_the_snapshot_is_the_one_the_sim_gives() {
    let c = cast();
    let s = c.s;
    for factory in [c.barracks, c.workshop] {
        let world = &s.world;
        let anchor = world.cell_of(factory).unwrap();
        let footprint = world.footprint_of(factory).unwrap();
        let kind = world
            .snapshot()
            .iter()
            .find(|v| v.id == factory)
            .unwrap()
            .kind;
        let side = world
            .building_table()
            .get(kind)
            .and_then(|b| b.exit_side)
            .unwrap();
        let (x, y) = sf_sim::exit_cell(anchor, footprint, side);
        assert_eq!(
            exit_cell_centre(&s.snap(), factory),
            Some((x as f32 + 0.5, y as f32 + 0.5)),
            "{factory}"
        );
    }
    // A power plant is no factory: no exit.
    assert_eq!(exit_cell_centre(&s.snap(), c.plant), None);
}

// ---- the sidebar follows the selection ---------------------------------------------------

#[test]
fn the_tab_follows_a_selection_that_changes_to_a_factory_and_only_then() {
    let c = cast();
    let s = c.s;
    let snap = s.snap();
    let mut follow = TabFollow::default();
    // Nothing selected, then nothing again: no change, no answer.
    assert_eq!(follow.update(&snap, &Selection::new(LOCAL_PLAYER)), None);
    // The barracks: the Infantry tab, once.
    let barracks = s.select(&[c.barracks]);
    assert_eq!(follow.update(&snap, &barracks), Some(Tab::Infantry));
    // The same selection on the next frames says nothing: a tab picked by hand stays.
    for _ in 0..100 {
        assert_eq!(follow.update(&snap, &barracks), None);
    }
    // The workshop: the Vehicles tab.
    let workshop = s.select(&[c.workshop]);
    assert_eq!(follow.update(&snap, &workshop), Some(Tab::Vehicles));
    // Both: the first in the stable order (the barracks was placed first, so it has the lower id).
    let both = s.select(&[c.workshop, c.barracks]);
    assert!(c.barracks < c.workshop);
    assert_eq!(follow.update(&snap, &both), Some(Tab::Infantry));
    // Units only, or a power plant: a change, but no factory in it, so no tab.
    assert_eq!(follow.update(&snap, &s.select(&[c.tank])), None);
    assert_eq!(follow.update(&snap, &s.select(&[c.plant])), None);
    assert_eq!(follow.update(&snap, &Selection::new(LOCAL_PLAYER)), None);
    // A mixed selection with a factory in it follows the factory.
    assert_eq!(
        follow.update(&snap, &s.select(&[c.tank, c.workshop])),
        Some(Tab::Vehicles)
    );
}

#[test]
fn an_enemy_factory_inspected_does_not_move_the_tab() {
    let c = cast();
    let s = c.s;
    let snap = s.snap();
    let mut follow = TabFollow::default();
    let mut looking = Selection::new(LOCAL_PLAYER);
    looking.click(&snap, Some(c.enemy_barracks), false);
    assert_eq!(looking.inspected(), Some(c.enemy_barracks));
    assert_eq!(follow.update(&snap, &looking), None);
}
