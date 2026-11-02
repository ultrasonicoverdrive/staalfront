//! Power (WP6.4): made and used, damage, the speed rule, the defence hook, when the totals are
//! counted, and what the checksum covers. Inside the crate because damage is set on the hit-point
//! column directly (the combat that does it in a game is tested elsewhere). Every kind has its own
//! power number so that a swapped or dropped term shows.

use sf_math::Fx;

use super::*;
use crate::buildings::{BuildingFunction, BuildingStats, BuildingTable};
use crate::command::Command;
use crate::ids::{EntityId, Kind};
use crate::terrain::Cell;
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::world::WorldConfig;

const ANCHOR: Kind = Kind(1);
const PLANT: Kind = Kind(2); // +100, 600 hit points
const GEN: Kind = Kind(3); // +55, 400 hit points
const BARRACKS: Kind = Kind(4); // -20
const HUT: Kind = Kind(5); // -30
const LAB: Kind = Kind(6); // -45
const IRON: Kind = Kind(7); // +10, cannot be damaged (0 hit points in the table)
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const RULES: PowerRules = PowerRules {
    shortage_floor_permille: 250,
    defence_cutoff_permille: 1000,
};

fn fp() -> Footprint {
    Footprint {
        width: 2,
        height: 2,
    }
}

fn unit(kind: Kind, hp: u32) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(fp()),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn building(kind: Kind, function: BuildingFunction, ticks: u32, power: i32) -> BuildingStats {
    BuildingStats {
        kind,
        footprint: fp(),
        function,
        exit_side: function
            .needs_exit()
            .then_some(crate::buildings::Side::PosX),
        build_ticks: ticks,
        cost: (ticks > 0).then_some(10),
        power,
        factions: 3,
        requires: Vec::new(),
        build_radius: if function == BuildingFunction::Anchor {
            30
        } else {
            0
        },
    }
}

fn plain() -> World {
    World::new(WorldConfig::new(1).with_map_size(64, 64))
        .unwrap()
        .with_units(
            UnitTable::new(vec![
                unit(ANCHOR, 50),
                unit(PLANT, 600),
                unit(GEN, 400),
                unit(BARRACKS, 700),
                unit(HUT, 300),
                unit(LAB, 500),
                unit(IRON, 0),
            ])
            .unwrap(),
        )
        .unwrap()
        .with_buildings(
            BuildingTable::new(vec![
                building(ANCHOR, BuildingFunction::Anchor, 0, 0),
                building(PLANT, BuildingFunction::PowerPlant, 20, 100),
                building(GEN, BuildingFunction::PowerPlant, 20, 55),
                building(BARRACKS, BuildingFunction::InfantryFactory, 20, -20),
                building(HUT, BuildingFunction::None, 20, -30),
                building(LAB, BuildingFunction::None, 20, -45),
                building(IRON, BuildingFunction::PowerPlant, 20, 10),
            ])
            .unwrap(),
        )
        .unwrap()
        .with_player_factions(&[(P0, 1), (P1, 2)])
        .unwrap()
}

fn world() -> World {
    plain().with_power(RULES).unwrap()
}

fn place(w: &mut World, owner: PlayerId, kind: Kind, cell: (u32, u32)) -> EntityId {
    let tick = w.tick();
    w.schedule(
        tick,
        owner,
        Command::PlaceStructure {
            kind,
            cell: Cell::new(cell.0, cell.1),
            owner,
        },
    )
    .unwrap();
    w.step();
    w.occupant_at(Cell::new(cell.0, cell.1)).unwrap()
}

fn hp(w: &mut World, id: EntityId, hp: u32) {
    let i = id.index() as usize;
    w.entities.hp[i] = hp;
}

fn totals(w: &World, p: PlayerId) -> (u64, u64) {
    let t = w.power_of(p).unwrap();
    (t.made, t.used)
}

#[test]
fn made_and_used_are_the_sums_of_the_standing_structures_of_each_player() {
    let mut w = world();
    place(&mut w, P0, ANCHOR, (4, 4));
    place(&mut w, P0, PLANT, (10, 4));
    place(&mut w, P0, GEN, (14, 4));
    place(&mut w, P0, BARRACKS, (18, 4));
    place(&mut w, P0, HUT, (22, 4));
    place(&mut w, P0, LAB, (26, 4));
    place(&mut w, P1, LAB, (10, 30));
    place(&mut w, P1, IRON, (14, 30));
    w.step();
    assert_eq!(totals(&w, P0), (155, 95));
    assert_eq!(totals(&w, P1), (10, 45));
    // A player with nothing, and a world without power rules.
    assert_eq!(totals(&w, PlayerId(7)), (0, 0));
    let free = plain();
    assert_eq!(free.power_of(P0), None);
}

#[test]
fn the_totals_are_those_counted_at_the_start_of_the_last_step() {
    let mut w = world();
    place(&mut w, P0, PLANT, (10, 4));
    // The plant was placed during that step: it counts from the next one.
    assert_eq!(totals(&w, P0), (0, 0));
    w.step();
    assert_eq!(totals(&w, P0), (100, 0));
}

#[test]
fn a_structure_under_construction_makes_and_uses_nothing_until_it_stands() {
    let mut w = world();
    place(&mut w, P0, ANCHOR, (4, 4));
    place(&mut w, P0, PLANT, (10, 4));
    let tick = w.tick();
    // A hut (-30) and a generator (+55) are built, 20 ticks each.
    for (kind, cell) in [(HUT, (14, 4)), (GEN, (18, 4))] {
        w.schedule(
            tick,
            P0,
            Command::BuildStructure {
                kind,
                cell: Cell::new(cell.0, cell.1),
            },
        )
        .unwrap();
    }
    w.step();
    assert_eq!(w.under_construction(), 2);
    for _ in 0..10 {
        w.step();
        assert_eq!(totals(&w, P0), (100, 0));
    }
    // Complete after 20 ticks; counted from the step after that.
    let mut seen = None;
    for _ in 0..30 {
        let done = w.step().structures_completed;
        if done > 0 {
            assert_eq!(totals(&w, P0), (100, 0), "not in the step it completes");
            seen = Some(w.tick());
            break;
        }
    }
    assert!(seen.is_some());
    w.step();
    assert_eq!(w.under_construction(), 0);
    assert_eq!(totals(&w, P0), (155, 30));
}

#[test]
fn damage_lowers_what_a_producer_makes_by_hit_points_rounded_down() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 4));
    let gen_ = place(&mut w, P0, GEN, (14, 4));
    let barracks = place(&mut w, P0, BARRACKS, (18, 4));
    let iron = place(&mut w, P0, IRON, (22, 4));
    w.step();
    assert_eq!(totals(&w, P0), (165, 20));
    // 450 of 600: 75; 399 of 400: 54 (54.86 down). The iron plant cannot be damaged: all of 10.
    hp(&mut w, plant, 450);
    hp(&mut w, gen_, 399);
    hp(&mut w, iron, 5);
    w.step();
    assert_eq!(totals(&w, P0), (75 + 54 + 10, 20));
    // 599 of 600 is 99, one hit point is 0.
    hp(&mut w, plant, 599);
    hp(&mut w, gen_, 1);
    w.step();
    assert_eq!(totals(&w, P0), (99 + 10, 20));
    // A damaged consumer is not cheaper to run.
    hp(&mut w, barracks, 1);
    w.step();
    assert_eq!(totals(&w, P0), (109, 20));
}

#[test]
fn a_structure_at_zero_hit_points_counts_for_nothing() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 4));
    let hut = place(&mut w, P0, HUT, (14, 4));
    w.step();
    assert_eq!(totals(&w, P0), (100, 30));
    hp(&mut w, plant, 0);
    hp(&mut w, hut, 0);
    w.step();
    assert_eq!(totals(&w, P0), (0, 0));
}

#[test]
fn a_destroyed_structure_leaves_the_totals_at_the_next_step() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 4));
    place(&mut w, P0, GEN, (14, 4));
    place(&mut w, P0, HUT, (18, 4));
    w.step();
    assert_eq!(totals(&w, P0), (155, 30));
    let tick = w.tick();
    w.schedule(tick, P0, Command::Despawn { id: plant })
        .unwrap();
    w.step();
    w.step();
    assert_eq!(totals(&w, P0), (55, 30));
}

#[test]
fn the_speed_rule_with_distinct_numbers() {
    let r = |floor| PowerRules {
        shortage_floor_permille: floor,
        defence_cutoff_permille: 0,
    };
    let t = |made, used| PowerTotals { made, used };
    // (made, used, floor) -> speed
    let cases = [
        (0, 0, 250, 1000),     // nothing used: full, even with nothing made
        (100, 50, 250, 1000),  // plenty
        (100, 100, 250, 1000), // exactly enough
        (99, 100, 250, 990),
        (50, 100, 250, 500),
        (33, 100, 250, 330),
        (1, 3, 250, 333),    // rounded down
        (10, 100, 250, 250), // the floor
        (0, 100, 250, 250),
        (10, 100, 0, 100),
        (0, 100, 0, 0), // floor 0: stopped
        (1, 1000, 0, 1),
        (5, 100, 700, 700), // a high floor wins over the ratio
    ];
    for (made, used, floor, want) in cases {
        assert_eq!(
            speed_for(t(made, used), r(floor)),
            want,
            "made {made} used {used} floor {floor}"
        );
    }
}

#[test]
fn a_construction_advances_on_exactly_speed_of_every_thousand_ticks() {
    for speed in [0u32, 1, 250, 333, 416, 500, 999, 1000] {
        for start in [0u64, 7, 1000] {
            let n = (start..start + 1000)
                .filter(|&t| advances_on(Tick(t), speed))
                .count();
            assert_eq!(n as u32, speed, "speed {speed} from tick {start}");
        }
    }
    // Spread out: at 500 every other tick, never two in a row.
    assert!(
        (0..100).all(|t| advances_on(Tick(t), 500) != advances_on(Tick(t + 1), 500)),
        "alternates"
    );
    // A speed above full counts as full.
    assert!(advances_on(Tick(3), 5000));
}

#[test]
fn the_speed_and_the_defence_hook_follow_the_totals() {
    let mut w = world();
    place(&mut w, P0, PLANT, (10, 4));
    place(&mut w, P0, BARRACKS, (14, 4));
    w.step();
    // 100 made, 20 used.
    assert_eq!(w.power_speed_permille(P0), 1000);
    assert_eq!(w.production_speed_permille(P0), 1000);
    assert!(w.defences_powered(P0));
    // Three labs more: 155 used against 100 made, speed 645.
    for k in 0..3 {
        place(&mut w, P0, LAB, (18 + 4 * k, 4));
    }
    w.step();
    assert_eq!(totals(&w, P0), (100, 155));
    assert_eq!(w.power_speed_permille(P0), 645);
    assert_eq!(w.production_speed_permille(P0), 645);
    assert!(!w.defences_powered(P0), "cutoff 1000: any shortage");
    // Player 1 has nothing: unaffected.
    assert_eq!(w.power_speed_permille(P1), 1000);
    assert!(w.defences_powered(P1));
}

#[test]
fn the_defence_cutoff_is_a_number_of_the_rules() {
    let make = |cutoff| {
        let mut w = plain()
            .with_power(PowerRules {
                shortage_floor_permille: 250,
                defence_cutoff_permille: cutoff,
            })
            .unwrap();
        place(&mut w, P0, GEN, (10, 4)); // +55
        place(&mut w, P0, LAB, (14, 4)); // -45
        place(&mut w, P0, HUT, (18, 4)); // -30: 55 of 75 made, speed 733
        w.step();
        w
    };
    assert_eq!(make(0).power_speed_permille(P0), 733);
    assert!(make(0).defences_powered(P0));
    assert!(make(733).defences_powered(P0));
    assert!(!make(734).defences_powered(P0));
    assert!(!make(1000).defences_powered(P0));
}

#[test]
fn the_rules_are_given_once_after_the_tables_and_before_the_match() {
    assert_eq!(
        World::new(WorldConfig::new(1))
            .unwrap()
            .with_power(RULES)
            .unwrap_err(),
        ConfigError::Power
    );
    assert_eq!(
        world().with_power(RULES).unwrap_err(),
        ConfigError::Power,
        "twice"
    );
    for (floor, cutoff, ok) in [
        (0, 0, true),
        (1000, 1000, true),
        (1001, 0, false),
        (0, 1001, false),
    ] {
        let rules = PowerRules {
            shortage_floor_permille: floor,
            defence_cutoff_permille: cutoff,
        };
        assert_eq!(rules.is_valid(), ok);
        assert_eq!(plain().with_power(rules).is_ok(), ok);
    }
    let mut started = plain();
    started.step();
    started
        .schedule(
            started.tick(),
            P0,
            Command::Despawn {
                id: EntityId::new(0, 0),
            },
        )
        .unwrap();
    assert_eq!(started.with_power(RULES).unwrap_err(), ConfigError::Power);
}

// ---- the checksum ------------------------------------------------------------------------------

fn built(rules: Option<PowerRules>) -> World {
    let mut w = match rules {
        Some(r) => plain().with_power(r).unwrap(),
        None => plain(),
    };
    place(&mut w, P0, PLANT, (10, 4));
    place(&mut w, P0, LAB, (14, 4));
    place(&mut w, P1, GEN, (10, 30));
    w.step();
    w
}

#[test]
fn the_section_is_written_only_in_a_world_with_power_rules() {
    let without = built(None);
    let with = built(Some(RULES));
    assert_ne!(without.checksum(), with.checksum());
    // Same world, twice: same value.
    assert_eq!(with.checksum(), built(Some(RULES)).checksum());
    assert_eq!(without.checksum(), built(None).checksum());
    // A world without power rules has no state of power at all.
    assert!(without.power.is_none());
}

#[test]
fn a_world_without_power_rules_hashes_to_this_value() {
    // Measured on the base of this work package (`origin/wp/7.6-m1-match`, no power code at all) with the
    // same world built by the public API: the value is the same, so a world without the rules writes
    // nothing for power. The shipped pins (`golden.rs`, every `.expected` of an earlier phase) say the
    // same about whole scenarios.
    assert_eq!(built(None).checksum(), 6_550_792_836_568_588_549);
}

#[test]
fn both_tuning_numbers_and_every_total_are_in_the_checksum() {
    let base = built(Some(RULES)).checksum();
    let floor = built(Some(PowerRules {
        shortage_floor_permille: 251,
        ..RULES
    }));
    let cutoff = built(Some(PowerRules {
        defence_cutoff_permille: 999,
        ..RULES
    }));
    assert_ne!(floor.checksum(), base);
    assert_ne!(cutoff.checksum(), base);
    assert_ne!(floor.checksum(), cutoff.checksum());
    // The totals: change each of the four numbers of the two players in turn.
    for which in 0..4 {
        let mut w = built(Some(RULES));
        let p = w.power.as_mut().unwrap();
        assert_eq!(p.totals.len(), 2);
        match which {
            0 => p.totals[0].1.made += 1,
            1 => p.totals[0].1.used += 1,
            2 => p.totals[1].1.made += 1,
            _ => p.totals[1].1.used += 1,
        }
        assert_ne!(w.checksum(), base, "total {which}");
    }
    // And the player they belong to.
    let mut w = built(Some(RULES));
    w.power.as_mut().unwrap().totals[1].0 = PlayerId(5);
    assert_ne!(w.checksum(), base);
}

// ---- the cost ----------------------------------------------------------------------------------

/// The cost of the recount: one pass over the slots, with 40 structures among 5,000 wanderers. Run with
/// `cargo test --release -p sf_sim --lib power::tests::cost_of_the_recount -- --ignored --nocapture`.
/// (Not a test of a rule: it prints nanoseconds per call and asserts only that it is far below a tick.)
#[test]
#[ignore = "timing; run by hand in release"]
fn cost_of_the_recount() {
    let mut w = world();
    for k in 0..40u32 {
        let kind = [PLANT, BARRACKS, HUT, LAB][(k % 4) as usize];
        place(
            &mut w,
            PlayerId((k % 2) as u8),
            kind,
            (2 + 3 * (k % 20), 2 + 3 * (k / 20)),
        );
    }
    for n in 0..5000u32 {
        w.spawn(
            Kind(9),
            sf_math::Vec2::from_ints(1 + (n % 60) as i32, 20 + (n / 60) as i32 % 40),
            PlayerId((n % 4) as u8),
        )
        .unwrap();
    }
    let calls = 20_000u32;
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = std::time::Instant::now();
        for _ in 0..calls {
            w.refresh_power();
        }
        times.push(start.elapsed().as_nanos() / u128::from(calls));
    }
    println!("refresh_power with 40 structures among 5,040 entities, ns per call: {times:?}");
    let best = times.iter().copied().min().unwrap();
    assert!(
        best < 500_000,
        "a tick is 50,000,000 ns; the recount took {best} ns"
    );
}
