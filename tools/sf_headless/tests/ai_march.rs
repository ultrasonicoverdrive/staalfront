//! WP9.2a-2, the gather cell and the one-body start (`docs/rules/ai.md`, "The start-site advantage"): the gather
//! cell is never an ore cell and keeps `gather_refinery_gap` cells from every own refinery, and every army unit
//! is sent to it, so a wave starts from one place. Both are checked on the real `m1_duel` games, one per start
//! site: game 0 has seat 0 on the south-west site (12, 68), game 4 the same seating with the sites swapped (seat
//! 0 on the north-east site (72, 14)). A control with the rules off shows what each test would catch.
//!
//! WP9.2b-1: these tests pin the gather rules on the unit mix of WP9.2a-2 (rocket trooper 2), which is still the
//! shipped mix (the measured 4:4:3, 4:3:3 and 5:3:3 were not shipped).

use std::collections::BTreeMap;

use sf_ai::{Ai, AiState, Lens, Observation, army};
use sf_data::ai_profile::AiProfile;
use sf_headless::tournament::{factions_for, load_setup, prepare_sited, seeds_for, sites_swapped};
use sf_sim::{BuildingFunction, Cell, PlayerId, World, gap};

/// Seat `seat` (0 or 1) of `game` and the start site it stands on: `true` for the south-west one.
fn on_south_west(game: u32, seat: usize) -> bool {
    (seat == 0) != sites_swapped(game)
}

/// The shipped numbers of the two rules.
const GAP: u8 = 2;

/// A prepared `m1_duel` game `game` with the shipped assets; `rules_off` puts the profile of every seat back to
/// the old rules (`gather_refinery_gap` 0, `gather_everyone` false).
fn duel(game: u32, rules_off: bool) -> (Box<World>, Vec<Ai>) {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let setup = load_setup(&assets, &assets.join("matches/m1_duel.ron")).unwrap();
    let (world_seed, ai_seed) = seeds_for(458_758, game);
    let prepared = prepare_sited(
        &setup,
        factions_for(setup.file_factions(), game),
        sites_swapped(game),
        world_seed,
        ai_seed,
    )
    .unwrap();
    let mut ais = prepared.ais;
    if rules_off {
        for ai in &mut ais {
            let old = ai.state().clone();
            let mut profile = old.profile.clone();
            profile.army.gather_refinery_gap = 0;
            profile.army.gather_everyone = false;
            *ai = Ai::new(old.player, profile, ai_seed, old.start_sites);
        }
    }
    (prepared.world, ais)
}

/// One tick of the game: the AIs that are due think, their commands are scheduled, the world steps.
fn step(world: &mut World, ais: &mut [Ai], wanted: &mut Vec<sf_sim::Command>) {
    for ai in ais.iter_mut() {
        if ai.due(world.tick()) {
            wanted.clear();
            ai.think(world, wanted);
            for command in wanted.drain(..) {
                world.schedule(world.tick(), ai.player(), command).unwrap();
            }
        }
    }
    world.step();
}

fn cheb(a: Cell, b: Cell) -> u32 {
    a.x.abs_diff(b.x).max(a.y.abs_diff(b.y))
}

/// The refineries (harvester docks) of `player` as rectangles `(x, y, width, height)`.
fn docks(world: &World, player: PlayerId) -> Vec<(i64, i64, i64, i64)> {
    let obs = Observation::build(world, player, &BTreeMap::new());
    let lens = Lens::new(world, player);
    obs.own
        .iter()
        .filter(|o| {
            lens.building(o.kind)
                .is_some_and(|b| b.function == BuildingFunction::HarvesterDock)
        })
        .filter_map(|o| {
            o.footprint.map(|f| {
                (
                    i64::from(o.cell.x),
                    i64::from(o.cell.y),
                    i64::from(f.width),
                    i64::from(f.height),
                )
            })
        })
        .collect()
}

/// The gather cell the AI works out from scratch on the observation of `world` (`ensure_gather` with the kept
/// cell cleared).
fn worked_out(world: &World, state: &AiState) -> Option<Cell> {
    let obs = Observation::build(world, state.player, &BTreeMap::new());
    let lens = Lens::new(world, state.player);
    let mut fresh = state.clone();
    fresh.gather = None;
    army::ensure_gather(&mut fresh, &obs, &lens)
}

/// Runs `game` to tick `until`.
fn run_to(game: u32, rules_off: bool, until: u64) -> (Box<World>, Vec<Ai>) {
    let (mut world, mut ais) = duel(game, rules_off);
    let mut wanted = Vec::new();
    while world.tick().0 < until {
        step(&mut world, &mut ais, &mut wanted);
    }
    (world, ais)
}

#[test]
fn the_shipped_profile_sets_both_rules() {
    let a = AiProfile::embedded().army;
    assert_eq!((a.gather_refinery_gap, a.gather_everyone), (GAP, true));
}

/// Checks the gather cell of both seats of `game` now: never an ore cell, and at least [`GAP`] cells from every
/// refinery (there must be one when `with_refinery`), for the cell the AI keeps and for the one it works out again.
fn check_gather_cells(world: &World, ais: &[Ai], game: u32, with_refinery: bool) {
    let tick = world.tick().0;
    for (seat, ai) in ais.iter().enumerate() {
        let site = if on_south_west(game, seat) {
            "SW"
        } else {
            "NE"
        };
        let lens = Lens::new(world, ai.player());
        let own_docks = docks(world, ai.player());
        assert_eq!(
            !own_docks.is_empty(),
            with_refinery,
            "game {game} {site} tick {tick}: refinery"
        );
        let kept = ai.state().gather.expect("a gather cell");
        let fresh = worked_out(world, ai.state()).expect("a gather cell");
        println!("game {game} {site} tick {tick}: gather cell kept {kept:?}, worked out {fresh:?}");
        for (what, cell) in [("kept", kept), ("worked out", fresh)] {
            assert!(
                !lens.harvestable(cell),
                "game {game} {site} tick {tick}: the {what} gather cell {cell:?} is an ore cell"
            );
            for dock in &own_docks {
                let g = gap((i64::from(cell.x), i64::from(cell.y), 1, 1), *dock);
                assert!(
                    g >= i64::from(GAP),
                    "game {game} {site} tick {tick}: the {what} gather cell {cell:?} is {g} cells from the refinery {dock:?}"
                );
            }
        }
    }
}

#[test]
fn the_gather_cell_is_never_an_ore_cell_and_keeps_its_gap_on_both_start_sites() {
    for game in [0u32, 4] {
        let (mut world, mut ais) = duel(game, false);
        let mut wanted = Vec::new();
        // At tick 200 only the construction centre stands: the cell must already be off the ore.
        while world.tick().0 < 200 {
            step(&mut world, &mut ais, &mut wanted);
        }
        check_gather_cells(&world, &ais, game, false);
        // At 6,000 both seats have their refinery: the cell kept since tick 200 must have moved off it if it
        // was too close (the north-east seat's first choice lies beside where its refinery goes up).
        while world.tick().0 < 6_000 {
            step(&mut world, &mut ais, &mut wanted);
        }
        check_gather_cells(&world, &ais, game, true);
    }
}

#[test]
fn with_the_rules_off_the_north_east_gather_cell_is_an_ore_cell() {
    // The control of the test above: the old rule picks (66, 26) on the north-east site, an ore cell of its own
    // field and next to its refinery; the test above fails on it.
    let (world, ais) = run_to(0, true, 6_000);
    let ne = &ais[1];
    let kept = ne.state().gather.expect("a gather cell");
    assert_eq!(kept, Cell::new(66, 26));
    assert!(Lens::new(&world, ne.player()).harvestable(kept));
    let dock = docks(&world, ne.player())[0];
    assert!(gap((66, 26, 1, 1), dock) < i64::from(GAP));
}

/// How far from the gather cell the members of the first strike of each seat stand when it goes out:
/// `[game][seat]`.
fn launch_spread(game: u32, rules_off: bool) -> [u32; 2] {
    let (mut world, mut ais) = duel(game, rules_off);
    let mut wanted = Vec::new();
    let mut spread = [None; 2];
    while world.tick().0 < 20_000 && spread.iter().any(Option::is_none) {
        step(&mut world, &mut ais, &mut wanted);
        for (seat, ai) in ais.iter().enumerate() {
            let st = ai.state();
            if spread[seat].is_some() {
                continue;
            }
            let (Some(g), Some(gather)) = (st.groups.first(), st.gather) else {
                continue;
            };
            let obs = Observation::build(&world, st.player, &BTreeMap::new());
            let far = obs
                .own
                .iter()
                .filter(|o| g.members.binary_search(&o.id).is_ok())
                .map(|o| cheb(o.cell, gather))
                .max()
                .unwrap();
            spread[seat] = Some(far);
        }
    }
    [
        spread[0].expect("seat 0 launched"),
        spread[1].expect("seat 1 launched"),
    ]
}

/// The most cells any member of a wave stands from the gather cell when it goes out, with the rules on, over the
/// first 64 games of `m1_duel` (the two sites, 32 games each; a launch before tick 20,000 in 61 games on SW and 64
/// on NE): 1 to 4 cells, mean 1.7 on SW and 1.6 on NE. With the rules off it is 7 in all 63 launches on SW and 2 to
/// 5 (mostly 4) on NE.
const LAUNCH_SPREAD_MAX: u32 = 4;

#[test]
fn the_wave_starts_from_one_place_on_both_start_sites() {
    // Pinned exactly for the two games (`[seat 0, seat 1]`): in game 0 seat 0 is on SW, in game 4 on NE.
    for (game, want) in [(0u32, [2u32, 1]), (4, [2, 1])] {
        let spread = launch_spread(game, false);
        assert_eq!(spread, want, "game {game}");
        for (seat, far) in spread.into_iter().enumerate() {
            let site = if on_south_west(game, seat) {
                "SW"
            } else {
                "NE"
            };
            assert!(
                far <= LAUNCH_SPREAD_MAX,
                "game {game} {site}: a member of the wave stands {far} cells from the gather cell"
            );
        }
    }
}

#[test]
fn with_the_rules_off_the_south_west_wave_starts_seven_cells_from_the_gather_cell() {
    // The control: the two tanks of the south-west factory are made within `gather_radius` (8) of the gather cell,
    // count as arrived and are never sent; they stand 7 cells from it when the wave goes out (63 of 63 launches
    // measured), the north-east wave 4 here (2 to 5 over 64 games; its units stand about the ore field); the test
    // above fails on it.
    assert_eq!(launch_spread(0, true), [7, 4], "game 0: seat 0 is on SW");
    assert_eq!(launch_spread(4, true), [4, 7], "game 4: seat 0 is on NE");
}

/// How far from the staging cell the live members of the first strike of each seat stand at the staging exit
/// (the first army turn the strike is no longer `Staging`): `[seat]`, `None` when that seat's first strike has
/// no staging cell or does not leave it before tick 20,000.
fn exit_spread(game: u32, rules_off: bool) -> [Option<u32>; 2] {
    let (mut world, mut ais) = duel(game, rules_off);
    let mut wanted = Vec::new();
    let mut spread = [None; 2];
    let mut done = [false; 2];
    while world.tick().0 < 20_000 && done.iter().any(|d| !d) {
        step(&mut world, &mut ais, &mut wanted);
        for (seat, ai) in ais.iter().enumerate() {
            let st = ai.state();
            if done[seat] {
                continue;
            }
            let Some(g) = st.groups.iter().find(|g| g.id == 0) else {
                continue;
            };
            let Some(cell) = g.staging else {
                done[seat] = true;
                continue;
            };
            if g.stage == sf_ai::state::GroupStage::Staging {
                continue;
            }
            let obs = Observation::build(&world, st.player, &BTreeMap::new());
            spread[seat] = obs
                .own
                .iter()
                .filter(|o| g.members.binary_search(&o.id).is_ok())
                .map(|o| cheb(o.cell, cell))
                .max();
            done[seat] = true;
        }
    }
    spread
}

/// A value per seat, `[seat 0, seat 1]`.
type Seats = [Option<u32>; 2];

/// The staging-exit spread, pinned exactly per game (`[seat 0, seat 1]`, `None` = that seat's first strike does
/// not leave a staging cell before tick 20,000). Only one seat of each game gets there, so three games are used:
/// game 0 (seat 0 on SW), game 4 (seat 0 on NE) and game 3 (seat 1 on NE, the north-east game with a rules-off
/// exit; in games 0 and 4 the north-east seat has none with the rules off). With the rules on: SW 0, NE 1 and 1;
/// with them off: SW 3 (game 0) and 4 (game 4), NE 4 (game 3).
const EXIT_SPREAD: [(u32, Seats, Seats); 3] = [
    (0, [Some(0), None], [Some(3), None]),
    (4, [Some(1), None], [None, Some(4)]),
    (3, [None, Some(1)], [None, Some(4)]),
];

#[test]
fn the_staging_exit_spread_is_pinned_on_both_start_sites() {
    for (game, on, _) in EXIT_SPREAD {
        assert_eq!(exit_spread(game, false), on, "game {game}, rules on");
    }
}

#[test]
fn with_the_rules_off_the_wave_leaves_the_staging_cell_spread_out() {
    // The control: both 9.2a-2 numbers at 0; the test above fails on these.
    for (game, _, off) in EXIT_SPREAD {
        assert_eq!(exit_spread(game, true), off, "game {game}, rules off");
    }
}

// ---- WP9.3e-1: the formation march on the two start sites -------------------------------------------------------------

/// The break radius of the rows that were measured (`fm8`, the one with the break outside every sight of the mix).
const BREAK_RADIUS: u8 = 8;

/// `m1_duel` game `game` with both seats on the formation march (`march_break_radius` [`BREAK_RADIUS`] and the staging search).
fn duel_with_the_march(game: u32) -> (Box<World>, Vec<Ai>) {
    let (world, mut ais) = duel(game, false);
    for ai in &mut ais {
        let old = ai.state().clone();
        let mut profile = old.profile.clone();
        profile.army.march_break_radius = BREAK_RADIUS;
        profile.army.stage_search = true;
        *ai = Ai::new(
            old.player,
            profile,
            seeds_for(458_758, game).1,
            old.start_sites,
        );
    }
    (world, ais)
}

/// What the first strike of a seat did on its staging leg, read from the world: the tick of the staging exit (the first turn
/// the group is no longer `Staging`), how many members stood on a `Move` at most while it staged, how many members it had,
/// the largest Chebyshev distance of a member from the staging cell at the exit, and the Chebyshev distance of the nearest
/// seen enemy to any member at the exit (`None`: none seen).
#[derive(Debug, PartialEq, Eq)]
struct Leg {
    exit_tick: u64,
    most_on_move: usize,
    members: usize,
    exit_spread: u32,
    /// The largest Chebyshev distance of a member from the mean cell of the members at the exit (the group's own spread).
    group_spread: u32,
    enemy_seen_at: Option<u32>,
}

fn staging_legs(game: u32, march: bool) -> [Option<Leg>; 2] {
    let (mut world, mut ais) = if march {
        duel_with_the_march(game)
    } else {
        duel(game, false)
    };
    let mut wanted = Vec::new();
    let mut most = [0usize; 2];
    let mut legs: [Option<Leg>; 2] = [None, None];
    while world.tick().0 < 20_000 && legs.iter().any(|l| l.is_none()) {
        step(&mut world, &mut ais, &mut wanted);
        for (seat, ai) in ais.iter().enumerate() {
            let st = ai.state();
            if legs[seat].is_some() {
                continue;
            }
            let Some(g) = st.groups.iter().find(|g| g.id == 0) else {
                continue;
            };
            let Some(cell) = g.staging else { continue };
            let obs = Observation::build(&world, st.player, &BTreeMap::new());
            let mine = |o: &&sf_ai::Own| g.members.binary_search(&o.id).is_ok();
            if g.stage == sf_ai::state::GroupStage::Staging {
                let on_move = obs
                    .own
                    .iter()
                    .filter(mine)
                    .filter(|o| matches!(o.order, sf_sim::Order::Move(_)))
                    .count();
                most[seat] = most[seat].max(on_move);
                continue;
            }
            let enemy_seen_at = obs
                .own
                .iter()
                .filter(mine)
                .flat_map(|o| {
                    obs.visible
                        .iter()
                        .filter(|s| s.owner != st.player)
                        .map(move |s| cheb(o.cell, s.cell))
                })
                .min();
            legs[seat] = Some(Leg {
                exit_tick: world.tick().0,
                most_on_move: most[seat],
                members: obs.own.iter().filter(mine).count(),
                exit_spread: obs
                    .own
                    .iter()
                    .filter(mine)
                    .map(|o| cheb(o.cell, cell))
                    .max()
                    .unwrap_or(0),
                group_spread: {
                    let cells: Vec<Cell> = obs.own.iter().filter(mine).map(|o| o.cell).collect();
                    let n = cells.len() as u32;
                    let mean = Cell::new(
                        cells.iter().map(|c| c.x).sum::<u32>() / n.max(1),
                        cells.iter().map(|c| c.y).sum::<u32>() / n.max(1),
                    );
                    cells.iter().map(|c| cheb(*c, mean)).max().unwrap_or(0)
                },
                enemy_seen_at,
            });
        }
    }
    legs
}

/// `(exit tick, most members on a Move, members, exit spread, group spread, nearest enemy seen)`.
type LegWant = (u64, usize, usize, u32, u32, Option<u32>);

/// The first strike of each seat on its staging leg with the march on, per game (`[seat 0, seat 1]`): in game 0 seat 0 is on SW,
/// in game 4 on NE. Measured on the shipped assets: every strike goes out on a `Move` as one group of six and breaks 920 to
/// 1,040 ticks after the launch (a launch at about 10,080), 20 to 25 cells short of its staging cell, with the other wave in
/// sight at 2 to 4 cells; the spread of the group at the break is 3 to 14.
const LEGS_ON: [(u32, [LegWant; 2]); 2] = [
    (
        0,
        [
            (11001, 6, 6, 22, 3, Some(4)),
            (11038, 6, 6, 21, 14, Some(2)),
        ],
    ),
    (
        4,
        [
            (11001, 6, 6, 25, 14, Some(4)),
            (10998, 6, 6, 20, 4, Some(4)),
        ],
    ),
];

#[test]
fn the_staging_leg_with_the_march_on_is_pinned_on_both_start_sites() {
    for (game, want) in LEGS_ON {
        let got = staging_legs(game, true);
        for seat in 0..2 {
            let site = if on_south_west(game, seat) {
                "SW"
            } else {
                "NE"
            };
            let leg = got[seat]
                .as_ref()
                .unwrap_or_else(|| panic!("game {game} {site}: no leg"));
            assert_eq!(
                (
                    leg.exit_tick,
                    leg.most_on_move,
                    leg.members,
                    leg.exit_spread,
                    leg.group_spread,
                    leg.enemy_seen_at
                ),
                want[seat],
                "game {game} {site}"
            );
            // The group went out on a `Move` and broke on seeing the other wave within the radius.
            assert_eq!(leg.most_on_move, leg.members, "game {game} {site}");
            assert!(
                leg.enemy_seen_at
                    .is_some_and(|d| d <= u32::from(BREAK_RADIUS))
            );
        }
    }
}

#[test]
fn with_the_march_off_no_member_is_ever_on_a_move_while_the_strike_stages() {
    // The control: the shipped numbers; the strike goes out on an `AttackMove` and the test above would fail on it.
    let mut legs = 0;
    for game in [0u32, 4] {
        for (seat, leg) in staging_legs(game, false).iter().enumerate() {
            if let Some(leg) = leg {
                assert_eq!(leg.most_on_move, 0, "game {game} seat {seat}: {leg:?}");
                legs += 1;
            }
        }
    }
    assert!(
        legs >= 2,
        "the control has staging legs to look at ({legs})"
    );
}
