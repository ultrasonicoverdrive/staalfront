//! Ore on the map (WP7.1): the amount per cell, taking, regrowth, the nearest-ore search and the
//! checksum section, through the public API. The rules are in `docs/rules/economy.md`.

use sf_math::Rng;
use sf_sim::{
    Cell, Command, ConfigError, Kind, OreRules, PlayerId, Terrain, TerrainDef, TerrainFlags,
    TerrainId, Tick, World, WorldConfig,
};

const GRASS: TerrainId = TerrainId(0);
const ORE: TerrainId = TerrainId(4);

fn defs() -> Vec<TerrainDef> {
    vec![
        TerrainDef {
            id: GRASS,
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: ORE,
            flags: TerrainFlags {
                buildable: false,
                harvestable: true,
                ground: true,
            },
        },
    ]
}

/// No growth: for the tests that count exact amounts.
const STILL: OreRules = OreRules {
    start: 100,
    max: 120,
    regrow_amount: 0,
    regrow_period: 10,
};

const RULES: OreRules = OreRules {
    start: 100,
    max: 120,
    regrow_amount: 5,
    regrow_period: 10,
};

/// A `w` x `h` map; `is_ore(x, y)` says which cells are ore.
fn world_with(w: u32, h: u32, is_ore: impl Fn(u32, u32) -> bool, rules: OreRules) -> World {
    let ids: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| if is_ore(x, y) { ORE.0 } else { GRASS.0 })
        .collect();
    let terrain = Terrain::from_layer(w, h, &ids, &defs()).unwrap();
    World::with_terrain(WorldConfig::new(1).with_map_size(w, h), terrain)
        .unwrap()
        .with_ore(rules)
        .unwrap()
}

/// A 20 x 20 map with a 4 x 3 field at (5..9, 6..9).
fn field_world(rules: OreRules) -> World {
    world_with(
        20,
        20,
        |x, y| (5..9).contains(&x) && (6..9).contains(&y),
        rules,
    )
}

fn take(world: &mut World, cell: Cell, amount: u16) -> (u32, u32) {
    let tick = world.tick();
    world
        .schedule(tick, PlayerId(0), Command::TakeOre { cell, amount })
        .unwrap();
    let r = world.step();
    (r.commands_executed, r.commands_ignored)
}

#[test]
fn ore_starts_on_the_ore_cells_only() {
    let world = field_world(RULES);
    let ore = world.ore().unwrap();
    assert_eq!(ore.field_count(), 12);
    assert_eq!(ore.total(), 12 * 100);
    assert_eq!(world.ore_at(Cell::new(5, 6)), 100);
    assert_eq!(world.ore_at(Cell::new(8, 8)), 100);
    // Next to the field, off the map, and a world without rules.
    assert_eq!(world.ore_at(Cell::new(9, 6)), 0);
    assert_eq!(world.ore_at(Cell::new(4, 6)), 0);
    assert_eq!(world.ore_at(Cell::new(99, 99)), 0);
    let plain = World::new(WorldConfig::new(1).with_map_size(8, 8)).unwrap();
    assert!(plain.ore().is_none());
    assert_eq!(plain.ore_at(Cell::new(1, 1)), 0);
}

#[test]
fn a_world_without_a_terrain_layer_has_no_field_cells() {
    let world = World::new(WorldConfig::new(1).with_map_size(8, 8))
        .unwrap()
        .with_ore(RULES)
        .unwrap();
    assert_eq!(world.ore().unwrap().field_count(), 0);
}

#[test]
fn rules_must_be_valid_and_early() {
    let bad = |r: OreRules| {
        let w = World::new(WorldConfig::new(1).with_map_size(8, 8)).unwrap();
        assert_eq!(w.with_ore(r).err(), Some(ConfigError::OreRules));
    };
    bad(OreRules { max: 0, ..RULES });
    bad(OreRules {
        start: 121,
        ..RULES
    });
    bad(OreRules {
        regrow_period: 0,
        ..RULES
    });
    bad(OreRules {
        regrow_period: OreRules::MAX_PERIOD + 1,
        ..RULES
    });
    let mut started = World::new(WorldConfig::new(1).with_map_size(8, 8)).unwrap();
    started.step();
    assert_eq!(started.with_ore(RULES).err(), Some(ConfigError::OreRules));
}

#[test]
fn taking_gives_what_is_there_and_no_more() {
    let mut world = field_world(STILL);
    let cell = Cell::new(6, 7);
    assert_eq!(take(&mut world, cell, 30), (1, 0));
    assert_eq!(world.ore_at(cell), 70);
    // More than the cell holds: it gives all it has and ends at zero.
    assert_eq!(take(&mut world, cell, 500), (1, 0));
    assert_eq!(world.ore_at(cell), 0);
    // An empty cell, a cell outside a field and a take of nothing change nothing.
    assert_eq!(take(&mut world, cell, 1), (0, 1));
    assert_eq!(take(&mut world, Cell::new(0, 0), 1), (0, 1));
    assert_eq!(take(&mut world, Cell::new(99, 0), 1), (0, 1));
    assert_eq!(take(&mut world, Cell::new(5, 6), 0), (0, 1));
    assert_eq!(world.ore().unwrap().total(), 11 * 100);
}

#[test]
fn take_ore_is_ignored_in_a_world_without_ore_rules() {
    let mut world = World::new(WorldConfig::new(1).with_map_size(8, 8)).unwrap();
    assert_eq!(take(&mut world, Cell::new(1, 1), 10), (0, 1));
}

/// The rule that an emptied cell is still an ore cell: the terrain does not change, it is still
/// not buildable, and the cell is still a field cell that grows back.
#[test]
fn an_empty_cell_stays_an_ore_cell_and_grows_back() {
    let mut world = field_world(STILL);
    let cell = Cell::new(7, 8);
    take(&mut world, cell, 1000);
    assert_eq!(world.ore_at(cell), 0);
    assert_eq!(world.terrain_at(cell), Some(ORE));
    assert!(world.terrain().flags_at(cell).unwrap().harvestable);
    assert!(!world.terrain().flags_at(cell).unwrap().buildable);
    assert!(world.ore().unwrap().is_field(cell));
    // It is refused a structure, as before.
    world
        .schedule(
            world.tick(),
            PlayerId(0),
            Command::Place {
                kind: Kind(1),
                cell,
                owner: PlayerId(0),
            },
        )
        .unwrap();
    assert_eq!(world.step().commands_ignored, 1);
    // With growth on, the same empty cell holds one step after a whole period (the cell's turn
    // comes once in any ten ticks), and is still an ore cell.
    let mut world = field_world(RULES);
    take(&mut world, cell, 1000); // tick 0, the first of the ten ticks of a period
    for _ in 1..RULES.regrow_period {
        world.step();
    }
    assert_eq!(world.ore_at(cell), 5);
    assert_eq!(world.terrain_at(cell), Some(ORE));
}

#[test]
fn every_field_cell_grows_once_per_period_on_its_own_tick() {
    let mut world = field_world(RULES);
    let cells: Vec<Cell> = world.ore().unwrap().cells().map(|(c, _)| c).collect();
    for &c in &cells {
        take(&mut world, c, 50); // one command per tick: ticks 0 to 11
    }
    // The takes ran in ticks 0 to 11. Now watch two full periods tick by tick.
    let start = world.tick().0;
    let mut grown_at: Vec<Vec<u64>> = vec![Vec::new(); cells.len()];
    let mut last: Vec<u16> = cells.iter().map(|&c| world.ore_at(c)).collect();
    for _ in 0..2 * u64::from(RULES.regrow_period) {
        let tick = world.tick().0;
        world.step();
        let mut touched = 0usize;
        for (k, &c) in cells.iter().enumerate() {
            let now = world.ore_at(c);
            if now != last[k] {
                assert_eq!(now, last[k] + 5, "one step of 5");
                grown_at[k].push(tick);
                last[k] = now;
                touched += 1;
            }
        }
        // A tick never touches more than ceil(cells / period) cells (here 2 of 12).
        let period = usize::try_from(RULES.regrow_period).unwrap();
        assert!(
            touched <= cells.len().div_ceil(period),
            "tick {tick}: {touched}"
        );
    }
    for (k, ticks) in grown_at.iter().enumerate() {
        // Position k of the field list grows in the ticks with tick % 10 == k % 10.
        let expected: Vec<u64> = (start..start + 20)
            .filter(|t| t % u64::from(RULES.regrow_period) == (k as u64) % 10)
            .collect();
        assert_eq!(ticks, &expected, "cell {k} {:?}", cells[k]);
    }
}

#[test]
fn regrowth_stops_at_the_maximum_and_leaves_other_cells_alone() {
    let mut world = field_world(RULES);
    for _ in 0..200 {
        world.step();
    }
    // Start 100, max 120: every cell is at the maximum, after 4 steps of 5 (the last is clamped).
    for (_, amount) in world.ore().unwrap().cells() {
        assert_eq!(amount, 120);
    }
    assert_eq!(world.ore_at(Cell::new(0, 0)), 0);
    assert_eq!(world.ore().unwrap().total(), 12 * 120);
}

#[test]
fn a_step_that_would_pass_the_maximum_is_clamped() {
    let rules = OreRules {
        start: 118,
        ..RULES
    };
    let mut world = field_world(rules);
    for _ in 0..10 {
        world.step();
    }
    assert!(world.ore().unwrap().cells().all(|(_, a)| a == 120));
}

#[test]
fn zero_regrowth_amount_means_no_regrowth() {
    let mut world = field_world(OreRules {
        regrow_amount: 0,
        ..RULES
    });
    take(&mut world, Cell::new(5, 6), 40);
    for _ in 0..500 {
        world.step();
    }
    assert_eq!(world.ore_at(Cell::new(5, 6)), 60);
}

/// The running sum that goes into the checksum is what a full recount gives, after any mix of
/// takes and growth.
#[test]
fn the_running_sum_equals_a_full_recount() {
    let mut world = world_with(40, 40, |x, y| (x * 7 + y * 3) % 5 == 0, RULES);
    let mut rng = Rng::new(9);
    let cells: Vec<Cell> = world.ore().unwrap().cells().map(|(c, _)| c).collect();
    for _ in 0..600 {
        let c = cells[rng.below_usize(cells.len())];
        let tick = world.tick();
        world
            .schedule(
                tick,
                PlayerId(0),
                Command::TakeOre {
                    cell: c,
                    amount: rng.below(60) as u16,
                },
            )
            .unwrap();
        world.step();
        let ore = world.ore().unwrap();
        assert_eq!(ore.running_sum(), ore.recompute_sum());
    }
}

/// A brute-force nearest: the smallest squared distance, then row, then column.
fn brute(world: &World, from: Cell, radius: u32, skip: impl Fn(Cell) -> bool) -> Option<Cell> {
    let (w, h) = world.map_size();
    let mut best: Option<(u64, u32, u32)> = None;
    for y in 0..h {
        for x in 0..w {
            let c = Cell::new(x, y);
            let (dx, dy) = (x.abs_diff(from.x), y.abs_diff(from.y));
            if world.ore_at(c) == 0 || skip(c) || dx.max(dy) > radius {
                continue;
            }
            let k = (u64::from(dx * dx + dy * dy), y, x);
            if best.is_none_or(|b| k < b) {
                best = Some(k);
            }
        }
    }
    best.map(|(_, y, x)| Cell::new(x, y))
}

#[test]
fn nearest_ore_equals_the_brute_force_answer() {
    let mut world = world_with(
        37,
        29,
        |x, y| (x * 5 + y * 11) % 17 == 0 || (x / 3 + y / 4) % 7 == 1,
        RULES,
    );
    // Empty some cells so that field cells without ore are in the way.
    let mut rng = Rng::new(3);
    let cells: Vec<Cell> = world.ore().unwrap().cells().map(|(c, _)| c).collect();
    for &c in &cells {
        if rng.chance(2, 3) {
            take(&mut world, c, 1000);
        }
    }
    let (w, h) = world.map_size();
    let mut checked = 0;
    for from_y in (0..h).step_by(3) {
        for from_x in (0..w).step_by(2) {
            for radius in [0, 1, 3, 8, 100] {
                let from = Cell::new(from_x, from_y);
                assert_eq!(
                    world.nearest_ore(from, radius),
                    brute(&world, from, radius, |_| false),
                    "from {from} radius {radius}"
                );
                let skip = |c: Cell| (c.x + c.y).is_multiple_of(3);
                assert_eq!(
                    world.nearest_ore_where(from, radius, |c| !skip(c)),
                    brute(&world, from, radius, skip),
                    "filtered, from {from} radius {radius}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 500);
}

#[test]
fn nearest_ore_breaks_ties_by_row_then_column_and_skips_empty_cells() {
    // Two ore cells at equal distance 2 from (5, 5): (5, 3) above and (3, 5) to the left,
    // and (7, 5) to the right and (5, 7) below. The upper row wins; with it empty, the
    // left one of the middle row.
    let mut world = world_with(
        12,
        12,
        |x, y| matches!((x, y), (5, 3) | (3, 5) | (7, 5) | (5, 7)),
        STILL,
    );
    let from = Cell::new(5, 5);
    assert_eq!(world.nearest_ore(from, 10), Some(Cell::new(5, 3)));
    take(&mut world, Cell::new(5, 3), 1000);
    assert_eq!(world.nearest_ore(from, 10), Some(Cell::new(3, 5)));
    take(&mut world, Cell::new(3, 5), 1000);
    assert_eq!(world.nearest_ore(from, 10), Some(Cell::new(7, 5)));
    take(&mut world, Cell::new(7, 5), 1000);
    assert_eq!(world.nearest_ore(from, 10), Some(Cell::new(5, 7)));
    take(&mut world, Cell::new(5, 7), 1000);
    assert_eq!(world.nearest_ore(from, 10), None);
    // The radius is a limit: ore two cells away is not found within one.
    let world = world_with(12, 12, |x, y| (x, y) == (7, 5), RULES);
    assert_eq!(world.nearest_ore(from, 1), None);
    assert_eq!(world.nearest_ore(from, 2), Some(Cell::new(7, 5)));
    // A start off the map finds nothing.
    assert_eq!(world.nearest_ore(Cell::new(50, 50), 100), None);
    // A start on the ore finds that cell.
    assert_eq!(world.nearest_ore(Cell::new(7, 5), 0), Some(Cell::new(7, 5)));
}

#[test]
fn a_nearer_cell_on_a_later_ring_beats_a_farther_one_on_an_earlier_ring() {
    // From (10, 10): (10, 17) is 7 away on ring 7 (squared 49); (15, 15) is on ring 5 (squared
    // 50); (14, 14) is on ring 4 (squared 32) and is the answer. The ring walk must not stop at
    // the first ring that has ore.
    let world = world_with(
        30,
        30,
        |x, y| matches!((x, y), (10, 17) | (15, 15) | (14, 14)),
        RULES,
    );
    assert_eq!(
        world.nearest_ore(Cell::new(10, 10), 30),
        Some(Cell::new(14, 14))
    );
    // Without (14, 14): (15, 15) on ring 5 (50) is beaten by (10, 17) on ring 7 (49).
    let world = world_with(30, 30, |x, y| matches!((x, y), (10, 17) | (15, 15)), RULES);
    assert_eq!(
        world.nearest_ore(Cell::new(10, 10), 30),
        Some(Cell::new(10, 17))
    );
}

/// Equal distance on different rings: (13, 14) is on ring 4 and (15, 10) on ring 5, both at
/// squared distance 25 from (10, 10). The smaller row wins, so the walk must not stop at ring 5
/// just because 25 is not above 25.
#[test]
fn an_equal_distance_on_a_later_ring_can_still_win_on_the_tie() {
    let world = world_with(30, 30, |x, y| matches!((x, y), (13, 14) | (15, 10)), STILL);
    assert_eq!(
        world.nearest_ore(Cell::new(10, 10), 30),
        Some(Cell::new(15, 10))
    );
}

#[test]
fn the_checksum_has_an_ore_section_only_with_ore_rules() {
    let config = WorldConfig::new(1).with_map_size(20, 20);
    let mut plain = World::new(config).unwrap();
    let mut with = World::new(config).unwrap().with_ore(RULES).unwrap();
    // No field cells, no state, but the rules are part of the world: the checksums differ.
    assert_ne!(plain.step().checksum, with.step().checksum);
    // A different rule is a different world from the first tick.
    let mut other = World::new(config)
        .unwrap()
        .with_ore(OreRules { max: 121, ..RULES })
        .unwrap();
    assert_ne!(other.step().checksum, with_first(config));
}

fn with_first(config: WorldConfig) -> u64 {
    World::new(config)
        .unwrap()
        .with_ore(RULES)
        .unwrap()
        .step()
        .checksum
}

#[test]
fn a_take_and_a_growth_move_the_checksum_and_the_same_history_repeats_it() {
    let run = || {
        let mut world = field_world(RULES);
        let mut sums = Vec::new();
        for t in 0..60 {
            if t == 5 {
                world
                    .schedule(
                        Tick(t),
                        PlayerId(0),
                        Command::TakeOre {
                            cell: Cell::new(6, 6),
                            amount: 70,
                        },
                    )
                    .unwrap();
            }
            sums.push(world.step().checksum);
        }
        sums
    };
    let a = run();
    assert_eq!(a, run());
    let mut idle = field_world(RULES);
    let b: Vec<u64> = (0..60).map(|_| idle.step().checksum).collect();
    // Before the take (and its command in the queue) the two agree on everything but the
    // pending command; after it the ore differs for good.
    assert_ne!(a[10], b[10]);
    assert_ne!(a[59], b[59]);
}

#[test]
fn regrowth_in_a_full_field_changes_nothing() {
    // A field at its maximum is not touched: its total, its running sum and its version are
    // steady.
    let mut world = field_world(OreRules {
        start: 120,
        ..RULES
    });
    let ore = world.ore().unwrap();
    let (version, total, sum) = (ore.version(), ore.total(), ore.running_sum());
    for _ in 0..100 {
        world.step();
    }
    let ore = world.ore().unwrap();
    assert_eq!(ore.version(), version);
    assert_eq!(ore.total(), total);
    assert_eq!(ore.running_sum(), sum);
}

/// The cost of the ore on a map made entirely of ore (the worst case: 65,536 field cells),
/// against the same world without ore rules. Run by hand:
/// `cargo test --release -p sf_sim --test ore -- --ignored --nocapture ore_cost`.
#[test]
#[ignore = "timing, run by hand"]
fn ore_cost() {
    use std::time::Instant;
    let build = |ore: bool| {
        let ids = vec![ORE.0; 256 * 256];
        let terrain = Terrain::from_layer(256, 256, &ids, &defs()).unwrap();
        let w = World::with_terrain(WorldConfig::new(1).with_map_size(256, 256), terrain).unwrap();
        if ore {
            w.with_ore(OreRules {
                start: 0,
                max: 160,
                regrow_amount: 1,
                regrow_period: 200,
            })
            .unwrap()
        } else {
            w
        }
    };
    let time = |ore: bool| {
        let mut w = build(ore);
        let t = Instant::now();
        for _ in 0..2_000 {
            w.step();
        }
        t.elapsed().as_secs_f64() * 1e6 / 2_000.0
    };
    for round in 0..3 {
        let without = time(false);
        let with = time(true);
        println!("round {round}: without ore {without:.1} us/tick, with ore {with:.1} us/tick");
    }
}
