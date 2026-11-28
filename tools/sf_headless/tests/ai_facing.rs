//! WP9.6a, the direction-free switches (`docs/rules/ai.md`, "Direction-free rules"): each of the three rules
//! (placement ties, gather ring, scout quarters) is run through the production code (`placement::search`,
//! `army::ensure_gather`, `army::waypoints`) on a world that a half turn maps onto itself: open ground with a
//! few blocking cells set the same way round, one construction centre of the same faction on each side. Player 1's
//! answer must be the half turn of player 0's with the switch on, and the control with it off must differ (the
//! bias this work removes; a mirror test that cannot fail would prove nothing).
//!
//! Not part of the mirror: `exit_side` of the refinery and the factories (it points east for both players, a
//! rule of the unit definitions and out of scope), so kinds with an exit side are left out of the placement
//! check.

mod common;

use std::collections::BTreeMap;

use common::scenario_path;
use sf_ai::{AiState, Lens, Observation, army, facing, placement};
use sf_data::units::UnitDefs;
use sf_headless::run::{RunOptions, run_full};
use sf_headless::scenario::Scenario;
use sf_sim::{
    BuildingFunction, Cell, Kind, PlayerId, Terrain, TerrainDef, TerrainFlags, TerrainId, World,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const W: u32 = 96;
const H: u32 = 96;
/// The anchor of player 0 (4 x 4), and the half turn of it for player 1.
const A0: (u32, u32) = (12, 68);
const A1: (u32, u32) = (W - 4 - 12, H - 4 - 68);

fn turn_cell(c: Cell) -> Cell {
    Cell::new(W - 1 - c.x, H - 1 - c.y)
}

/// The cell of a footprint of `fp` cells that the half turn maps `c` onto.
fn turn_foot(c: Cell, (fw, fh): (u32, u32)) -> Cell {
    Cell::new(W - c.x - fw, H - c.y - fh)
}

/// The two states `(player 0, player 1)` and the world after one tick: open ground except for `walls` (and their
/// half turns), a construction centre of Meridian at each anchor, the start sites as the match gives them, the
/// switches set as asked.
struct Rig {
    world: World,
    s0: AiState,
    s1: AiState,
}

fn rig(walls: &[(u32, u32)], placement_on: bool, gather_on: bool, scout_on: bool) -> Rig {
    let mut s = Scenario::load(&scenario_path("ai_scout.ron")).unwrap();
    let mut def = s.match_def.clone().unwrap();
    def.players[0].objects.truncate(1);
    def.players[0].objects[0].cell = A0;
    def.players[1].faction = def.players[0].faction;
    def.players[1].credits = def.players[0].credits;
    def.players[1].objects.truncate(1);
    def.players[1].objects[0].cell = A1;
    let defs = UnitDefs::load(&scenario_path("ai_scout_defs"), &scenario_path("..")).unwrap();
    s.match_commands = sf_bridge::match_commands(&def, &defs).unwrap();
    s.match_def = Some(def);
    // The terrain: the definitions of the shipped map, every cell open ground, the walls blocked.
    let t = s.terrain.as_ref().expect("the scenario has a map");
    let defs_t: Vec<TerrainDef> = t
        .defined_ids()
        .map(|id| TerrainDef {
            id,
            flags: t.flags_of(id).unwrap(),
        })
        .collect();
    let open = defs_t
        .iter()
        .find(|d| d.flags == TerrainFlags::OPEN)
        .unwrap()
        .id;
    let wall = defs_t
        .iter()
        .find(|d| !d.flags.buildable && !d.flags.ground && !d.flags.harvestable)
        .unwrap()
        .id;
    let mut ids = vec![open.0; (W * H) as usize];
    for &(x, y) in walls {
        ids[(y * W + x) as usize] = wall.0;
        ids[((H - 1 - y) * W + (W - 1 - x)) as usize] = wall.0;
    }
    s.terrain = Some(Terrain::from_layer(W, H, &ids, &defs_t).unwrap());
    let _: TerrainId = open;
    let f = run_full(
        &s,
        &RunOptions {
            ticks: 1,
            every: 100,
        },
    )
    .unwrap();
    let mut s0 = f.ais[0].state().clone();
    s0.start_sites = vec![(P0, Cell::new(A0.0, A0.1)), (P1, Cell::new(A1.0, A1.1))];
    s0.gather = None;
    s0.profile.economy.placement_facing = placement_on;
    s0.profile.army.gather_facing = gather_on;
    s0.profile.army.scout_facing = scout_on;
    let mut s1 = s0.clone();
    s1.player = P1;
    Rig {
        world: *f.world,
        s0,
        s1,
    }
}

/// The kinds the placement check covers: a structure with a footprint, no exit side, not the anchor.
fn plain_kinds(world: &World) -> Vec<Kind> {
    (1..=12)
        .map(Kind)
        .filter(|&k| {
            world
                .building_table()
                .get(k)
                .is_some_and(|b| b.exit_side.is_none() && b.function != BuildingFunction::Anchor)
        })
        .collect()
}

/// The answer of `placement::search` for each plain kind, for both players.
/// `(kind, player 0's cell, player 1's cell, footprint)`.
type Placed = (Kind, Option<Cell>, Option<Cell>, (u32, u32));

fn placements(r: &Rig) -> Vec<Placed> {
    let o0 = Observation::build(&r.world, P0, &BTreeMap::new());
    let o1 = Observation::build(&r.world, P1, &BTreeMap::new());
    let mut l0 = Lens::new(&r.world, P0);
    let mut l1 = Lens::new(&r.world, P1);
    plain_kinds(&r.world)
        .into_iter()
        .map(|k| {
            let fp = r.world.building_table().get(k).unwrap().footprint;
            (
                k,
                placement::search(&mut r.s0.clone(), &o0, &mut l0, k),
                placement::search(&mut r.s1.clone(), &o1, &mut l1, k),
                (u32::from(fp.width), u32::from(fp.height)),
            )
        })
        .collect()
}

#[test]
fn the_placement_of_player_1_is_the_half_turn_of_player_0s_with_the_switch_on() {
    let on = placements(&rig(&[], true, false, false));
    let found = on.iter().filter(|p| p.1.is_some()).count();
    println!("placement on: {on:?}");
    assert!(found >= 1, "the check needs a kind that is placed: {on:?}");
    for (k, a, b, fp) in &on {
        assert_eq!(
            b.map(|c| turn_foot(c, *fp)),
            *a,
            "kind {k:?}: player 1 {b:?} is not the half turn of player 0 {a:?}"
        );
    }
}

#[test]
fn the_placement_with_the_switch_off_is_not_a_mirror() {
    // The control: the order (score, y, x) breaks ties north and west, so some kind lands differently.
    let off = placements(&rig(&[], false, false, false));
    println!("placement off: {off:?}");
    assert!(
        off.iter()
            .any(|(_, a, b, fp)| b.map(|c| turn_foot(c, *fp)) != *a),
        "no kind shows the compass bias, so the mirror test above proves nothing: {off:?}"
    );
}

/// Walls round the gather point of the facing rule (a 3 x 3 block on it, for both players), so that the search
/// has to go out to the rings, where the order of the cells matters.
fn gather_walls() -> Vec<(u32, u32)> {
    let first = facing::gather_point(
        (i64::from(A0.0), i64::from(A0.1)),
        (4, 4),
        (i64::from(W), i64::from(H)),
        i64::from(
            sf_data::ai_profile::AiProfile::embedded()
                .army
                .gather_distance,
        ),
    )
    .unwrap();
    let mut walls = Vec::new();
    for dy in -1..=1 {
        for dx in -1..=1 {
            walls.push(((first.0 + dx) as u32, (first.1 + dy) as u32));
        }
    }
    walls
}

fn gathers(r: &Rig) -> (Option<Cell>, Option<Cell>) {
    let o0 = Observation::build(&r.world, P0, &BTreeMap::new());
    let o1 = Observation::build(&r.world, P1, &BTreeMap::new());
    let l0 = Lens::new(&r.world, P0);
    let l1 = Lens::new(&r.world, P1);
    (
        army::ensure_gather(&mut r.s0.clone(), &o0, &l0),
        army::ensure_gather(&mut r.s1.clone(), &o1, &l1),
    )
}

#[test]
fn the_gather_cell_of_player_1_is_the_half_turn_of_player_0s_with_the_switch_on() {
    let (a, b) = gathers(&rig(&gather_walls(), false, true, false));
    println!("gather on: {a:?} {b:?}");
    let a = a.expect("player 0 finds a gather cell");
    assert_eq!(b, Some(turn_cell(a)));
    // The walls cover the point and the ring round it: the cell lies in ring 2 of the point (two cells away).
    let first = facing::gather_point(
        (i64::from(A0.0), i64::from(A0.1)),
        (4, 4),
        (i64::from(W), i64::from(H)),
        i64::from(
            sf_data::ai_profile::AiProfile::embedded()
                .army
                .gather_distance,
        ),
    )
    .unwrap();
    let ring = (i64::from(a.x) - first.0)
        .abs()
        .max((i64::from(a.y) - first.1).abs());
    assert!(ring >= 2, "ring {ring}");
}

#[test]
fn the_gather_cell_with_the_switch_off_is_not_a_mirror() {
    let (a, b) = gathers(&rig(&gather_walls(), false, false, false));
    println!("gather off: {a:?} {b:?}");
    assert!(a.is_some() && b.is_some());
    assert_ne!(b, a.map(turn_cell), "the control shows no bias");
}

fn legs(r: &Rig) -> (Vec<Cell>, Vec<Cell>) {
    let o0 = Observation::build(&r.world, P0, &BTreeMap::new());
    let o1 = Observation::build(&r.world, P1, &BTreeMap::new());
    (army::waypoints(&r.s0, &o0), army::waypoints(&r.s1, &o1))
}

#[test]
fn the_scout_legs_of_player_1_are_the_half_turn_of_player_0s_with_the_switch_on() {
    let (a, b) = legs(&rig(&[], false, false, true));
    println!("scout on: {a:?} {b:?}");
    assert_eq!(a.len(), 5);
    // The quarters turn with the map (the site legs are the other player's anchor cell, which is not the half turn
    // of the cell of the AI's own: an anchor is 4 x 4).
    let turned: Vec<Cell> = a[1..].iter().map(|&c| turn_cell(c)).collect();
    assert_eq!(turned, b[1..]);
    // The enemy's site first, then the quarter of the enemy, the one of the AI last.
    assert_eq!(a[0], Cell::new(A1.0, A1.1));
    assert_eq!(b[0], Cell::new(A0.0, A0.1));
    assert_eq!(a[1], Cell::new(71, 24));
    assert_eq!(a[4], Cell::new(24, 71));
}

#[test]
fn the_scout_legs_with_the_switch_off_are_the_compass_order_for_both_players() {
    let (a, b) = legs(&rig(&[], false, false, false));
    let quarters = [
        Cell::new(24, 24),
        Cell::new(72, 24),
        Cell::new(24, 72),
        Cell::new(72, 72),
    ];
    assert_eq!(a[1..], quarters);
    assert_eq!(b[1..], quarters);
    assert_ne!(
        b[1..].iter().map(|&c| turn_cell(c)).collect::<Vec<_>>(),
        a[1..],
        "the control shows no bias"
    );
}
