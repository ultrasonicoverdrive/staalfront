//! The first match (WP7.6) on its map: the `match` scenario with its checksum pinned, and what it
//! shows. See `assets/scenarios/match.ron`, `assets/matches/m1.ron` and `docs/rules/match.md`.
//!
//! The human (player 0) builds a power plant (done at tick 301), a barracks (705) and a refinery (1105),
//! makes four riflemen and five rocket troopers, sends them across the map at tick 2400, and the static
//! opponent (player 1) is defeated at tick 5687. The harvest loop pays for the units; the opponent never
//! builds, produces or attacks.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{EntityId, MatchResult, PlayerId, Tick, Verdict, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/match.expected");

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// The tick the match is decided in: the last of the opponent's structures and armed units falls.
const DECIDED: u64 = 5687;
/// The tick the first fight begins in (the army arrives at the opponent's base).
const FIRST_DEATH: u64 = 3689;

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("match.ron")).unwrap()
}

/// What a tick showed: its number (the tick that ran) and the report.
struct Row {
    tick: u64,
    deaths: u32,
    result: Option<MatchResult>,
    defeats: Vec<(PlayerId, Tick)>,
}

fn play(s: &Scenario) -> (World, Vec<Row>) {
    let mut w = s.build_world().unwrap();
    let mut rows = Vec::new();
    for _ in 0..s.ticks {
        let report = w.step();
        rows.push(Row {
            tick: report.tick.0,
            deaths: report.deaths,
            result: w.match_result(),
            defeats: w.defeats().to_vec(),
        });
    }
    (w, rows)
}

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
    // Every command of the match file and of the script was taken: nothing was refused.
    assert_eq!(outcome.commands_ignored, 0);
}

#[test]
fn the_starting_objects_enter_in_the_order_of_the_match_file() {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let report = w.step();
    // Tick 0: 1 object of the human and 10 of the opponent, as the file lists them.
    assert_eq!(
        report.commands_executed, 11,
        "the objects of the match file"
    );
    let owner_kind = |slot: u32| {
        let v = w
            .snapshot()
            .iter()
            .find(|v| v.id == EntityId::new(slot, 0))
            .unwrap_or_else(|| panic!("slot {slot}"));
        (v.owner.0, v.kind.0)
    };
    // Kinds: 0 construction_center, 1 power_plant, 2 refinery, 3 barracks, 6 defence_tower,
    // 11 harvester, 12 rifleman, 19 light_tank.
    let expect = [
        (0, 0),
        (1, 1),
        (1, 2),
        (1, 3),
        (1, 6),
        (1, 11),
        (1, 12),
        (1, 12),
        (1, 12),
        (1, 12),
        (1, 19),
    ];
    for (slot, want) in expect.iter().enumerate() {
        assert_eq!(owner_kind(slot as u32), *want, "slot {slot}");
    }
    assert_eq!(w.match_players(), &[P0, P1]);
}

#[test]
fn the_human_wins_in_the_tick_the_last_of_the_opponent_falls() {
    let s = scenario();
    let (w, rows) = play(&s);
    // Nothing is decided before.
    assert!(
        rows.iter()
            .take(DECIDED as usize - 1)
            .all(|r| r.result.is_none())
    );
    assert!(
        rows.iter()
            .take(DECIDED as usize - 1)
            .all(|r| r.defeats.is_empty())
    );
    // In tick 5687 (the row after the step that ran it) the opponent is defeated and the human wins.
    let row = &rows[DECIDED as usize];
    assert_eq!(row.tick, DECIDED);
    assert_eq!(row.defeats, vec![(P1, Tick(DECIDED))]);
    assert_eq!(
        row.result,
        Some(MatchResult {
            tick: Tick(DECIDED),
            verdict: Verdict::Won(P0)
        })
    );
    assert!(row.deaths > 0, "the last one fell in that tick");
    assert_eq!(w.defeat_tick(P1), Some(Tick(DECIDED)));
    assert_eq!(w.defeat_tick(P0), None);
    // The first fight was well before.
    let first = rows.iter().find(|r| r.deaths > 0).unwrap();
    assert_eq!(first.tick, FIRST_DEATH);
}

#[test]
fn nothing_changes_after_the_outcome() {
    let s = scenario();
    let (w, rows) = play(&s);
    let decided = rows[DECIDED as usize].result;
    assert!(decided.is_some());
    for r in &rows[DECIDED as usize..] {
        assert_eq!(r.result, decided, "tick {}", r.tick);
        assert_eq!(r.defeats.len(), 1, "tick {}", r.tick);
    }
    // 1,300 ticks of game went by after the decision; the world kept running (the human's harvester
    // still works), the result stayed.
    assert_eq!(w.tick().0, 7000);
    assert_eq!(rows.len() as u64 - DECIDED, 7000 - DECIDED);
    assert_eq!(w.match_result(), decided);
}

#[test]
fn a_world_without_match_rules_plays_the_same_game_and_has_no_outcome() {
    let with = scenario();
    let mut without = with.clone();
    // The same scenario, the same objects, factions and credits; only the rules of the match are
    // left out.
    without.match_file = None;
    let (a, _) = play(&with);
    let (b, rows) = play(&without);
    assert_eq!(b.match_result(), None);
    assert!(b.defeats().is_empty());
    assert!(b.match_players().is_empty());
    assert!(
        rows.iter()
            .all(|r| r.result.is_none() && r.defeats.is_empty())
    );
    // The match only watches: every entity is where it was and has the hit points it had.
    let state = |w: &World| -> Vec<(u32, u16, u8, i64, i64, Option<u32>)> {
        w.snapshot()
            .iter()
            .map(|v| {
                (
                    v.id.index(),
                    v.kind.0,
                    v.owner.0,
                    v.pos.x.raw(),
                    v.pos.y.raw(),
                    w.hp_of(v.id),
                )
            })
            .collect()
    };
    assert_eq!(state(&a), state(&b));
    // But it is state: the world with the rules hashes differently.
    assert_ne!(a.checksum(), b.checksum());
}

#[test]
fn the_opponent_stays_put_fires_back_and_never_builds_or_produces() {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    w.step();
    // Where the opponent's units start.
    let start: Vec<(u32, i64, i64)> = w
        .snapshot()
        .iter()
        .filter(|v| v.owner == P1 && w.footprint_of(v.id).is_none() && v.kind.0 != 11)
        .map(|v| (v.id.index(), v.pos.x.raw(), v.pos.y.raw()))
        .collect();
    assert_eq!(start.len(), 5, "four riflemen and a light tank");
    let mut most_entities = w.snapshot().iter().filter(|v| v.owner == P1).count();
    let mut farthest = 0;
    let mut human_hurt = false;
    for _ in 1..s.ticks {
        w.step();
        let mine = w.snapshot().iter().filter(|v| v.owner == P1).count();
        // It never gains an entity: no building, no production.
        assert!(mine <= most_entities, "tick {}", w.tick().0);
        most_entities = most_entities.min(mine);
        for v in w.snapshot().iter().filter(|v| v.owner == P1) {
            if let Some(&(_, x0, y0)) = start.iter().find(|s| s.0 == v.id.index()) {
                let (dx, dy) = ((v.pos.x.raw() - x0).abs(), (v.pos.y.raw() - y0).abs());
                farthest = farthest.max(dx.max(dy));
            }
        }
        // Its fire is seen as damage on the human's units.
        human_hurt |= w
            .snapshot()
            .iter()
            .filter(|v| v.owner == P0 && v.kind.0 == 12)
            .any(|v| w.hp_of(v.id).is_some_and(|hp| hp < 50));
    }
    // 6 cells is the pursuit distance of `sim.ron`; a unit goes no farther from where it stood.
    assert!(
        farthest <= 6 * (1 << 16),
        "farthest displacement {} cells",
        farthest as f64 / 65536.0
    );
    assert!(human_hurt, "the opponent's defenders fired back");
    // The human killed all ten things of the opponent; the opponent killed none of the human's.
    assert_eq!(w.tally_of(P0).kills, 10);
    assert_eq!(w.tally_of(P1).kills, 0);
    assert_eq!(w.tally_of(P1).losses, 10);
}

#[test]
fn the_harvest_pays_for_the_army() {
    let s = scenario();
    let (w, _) = play(&s);
    // The human's harvester delivered ore into the refinery, and credits were spent on structures
    // and units (4000 at the start).
    assert!(w.ore_delivered(P0) > 0);
    assert!(w.credits_spent(P0) > 3000);
    // The opponent's harvester mined too, and the credits were never spent.
    assert!(w.ore_delivered(P1) > 0);
    assert_eq!(w.credits_spent(P1), 0);
}

// ---- the map ----------------------------------------------------------------------------------

#[allow(dead_code)]
#[path = "../examples/gen_match_map.rs"]
mod gen_match_map;

fn committed_map() -> sf_data::map::Map {
    sf_data::map::Map::load(&common::scenario_path("../maps/m1")).unwrap()
}

#[test]
fn the_committed_map_is_what_the_generator_draws() {
    let drawn = gen_match_map::draw();
    let committed = committed_map();
    assert_eq!(committed.size_in_cells(), (96, 96));
    assert_eq!(
        committed.terrain().unwrap().ids(),
        drawn.terrain().unwrap().ids(),
        "re-run: cargo run -p sf_headless --example gen_match_map -- assets/maps/m1"
    );
}

#[test]
fn the_map_has_the_places_the_match_file_names() {
    let map = committed_map();
    let layer = map.terrain().unwrap();
    let types = sf_data::terrain::TerrainTypes::embedded();
    let id = |name: &str| types.id_of(name).unwrap();
    let (grass, water, rock, ore) = (id("grass"), id("water"), id("rock"), id("ore"));
    // Literal ids of the shipped terrain types: grass 0, water 2, rock 3, ore 4.
    assert_eq!((grass, water, rock, ore), (0, 2, 3, 4));
    // The lake, the two ridges and the ore fields (centres of the discs of the generator).
    assert_eq!(layer.get(46, 44), Some(water));
    assert_eq!(layer.get(35, 25), Some(rock));
    assert_eq!(layer.get(57, 60), Some(rock));
    for (x, y) in [(27, 66), (10, 86), (64, 28), (88, 10), (44, 14), (50, 78)] {
        assert_eq!(layer.get(x, y), Some(ore), "ore at ({x}, {y})");
    }
    // Ground under the starting structures of the match file is grass (buildable).
    for (x, y) in [(12, 68), (15, 71), (76, 20), (70, 26), (80, 14), (66, 22)] {
        assert_eq!(layer.get(x, y), Some(grass), "grass at ({x}, {y})");
    }
    // How much of each there is: a change to the layout shows here.
    let count = |t: u8| layer.ids().iter().filter(|&&i| i == t).count();
    assert_eq!(
        (count(water), count(rock), count(ore), layer.ids().len()),
        (WATER_CELLS, ROCK_CELLS, ORE_CELLS, 96 * 96)
    );
}

const WATER_CELLS: usize = 253;
const ROCK_CELLS: usize = 252;
const ORE_CELLS: usize = 174;

/// The cost of the outcome (WP7.6): the same scenario, with and without the match rules, alternating,
/// in microseconds per tick. Run with `cargo test --release -p sf_headless --test match_m1 -- --ignored
/// --nocapture cost_of_the_outcome`. A measurement, not an assertion on time (the machine decides).
#[test]
#[ignore]
fn cost_of_the_outcome() {
    let with = scenario();
    let mut without = with.clone();
    without.match_file = None;
    let time = |s: &Scenario| {
        let mut w = s.build_world().unwrap();
        let t = std::time::Instant::now();
        for _ in 0..s.ticks {
            w.step();
        }
        t.elapsed().as_secs_f64() * 1e6 / s.ticks as f64
    };
    let mut a = Vec::new();
    let mut b = Vec::new();
    for _ in 0..7 {
        a.push(time(&with));
        b.push(time(&without));
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(|x, y| x.partial_cmp(y).unwrap());
        v[v.len() / 2]
    };
    println!("with the match rules:    {:?}", a);
    println!("without the match rules: {:?}", b);
    println!(
        "median us per tick: with {:.3}, without {:.3}",
        median(&mut a),
        median(&mut b)
    );
}
