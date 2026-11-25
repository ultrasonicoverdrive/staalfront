//! WP13.1c-3a: repair, sale and the match outcome through save and restore. Worlds built through
//! `World::step` reach a structure being repaired, one being sold (at `sell_ticks` and at 1), a
//! defeat, a win and a draw; three players; a match that has not begun; repair and sale at zero
//! hit points (a world without destruction). Each round trips by `PartialEq`, resaves byte-equal
//! and goes on with the checksums of the uninterrupted run. Inside the crate to set values at their
//! bounds and one past them.

use sf_math::{Fx, Vec2};

use super::tests::settled;
use super::*;
use crate::buildings::{BuildingFunction, BuildingStats, BuildingTable};
use crate::command::Command;
use crate::destruction::DestructionConfig;
use crate::ids::per_tick;
use crate::outcome::{MatchResult, MatchRules, Verdict};
use crate::repair::RepairRules;
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::weapons::{Loadout, Mount, ProjectileKind, WeaponStats, WeaponTable};
use crate::world::WorldConfig;

pub(crate) const RIFLEMAN: Kind = Kind(1);
pub(crate) const BARN: Kind = Kind(3);
pub(crate) const HQ: Kind = Kind(6);
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
pub(crate) const SELL_TICKS: u32 = 10;
pub(crate) const BARN_HP: u32 = 100;

pub(crate) fn units() -> UnitTable {
    let structure = |kind, hp| UnitStats {
        kind,
        hp,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: 1,
            height: 1,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    };
    UnitTable::new(vec![
        UnitStats {
            kind: RIFLEMAN,
            hp: 50,
            movement_class: Some(0),
            speed_per_tick: per_tick(Fx::from_int(2)),
            turn_per_tick: 255,
            size: Fx::from_ratio(4, 10),
            footprint: None,
            weapon_range: Fx::from_int(6),
            turret_per_tick: 0,
        },
        structure(BARN, BARN_HP),
        structure(HQ, 40),
    ])
    .unwrap()
}

pub(crate) fn buildings() -> BuildingTable {
    let b = |kind, function| BuildingStats {
        kind,
        footprint: Footprint {
            width: 1,
            height: 1,
        },
        function,
        exit_side: None,
        // A barn has build ticks, so that a save can claim one under construction (refused with a
        // repair or a sale, so it has a cost too); placed by `PlaceStructure` it stands complete.
        build_ticks: if kind == BARN { 10 } else { 0 },
        cost: (kind == BARN).then_some(100),
        power: 0,
        factions: 1,
        requires: Vec::new(),
        build_radius: if function == BuildingFunction::Anchor {
            6
        } else {
            0
        },
    };
    BuildingTable::new(vec![
        b(BARN, BuildingFunction::None),
        b(HQ, BuildingFunction::Anchor),
    ])
    .unwrap()
}

pub(crate) fn weapons() -> WeaponTable {
    let rifle = WeaponStats {
        kind: 1,
        range: Fx::from_int(6),
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
    WeaponTable::new(
        vec![rifle],
        vec![Loadout {
            unit: RIFLEMAN,
            weapons: vec![1],
        }],
    )
    .unwrap()
}

pub(crate) const RULES: RepairRules = RepairRules {
    hp_per_tick: 1,
    full_cost_permille: 0,
    sell_ticks: SELL_TICKS,
    refund_permille: 500,
};

/// Units, weapons, buildings and repair; destruction and match rules for `players` if given.
pub(crate) fn fresh_for(players: Option<&[PlayerId]>, rules: RepairRules) -> World {
    fresh_sized(players, rules, 40)
}

/// [`fresh_for`] on a map 40 cells wide and `height` high.
pub(crate) fn fresh_sized(players: Option<&[PlayerId]>, rules: RepairRules, height: u32) -> World {
    let factions: Vec<(PlayerId, u8)> = (0..4).map(|p| (PlayerId(p), 1)).collect();
    let w = World::new(WorldConfig::new(7).with_map_size(40, height))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(weapons())
        .unwrap();
    let w = match players {
        Some(_) => w.with_destruction(DestructionConfig::DEFAULT).unwrap(),
        None => w,
    };
    let w = w
        .with_buildings(buildings())
        .unwrap()
        .with_player_factions(&factions)
        .unwrap()
        .with_repair(rules)
        .unwrap();
    match players {
        Some(p) => w.with_match(MatchRules::new(p).unwrap()).unwrap(),
        None => w,
    }
}

pub(crate) fn send(w: &mut World, tick: u64, c: Command) {
    w.schedule(Tick(tick), P0, c).unwrap();
}

pub(crate) fn place(w: &mut World, tick: u64, kind: Kind, owner: PlayerId, x: u32, y: u32) {
    let cell = crate::terrain::Cell::new(x, y);
    send(w, tick, Command::PlaceStructure { kind, cell, owner });
}

pub(crate) fn id(i: u32) -> EntityId {
    EntityId::new(i, 0)
}

/// A rifleman of `owner` at `(x, y)` ordered at `tick + 1` to attack slot `target`.
fn rifleman(w: &mut World, tick: u64, owner: PlayerId, x: i32, y: i32, me: u32, target: u32) {
    send(
        w,
        tick,
        Command::SpawnUnit {
            kind: RIFLEMAN,
            pos: Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF),
            owner,
        },
    );
    send(
        w,
        tick + 1,
        Command::Order {
            ids: vec![id(me)],
            order: Order::Attack(id(target)),
            queued: false,
        },
    );
}

/// Three players (each a construction centre: slots 0, 1, 2), a barn of player 0 (slot 3) that a
/// rifleman of player 1 (slot 4) shoots and is then removed, an idle rifleman of player 2 (slot 5);
/// the barn is repaired from tick 40; player 0 sells its construction centre at tick 70 (defeated
/// in that tick: it stops standing at the order) and its barn at 90; then with
/// `draw` players 1 and 2 sell theirs at tick 120 (a draw), else only player 1 does (player 2
/// wins).
fn match_world(draw: bool) -> World {
    let mut w = fresh_for(Some(&[P0, P1, P2]), RULES);
    for (k, p) in [P0, P1, P2].into_iter().enumerate() {
        place(&mut w, 0, HQ, p, 4 + 12 * k as u32, 30);
    }
    place(&mut w, 0, BARN, P0, 10, 10);
    rifleman(&mut w, 1, P1, 14, 10, 4, 3);
    // Slot 5: an idle rifleman of player 2 far away (a unit for the cases).
    send(
        &mut w,
        1,
        Command::SpawnUnit {
            kind: RIFLEMAN,
            pos: Vec2::new(Fx::from_int(30), Fx::from_int(4)),
            owner: P2,
        },
    );
    send(&mut w, 25, Command::Despawn { id: id(4) });
    let repair = |s| Command::Repair {
        structure: id(s),
        on: true,
    };
    send(&mut w, 40, repair(3));
    send(&mut w, 70, Command::Sell { structure: id(0) });
    send(&mut w, 90, Command::Sell { structure: id(3) });
    let sell = |w: &mut World, s: u32, p: PlayerId| {
        w.schedule(Tick(120), p, Command::Sell { structure: id(s) })
            .unwrap()
    };
    sell(&mut w, 1, P1);
    if draw {
        sell(&mut w, 2, P2);
    }
    w
}

fn round_trip_on(fresh: World, w: &World) -> World {
    let bytes = w.save().unwrap();
    let back = World::restore(fresh, &bytes).unwrap();
    assert_eq!(back.save().unwrap(), bytes, "a resave is the save");
    back
}

/// What a tick of the match world holds, for the claims.
#[derive(Default, Debug, PartialEq, Eq)]
struct Seen {
    repairing: bool,
    sale_full: bool,
    sale_one: bool,
    defeat_open: bool,
    won: bool,
    draw: bool,
    three: bool,
}

fn look(w: &World, seen: &mut Seen) {
    let e = &w.entities;
    let live = |i: usize| e.alive[i];
    seen.repairing |= (0..e.len()).any(|i| live(i) && e.repairing[i]);
    seen.sale_full |= (0..e.len()).any(|i| live(i) && e.sell_left[i] == SELL_TICKS);
    seen.sale_one |= (0..e.len()).any(|i| live(i) && e.sell_left[i] == 1);
    seen.defeat_open |= !w.defeats().is_empty() && w.match_result().is_none();
    seen.three |= w.match_players().len() == 3 && w.match_begun();
    match w.match_result().map(|r| r.verdict) {
        Some(Verdict::Won(_)) => seen.won = true,
        Some(Verdict::Draw) => seen.draw = true,
        None => {}
    }
}

/// The every-tick probe of the match world in every build, once ending in a win and once in a
/// draw: save, restore and resave after every tick, go on with the restored world and compare
/// every checksum with the uninterrupted run, and assert that each claimed state is present at
/// some save tick.
#[test]
fn repair_sale_and_the_outcome_restore_after_every_tick() {
    for draw in [false, true] {
        let ticks = 160;
        let mut straight = match_world(draw);
        let want: Vec<u64> = (0..ticks).map(|_| straight.step().checksum).collect();
        let mut w = match_world(draw);
        let mut seen = Seen::default();
        for (t, &sum) in want.iter().enumerate() {
            assert_eq!(w.step().checksum, sum, "draw {draw}, tick {t}");
            look(&w, &mut seen);
            let back = round_trip_on(fresh_for(Some(&[P0, P1, P2]), RULES), &w);
            assert_eq!(settled(back.clone()), settled(w.clone()), "tick {t}");
            w = back;
        }
        let want_seen = Seen {
            repairing: true,
            sale_full: true,
            sale_one: true,
            defeat_open: true,
            won: !draw,
            draw,
            three: true,
        };
        assert_eq!(seen, want_seen, "draw {draw}");
        let result = w.match_result().unwrap();
        assert_eq!(result.tick, Tick(120));
        assert_eq!(
            w.defeats(),
            if draw {
                &[(P0, Tick(70)), (P1, Tick(120)), (P2, Tick(120))][..]
            } else {
                &[(P0, Tick(70)), (P1, Tick(120))][..]
            }
        );
    }
}

/// A match that has not begun (only player 0 has a construction centre) round trips with
/// `begun` false.
#[test]
fn a_match_that_has_not_begun_round_trips() {
    let fresh = || fresh_for(Some(&[P0, P1]), RULES);
    let mut w = fresh();
    place(&mut w, 0, HQ, P0, 4, 4);
    for _ in 0..5 {
        w.step();
    }
    assert!(!w.match_begun() && w.entities.alive[0]);
    let mut back = round_trip_on(fresh(), &w);
    assert_eq!(settled(back.clone()), settled(w.clone()));
    // Player 1 places its construction centre after the restore: both worlds begin the match in
    // the same tick and go on equal.
    for x in [&mut w, &mut back] {
        let at = x.tick.0;
        x.schedule(
            Tick(at),
            P0,
            Command::PlaceStructure {
                kind: HQ,
                cell: crate::terrain::Cell::new(20, 20),
                owner: P1,
            },
        )
        .unwrap();
    }
    for t in 0..20 {
        assert_eq!(back.step().checksum, w.step().checksum, "tick {t}");
    }
    assert!(w.match_begun() && back.match_begun());
}

/// Without destruction nothing removes a structure at zero hit points: a sale goes on at zero, and
/// a repair ordered in the tick of the shot that takes the structure to zero keeps its flag to the
/// end of that tick (the next `repair_step` clears it). Both reached through `World::step`, both
/// round trip and go on equal.
#[test]
fn repair_and_sale_at_zero_hit_points_round_trip() {
    // A sale long enough for the riflemen to bring the barn to zero first.
    let rules = RepairRules {
        sell_ticks: 100,
        ..RULES
    };
    let fresh = || fresh_for(None, rules);
    // Slot 0 the barn that is repaired, 1 the one that is sold, 2 and 3 the riflemen.
    let mut w = fresh();
    place(&mut w, 0, BARN, P0, 10, 10);
    place(&mut w, 0, BARN, P0, 10, 20);
    rifleman(&mut w, 1, P1, 14, 10, 2, 0);
    rifleman(&mut w, 1, P1, 14, 20, 3, 1);
    send(&mut w, 3, Command::Sell { structure: id(1) });
    let mut repaired_at_zero = None;
    let mut sold_at_zero = None;
    for _ in 0..80 {
        // Try a repair of slot 0 in this tick on a copy: keep it if the tick leaves the flag on a
        // barn at zero hit points.
        if repaired_at_zero.is_none()
            && w.entities.len() > 3
            && w.entities.hp[0] > 0
            && w.entities.hp[0] < BARN_HP
        {
            let mut trial = w.clone();
            let now = trial.tick.0;
            send(
                &mut trial,
                now,
                Command::Repair {
                    structure: id(0),
                    on: true,
                },
            );
            trial.step();
            if trial.entities.repairing[0] && trial.entities.hp[0] == 0 {
                repaired_at_zero = Some(trial);
            }
        }
        w.step();
        if sold_at_zero.is_none() && w.entities.alive[1] && w.entities.hp[1] == 0 {
            assert!(w.entities.sell_left[1] > 0);
            sold_at_zero = Some(w.clone());
        }
    }
    for (what, w) in [
        ("repair", repaired_at_zero.expect("a repair at zero")),
        ("sale", sold_at_zero.expect("a sale at zero")),
    ] {
        let mut back = round_trip_on(fresh(), &w);
        assert_eq!(settled(back.clone()), settled(w.clone()), "{what}");
        let mut straight = w;
        for t in 0..15 {
            assert_eq!(back.step().checksum, straight.step().checksum, "{what} {t}");
        }
    }
}

/// Every build, every tick: an own structure given a repair and a sale in the same tick, in both
/// orders (repair then sale: the sale clears the flag; sale then repair: the repair is refused),
/// and a structure destroyed while being repaired and one destroyed while being sold (the freed
/// slot holds neither). Saved, restored and resaved after every tick, going on with the restored
/// world: every checksum is the uninterrupted run's.
#[test]
fn repair_and_sale_together_and_under_fire_restore_after_every_tick() {
    // A sale longer than the barn lasts under fire (2 hit points a tick against 1 repaired).
    let rules = RepairRules {
        sell_ticks: 200,
        ..RULES
    };
    let fresh = || fresh_for(Some(&[P0, P1]), rules);
    // Slots 0 and 1 the construction centres, 2 to 5 barns of player 0, 6 to 9 riflemen of
    // player 1 on them; the two on barns 2 and 3 are removed at tick 25 by a despawn.
    let build = || {
        let mut w = fresh();
        place(&mut w, 0, HQ, P0, 4, 30);
        place(&mut w, 0, HQ, P1, 34, 30);
        for (x, y) in [(10, 10), (10, 20), (20, 10), (20, 20)] {
            place(&mut w, 0, BARN, P0, x, y);
        }
        for (k, (x, y)) in [(14, 10), (14, 20), (24, 10), (24, 20)]
            .into_iter()
            .enumerate()
        {
            rifleman(&mut w, 1, P1, x, y, 6 + k as u32, 2 + k as u32);
        }
        send(&mut w, 25, Command::Despawn { id: id(6) });
        send(&mut w, 25, Command::Despawn { id: id(7) });
        let repair = |s| Command::Repair {
            structure: id(s),
            on: true,
        };
        let sell = |s| Command::Sell { structure: id(s) };
        for c in [repair(2), sell(2), sell(3), repair(3), repair(4), sell(5)] {
            send(&mut w, 30, c);
        }
        w
    };
    let ticks = 160;
    let mut straight = build();
    let want: Vec<u64> = (0..ticks).map(|_| straight.step().checksum).collect();
    let mut w = build();
    let mut repaired_before_removal = false;
    let mut sold_before_removal = false;
    for (t, &sum) in want.iter().enumerate() {
        assert_eq!(w.step().checksum, sum, "tick {t}");
        let e = &w.entities;
        if t == 30 {
            assert!(
                e.hp[2] < BARN_HP && e.hp[3] < BARN_HP,
                "damaged before the orders"
            );
            assert_eq!(
                (e.sell_left[2], e.repairing[2]),
                (200, false),
                "repair then sale"
            );
            assert_eq!(
                (e.sell_left[3], e.repairing[3]),
                (200, false),
                "sale then repair"
            );
            assert!(e.repairing[4] && e.sell_left[5] == 200);
        }
        repaired_before_removal |= e.alive[4] && e.repairing[4] && e.hp[4] <= 2;
        sold_before_removal |= e.alive[5] && e.sell_left[5] > 0 && e.hp[5] <= 20;
        let back = round_trip_on(fresh(), &w);
        assert_eq!(settled(back.clone()), settled(w.clone()), "tick {t}");
        w = back;
    }
    assert!(repaired_before_removal && sold_before_removal);
    let e = &w.entities;
    for s in [4, 5] {
        assert!(!e.alive[s], "barn {s} destroyed");
        assert_eq!((e.repairing[s], e.sell_left[s]), (false, 0), "slot {s}");
    }
}

type Case = (&'static str, &'static str, fn(&mut World, bool));

fn oc(w: &mut World) -> &mut crate::outcome::Outcome {
    w.outcome.as_deref_mut().unwrap()
}

/// One case per restore check of WP13.1c-3a, on the match world after tick 100 (player 0 defeated
/// at 70, its barn being sold since 90, three players, undecided): the value at its bound
/// restores, one past it is refused with the message.
fn cases() -> Vec<Case> {
    vec![
        (
            "a sale the repair stage never leaves",
            "sell_left",
            |w, at| w.entities.sell_left[3] = SELL_TICKS + u32::from(!at),
        ),
        (
            "a sale the repair stage never leaves",
            "sell_left on a unit",
            |w, at| {
                // At the bound: on a structure (slot 2, player 2's construction centre).
                let slot = if at { 2 } else { 5 };
                w.entities.sell_left[slot] = 1;
            },
        ),
        (
            "a sale the repair stage never leaves",
            "sell_left while repairing",
            |w, at| {
                // Below full hit points, so that only the sale check refuses it.
                w.entities.hp[3] = BARN_HP - 1;
                w.entities.repairing[3] = !at;
            },
        ),
        (
            "a repair the repair stage never leaves",
            "repairing at full hit points",
            |w, at| {
                // Slot 2 at full hit points (40).
                w.entities.hp[2] = 40 - u32::from(at);
                w.entities.repairing[2] = true;
            },
        ),
        (
            "a repair the repair stage never leaves",
            "repairing on a unit",
            |w, at| {
                // Both below full hit points: the unit is refused for not being a structure.
                let slot = if at { 2 } else { 5 };
                w.entities.hp[2] = 39;
                w.entities.hp[5] = 1;
                w.entities.repairing[slot] = true;
            },
        ),
        (
            "a defeat the match stage never records",
            "defeat tick",
            |w, at| {
                let now = w.tick.0;
                oc(w).defeats[0].1 = Tick(now - u64::from(at));
            },
        ),
        (
            "a defeat the match stage never records",
            "defeat player",
            |w, at| oc(w).defeats[0].0 = if at { P0 } else { PlayerId(3) },
        ),
        (
            "a defeat the match stage never records",
            "defeat twice",
            |w, at| {
                oc(w).defeats.push((if at { P1 } else { P0 }, Tick(80)));
                // Two defeats of three players: decided, so the result is the win of player 2 (`P2`).
                oc(w).result = Some(MatchResult {
                    tick: Tick(80),
                    verdict: Verdict::Won(P2),
                });
            },
        ),
        ("the defeats are out of order", "defeat order", |w, at| {
            let o = oc(w);
            o.defeats = if at {
                vec![(P0, Tick(70)), (P1, Tick(70))]
            } else {
                vec![(P1, Tick(70)), (P0, Tick(70))]
            };
            o.result = Some(MatchResult {
                tick: Tick(70),
                verdict: Verdict::Won(P2),
            });
        }),
        ("a defeat before the match began", "begun", |w, at| {
            let o = oc(w);
            o.begun = false;
            if at {
                o.defeats.clear();
            }
        }),
        (
            "a match result the match stage never leaves",
            "undecided",
            |w, at| {
                // One defeat of three players: undecided. Two without a result is refused.
                if !at {
                    oc(w).defeats.push((P1, Tick(80)));
                }
            },
        ),
        (
            "a match result the match stage never leaves",
            "won tick",
            |w, at| {
                let o = oc(w);
                o.defeats.push((P1, Tick(80)));
                o.result = Some(MatchResult {
                    tick: Tick(if at { 80 } else { 81 }),
                    verdict: Verdict::Won(P2),
                });
            },
        ),
        (
            "a match result the match stage never leaves",
            "won by a defeated player",
            |w, at| {
                let o = oc(w);
                o.defeats.push((P1, Tick(80)));
                o.result = Some(MatchResult {
                    tick: Tick(80),
                    verdict: Verdict::Won(if at { P2 } else { P1 }),
                });
            },
        ),
        (
            "a match result the match stage never leaves",
            "won with too few defeats",
            |w, at| {
                let o = oc(w);
                if at {
                    o.defeats.push((P1, Tick(80)));
                }
                o.result = Some(MatchResult {
                    tick: if at { Tick(80) } else { Tick(70) },
                    verdict: Verdict::Won(P2),
                });
            },
        ),
        (
            "a match result the match stage never leaves",
            "draw",
            |w, at| {
                // The last two fall together: a draw. Apart, the one left would have won.
                let o = oc(w);
                o.defeats.push((P1, Tick(80)));
                o.defeats.push((P2, Tick(if at { 80 } else { 81 })));
                o.result = Some(MatchResult {
                    tick: Tick(if at { 80 } else { 81 }),
                    verdict: Verdict::Draw,
                });
            },
        ),
        (
            "a repair the repair stage never leaves",
            "repairing under construction",
            |w, at| {
                w.entities.hp[3] = BARN_HP - 1;
                w.entities.sell_left[3] = 0;
                w.entities.repairing[3] = true;
                w.entities.build_left[3] = u32::from(!at);
            },
        ),
        (
            "a sale the repair stage never leaves",
            "sell_left under construction",
            |w, at| w.entities.build_left[3] = u32::from(!at),
        ),
        (
            "a sale the repair stage never leaves",
            "sell_left without a table entry",
            |w, at| {
                // Past the bound slot 2 keeps its cell but loses its table entry (a legacy
                // `Place` entity, at zero hit points): only `def != NO_DEF` refuses the sale.
                w.entities.sell_left[2] = 1;
                if !at {
                    w.entities.def[2] = NO_DEF;
                    w.entities.hp[2] = 0;
                }
            },
        ),
        (
            "a match result the match stage never leaves",
            "won by a player not in the match",
            |w, at| {
                let o = oc(w);
                o.defeats.push((P1, Tick(80)));
                o.result = Some(MatchResult {
                    tick: Tick(80),
                    verdict: Verdict::Won(if at { P2 } else { PlayerId(3) }),
                });
            },
        ),
        (
            "a match result the match stage never leaves",
            "draw with too few defeats",
            |w, at| {
                // The last two fall together; past the bound one player is left undefeated.
                let o = oc(w);
                o.defeats = if at {
                    vec![(P0, Tick(80)), (P1, Tick(80)), (P2, Tick(80))]
                } else {
                    vec![(P0, Tick(80)), (P1, Tick(80))]
                };
                o.result = Some(MatchResult {
                    tick: Tick(80),
                    verdict: Verdict::Draw,
                });
            },
        ),
        ("unknown match result", "result tag", |w, at| {
            if !at {
                // Replaced in the bytes below.
                oc(w).result = Some(MatchResult {
                    tick: Tick(0x77),
                    verdict: Verdict::Draw,
                });
            }
        }),
    ]
}

fn mid_match() -> World {
    let mut w = match_world(false);
    for _ in 0..100 {
        w.step();
    }
    // The state the cases start from.
    assert_eq!(w.defeats(), &[(P0, Tick(70))]);
    assert!(w.entities.sell_left[3] > 0 && w.entities.alive[5] && w.match_result().is_none());
    w
}

/// The bytes of `w`; for the result-tag case the tag byte of the draw (2) is made 3 (unknown).
fn bytes_of(name: &str, w: &World) -> Vec<u8> {
    let mut bytes = w.save().unwrap();
    if name == "result tag" && w.match_result().is_some() {
        let (_, start, len) = *super::tests::sections(&bytes)
            .iter()
            .find(|s| s.0 == b'G')
            .unwrap();
        // The section ends with the tag and the tick (8 bytes).
        let tag = start + len - 9;
        assert_eq!(bytes[tag], 2);
        bytes[tag] = 3;
    }
    bytes
}

#[test]
fn every_repair_and_outcome_bound_restores_and_one_past_it_is_refused() {
    let base = mid_match();
    let fresh = || fresh_for(Some(&[P0, P1, P2]), RULES);
    for (what, name, change) in cases() {
        let mut ok = base.clone();
        change(&mut ok, true);
        let mut back = World::restore(fresh(), &bytes_of(name, &ok))
            .unwrap_or_else(|e| panic!("{name} at its bound: {e}"));
        for _ in 0..30 {
            back.step();
        }
        let mut bad = base.clone();
        change(&mut bad, false);
        assert_eq!(
            World::restore(fresh(), &bytes_of(name, &bad)),
            Err(SaveError::Corrupt(what)),
            "{name}"
        );
    }
}

/// A repair or a sale in a world without repair rules is refused (nothing there sets them).
#[test]
fn repair_or_sale_without_repair_rules_is_refused() {
    let w = mid_match();
    let mut bare = w.clone();
    bare.repair = None;
    let mut fresh = fresh_for(Some(&[P0, P1, P2]), RULES);
    fresh.repair = None;
    assert_eq!(
        World::restore(fresh.clone(), &bare.save().unwrap()),
        Err(SaveError::Corrupt("a sale the repair stage never leaves"))
    );
    // A repair: player 2's construction centre below full hit points, the sale taken away.
    bare.entities.sell_left[3] = 0;
    bare.entities.hp[2] = 39;
    bare.entities.repairing[2] = true;
    assert_eq!(
        World::restore(fresh.clone(), &bare.save().unwrap()),
        Err(SaveError::Corrupt("a repair the repair stage never leaves"))
    );
    // Without either it restores.
    bare.entities.repairing[2] = false;
    assert!(World::restore(fresh, &bare.save().unwrap()).is_ok());
}

/// The match rules enter the rules digest by number: another player list (other players, or one
/// more) is other rules; so is each repair number, and a world without match rules.
#[test]
fn the_match_and_repair_numbers_are_in_the_digest() {
    let base = fresh_for(Some(&[P0, P1, P2]), RULES).rules_digest();
    let others = [
        fresh_for(Some(&[P0, P1]), RULES),
        fresh_for(Some(&[P0, P1, PlayerId(3)]), RULES),
        fresh_for(Some(&[P0, P1, P2, PlayerId(3)]), RULES),
        fresh_for(
            Some(&[P0, P1, P2]),
            RepairRules {
                hp_per_tick: 2,
                ..RULES
            },
        ),
        fresh_for(
            Some(&[P0, P1, P2]),
            RepairRules {
                full_cost_permille: 1,
                ..RULES
            },
        ),
        fresh_for(
            Some(&[P0, P1, P2]),
            RepairRules {
                sell_ticks: SELL_TICKS + 1,
                ..RULES
            },
        ),
        fresh_for(
            Some(&[P0, P1, P2]),
            RepairRules {
                refund_permille: 501,
                ..RULES
            },
        ),
    ];
    for (k, w) in others.iter().enumerate() {
        assert_ne!(w.rules_digest(), base, "change {k}");
    }
    let mut no_match = fresh_for(Some(&[P0, P1, P2]), RULES);
    no_match.outcome = None;
    assert_ne!(no_match.rules_digest(), base, "no match rules");
    // And a save is refused on other rules.
    let bytes = mid_match().save().unwrap();
    assert_eq!(
        World::restore(fresh_for(Some(&[P0, P1]), RULES), &bytes),
        Err(SaveError::Rules)
    );
}

/// The coverage of the section and the columns: every name `save_coverage` gives for them is set
/// by one of the worlds above.
#[test]
fn the_repair_and_outcome_coverage_names_are_reached() {
    let mut names = Vec::new();
    for draw in [false, true] {
        let mut w = match_world(draw);
        for _ in 0..160 {
            w.step();
            for n in w.save_coverage() {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
    }
    for n in [
        "repairing",
        "sell_left",
        "outcome.begun",
        "outcome.defeats",
        "outcome.result.won",
        "outcome.result.draw",
    ] {
        assert!(names.contains(&n), "{n}: {names:?}");
    }
}
