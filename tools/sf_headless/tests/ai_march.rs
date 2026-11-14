//! WP9.2a-2, the gather cell and the one-body start (`docs/rules/ai.md`, "The start-site advantage"): the gather
//! cell is never an ore cell and keeps `gather_refinery_gap` cells from every own refinery, and every army unit
//! is sent to it, so a wave starts from one place. Both are checked on the real `m1_duel` games, one per start
//! site: game 0 has seat 0 on the south-west site (12, 68), game 4 the same seating with the sites swapped (seat
//! 0 on the north-east site (72, 14)). A control with the rules off shows what each test would catch.

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
/// 16 games measured (the two sites, 8 games each: 1 to 3).
const LAUNCH_SPREAD_MAX: u32 = 3;

#[test]
fn the_wave_starts_from_one_place_on_both_start_sites() {
    for game in [0u32, 4] {
        let spread = launch_spread(game, false);
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
    // count as arrived and are never sent; they stand 7 cells from it when the wave goes out (16 of 16 games
    // measured), and the test above fails on it.
    for game in [0u32, 4] {
        let spread = launch_spread(game, true);
        for (seat, far) in spread.into_iter().enumerate() {
            if on_south_west(game, seat) {
                assert_eq!(far, 7, "game {game}");
            } else {
                assert!(far <= 5, "game {game}: {far}");
            }
        }
    }
}
