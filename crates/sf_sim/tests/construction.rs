//! Construction (WP6.3, `docs/rules/construction.md`): a structure built with `BuildStructure` is under
//! construction for its `build_ticks` and counts for nothing until it is complete. Hand-made tables, so the
//! rules are tested apart from the shipped data (the bridge tests hold the shipped numbers against them).
//!
//! Map 40 x 30, open ground. Player 0 is faction bit 1 with its anchor on (10, 10) (2 x 2, build radius 4).

use sf_math::Fx;
use sf_sim::{
    BuildingFunction, BuildingStats, BuildingTable, Cell, Command, Construction, EntityId,
    Footprint, Kind, PlacementError, PlayerId, StepReport, UnitStats, UnitTable, World,
    WorldConfig,
};

const ANCHOR: Kind = Kind(1); // 2 x 2, radius 4, cannot be built
const PLANT: Kind = Kind(2); // 2 x 2, 40 ticks
const TOWER: Kind = Kind(3); // 1 x 1, 25 ticks, needs a plant
const OUTPOST: Kind = Kind(4); // 2 x 2, an anchor that can be built: 30 ticks, radius 2
const DECOY: Kind = Kind(5); // 1 x 1, 8 ticks

const BUILD_TICKS: [(Kind, u32); 4] = [(PLANT, 40), (TOWER, 25), (OUTPOST, 30), (DECOY, 8)];

fn unit(kind: Kind, w: u8, h: u8) -> UnitStats {
    UnitStats {
        kind,
        hp: 500,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: w,
            height: h,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn entry(
    kind: Kind,
    (w, h): (u8, u8),
    function: BuildingFunction,
    requires: &[Kind],
    radius: u16,
) -> BuildingStats {
    let ticks = BUILD_TICKS
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or(0, |&(_, t)| t);
    BuildingStats {
        kind,
        footprint: Footprint {
            width: w,
            height: h,
        },
        function,
        exit_side: None,
        build_ticks: ticks,
        cost: (ticks > 0).then_some(100),
        power: 0,
        factions: 1,
        requires: requires.to_vec(),
        build_radius: radius,
    }
}

fn world() -> World {
    World::new(WorldConfig::new(3).with_map_size(40, 30))
        .unwrap()
        .with_units(
            UnitTable::new(vec![
                unit(ANCHOR, 2, 2),
                unit(PLANT, 2, 2),
                unit(TOWER, 1, 1),
                unit(OUTPOST, 2, 2),
                unit(DECOY, 1, 1),
            ])
            .unwrap(),
        )
        .unwrap()
        .with_buildings(
            BuildingTable::new(vec![
                entry(ANCHOR, (2, 2), BuildingFunction::Anchor, &[], 4),
                entry(PLANT, (2, 2), BuildingFunction::PowerPlant, &[], 0),
                entry(TOWER, (1, 1), BuildingFunction::Defence, &[PLANT], 0),
                entry(OUTPOST, (2, 2), BuildingFunction::Anchor, &[], 2),
                entry(DECOY, (1, 1), BuildingFunction::Decoy, &[], 0),
            ])
            .unwrap(),
        )
        .unwrap()
        .with_player_factions(&[(PlayerId(0), 1)])
        .unwrap()
}

fn send(w: &mut World, command: Command) -> StepReport {
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), command).unwrap();
    w.step()
}

/// A world with the base placed the way a scenario places it (complete from the start).
fn rig() -> World {
    let mut w = world();
    send(
        &mut w,
        Command::PlaceStructure {
            kind: ANCHOR,
            cell: Cell::new(10, 10),
            owner: PlayerId(0),
        },
    );
    w
}

fn build(w: &mut World, kind: Kind, x: u32, y: u32) -> StepReport {
    send(
        w,
        Command::BuildStructure {
            kind,
            cell: Cell::new(x, y),
        },
    )
}

fn at(w: &World, x: u32, y: u32) -> EntityId {
    w.occupant_at(Cell::new(x, y)).expect("a structure there")
}

fn steps(w: &mut World, n: u32) {
    for _ in 0..n {
        w.step();
    }
}

#[test]
fn a_built_structure_is_complete_after_exactly_its_build_ticks() {
    let mut w = rig();
    let placed = build(&mut w, PLANT, 14, 10);
    assert_eq!(placed.commands_executed, 1);
    let placed_in = placed.tick;
    let plant = at(&w, 14, 10);
    // Placed: all of its ticks are still to go.
    assert_eq!(
        w.construction_of(plant),
        Some(Construction {
            left: 40,
            total: 40
        })
    );
    assert!(!w.stands(plant));
    assert_eq!(w.under_construction(), 1);
    assert_eq!(placed.structures_completed, 0);
    // 39 more steps: one tick to go, still not complete, no event.
    for n in 1..40 {
        let r = w.step();
        assert_eq!(r.structures_completed, 0, "step {n}");
        assert_eq!(w.construction_of(plant).unwrap().left, 40 - n);
        assert!(!w.stands(plant));
    }
    assert_eq!(w.construction_of(plant).unwrap().left, 1);
    // The 40th step completes it: the state and the event are in the same step's report.
    let done = w.step();
    assert_eq!(done.structures_completed, 1);
    assert_eq!(done.tick.0, placed_in.0 + 40);
    assert_eq!(w.construction_of(plant), None);
    assert!(w.stands(plant));
    assert_eq!(w.under_construction(), 0);
    let events = w.completions();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (
            events[0].tick,
            events[0].id,
            events[0].kind,
            events[0].owner
        ),
        (done.tick, plant, PLANT, PlayerId(0))
    );
    // The list is for one step only, and a complete structure never fires again.
    let next = w.step();
    assert_eq!(next.structures_completed, 0);
    assert!(w.completions().is_empty());
}

#[test]
fn a_pre_placed_structure_starts_complete() {
    let mut w = rig();
    let anchor = at(&w, 10, 10);
    assert_eq!(w.construction_of(anchor), None);
    assert!(w.stands(anchor));
    assert_eq!(w.under_construction(), 0);
    assert!(w.completions().is_empty());
    // Also one of a kind that can be built: the mission's PlaceStructure is not a build.
    let r = send(
        &mut w,
        Command::PlaceStructure {
            kind: PLANT,
            cell: Cell::new(20, 20),
            owner: PlayerId(0),
        },
    );
    let plant = at(&w, 20, 20);
    assert_eq!(w.construction_of(plant), None);
    assert!(w.stands(plant));
    assert_eq!(r.structures_completed, 0);
}

#[test]
fn a_structure_under_construction_is_no_prerequisite_until_it_completes() {
    let mut w = rig();
    let first = build(&mut w, PLANT, 14, 10);
    let plant = at(&w, 14, 10);
    let done_at = first.tick.0 + 40;
    // The tower needs a plant standing.
    assert_eq!(
        w.check_placement(TOWER, Cell::new(14, 13), PlayerId(0)),
        Err(PlacementError::MissingPrerequisite(PLANT))
    );
    let r = build(&mut w, TOWER, 14, 13);
    assert_eq!(r.placements_rejected, 1);
    assert_eq!(
        w.placement_rejections()[0].reason,
        PlacementError::MissingPrerequisite(PLANT)
    );
    // Right up to the tick before it completes (steps up to `done_at - 1` have run).
    while w.tick().0 < done_at {
        w.step();
    }
    assert!(!w.stands(plant));
    assert_eq!(
        w.check_placement(TOWER, Cell::new(14, 13), PlayerId(0)),
        Err(PlacementError::MissingPrerequisite(PLANT))
    );
    // The step of `done_at` completes it, and from then on the tower may be built.
    let r = w.step();
    assert_eq!((r.tick.0, r.structures_completed), (done_at, 1));
    assert!(w.stands(plant));
    assert_eq!(
        w.check_placement(TOWER, Cell::new(14, 13), PlayerId(0)),
        Ok(())
    );
    assert_eq!(build(&mut w, TOWER, 14, 13).commands_executed, 1);
}

#[test]
fn the_command_of_the_completing_tick_sees_the_structure_standing() {
    let mut w = rig();
    let first = build(&mut w, PLANT, 14, 10);
    let completes_in = first.tick.0 + 40;
    // Run up to the tick before, then schedule the tower for the tick that completes the plant.
    while w.tick().0 < completes_in {
        w.step();
    }
    assert_eq!(w.tick().0, completes_in);
    let r = send(
        &mut w,
        Command::BuildStructure {
            kind: TOWER,
            cell: Cell::new(14, 13),
        },
    );
    assert_eq!(r.tick.0, completes_in);
    assert_eq!(
        r.structures_completed, 1,
        "the plant completed in this step"
    );
    assert_eq!(r.commands_executed, 1, "and the tower was accepted in it");
    assert_eq!(r.placements_rejected, 0);
}

#[test]
fn a_base_under_construction_gives_no_reach() {
    let mut w = rig();
    // The outpost is an anchor you can build, 4 cells inside the base's reach (gap 4 from (10..12)).
    let first = build(&mut w, OUTPOST, 16, 10);
    assert_eq!(first.commands_executed, 1);
    let outpost = at(&w, 16, 10);
    // x 20 is a gap of 2 from the outpost (its edge is x 18), inside the outpost's radius of 2, and
    // a gap of 8 from the base, outside the base's radius of 4.
    let beyond = Cell::new(20, 10);
    assert_eq!(
        w.check_placement(DECOY, beyond, PlayerId(0)),
        Err(PlacementError::TooFarFromBase)
    );
    steps(&mut w, 29);
    assert!(!w.stands(outpost));
    assert_eq!(
        w.check_placement(DECOY, beyond, PlayerId(0)),
        Err(PlacementError::TooFarFromBase)
    );
    w.step();
    assert!(w.stands(outpost));
    assert_eq!(w.check_placement(DECOY, beyond, PlayerId(0)), Ok(()));
}

#[test]
fn it_takes_its_cells_from_the_first_tick() {
    let mut w = rig();
    build(&mut w, PLANT, 14, 10);
    // Another build on the same cells is refused at once, and the cells hold the structure.
    assert_eq!(
        w.check_placement(DECOY, Cell::new(15, 11), PlayerId(0)),
        Err(PlacementError::Occupied)
    );
    assert_eq!(w.occupied_cells(), 4 + 4);
    // Completing changes nothing about the cells.
    steps(&mut w, 40);
    assert_eq!(w.occupied_cells(), 4 + 4);
}

#[test]
fn destroying_a_structure_under_construction_drops_its_state() {
    let mut w = rig();
    build(&mut w, PLANT, 14, 10);
    let plant = at(&w, 14, 10);
    assert_eq!(w.under_construction(), 1);
    send(&mut w, Command::Despawn { id: plant });
    assert_eq!(w.under_construction(), 0);
    assert_eq!(w.construction_of(plant), None, "a stale id has no state");
    assert!(!w.stands(plant));
    // The slot is reused by a structure placed complete: it does not inherit the old progress.
    send(
        &mut w,
        Command::PlaceStructure {
            kind: DECOY,
            cell: Cell::new(30, 20),
            owner: PlayerId(0),
        },
    );
    let decoy = at(&w, 30, 20);
    assert_eq!(decoy.index(), plant.index(), "the slot was reused");
    assert_eq!(w.construction_of(decoy), None);
    assert!(w.stands(decoy));
    // Nothing fires for the one that was destroyed.
    let total: u32 = (0..60).map(|_| w.step().structures_completed).sum();
    assert_eq!(total, 0);
}

#[test]
fn construction_is_in_the_checksum_and_is_gone_when_it_completes() {
    // The same structure, built (A) or placed complete (B), from the same state.
    let mut a = rig();
    let mut b = rig();
    build(&mut a, DECOY, 14, 10);
    send(
        &mut b,
        Command::PlaceStructure {
            kind: DECOY,
            cell: Cell::new(14, 10),
            owner: PlayerId(0),
        },
    );
    assert_eq!(a.tick(), b.tick());
    // Under construction the state differs, nothing else does.
    assert_ne!(a.checksum(), b.checksum());
    // One tick later still different (7 ticks to go against none)...
    a.step();
    b.step();
    assert_ne!(a.checksum(), b.checksum());
    // ...and different progress is different state: a second world one tick behind in its build.
    let mut c = rig();
    c.step();
    build(&mut c, DECOY, 14, 10);
    let mut d = rig();
    build(&mut d, DECOY, 14, 10);
    d.step();
    assert_eq!(c.tick(), d.tick());
    assert_ne!(
        c.checksum(),
        d.checksum(),
        "the ticks still to go are hashed"
    );
    // Once complete, the section is gone: the world is the one that was placed complete.
    steps(&mut a, 8);
    steps(&mut b, 8);
    assert_eq!(a.under_construction(), 0);
    assert_eq!(a.tick(), b.tick());
    assert_eq!(a.checksum(), b.checksum());
}

#[test]
fn constructions_are_listed_by_slot_with_their_progress() {
    let mut w = rig();
    build(&mut w, PLANT, 14, 10);
    steps(&mut w, 9);
    build(&mut w, DECOY, 14, 14);
    let list: Vec<(u32, u32, u32, u16)> = w
        .constructions()
        .map(|(id, c)| (id.index(), c.left, c.total, c.permille()))
        .collect();
    // The plant was placed 10 steps ago (40 - 10 left), the decoy just now (8 of 8).
    assert_eq!(list.len(), 2);
    assert!(list[0].0 < list[1].0, "ascending slot order");
    assert_eq!((list[0].1, list[0].2, list[0].3), (30, 40, 250));
    assert_eq!((list[1].1, list[1].2, list[1].3), (8, 8, 0));
}

#[test]
fn progress_in_thousandths_never_reaches_a_thousand_before_it_is_complete() {
    let c = |left, total| Construction { left, total }.permille();
    assert_eq!(c(40, 40), 0);
    assert_eq!(c(30, 40), 250);
    assert_eq!(c(20, 40), 500);
    assert_eq!(c(1, 40), 975);
    assert_eq!(c(1, 3000), 999);
    assert_eq!(c(1, 10_000), 999);
}

// --- destruction (WP5.5) and construction ---

mod shot_down {
    use super::*;
    use sf_sim::{
        DestructionConfig, Loadout, Mount, Order, ProjectileKind, WeaponStats, WeaponTable,
    };

    const GUNNER: Kind = Kind(20);
    const GUN: u16 = 1;

    fn world() -> World {
        let unit = |kind: Kind, hp: u32, footprint: Option<Footprint>, range: i32| UnitStats {
            kind,
            hp,
            movement_class: footprint.is_none().then_some(0),
            speed_per_tick: if footprint.is_none() {
                sf_sim::per_tick(Fx::from_int(2))
            } else {
                Fx::ZERO
            },
            turn_per_tick: 255,
            size: Fx::from_ratio(4, 10),
            footprint,
            weapon_range: Fx::from_int(range),
            turret_per_tick: 0,
        };
        let fp = |w, h| {
            Some(Footprint {
                width: w,
                height: h,
            })
        };
        let gun = WeaponStats {
            kind: GUN,
            range: Fx::from_int(8),
            min_range: Fx::ZERO,
            reload_ticks: 10,
            burst: 1,
            burst_delay_ticks: 0,
            damage: 20,
            damage_type: 0,
            projectile: ProjectileKind::Instant,
            projectile_speed_per_tick: Fx::ZERO,
            flight_height: Fx::ZERO,
            guide_turn_per_tick: 0,
            splash_radius: Fx::ZERO,
            splash_inner: Fx::ZERO,
            friendly_fire: true,
            mount: Mount::Hull,
            fire_arc: 128,
        };
        // The plant has 40 hit points here: two shots of 20.
        let mut plant = unit(PLANT, 40, fp(2, 2), 0);
        plant.hp = 40;
        World::new(WorldConfig::new(3).with_map_size(40, 30))
            .unwrap()
            .with_units(
                UnitTable::new(vec![
                    unit(ANCHOR, 500, fp(2, 2), 0),
                    plant,
                    unit(GUNNER, 50, None, 8),
                ])
                .unwrap(),
            )
            .unwrap()
            .with_weapons(
                WeaponTable::new(
                    vec![gun],
                    vec![Loadout {
                        unit: GUNNER,
                        weapons: vec![GUN],
                    }],
                )
                .unwrap(),
            )
            .unwrap()
            .with_destruction(DestructionConfig::DEFAULT)
            .unwrap()
            .with_buildings(
                BuildingTable::new(vec![
                    entry(ANCHOR, (2, 2), BuildingFunction::Anchor, &[], 4),
                    entry(PLANT, (2, 2), BuildingFunction::PowerPlant, &[], 0),
                ])
                .unwrap(),
            )
            .unwrap()
            .with_player_factions(&[(PlayerId(0), 1)])
            .unwrap()
    }

    #[test]
    fn a_structure_shot_down_while_it_goes_up_never_completes() {
        let mut w = world();
        send(
            &mut w,
            Command::PlaceStructure {
                kind: ANCHOR,
                cell: Cell::new(10, 10),
                owner: PlayerId(0),
            },
        );
        build(&mut w, PLANT, 14, 10);
        let plant = at(&w, 14, 10);
        assert!(w.construction_of(plant).is_some());
        // An enemy gunner takes the half-built plant under fire (an attack order names it).
        let tick = w.tick();
        w.schedule(
            tick,
            PlayerId(1),
            Command::SpawnUnit {
                kind: GUNNER,
                pos: sf_math::Vec2::from_ints(20, 11),
                owner: PlayerId(1),
            },
        )
        .unwrap();
        w.step();
        let gunner = w
            .snapshot()
            .iter()
            .find(|v| v.kind == GUNNER)
            .map(|v| v.id)
            .unwrap();
        send(
            &mut w,
            Command::Order {
                ids: vec![gunner],
                order: Order::Attack(plant),
                queued: false,
            },
        );
        // Two shots of 20 against 40 hit points, 10 ticks apart: gone well before 40 ticks.
        let mut completed = 0;
        for _ in 0..200 {
            completed += w.step().structures_completed;
        }
        assert!(!w.is_alive(plant), "the plant was destroyed");
        assert_eq!(completed, 0, "a destroyed structure never completes");
        assert_eq!(w.under_construction(), 0, "its state went with it");
        assert_eq!(w.construction_of(plant), None);
        assert!(!w.stands(plant));
        // The cells are free again; nothing is left to finish.
        assert!(w.occupant_at(Cell::new(14, 10)).is_none());
        assert!(w.completions().is_empty());
    }
}
