//! The first match (WP7.6) on its map: the `match` scenario with its checksum pinned, and what it
//! shows. See `assets/scenarios/match.ron`, `assets/matches/m1.ron` and `docs/rules/match.md`.
//!
//! The human (player 0) builds a power plant (done at tick 301), a barracks (705) and a refinery (1105),
//! makes four riflemen and five rocket troopers, sends them across the map at tick 2400, and the static
//! opponent (player 1) is defeated at tick 6238, the tick its construction centre falls (WP7.7: before it
//! was "no structure and no armed unit left", tick 5967, and 5687 before WP6.7: its defence tower fires
//! now). The harvest loop pays for the units; the opponent never builds, produces or attacks. The
//! targeting gate (`acquire_needs_sight`) is on, as in the game (WP7.7).

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::Scenario;
use sf_sim::{EntityId, MatchResult, PlayerId, Tick, Verdict, World};

const EXPECTED: &str = include_str!("../../../assets/scenarios/match.expected");

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// The tick the match is decided in: the opponent's construction centre (slot 11) falls (WP7.7).
const DECIDED: u64 = 6238;
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
    // Tick 0: 1 object of the human and 11 of the opponent, as the file lists them.
    assert_eq!(
        report.commands_executed, 12,
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
    // 11 harvester, 12 rifleman, 19 light_tank. The opponent's construction centre is the last (WP7.7).
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
        (1, 0),
    ];
    for (slot, want) in expect.iter().enumerate() {
        assert_eq!(owner_kind(slot as u32), *want, "slot {slot}");
    }
    assert_eq!(w.match_players(), &[P0, P1]);
}

#[test]
fn the_human_wins_in_the_tick_the_opponents_construction_centre_falls() {
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
    // In tick 6238 (the row after the step that ran it) the opponent is defeated and the human wins.
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
    assert!(row.deaths > 0, "the construction centre fell in that tick");
    assert_eq!(w.defeat_tick(P1), Some(Tick(DECIDED)));
    assert_eq!(w.defeat_tick(P0), None);
    // The first fight was well before.
    let first = rows.iter().find(|r| r.deaths > 0).unwrap();
    assert_eq!(first.tick, FIRST_DEATH);
}

/// WP7.7: what falls in the deciding tick is the opponent's construction centre (slot 11), and the
/// opponent still owns other things then (the old rule needed every structure and armed unit gone).
#[test]
fn the_deciding_blow_is_the_construction_centre_and_the_opponent_still_has_other_things() {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let cc = EntityId::new(11, 0);
    let mut fell = None;
    let mut others_then = Vec::new();
    for _ in 0..s.ticks {
        let report = w.step();
        if fell.is_none() && !w.is_alive(cc) {
            fell = Some(report.tick.0);
            others_then = w
                .snapshot()
                .iter()
                .filter(|v| v.owner == P1)
                .map(|v| v.kind.0)
                .collect();
        }
    }
    assert_eq!(fell, Some(DECIDED));
    assert_eq!(w.defeat_tick(P1), Some(Tick(DECIDED)));
    // Kinds: 3 is the barracks. Measured, not chosen: the army had killed nearly everything else by then.
    assert_eq!(others_then, vec![3], "the opponent still had its barracks");
}

#[test]
fn nothing_changes_after_the_outcome() {
    let s = scenario();
    let (w, rows) = play(&s);
    // The expectation is written down here, not read from the run: it does not depend on the run
    // having decided at the tick the other test pins.
    let outcome = MatchResult {
        tick: Tick(DECIDED),
        verdict: Verdict::Won(P0),
    };
    let defeats = vec![(P1, Tick(DECIDED))];
    assert_eq!(rows.len() as u64, 7000, "the scenario runs 7,000 ticks");
    let after: Vec<&Row> = rows.iter().filter(|r| r.tick >= DECIDED).collect();
    // Every tick from the decision to the end: 762 of them (the steps that ran
    // ticks 6238 to 6999; the world is at tick 7000 after the last).
    assert_eq!(after.len() as u64, 7000 - DECIDED);
    for r in &after {
        assert_eq!(r.result, Some(outcome), "tick {}", r.tick);
        assert_eq!(r.defeats, defeats, "tick {}", r.tick);
    }
    // The world kept running after the decision (the human's harvester still works), and the
    // state the match keeps is what it was: the result, the defeat of the opponent and none of the
    // human, the players.
    assert_eq!(w.tick().0, 7000);
    assert_eq!(w.match_result(), Some(outcome));
    assert_eq!(w.defeats(), defeats.as_slice());
    assert_eq!(w.defeat_tick(P1), Some(Tick(DECIDED)));
    assert_eq!(w.defeat_tick(P0), None);
    assert_eq!(w.match_players(), &[P0, P1]);
    assert!(w.match_begun());
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
    // By the end of the scenario (the match was decided long before: the world keeps running) the human
    // killed all eleven things of the opponent. Before WP6.7 the opponent killed none of the human's;
    // since its tower fires it kills six (four riflemen and two rocket troopers).
    assert_eq!(w.tally_of(P0).kills, 11);
    assert_eq!(w.tally_of(P1).kills, 6);
    assert_eq!(w.tally_of(P1).losses, 11);
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
#[allow(clippy::cast_precision_loss)]
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

/// Power (WP6.4) is on in the match. It moved the pin once, from `0x54a140fc12273e31` (the scenario with
/// the `power` line taken out) to the value of that day, by its section alone; the proof is below, as
/// a comparison of the two worlds tick by tick. (Since WP6.7 the tower fires, so the world without the
/// power line no longer ends at the WP7.6 value. The WP6.4 value `0x49f207148bde095c` is still the
/// measured cause of the move: with the structure branch of targeting and section `d` switched off, the
/// match gives exactly that value again, as measured in the review of WP6.7.)
#[test]
fn power_in_the_match_moves_the_pin_by_its_section_only() {
    let with = scenario();
    assert!(with.power.is_some());
    let mut without = with.clone();
    without.power = None;
    // With them, the game is the same game, tick by tick: the same entities in the same places with the
    // same hit points, the same credits, and the speed never leaves full (no shortage in the script).
    let mut a = with.build_world().unwrap();
    let mut b = without.build_world().unwrap();
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
    for t in 0..with.ticks {
        a.step();
        b.step();
        assert_eq!(a.credits(P0), b.credits(P0), "tick {t}");
        assert_eq!(a.credits(P1), b.credits(P1), "tick {t}");
        assert_eq!(a.power_speed_permille(P0), 1000, "tick {t}");
        // The opponent's plant is shot at from the first fight on: its output falls with its hit
        // points and its (idle) factory slows down. That changes nothing in this game: it never builds.
        if t < FIRST_DEATH - 100 {
            assert_eq!(a.power_speed_permille(P1), 1000, "tick {t}");
        }
        if t % 25 == 0 {
            assert_eq!(state(&a), state(&b), "tick {t}");
        }
    }
    assert_eq!(a.match_result(), b.match_result());
    assert_ne!(a.checksum(), b.checksum());
    // And what the human made and used at the end is the plant against the refinery and the barracks.
    let t = a.power_of(P0).unwrap();
    assert_eq!((t.made, t.used), (100, 50));
}

/// The three pins of the match (WP7.7), measured on the same scenario: the shipped one (vision with the
/// targeting gate on), the same with the gate switched off, and the same without the vision line at all.
/// Before WP7.7 the shipped pin was the world with vision and the gate off, `0x771de0f816275bbc`, and the
/// one without the vision line `0x164403a92986654f`; both moved with the opponent's construction centre
/// and the new rule (see `docs/rules/match.md`).
const PIN_GATE_OFF: &str = "0x4d2b34f903b93f78";
const PIN_NO_VISION: &str = "0xdccf390f64809d17";

#[test]
fn the_pins_of_the_match_with_the_gate_on_off_and_without_vision() {
    let on = scenario();
    assert!(on.vision.as_ref().unwrap().acquire_needs_sight, "WP7.7");
    let mut off = on.clone();
    off.vision.as_mut().unwrap().acquire_needs_sight = false;
    let mut none = on.clone();
    none.vision = None;
    let run_to_end = |s: &Scenario| run(s, &RunOptions::for_scenario(s)).unwrap().final_checksum;
    assert_eq!(run_to_end(&on), parse_checksum(EXPECTED).unwrap());
    assert_eq!(run_to_end(&off), parse_checksum(PIN_GATE_OFF).unwrap());
    assert_eq!(run_to_end(&none), parse_checksum(PIN_NO_VISION).unwrap());
}

/// The gate parts the worlds at tick 4692 (WP8.2a measured 4282 with the cell test of sight; the edge
/// test of WP7.7 lets the rocket troopers pick the tower at the limit of their reach, so the first
/// difference is something else, later). Vision without the gate changes nothing:
/// `vision.rs`: `the_first_match_plays_the_same_with_vision_on`.
#[test]
fn the_gate_parts_the_gated_and_the_ungated_match_at_tick_4692() {
    let on = scenario();
    let mut off = on.clone();
    off.vision.as_mut().unwrap().acquire_needs_sight = false;
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
    let (mut a, mut b) = (off.build_world().unwrap(), on.build_world().unwrap());
    let mut parted = None;
    for _ in 0..on.ticks {
        a.step();
        b.step();
        if parted.is_none() && state(&a) != state(&b) {
            parted = Some(a.tick().0);
        }
    }
    assert_eq!(parted, Some(4692));
    assert_ne!(a.checksum(), b.checksum());
    // Without the gate the match is decided 10 ticks earlier, with the same six units lost.
    assert_eq!(a.defeat_tick(P1), Some(Tick(6228)));
    assert_eq!(b.defeat_tick(P1), Some(Tick(DECIDED)));
    assert_eq!(a.tally_of(P1).kills, b.tally_of(P1).kills);
}

/// The opponent's tower (slot 4) fires in the match (WP6.7): the first shot is at tick 3830, 140 ticks
/// after the first fight began (the human's infantry has to come within its reach of 6 cells), it fires
/// 40 shots, and it falls at tick 4509 to the human's rocket troopers. Those 40 shots (8 damage each) and the
/// opponent's riflemen are what kill six of the human's nine.
#[test]
fn the_opponents_tower_fires_in_the_match() {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let tower = EntityId::new(4, 0);
    let (mut first, mut count, mut fell) = (None, 0, None);
    for _ in 0..s.ticks {
        let r = w.step();
        let mine = w.shots().iter().filter(|sh| sh.attacker == tower).count();
        if mine > 0 {
            first.get_or_insert(r.tick.0);
            count += mine;
        }
        if fell.is_none() && !w.is_alive(tower) {
            fell = Some(r.tick.0);
        }
    }
    assert_eq!(first, Some(3830));
    assert_eq!(count, 40);
    assert_eq!(fell, Some(TOWER_FELL));
    assert!(
        first.unwrap() > FIRST_DEATH,
        "it fires after the first fight began"
    );
}

/// The tick the opponent's tower is destroyed in.
const TOWER_FELL: u64 = 4509;

/// The tower uses 20 of the opponent's power: its plant (100 at 600 hit points), refinery (30), barracks
/// (20) and tower (20) leave the player at full power while the plant makes at least 70, that is at 420 hit
/// points or more. A human light tank is put at the plant from tick 100 to measure it: the defences go dark
/// the first time the plant has less than 420 of its 600 hit points.
#[test]
fn the_opponents_tower_goes_dark_when_its_plant_is_below_420_of_600() {
    let s = scenario();
    let mut w = s.build_world().unwrap();
    let plant = EntityId::new(1, 0);
    for _ in 0..100 {
        w.step();
    }
    assert!(w.defences_powered(P1));
    let tick = w.tick();
    w.schedule(
        tick,
        P0,
        sf_sim::Command::SpawnUnit {
            kind: sf_sim::Kind(19),
            pos: sf_math::Vec2::from_ints(82, 24),
            owner: P0,
        },
    )
    .unwrap();
    w.step();
    let tank = w
        .snapshot()
        .iter()
        .find(|v| v.owner == P0 && v.kind.0 == 19)
        .unwrap()
        .id;
    let tick = w.tick();
    w.schedule(
        tick,
        P0,
        sf_sim::Command::Order {
            ids: vec![tank],
            order: sf_sim::Order::Attack(plant),
            queued: false,
        },
    )
    .unwrap();
    let (mut last_powered_hp, mut dark_hp) = (None, None);
    for _ in 0..3000 {
        let hp_before = w.hp_of(plant).unwrap_or(0);
        w.step();
        // The totals of a step are counted from the state the step before left.
        if w.defences_powered(P1) {
            last_powered_hp = Some(hp_before);
        } else if dark_hp.is_none() {
            dark_hp = Some(hp_before);
        }
        if !w.is_alive(plant) {
            break;
        }
    }
    let (dark, lit) = (dark_hp.expect("it went dark"), last_powered_hp.unwrap());
    println!("last powered with the plant at {lit} hit points, dark with it at {dark}");
    assert!(dark < 420, "dark at {dark} hit points");
    assert!(lit >= 420, "last lit at {lit} hit points");
}
