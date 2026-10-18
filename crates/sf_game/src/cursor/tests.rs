//! The cursor context without a window (WP3.5): every row of the decision
//! table in `docs/rules/cursor.md`, the modifiers, the terrain, the screen
//! edges, and that a right-click gives exactly the order the cursor shows.
//! Which entity is under the mouse on the screen is `sf_render`'s (tested
//! there for models and icons); here that answer is handed in as a [`Hover`].

use super::*;
use crate::commands::{self, LOCAL_PLAYER};
use crate::playfield;
use crate::sandbox::{MERIDIAN, NEUTRAL, VEYRAN};
use sf_data::units::UnitDefs;
use sf_sim::{Command, Kind, PlayerId, WorldConfig};

const W: u32 = 64;
const SIZE: u32 = 64;

/// Terrain type ids of `assets/config/terrain_types.ron`.
const GRASS: u8 = 0;
const WATER: u8 = 2;
const ROCK: u8 = 3;

/// A cell that is grass, one in the river (columns 20 to 29) and one in the
/// rock (columns 40 to 44).
const LAND: Cell = Cell::new(10, 10);
const RIVER: Cell = Cell::new(25, 10);
const CLIFF: Cell = Cell::new(42, 10);

struct Scene {
    world: World,
    defs: UnitDefs,
    passability: Passability,
}

impl Scene {
    fn new() -> Self {
        let defs = UnitDefs::embedded();
        let types = TerrainTypes::embedded();
        let mut ids = vec![GRASS; (W * SIZE) as usize];
        for y in 0..SIZE {
            for x in 0..W {
                let id = match x {
                    20..=29 => WATER,
                    40..=44 => ROCK,
                    _ => GRASS,
                };
                ids[(y * W + x) as usize] = id;
            }
        }
        let terrain = Terrain::from_layer(W, SIZE, &ids, &playfield::sim_definitions(&types))
            .expect("the layer is valid");
        let world = World::with_terrain(WorldConfig::new(7).with_map_size(W, SIZE), terrain)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let passability = Passability::new(defs.classes(), &types);
        Scene {
            world,
            defs,
            passability,
        }
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

    /// The cursor, with no modifier.
    fn cursor(&self, sel: &Selection, hover: &Hover) -> CursorKind {
        self.with(sel, hover, Modifiers::default())
    }

    fn with(&self, sel: &Selection, hover: &Hover, mods: Modifiers) -> CursorKind {
        cursor_context(&self.world, sel, hover, mods, &self.passability)
    }

    /// Selects `ids` as a drag would: own mobile units.
    fn drag(&self, ids: &[EntityId]) -> Selection {
        let mut sel = Selection::new(LOCAL_PLAYER);
        sel.drag(&self.world, ids, false);
        sel
    }

    /// Selects one entity as a click would (own structures too).
    fn click(&self, id: EntityId) -> Selection {
        let mut sel = Selection::new(LOCAL_PLAYER);
        sel.click(&self.world, Some(id), false);
        sel
    }
}

const SHIFT: Modifiers = Modifiers {
    queue: true,
    attack_move: false,
};
const FIGHT: Modifiers = Modifiers {
    queue: false,
    attack_move: true,
};

fn nobody() -> Selection {
    Selection::new(LOCAL_PLAYER)
}

#[test]
fn the_roles_the_tests_use_are_armed_or_not_as_assumed() {
    let mut s = Scene::new();
    let armed = |s: &Scene, id| s.world.stats_of(id).unwrap().weapon_range > sf_math::Fx::ZERO;
    for role in ["light_tank", "rifleman", "scout", "gunboat"] {
        let id = s.unit(role, MERIDIAN, 5, 5);
        assert!(armed(&s, id), "{role} has a weapon");
    }
    for role in ["engineer", "harvester", "medic", "amphibious_transport"] {
        let id = s.unit(role, MERIDIAN, 6, 6);
        assert!(!armed(&s, id), "{role} has none");
    }
    let tower = s.structure("defence_tower", MERIDIAN, 8, 30);
    assert!(armed(&s, tower), "the defence tower has a weapon");
}

// ---- nothing selected -----------------------------------------------------

#[test]
fn with_nothing_selected_the_cursor_selects_or_inspects() {
    let mut s = Scene::new();
    let own = s.unit("light_tank", MERIDIAN, 10, 10);
    let own_building = s.structure("power_plant", MERIDIAN, 5, 30);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let enemy_building = s.structure("barracks", VEYRAN, 50, 30);
    let neutral = s.unit("civilian", NEUTRAL, 50, 12);
    let sel = nobody();
    let cases = [
        (Hover::ground(LAND), CursorKind::Select),
        (Hover::over(own, LAND), CursorKind::Select),
        (
            Hover::over(own_building, Cell::new(5, 30)),
            CursorKind::Select,
        ),
        (Hover::over(enemy, Cell::new(50, 10)), CursorKind::Inspect),
        (
            Hover::over(enemy_building, Cell::new(50, 30)),
            CursorKind::Inspect,
        ),
        (Hover::over(neutral, Cell::new(50, 12)), CursorKind::Inspect),
        // Even water and the sky: there is nothing to order.
        (Hover::ground(RIVER), CursorKind::Select),
        (Hover::default(), CursorKind::Select),
    ];
    for (hover, expected) in cases {
        assert_eq!(s.cursor(&sel, &hover), expected, "{hover:?}");
        // The modifiers mean nothing without units to order.
        for mods in [SHIFT, FIGHT] {
            assert_eq!(s.with(&sel, &hover, mods), expected, "{hover:?} {mods:?}");
        }
    }
}

#[test]
fn a_stale_hovered_id_counts_as_bare_ground() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 10, 10);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    s.world
        .schedule(s.world.tick(), LOCAL_PLAYER, Command::Despawn { id: enemy })
        .unwrap();
    s.world.step();
    let sel = s.drag(&[tank]);
    assert_eq!(
        s.cursor(&sel, &Hover::over(enemy, LAND)),
        CursorKind::Move,
        "a gone enemy is no target"
    );
    assert_eq!(
        s.cursor(&nobody(), &Hover::over(enemy, LAND)),
        CursorKind::Select
    );
}

// ---- own mobile units selected: ground ------------------------------------

#[test]
fn over_open_ground_the_cursor_is_move_queue_or_attack_move() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let sel = s.drag(&[tank]);
    let ground = Hover::ground(LAND);
    assert_eq!(s.cursor(&sel, &ground), CursorKind::Move);
    assert_eq!(s.with(&sel, &ground, SHIFT), CursorKind::Queue);
    assert_eq!(s.with(&sel, &ground, FIGHT), CursorKind::AttackMove);
    // Both held: the attack-move is the more specific order (and goes to the
    // queue: `is_queued`).
    let both = Modifiers {
        queue: true,
        attack_move: true,
    };
    assert_eq!(s.with(&sel, &ground, both), CursorKind::AttackMove);
    assert!(is_queued(CursorKind::AttackMove, both));
    // No point under the mouse (the sky): nothing to order.
    assert_eq!(s.cursor(&sel, &Hover::default()), CursorKind::Select);
}

#[test]
fn attack_move_needs_a_weapon_in_the_selection() {
    let mut s = Scene::new();
    let harvester = s.unit("harvester", MERIDIAN, 12, 10);
    let tank = s.unit("light_tank", MERIDIAN, 14, 10);
    let ground = Hover::ground(LAND);
    // Unarmed units would only walk: the cursor says move, and the order is a move.
    let unarmed = s.drag(&[harvester]);
    assert_eq!(s.with(&unarmed, &ground, FIGHT), CursorKind::Move);
    let command = commands::right_click(&s.world, &unarmed, &ground, FIGHT, &s.passability);
    assert!(matches!(
        command,
        Some(Command::Order {
            order: Order::Move(_),
            ..
        })
    ));
    // One armed unit in the group is enough.
    let mixed = s.drag(&[harvester, tank]);
    assert_eq!(s.with(&mixed, &ground, FIGHT), CursorKind::AttackMove);
}

// ---- enemies, neutrals and friends -----------------------------------------

#[test]
fn over_an_enemy_an_armed_selection_attacks() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let harvester = s.unit("harvester", MERIDIAN, 14, 10);
    let enemy_unit = s.unit("rifleman", VEYRAN, 50, 10);
    let enemy_building = s.structure("barracks", VEYRAN, 50, 30);
    let at_unit = Hover::over(enemy_unit, Cell::new(50, 10));
    let at_building = Hover::over(enemy_building, Cell::new(50, 30));
    let armed = s.drag(&[tank]);
    let mixed = s.drag(&[tank, harvester]);
    let unarmed = s.drag(&[harvester]);
    for hover in [at_unit, at_building] {
        assert_eq!(s.cursor(&armed, &hover), CursorKind::Attack);
        assert_eq!(
            s.cursor(&mixed, &hover),
            CursorKind::Attack,
            "one weapon is enough"
        );
        // The modifiers do not turn it into something else.
        assert_eq!(s.with(&armed, &hover, FIGHT), CursorKind::Attack);
        assert_eq!(s.with(&armed, &hover, SHIFT), CursorKind::Attack);
        // Nothing that can shoot: the enemy is only a place on the ground.
        assert_eq!(s.cursor(&unarmed, &hover), CursorKind::Move);
    }
}

#[test]
fn an_enemy_in_the_water_is_still_attacked() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let boat = s.unit("gunboat", VEYRAN, 25, 10);
    let sel = s.drag(&[tank]);
    assert_eq!(
        s.cursor(&sel, &Hover::over(boat, RIVER)),
        CursorKind::Attack
    );
}

#[test]
fn a_neutral_thing_is_ground_to_an_order_and_never_attacked() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let civilian = s.unit("civilian", NEUTRAL, 15, 12);
    let depot = s.structure("fuel_depot", NEUTRAL, 16, 30);
    let sel = s.drag(&[tank]);
    for hover in [
        Hover::over(civilian, Cell::new(15, 12)),
        Hover::over(depot, Cell::new(16, 30)),
    ] {
        assert_eq!(s.cursor(&sel, &hover), CursorKind::Move);
    }
}

#[test]
fn over_one_of_ours_the_cursor_guards_without_a_modifier() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let rifle = s.unit("rifleman", MERIDIAN, 14, 10);
    let plant = s.structure("power_plant", MERIDIAN, 5, 30);
    let sel = s.drag(&[tank]);
    let at_rifle = Hover::over(rifle, Cell::new(14, 10));
    let at_plant = Hover::over(plant, Cell::new(5, 30));
    assert_eq!(s.cursor(&sel, &at_rifle), CursorKind::Guard);
    assert_eq!(
        s.cursor(&sel, &at_plant),
        CursorKind::Guard,
        "a building too"
    );
    // The queue modifier queues the guard; attack-move has nothing to do with it.
    assert_eq!(s.with(&sel, &at_rifle, SHIFT), CursorKind::Guard);
    assert!(is_queued(CursorKind::Guard, SHIFT));
    assert_eq!(s.with(&sel, &at_rifle, FIGHT), CursorKind::Guard);
}

#[test]
fn a_unit_cannot_guard_itself_so_the_only_selected_unit_is_ground() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let rifle = s.unit("rifleman", MERIDIAN, 14, 10);
    let at_tank = Hover::over(tank, Cell::new(12, 10));
    // Alone: a move to the spot under it.
    assert_eq!(s.cursor(&s.drag(&[tank]), &at_tank), CursorKind::Move);
    // Together with another: the other one guards it.
    assert_eq!(
        s.cursor(&s.drag(&[tank, rifle]), &at_tank),
        CursorKind::Guard
    );
}

// ---- terrain ----------------------------------------------------------------

#[test]
fn ground_no_selected_unit_can_enter_is_no_go() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let rifle = s.unit("rifleman", MERIDIAN, 14, 10);
    let sel = s.drag(&[tank, rifle]);
    assert_eq!(s.cursor(&sel, &Hover::ground(LAND)), CursorKind::Move);
    for cell in [RIVER, CLIFF] {
        let hover = Hover::ground(cell);
        assert_eq!(s.cursor(&sel, &hover), CursorKind::NoGo, "{cell}");
        // The modifiers cannot make water walkable.
        assert_eq!(s.with(&sel, &hover, SHIFT), CursorKind::NoGo);
        assert_eq!(s.with(&sel, &hover, FIGHT), CursorKind::NoGo);
    }
    // Off the map: nobody can go there either.
    let off = Hover {
        point: Some(sf_math::Vec2::from_ints(-3, 10)),
        ..Hover::default()
    };
    assert_eq!(s.cursor(&sel, &off), CursorKind::NoGo);
    let far = Hover {
        point: Some(sf_math::Vec2::from_ints(100, 10)),
        ..Hover::default()
    };
    assert_eq!(s.cursor(&sel, &far), CursorKind::NoGo);
}

#[test]
fn every_movement_class_reads_its_own_flag() {
    let mut s = Scene::new();
    let foot = s.unit("rifleman", MERIDIAN, 5, 5);
    let tracked = s.unit("light_tank", MERIDIAN, 6, 5);
    let hover = s.unit("amphibious_transport", MERIDIAN, 7, 5);
    let vessel = s.unit("gunboat", MERIDIAN, 8, 5);
    let (land, river, cliff) = (
        Hover::ground(LAND),
        Hover::ground(RIVER),
        Hover::ground(CLIFF),
    );
    for (who, on_land, on_river, on_cliff) in [
        (foot, true, false, false),
        (tracked, true, false, false),
        // Land and water, but not rock.
        (hover, true, true, false),
        // Water only.
        (vessel, false, true, false),
    ] {
        let sel = s.drag(&[who]);
        for (hover, can) in [(land, on_land), (river, on_river), (cliff, on_cliff)] {
            let expected = if can {
                CursorKind::Move
            } else {
                CursorKind::NoGo
            };
            assert_eq!(s.cursor(&sel, &hover), expected, "{who} on {hover:?}");
        }
    }
    // One unit that can go is enough: the order goes to all and the sim
    // sorts out who arrives (path finding is phase 4).
    let mixed = s.drag(&[foot, hover]);
    assert_eq!(s.cursor(&mixed, &river), CursorKind::Move);
}

#[test]
fn a_class_the_game_does_not_know_may_go_anywhere_and_a_bridge_is_land() {
    let s = Scene::new();
    let terrain = s.world.terrain();
    // An unknown class and no class at all: never blocked by flags.
    assert!(s.passability.can_enter(terrain, RIVER, Some(999)));
    assert!(s.passability.can_enter(terrain, RIVER, None));
    // A cell the map does not have: nobody.
    assert!(!s.passability.can_enter(terrain, Cell::new(64, 0), None));
    // The names of the data decide: foot, wheeled and tracked read their flags.
    let classes = s.defs.classes();
    let kind = |name: &str| classes.movement_kind(name).unwrap();
    for name in ["foot", "wheeled", "tracked"] {
        assert!(
            s.passability.can_enter(terrain, LAND, Some(kind(name))),
            "{name}"
        );
        assert!(
            !s.passability.can_enter(terrain, RIVER, Some(kind(name))),
            "{name}"
        );
    }
    assert_eq!(s.passability.mobility(kind("hover")), Some(Mobility::Hover));
    assert_eq!(s.passability.mobility(kind("air")), Some(Mobility::Air));
    assert!(s.passability.can_enter(terrain, CLIFF, Some(kind("air"))));
}

// ---- structures --------------------------------------------------------------

#[test]
fn selected_structures_take_no_orders_so_the_cursor_only_selects_and_inspects() {
    let mut s = Scene::new();
    let plant = s.structure("power_plant", MERIDIAN, 5, 30);
    let tower = s.structure("defence_tower", MERIDIAN, 8, 30);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let own = s.unit("light_tank", MERIDIAN, 12, 10);
    for sel in [s.click(plant), s.click(tower)] {
        assert_eq!(sel.len(), 1);
        assert!(sel.orderable(&s.world).is_empty());
        // Over the own structure (even the one selected): select.
        assert_eq!(
            s.cursor(&sel, &Hover::over(plant, Cell::new(5, 30))),
            CursorKind::Select
        );
        assert_eq!(s.cursor(&sel, &Hover::over(own, LAND)), CursorKind::Select);
        // Over ground: no move, structures do not walk.
        assert_eq!(s.cursor(&sel, &Hover::ground(LAND)), CursorKind::Select);
        // An enemy: look at it. The tower's weapon does not make it an
        // attack: structures are never given orders (`Selection::orderable`).
        assert_eq!(
            s.cursor(&sel, &Hover::over(enemy, Cell::new(50, 10))),
            CursorKind::Inspect
        );
    }
}

#[test]
fn structures_in_a_mixed_selection_are_ignored_by_the_cursor() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let plant = s.structure("power_plant", MERIDIAN, 5, 30);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let mut sel = s.click(plant);
    sel.click(&s.world, Some(tank), true);
    assert_eq!(sel.len(), 2);
    assert_eq!(s.cursor(&sel, &Hover::ground(LAND)), CursorKind::Move);
    assert_eq!(s.cursor(&sel, &Hover::ground(RIVER)), CursorKind::NoGo);
    assert_eq!(
        s.cursor(&sel, &Hover::over(enemy, Cell::new(50, 10))),
        CursorKind::Attack
    );
    // The structure is not "another unit" for a guard of the tank.
    assert_eq!(
        s.cursor(&sel, &Hover::over(tank, Cell::new(12, 10))),
        CursorKind::Move
    );
    // An inspected enemy does not count as a selected unit.
    let mut looking = Selection::new(LOCAL_PLAYER);
    looking.click(&s.world, Some(enemy), false);
    assert_eq!(looking.inspected(), Some(enemy));
    assert_eq!(s.cursor(&looking, &Hover::ground(LAND)), CursorKind::Select);
}

// ---- the screen edges ----------------------------------------------------------

#[test]
fn the_screen_edges_give_the_eight_scroll_cursors_and_win_over_everything() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let edge = |left, right, top, bottom| Edges {
        left,
        right,
        top,
        bottom,
    };
    let table = [
        (edge(false, false, true, false), CursorKind::ScrollN),
        (edge(false, true, true, false), CursorKind::ScrollNe),
        (edge(false, true, false, false), CursorKind::ScrollE),
        (edge(false, true, false, true), CursorKind::ScrollSe),
        (edge(false, false, false, true), CursorKind::ScrollS),
        (edge(true, false, false, true), CursorKind::ScrollSw),
        (edge(true, false, false, false), CursorKind::ScrollW),
        (edge(true, false, true, false), CursorKind::ScrollNw),
    ];
    assert_eq!(table.len(), 8);
    let selections = [nobody(), s.drag(&[tank])];
    for (edges, expected) in table {
        assert!(expected.is_scroll());
        for sel in &selections {
            for hover in [
                Hover::default(),
                Hover::ground(LAND),
                Hover::over(enemy, Cell::new(50, 10)),
                Hover::ground(RIVER),
            ] {
                let hover = Hover { edges, ..hover };
                assert_eq!(s.cursor(sel, &hover), expected, "{hover:?}");
                assert_eq!(s.with(sel, &hover, SHIFT), expected);
                assert_eq!(order_for(expected, &hover), None, "a scroll orders nothing");
            }
        }
    }
    // No edge, and opposite edges at once (a window as narrow as two bands),
    // are not a scroll.
    assert_eq!(scroll_cursor(Edges::default()), None);
    assert_eq!(scroll_cursor(edge(true, true, false, false)), None);
    assert_eq!(scroll_cursor(edge(false, false, true, true)), None);
    // They cancel on one axis only.
    assert_eq!(
        scroll_cursor(edge(true, true, true, false)),
        Some(CursorKind::ScrollN)
    );
}

// ---- placeholders and the table of kinds -------------------------------------------

#[test]
fn the_kinds_are_numbered_named_and_classified_consistently() {
    let mut names: Vec<&str> = CursorKind::ALL.iter().map(|k| k.name()).collect();
    for (i, kind) in CursorKind::ALL.into_iter().enumerate() {
        assert_eq!(kind.index(), i, "{kind:?} is out of place in ALL");
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), CursorKind::ALL.len(), "a name twice");
    assert_eq!(CursorKind::ALL.iter().filter(|k| k.is_scroll()).count(), 8);
    assert_eq!(
        CursorKind::ALL
            .iter()
            .filter(|k| k.is_placeholder())
            .count(),
        8
    );
    assert!(
        CursorKind::ALL
            .iter()
            .all(|k| !(k.is_scroll() && k.is_placeholder()))
    );
}

#[test]
fn placeholders_never_come_out_of_the_context_and_never_order() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let harvester = s.unit("harvester", MERIDIAN, 13, 10);
    let rifle = s.unit("rifleman", MERIDIAN, 14, 10);
    let plant = s.structure("power_plant", MERIDIAN, 5, 30);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let enemy_building = s.structure("barracks", VEYRAN, 50, 30);
    let civilian = s.unit("civilian", NEUTRAL, 50, 12);
    let selections = [
        nobody(),
        s.drag(&[tank]),
        s.drag(&[harvester]),
        s.drag(&[tank, rifle, harvester]),
        s.click(plant),
        s.click(tank),
    ];
    let hovers = [
        Hover::default(),
        Hover::ground(LAND),
        Hover::ground(RIVER),
        Hover::ground(CLIFF),
        Hover::over(tank, LAND),
        Hover::over(rifle, LAND),
        Hover::over(plant, Cell::new(5, 30)),
        Hover::over(enemy, Cell::new(50, 10)),
        Hover::over(enemy_building, Cell::new(50, 30)),
        Hover::over(civilian, Cell::new(50, 12)),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for sel in &selections {
        for hover in &hovers {
            for queue in [false, true] {
                for attack_move in [false, true] {
                    let mods = Modifiers { queue, attack_move };
                    let kind = s.with(sel, hover, mods);
                    assert!(!kind.is_placeholder(), "{kind:?} for {hover:?} {mods:?}");
                    seen.insert(kind.index());
                }
            }
        }
    }
    // The sweep reaches every real cursor but the scrolls (those are the
    // edges', tested above).
    for kind in CursorKind::ALL {
        if !kind.is_scroll() && !kind.is_placeholder() {
            assert!(seen.contains(&kind.index()), "{kind:?} is never the answer");
        }
    }
    // And the placeholders give no order whatever is under the mouse.
    for kind in CursorKind::ALL.into_iter().filter(|k| k.is_placeholder()) {
        for hover in &hovers {
            assert_eq!(order_for(kind, hover), None, "{kind:?}");
            assert!(!is_queued(kind, SHIFT), "{kind:?}");
        }
    }
}

// ---- one source of truth ------------------------------------------------------------

/// What the order of a right-click is, in words, for the table below.
fn what(command: &Option<Command>) -> String {
    match command {
        None => "nothing".to_owned(),
        Some(Command::Order { order, queued, .. }) => {
            format!("{order:?}{}", if *queued { " queued" } else { "" })
        }
        Some(other) => format!("{other:?}"),
    }
}

#[test]
fn a_right_click_gives_exactly_the_order_the_cursor_shows() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let harvester = s.unit("harvester", MERIDIAN, 13, 10);
    let rifle = s.unit("rifleman", MERIDIAN, 14, 10);
    let plant = s.structure("power_plant", MERIDIAN, 5, 30);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let civilian = s.unit("civilian", NEUTRAL, 50, 12);
    let selections = [
        nobody(),
        s.drag(&[tank]),
        s.drag(&[harvester]),
        s.drag(&[tank, rifle, harvester]),
        s.click(plant),
    ];
    let hovers = [
        Hover::default(),
        Hover::ground(LAND),
        Hover::ground(RIVER),
        Hover::over(tank, LAND),
        Hover::over(rifle, LAND),
        Hover::over(plant, Cell::new(5, 30)),
        Hover::over(enemy, Cell::new(50, 10)),
        Hover::over(civilian, Cell::new(50, 12)),
        Hover {
            edges: Edges {
                left: true,
                ..Edges::default()
            },
            ..Hover::ground(LAND)
        },
    ];
    let mut orders = 0;
    for sel in &selections {
        for hover in &hovers {
            for queue in [false, true] {
                for attack_move in [false, true] {
                    let mods = Modifiers { queue, attack_move };
                    let kind = s.with(sel, hover, mods);
                    let command = commands::right_click(&s.world, sel, hover, mods, &s.passability);
                    let expected = order_for(kind, hover);
                    let ctx = format!("{kind:?} for {hover:?} {mods:?}: {}", what(&command));
                    match (&command, expected) {
                        (None, None) => {}
                        // No units to give it to: no command, whatever the cursor.
                        (None, Some(_)) => assert!(sel.orderable(&s.world).is_empty(), "{ctx}"),
                        (Some(Command::Order { ids, order, queued }), Some(want)) => {
                            assert_eq!(*order, want, "{ctx}");
                            assert_eq!(*queued, is_queued(kind, mods), "{ctx}");
                            // The order goes to the units that can take it, all of them.
                            assert_eq!(*ids, sel.orderable(&s.world), "{ctx}");
                            orders += 1;
                        }
                        (Some(_), _) => panic!("a click that is not the cursor's: {ctx}"),
                    }
                }
            }
        }
    }
    assert!(orders > 50, "the sweep reached the orders: {orders}");
}

#[test]
fn each_cursor_gives_its_order_and_the_sim_carries_it_out() {
    let mut s = Scene::new();
    let tank = s.unit("light_tank", MERIDIAN, 12, 10);
    let rifle = s.unit("rifleman", MERIDIAN, 14, 10);
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let sel = s.drag(&[tank]);
    let ground = Hover::ground(Cell::new(15, 20));
    let at_enemy = Hover::over(enemy, Cell::new(50, 10));
    let at_rifle = Hover::over(rifle, Cell::new(14, 10));
    let point = ground.point.unwrap();
    let go = |s: &mut Scene, hover: &Hover, mods: Modifiers| {
        let command = commands::right_click(&s.world, &sel, hover, mods, &s.passability)
            .expect("the cursor promised an order");
        commands::issue(&mut s.world, command);
        s.world.step();
    };
    go(&mut s, &ground, Modifiers::default());
    assert_eq!(s.world.order_of(tank), Some(Order::Move(point)));
    go(&mut s, &ground, FIGHT);
    assert_eq!(s.world.order_of(tank), Some(Order::AttackMove(point)));
    go(&mut s, &at_enemy, Modifiers::default());
    assert_eq!(s.world.order_of(tank), Some(Order::Attack(enemy)));
    go(&mut s, &at_rifle, Modifiers::default());
    assert_eq!(
        s.world.order_of(tank),
        Some(Order::Guard(GuardTarget::Entity(rifle)))
    );
    // Shift queues behind the current order.
    go(&mut s, &ground, SHIFT);
    assert_eq!(
        s.world.order_of(tank),
        Some(Order::Guard(GuardTarget::Entity(rifle)))
    );
    assert_eq!(
        s.world.queued_orders_of(tank),
        Some(vec![Order::Move(point)])
    );
    // A no-go and a select give nothing, and the world has nothing to run.
    let before = s.world.pending_commands().len();
    for hover in [Hover::ground(RIVER), Hover::default()] {
        assert!(
            commands::right_click(&s.world, &sel, &hover, Modifiers::default(), &s.passability)
                .is_none()
        );
    }
    assert_eq!(s.world.pending_commands().len(), before);
}

#[test]
fn nothing_selected_means_a_right_click_does_nothing_even_over_an_enemy() {
    let mut s = Scene::new();
    let enemy = s.unit("rifleman", VEYRAN, 50, 10);
    let hover = Hover::over(enemy, Cell::new(50, 10));
    assert_eq!(s.cursor(&nobody(), &hover), CursorKind::Inspect);
    assert!(
        commands::right_click(
            &s.world,
            &nobody(),
            &hover,
            Modifiers::default(),
            &s.passability
        )
        .is_none()
    );
}

// ---- the pictures ----------------------------------------------------------------------

#[test]
fn every_kind_has_a_picture_in_the_shipped_set() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/ui/cursors");
    let set = CursorSet::load(&dir, &mut sf_ui::Atlas::default()).expect("the shipped set loads");
    let art = CursorArt::new(set).expect("every kind has a picture");
    let mut used: Vec<usize> = CursorKind::ALL.iter().map(|&k| art.sprite_of(k)).collect();
    used.sort_unstable();
    used.dedup();
    assert_eq!(
        used.len(),
        CursorKind::ALL.len(),
        "two kinds share a picture"
    );
    assert_eq!(
        art.set().len(),
        CursorKind::ALL.len(),
        "a picture no kind uses"
    );
}

#[test]
fn a_set_that_lacks_a_cursor_is_refused_by_name() {
    let dir = std::env::temp_dir().join(format!("sf_game_cursor_lack_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    sf_render::image::write_png(
        &dir.join("select.png"),
        &sf_render::Rgba8::new(8, 8, vec![255; 8 * 8 * 4]),
    )
    .unwrap();
    std::fs::write(
        dir.join("cursors.ron"),
        r#"(size_px: 16, cursors: {"select": (file: "select.png", hotspot: (0, 0))})"#,
    )
    .unwrap();
    let set = CursorSet::load(&dir, &mut sf_ui::Atlas::default()).unwrap();
    let err = CursorArt::new(set).err().expect("refused");
    assert!(
        err.contains("attack_move") && err.contains("scroll_n"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
