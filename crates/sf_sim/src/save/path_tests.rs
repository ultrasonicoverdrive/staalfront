//! Tests of the sections WP13.1b saves: the path service, the flowfields, the formations and the
//! turn carry (`docs/rules/save.md`). The world is a walled map with a small path budget, so that
//! routes are planned over several ticks, a group order big enough for a flowfield and a
//! formation, trucks that turn in arcs, and structures placed one at a time (and one taken away)
//! while all that goes on.

use sf_math::{Fx, Vec2};

use super::*;
use crate::movement::{ClassCosts, Cost, MovementTable};
use crate::orders::Order;
use crate::path::ReplanConfig;
use crate::terrain::{Cell, Terrain, TerrainDef, TerrainFlags, TerrainId};
use crate::turning::Turning;
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::world::{World, WorldConfig};

const FOOT: Kind = Kind(10);
const TRUCK: Kind = Kind(11);
const HQ: Kind = Kind(20);
const W: u32 = 96;
const H: u32 = 64;
/// The smallest budget a world takes, so that a route can take more than a tick to plan.
const BUDGET: u32 = crate::path::MIN_NODE_BUDGET;

fn unit(kind: Kind, class: u16) -> UnitStats {
    UnitStats {
        kind,
        hp: 100,
        movement_class: Some(class),
        speed_per_tick: Fx::from_ratio(1, 4),
        turn_per_tick: 8,
        size: Fx::from_ratio(1, 2),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn table() -> UnitTable {
    UnitTable::new(vec![
        unit(FOOT, 0),
        unit(TRUCK, 1),
        UnitStats {
            kind: HQ,
            hp: 400,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ONE,
            footprint: Some(Footprint {
                width: 2,
                height: 2,
            }),
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
    ])
    .unwrap()
}

/// Grass (0) and rock (1): two walls with gaps.
fn terrain() -> Terrain {
    let mut ids = vec![0u8; (W * H) as usize];
    for y in 0..H {
        if !(10..13).contains(&y) && !(48..52).contains(&y) {
            ids[(y * W + 36) as usize] = 1;
        }
        if !(28..33).contains(&y) {
            ids[(y * W + 66) as usize] = 1;
        }
    }
    let defs = [
        TerrainDef {
            id: TerrainId(0),
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: TerrainId(1),
            flags: TerrainFlags::default(),
        },
    ];
    Terrain::from_layer(W, H, &ids, &defs).unwrap()
}

fn movement() -> MovementTable {
    let class = |class| ClassCosts {
        class,
        costs: vec![
            (TerrainId(0), Cost::Speed(1000)),
            (TerrainId(1), Cost::Impassable),
        ],
    };
    MovementTable::new(vec![class(0), class(1)])
        .unwrap()
        .with_turning(vec![(1, Turning::arc(Fx::from_int(2), 16, 250))])
        .unwrap()
}

/// The tick-0 world the host builds: the same every time.
fn fresh() -> World {
    World::with_terrain(WorldConfig::new(7).with_map_size(W, H), terrain())
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(movement())
        .unwrap()
        .with_path_budget(BUDGET)
        .unwrap()
}

fn p(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

/// Twelve foot soldiers ordered as one group across both walls (a flowfield and a formation),
/// six trucks ordered one by one (their own routes, arc turns), and three structures placed one at
/// a time in their way, the first taken away again.
fn moving(mut w: World) -> World {
    let s = |w: &mut World, t: u64, c: Command| w.schedule(Tick(t), PlayerId(0), c).unwrap();
    for n in 0..12 {
        s(
            &mut w,
            0,
            Command::SpawnUnit {
                kind: FOOT,
                pos: p(6 + n % 4, 20 + n / 4),
                owner: PlayerId(0),
            },
        );
    }
    for n in 0..6 {
        s(
            &mut w,
            0,
            Command::SpawnUnit {
                kind: TRUCK,
                pos: p(6 + 3 * n, 42),
                owner: PlayerId(0),
            },
        );
    }
    let id = |i: u32| EntityId::new(i, 0);
    s(
        &mut w,
        1,
        Command::Order {
            ids: (0..12).map(id).collect(),
            order: Order::Move(p(84, 30)),
            queued: false,
        },
    );
    for n in 0..6u32 {
        s(
            &mut w,
            2 + u64::from(n),
            Command::Order {
                ids: vec![id(12 + n)],
                order: Order::Move(p(80 + n as i32, 8 + 8 * n as i32)),
                queued: false,
            },
        );
    }
    // Structures one at a time: two inside sectors, one across the border of two sectors (x 47 and
    // 48; sectors are 16 cells), one in the bottom-right corner sector of the map.
    for (t, cell) in PLACEMENTS {
        s(
            &mut w,
            t,
            Command::PlaceStructure {
                kind: HQ,
                cell,
                owner: PlayerId(0),
            },
        );
    }
    // The first structure (slot 18) and the one in the corner (slot 22) are taken away again: their
    // sectors are rebuilt once more.
    s(&mut w, 130, Command::Despawn { id: id(18) });
    s(&mut w, 150, Command::Despawn { id: id(22) });
    w
}

/// When and where `moving` places its structures (slots 18 to 22 in this order).
const PLACEMENTS: [(u64, Cell); 5] = [
    (9, Cell::new(50, 22)),
    (41, Cell::new(50, 34)),
    (60, Cell::new(47, 18)),
    (77, Cell::new(76, 26)),
    (90, Cell::new(94, 62)),
];

/// The world with what a restore does not bring back put equal on both sides (`settled` of the
/// core tests): the reports and stats of the last tick, the interpolation of facings and turrets,
/// the checksum history.
fn settled(mut w: World) -> World {
    let e = &mut w.entities;
    e.prev_facing.clone_from(&e.facing);
    e.prev_turret.clone_from(&e.turret);
    w.stalled.clear();
    w.completions.clear();
    w.placement.rejections.clear();
    w.destruction.deaths.clear();
    w.pathing.clear_stats();
    w.crowd.stats = Default::default();
    w.formations.stats = Default::default();
    w.history.clear();
    w.history_base = w.tick;
    w
}

/// A state a test claims a save carries across: its name and when it is there.
type Claim = (&'static str, fn(&SaveContents) -> bool);

const CLAIMS: [Claim; 6] = [
    ("a route being planned", |c| c.path_job),
    ("a flowfield being built", |c| c.field_building),
    ("a flowfield", |c| c.fields > 0),
    ("a formation", |c| c.formations > 0),
    ("a turn carry", |c| c.turning > 0),
    ("routes", |c| c.routes >= 6),
];

/// Saves after every one of the first 60 ticks and every seventh after that, and for each claim at
/// up to four ticks where it holds (at least three apart). At every save: the restored world is
/// equal, its resave is the save, its graphs are the original's; at a claim's save ticks the
/// claimed state is in the world and in the restored one.
#[test]
fn a_moving_world_round_trips_at_every_save_tick() {
    let mut w = moving(fresh());
    let mut made = [0u32; CLAIMS.len()];
    let mut last_at = [0u64; CLAIMS.len()];
    for t in 1..=320u64 {
        w.step();
        let now = w.save_contents();
        let due: Vec<usize> = (0..CLAIMS.len())
            .filter(|&k| made[k] < 4 && t >= last_at[k] + 3 && (CLAIMS[k].1)(&now))
            .collect();
        if !(t <= 60 || t % 7 == 0 || !due.is_empty()) {
            continue;
        }
        let bytes = w.save().unwrap();
        let back = World::restore(fresh(), &bytes).unwrap();
        assert!(
            back.pathing.same_graphs(&w.pathing),
            "the rebuilt graphs after {t} ticks"
        );
        assert_eq!(back.save().unwrap(), bytes, "resave after {t} ticks");
        assert_eq!(back.checksum(), w.checksum(), "checksum after {t} ticks");
        assert_eq!(back.save_contents(), now, "contents after {t} ticks");
        for k in due {
            let (what, holds) = CLAIMS[k];
            assert!(
                holds(&now) && holds(&back.save_contents()),
                "{what} after {t} ticks"
            );
            made[k] += 1;
            last_at[k] = t;
        }
        assert_eq!(settled(back), settled(w.clone()), "after {t} ticks");
    }
    for (k, (what, _)) in CLAIMS.iter().enumerate() {
        assert!(made[k] >= 2, "{what}: {} saves", made[k]);
    }
}

/// For each claim, a save at the first tick from 5 on where it holds (asserted there, in the world
/// and in the restored one), then the run goes on: every checksum equals the uninterrupted run.
/// Also a chain of saves every 13 ticks, each world restored from the one before, with the
/// contents equal at every link.
#[test]
fn a_moving_world_continues_like_the_original() {
    let total = 400;
    let mut straight = moving(fresh());
    for _ in 0..total {
        straight.step();
    }
    for (what, holds) in CLAIMS {
        let mut w = moving(fresh());
        for _ in 0..5 {
            w.step();
        }
        while !holds(&w.save_contents()) {
            w.step();
            assert!(w.tick.0 < total, "{what} never holds");
        }
        let n = w.tick.0;
        let mut back = World::restore(fresh(), &w.save().unwrap()).unwrap();
        assert!(
            holds(&back.save_contents()),
            "{what} restored after {n} ticks"
        );
        for _ in n..total {
            back.step();
        }
        assert_eq!(
            back.checksum_history(),
            &straight.checksum_history()[n as usize..],
            "saved with {what} after {n} ticks"
        );
    }
    let mut chain = moving(fresh());
    for t in 1..=total {
        chain.step();
        if t % 13 == 0 {
            let start = chain.checksum_history_start().0 as usize;
            assert_eq!(
                chain.checksum_history(),
                &straight.checksum_history()[start..t as usize],
                "the chain up to tick {t}"
            );
            let contents = chain.save_contents();
            chain = World::restore(fresh(), &chain.save().unwrap()).unwrap();
            assert_eq!(
                chain.save_contents(),
                contents,
                "the chain link at tick {t}"
            );
        }
    }
    assert_eq!(chain.checksum(), straight.checksum());
}

/// Risk 1 of the design memo: the graphs a restore rebuilds in one go equal the ones the original
/// changed one placement at a time, also for a structure across a sector border (portal runs on
/// that border are split) and one in the corner sector of the map, and after two were taken away.
/// And the rebuild is needed: the fresh world's graphs are not those.
#[test]
fn rebuilt_graphs_equal_the_ones_placements_made() {
    let mut w = moving(fresh());
    let mut checked = 0;
    let after: Vec<u64> = PLACEMENTS
        .iter()
        .map(|&(t, _)| t + 1)
        .chain([131, 151])
        .collect();
    for t in 1..=160 {
        w.step();
        if t == 91 {
            assert!(
                w.is_occupied(Cell::new(95, 63)),
                "the corner structure stands"
            );
            assert!(
                w.is_occupied(Cell::new(48, 19)),
                "the border structure stands"
            );
        }
        if after.contains(&t) {
            let back = World::restore(fresh(), &w.save().unwrap()).unwrap();
            assert!(back.pathing.same_graphs(&w.pathing), "after tick {t}");
            assert!(
                !fresh().pathing.same_graphs(&w.pathing),
                "the structures changed the graphs by tick {t}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 7);
    for &(_, cell) in &PLACEMENTS[1..4] {
        assert!(w.is_occupied(cell), "{cell:?} stands");
    }
    assert_eq!(w.occupied_cells(), 12, "three structures stand at the end");
    // The border one really lies across two sectors, the corner one in the last sector.
    let s = |x, y| w.pathing.sector_of(Cell::new(x, y)).unwrap();
    assert_ne!(s(47, 18), s(48, 18));
    assert_eq!(
        s(95, 63) as usize + 1,
        (W as usize / 16) * (H as usize / 16)
    );
}
/// A world whose save holds a job in flight and a flowfield being built, for the corruption
/// cases: the first such tick.
fn mid_flight() -> World {
    let mut w = moving(fresh());
    loop {
        w.step();
        let c = w.save_contents();
        if c.path_job && w.pathing.fields().scratch.is_some() && c.formations > 0 {
            return w;
        }
        assert!(w.tick.0 < 400, "no tick with a job in flight and a field");
    }
}

type Corruption = (&'static str, fn(&mut World));

/// One impossible world per check of the sections of WP13.1b, made by changing a valid world
/// before it is saved (`save` does not judge what it writes), with the exact error.
fn corruptions() -> Vec<Corruption> {
    vec![
        ("a job counter the planner never reaches", |w| {
            w.pathing.corrupt_for_test(0)
        }),
        ("a job for a slot or graph that does not exist", |w| {
            let n = w.entities.len() as u32;
            w.pathing.set_slots_for_test(Some(n), None)
        }),
        ("a job's ready leg has no cells", |w| {
            w.pathing.corrupt_for_test(2)
        }),
        ("a job's smoothing is past its route", |w| {
            w.pathing.corrupt_for_test(3)
        }),
        ("the planner's memory names a cell off the map", |w| {
            w.pathing.corrupt_for_test(4)
        }),
        ("the planner's memory names a node out of range", |w| {
            w.pathing.corrupt_for_test(5)
        }),
        ("a local search parent", |w| w.pathing.corrupt_for_test(6)),
        ("a search heap element", |w| w.pathing.corrupt_for_test(7)),
        ("an abstract search parent", |w| {
            w.pathing.corrupt_for_test(8)
        }),
        (
            "the path service starts at a slot it never looked at",
            |w| {
                // The first slot it can never have left: the count of slots it looks at.
                let n = w.entities.route.len().min(w.entities.len()) as u32;
                w.pathing.set_slots_for_test(None, Some(n))
            },
        ),
        ("sector epochs of another map", |w| {
            w.pathing.corrupt_for_test(10)
        }),
        ("a field counter at zero", |w| {
            w.pathing.corrupt_for_test(11)
        }),
        ("more claims or looks left than a tick gives", |w| {
            w.pathing.corrupt_for_test(12)
        }),
        ("the field builder's memory out of range", |w| {
            w.pathing.corrupt_for_test(13)
        }),
        ("a corridor search node", |w| w.pathing.corrupt_for_test(14)),
        ("a job's search does not fit its stage", |w| {
            w.pathing.corrupt_for_test(15)
        }),
        ("a field build out of range", |w| {
            w.pathing.corrupt_for_test(17)
        }),
        ("a field build's search does not fit its stage", |w| {
            w.pathing.corrupt_for_test(18)
        }),
        ("a route names a cell off the map", |w| {
            w.entities.route[12].want = W * H
        }),
        ("a route overlay", |w| {
            w.entities.route[12].overlay = crate::path::Overlay::from_cells(&[5, 3])
        }),
        ("a route names a portal that does not exist", |w| {
            w.entities.route[12].abs_push(u32::MAX);
        }),
        ("more routes than slots", |w| {
            let n = w.entities.len();
            w.entities.route[n].state = crate::path::RouteState::Wanted;
        }),
        ("a field direction", |w| {
            w.pathing.fields_mut().entries[0].dir[0] = 9
        }),
        ("a field region", |w| {
            w.pathing.fields_mut().entries[0].sectors.push(0);
        }),
        ("a field's arrays are not the size of its region", |w| {
            w.pathing.fields_mut().entries[0].cost.push(0);
        }),
        ("a field names a graph or cell that does not exist", |w| {
            w.pathing.fields_mut().entries[0].graph = 2
        }),
        ("a field has more slots than it hands out", |w| {
            let f = &mut w.pathing.fields_mut().entries[0];
            f.taken.resize(crate::path::MAX_FIELD_SLOTS + 1, 0);
        }),
        ("a field build out of range", |w| {
            w.pathing
                .fields_mut()
                .cur
                .get_or_insert(crate::path::field_cur_for_test())
                .entry = 9
        }),
        ("the formation counter at zero", |w| {
            w.formations.corrupt_for_test(0)
        }),
        ("a formation counter the update never reaches", |w| {
            w.formations.corrupt_for_test(1)
        }),
        ("a formation counter the update never reaches", |w| {
            w.formations.corrupt_for_test(2)
        }),
        ("a formation's members", |w| {
            w.formations.corrupt_for_test(3)
        }),
        ("a formation number", |w| w.formations.corrupt_for_test(4)),
        ("a formation counter the update never reaches", |w| {
            w.formations.corrupt_for_test(5)
        }),
        ("a formation's members", |w| {
            w.formations.corrupt_for_test(6)
        }),
        ("a membership of no formation", |w| {
            w.entities.form[13].serial = 77
        }),
        ("a turn carry of a whole step or more", |w| {
            w.entities.turn_carry[12] = 1000
        }),
        ("an entity stood outside the map", |w| {
            w.entities.prev_pos[12] = p(-1, 4)
        }),
    ]
}

#[test]
fn every_impossible_path_or_formation_state_is_corrupt() {
    let base = mid_flight();
    let bytes = base.save().unwrap();
    World::restore(fresh(), &bytes).expect("the base world restores");
    for (k, (what, change)) in corruptions().into_iter().enumerate() {
        let mut w = base.clone();
        change(&mut w);
        let bytes = w.save().expect("save does not judge");
        assert_eq!(
            World::restore(fresh(), &bytes),
            Err(SaveError::Corrupt(what)),
            "case {k}"
        );
    }
}

/// At the bounds the simulation reaches, a world restores and steps: the turn carry one below a
/// step, the formation counters at their most.
#[test]
fn path_and_formation_counters_at_their_maximum_restore_and_step() {
    let mut w = mid_flight();
    w.entities.turn_carry[12] = 999;
    w.formations.set_counters_to_maximum_for_test();
    // Counters that only count up, at their most: they saturate.
    w.pathing.corrupt_for_test(16);
    w.pathing.corrupt_for_test(19);
    // A member that left its formation this tick and is still listed (pruned next tick).
    let m = (0..12).find(|&i| w.entities.form[i].serial != 0).unwrap();
    w.entities.form[m] = crate::formation::Member::NONE;
    let mut back = World::restore(fresh(), &w.save().unwrap()).unwrap();
    for _ in 0..60 {
        back.step();
    }
}

fn turning_world(t: Turning) -> World {
    fresh()
        .with_movement(movement().with_turning(vec![(1, t)]).unwrap())
        .unwrap()
}

/// Every setting of the path service, the flowfields, the replan and the turning that two worlds
/// can differ in is a rule: a save made with one does not restore onto the other.
#[test]
fn other_path_settings_are_other_rules() {
    let bytes = moving(fresh()).save().unwrap();
    let others: Vec<World> = vec![
        fresh().with_path_budget(BUDGET + 1).unwrap(),
        fresh().with_flowfield_threshold(9).unwrap(),
        fresh()
            .with_replan(ReplanConfig {
                cooldown: ReplanConfig::DEFAULT.cooldown + 1,
                ..ReplanConfig::DEFAULT
            })
            .unwrap(),
        fresh()
            .with_replan(ReplanConfig {
                max: ReplanConfig::DEFAULT.max + 1,
                ..ReplanConfig::DEFAULT
            })
            .unwrap(),
        fresh()
            .with_replan(ReplanConfig {
                retarget_cooldown: ReplanConfig::DEFAULT.retarget_cooldown + 1,
                ..ReplanConfig::DEFAULT
            })
            .unwrap(),
        // Every field of a turning profile: the radius, the drive tolerance, the pivot share, the
        // mode.
        turning_world(Turning::arc(Fx::from_int(3), 16, 250)),
        turning_world(Turning::arc(Fx::from_int(2), 17, 250)),
        turning_world(Turning::arc(Fx::from_int(2), 16, 251)),
        turning_world(Turning::pivot(16)),
        fresh()
            .with_formation(crate::formation::FormationConfig {
                max_units: crate::formation::FormationConfig::DEFAULT.max_units - 1,
                ..crate::formation::FormationConfig::DEFAULT
            })
            .unwrap(),
    ];
    for (k, other) in others.into_iter().enumerate() {
        assert_eq!(World::restore(other, &bytes), Err(SaveError::Rules), "{k}");
    }
    assert!(World::restore(fresh(), &bytes).is_ok());
}

/// The numbers that enter the file: the section tags, the tags of the route states, field states
/// and build stages, formation states and member modes. A renumbering reads old saves as other
/// states.
#[test]
fn the_new_tags_are_pinned() {
    assert_eq!(SECTIONS, *b"WEQPFM");
    use crate::formation::MemberMode;
    assert_eq!(
        [
            MemberMode::None as u8,
            MemberMode::Lead as u8,
            MemberMode::Direct as u8,
            MemberMode::Flow as u8,
            MemberMode::Wait as u8,
        ],
        [0, 1, 2, 3, 4]
    );
    assert_eq!(
        crate::path::state_tags_for_test(),
        (
            [0, 1, 2, 3],
            [0, 1, 2],
            [0, 1, 2, 3, 4, 5, 6],
            [0, 1, 2],
            [0, 1, 2, 3, 4, 5, 6, 7, 8]
        )
    );
    assert_eq!(TURN_CARRY_LIMIT, 1000);
}

/// Bit flips in the sections of WP13.1b: every restore is an error or a world that steps four ticks
/// without a panic. Every bit of the first 48 bytes of each section (the counters, the job, the
/// routes), then one bit in every 509th byte of the rest (the search arrays, the fields).
#[test]
fn no_flip_in_the_path_sections_panics() {
    let w = mid_flight();
    let bytes = w.save().unwrap();
    let template = fresh();
    let mut tried = 0;
    let mut restored = 0;
    for &(tag, start, len) in &super::tests::sections(&bytes) {
        if !b"PFM".contains(&tag) {
            continue;
        }
        let dense = len.min(48);
        let picks = (0..dense * 8)
            .map(|b| (start + b / 8, b % 8))
            .chain((dense..len).step_by(509).map(|o| (start + o, o % 8)));
        for (byte, bit) in picks {
            let mut d = bytes.clone();
            d[byte] ^= 1 << bit;
            tried += 1;
            let r = std::panic::catch_unwind(|| match World::restore(template.clone(), &d) {
                Ok(mut w) => {
                    for _ in 0..4 {
                        w.step();
                    }
                    true
                }
                Err(_) => false,
            });
            match r {
                Ok(ok) => restored += usize::from(ok),
                Err(_) => panic!("flip {byte}:{bit} (section {}) panicked", tag as char),
            }
        }
    }
    assert!(
        tried > 600 && restored > 0,
        "{tried} flips, {restored} restored"
    );
}

/// The crowd fix of WP13.1b: with the most stuck attempts a world allows (255), a unit stuck at
/// that count while its replan cools down sidesteps again without an overflow.
#[test]
fn the_most_stuck_attempts_do_not_overflow() {
    let cfg = crate::crowd::CollisionConfig {
        stuck_attempts: u8::MAX,
        ..crate::crowd::CollisionConfig::DEFAULT
    };
    assert!(cfg.is_valid());
    let fresh = || fresh().with_collision(cfg).unwrap();
    let mut w = moving(fresh());
    for _ in 0..30 {
        w.step();
    }
    for i in 0..w.entities.len() {
        if w.entities.alive[i] && w.entities.def[i] != crate::entities::NO_DEF {
            let c = &mut w.entities.crowd[i];
            c.attempts = u8::MAX;
            c.cool = 5;
            c.quiet = cfg.stuck_ticks - 1;
        }
    }
    let mut back = World::restore(fresh(), &w.save().unwrap()).unwrap();
    let before = back.crowd_stats().sidesteps;
    for _ in 0..40 {
        back.step();
    }
    assert!(
        back.crowd_stats().sidesteps > before,
        "a unit sidestepped at the limit"
    );
    // The value itself, which the step uses: at the limit it stays there (a wrapping `+ 1` gives
    // 0 in a release build, which no panic would show).
    assert_eq!(crate::crowd::next_attempts(u8::MAX, u8::MAX), u8::MAX);
    assert_eq!(crate::crowd::next_attempts(254, u8::MAX), u8::MAX);
    assert_eq!(crate::crowd::next_attempts(3, 4), 4);
    assert_eq!(crate::crowd::next_attempts(4, 4), 4);
}
