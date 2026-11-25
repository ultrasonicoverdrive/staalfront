//! WP13.1c-3b: vision through save and restore. Worlds built through `World::step` reach shared and
//! unshared cells, fog (explored, not visible), a player index without a picture, a stamp of a
//! slot that died in the tick (after the pass), a structure being sold (a stamp of sight nothing)
//! and its removal, and a stamp the cap refused. Each round trips by `PartialEq`, resaves byte-equal
//! and goes on with the checksums of the uninterrupted run. Inside the vision module so that the
//! restore checks can be set at their bounds and one past them.

use sf_math::{Fx, Vec2};

use super::*;
use crate::command::Command;
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::orders::Order;
use crate::save::outcome_tests::{
    BARN, HQ, RIFLEMAN, RULES, SELL_TICKS, fresh_sized, id, place, send,
};
use crate::save::tests::{sections, settled};
use crate::save::{SaveContents, SaveError};
use crate::units::{UnitStats, UnitTable};
use crate::world::{World, WorldConfig};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const LEGACY: u32 = 2000;

fn vision_rules() -> VisionRules {
    VisionRules::new(LEGACY, vec![(RIFLEMAN, 5000), (BARN, 3000), (HQ, 4000)])
        .with_acquire_needs_sight(true)
        .with_shared(vec![(P0, P2)])
}

/// The match world of the WP13.1c-3a tables with vision, players 0 and 2, the first sharing with the other.
/// On a map of 40 by 41 cells: 1640 cells, so the last explored word has bits past the map.
fn fresh() -> World {
    bare().with_vision(vision_rules()).unwrap()
}

/// The same world without vision rules.
fn bare() -> World {
    fresh_sized(Some(&[P0, P2]), RULES, 41)
}

fn spawn(w: &mut World, tick: u64, owner: PlayerId, x: i32, y: i32) {
    w.schedule(
        Tick(tick),
        owner,
        Command::SpawnUnit {
            kind: RIFLEMAN,
            pos: Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF),
            owner,
        },
    )
    .unwrap();
}

/// Slots: 0 and 1 the construction centres of players 0 and 2, 2 and 3 barns of player 0, 4 a
/// rifleman of player 2 that shoots barn 2 down (it dies in the weapon stage, after the pass), 5 a
/// rifleman of player 0 that walks away (fog). Player 0 sells barn 3 at tick 30 (a stamp of sight
/// nothing for `SELL_TICKS` ticks, then the slot is freed). A rifleman of player 1 comes at tick 60
/// (until then index 1 has no picture).
fn vision_world() -> World {
    let mut w = fresh();
    place(&mut w, 0, HQ, P0, 4, 30);
    place(&mut w, 0, HQ, P2, 28, 30);
    place(&mut w, 0, BARN, P0, 10, 10);
    place(&mut w, 0, BARN, P0, 20, 20);
    spawn(&mut w, 1, P2, 14, 10);
    spawn(&mut w, 1, P0, 6, 6);
    send(
        &mut w,
        2,
        Command::Order {
            ids: vec![id(4)],
            order: Order::Attack(id(2)),
            queued: false,
        },
    );
    send(
        &mut w,
        3,
        Command::Order {
            ids: vec![id(5)],
            order: Order::Move(Vec2::new(Fx::from_int(36), Fx::from_int(4))),
            queued: false,
        },
    );
    send(&mut w, 30, Command::Sell { structure: id(3) });
    spawn(&mut w, 60, P1, 36, 36);
    w
}

fn round_trip(w: &World) -> World {
    let bytes = w.save().unwrap();
    let back = World::restore(fresh(), &bytes).unwrap();
    assert_eq!(back.save().unwrap(), bytes, "a resave is the save");
    back
}

fn names_of(w: &World, names: &mut Vec<&'static str>) {
    for n in w.save_coverage() {
        if n.starts_with("vision") && !names.contains(&n) {
            names.push(n);
        }
    }
}

/// The every-tick probe of the vision world in every build: save, restore and resave after every
/// tick, compare with the uninterrupted run (`PartialEq` and every checksum), audit the rebuilt
/// counts, and assert each claimed state at the tick it is first there.
#[test]
fn vision_restores_after_every_tick() {
    let ticks = 120;
    let mut straight = vision_world();
    let want: Vec<u64> = (0..ticks).map(|_| straight.step().checksum).collect();
    let mut w = vision_world();
    let mut names = Vec::new();
    let mut claimed: Vec<(&str, u64)> = Vec::new();
    for (t, &sum) in want.iter().enumerate() {
        assert_eq!(w.step().checksum, sum, "tick {t}");
        let c = w.save_contents();
        let before = names.len();
        names_of(&w, &mut names);
        for n in &names[before..] {
            claimed.push((n, w.tick().0));
        }
        let back = round_trip(&w);
        back.vision_audit().unwrap();
        // The reports of the last tick are derived and start empty.
        let restored = SaveContents {
            reports: c.reports,
            economy_reports: c.economy_reports,
            ..back.save_contents()
        };
        assert_eq!(restored, c, "tick {t}");
        assert_eq!(settled(back.clone()), settled(w.clone()), "tick {t}");
        w = back;
    }
    // Each state, and the tick of the first save that held it.
    for n in [
        "vision.player",
        "vision.player.gap",
        "vision.stamp",
        "vision.stamp.dead",
        "vision.stamp.blind",
        "vision.stamp.selling",
        "vision.fog",
        "vision.shared",
    ] {
        assert!(names.contains(&n), "{n}: {claimed:?}");
    }
    // The ones with a fixed tick: the sale stamps nothing from the tick of the order, the gap
    // closes with the first object of player 1 (slot 6).
    let first = |n: &str| claimed.iter().find(|c| c.0 == n).unwrap().1;
    assert_eq!(first("vision.stamp.selling"), 31);
    // The numbers `docs/rules/save.md` gives: fog from tick 9, the dead slot's stamp at tick 43.
    assert_eq!(first("vision.fog"), 9);
    assert_eq!(first("vision.stamp.dead"), 43);
    assert!(first("vision.player.gap") == 1);
    assert_eq!(w.save_contents().stamps_blind, 0, "the sold barn is gone");
    let e = &w.entities;
    assert!(
        (0..e.len()).all(|i| !e.alive[i] || e.kind[i] != BARN),
        "both barns are gone"
    );
}

/// The selling stamp at each tick of the sale, and the slot unstamped by the pass after the barn
/// is gone, through a round trip at each.
#[test]
fn a_sale_stamps_nothing_until_the_barn_is_gone_and_restores_at_each_tick() {
    let mut w = vision_world();
    for _ in 0..30 {
        w.step();
    }
    let mut seen = Vec::new();
    for _ in 0..(SELL_TICKS + 3) {
        w.step();
        let st = w.vision.as_deref().unwrap().stamps[3];
        seen.push((
            w.entities.alive[3],
            w.entities.sell_left[3],
            st.map(|s| (s.src.sight_milli, s.disc)),
        ));
        w = round_trip(&w);
    }
    // The step of the order (tick 30 to 31): sold from the order on, and the pass of that same
    // step (the commands run first) already stamps a sight of nothing.
    assert_eq!(seen[0], (true, SELL_TICKS, Some((0, false))));
    assert!(seen.iter().any(|s| !s.0), "{seen:?}");
    // Once the barn is gone the next pass unstamps the slot.
    assert_eq!(seen.last().unwrap().2, None, "{seen:?}");
}

/// The cap, reached through `World::step`: one player with one more unit than it may have discs.
/// The last unit's stamp is refused (no disc), the round trip keeps it refused, and after one unit
/// goes the refused one gets its disc on the next pass, in both worlds alike.
#[test]
fn a_stamp_the_cap_refused_round_trips() {
    const SCOUT: Kind = Kind(1);
    let table = UnitTable::new(vec![UnitStats {
        kind: SCOUT,
        hp: 10,
        movement_class: Some(0),
        speed_per_tick: Fx::from_ratio(1, 4),
        turn_per_tick: 16,
        size: Fx::from_ratio(4, 10),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }])
    .unwrap();
    let side = 300u32;
    let make = || {
        World::new(WorldConfig::new(3).with_map_size(side, side))
            .unwrap()
            .with_units(table.clone())
            .unwrap()
            .with_vision(VisionRules::new(0, vec![(SCOUT, 1000)]))
            .unwrap()
    };
    let n = MAX_DISCS_PER_PLAYER + 1;
    let mut w = make();
    for k in 0..n {
        let (x, y) = ((k % side) as i32, (k / side) as i32);
        w.schedule(
            Tick(0),
            P0,
            Command::SpawnUnit {
                kind: SCOUT,
                pos: Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF),
                owner: P0,
            },
        )
        .unwrap();
    }
    w.step();
    assert_eq!(w.vision_discs(P0), MAX_DISCS_PER_PLAYER);
    assert_eq!(w.save_contents().stamps_refused, 1);
    // The refused one is the last slot: the pass adds in slot order.
    let st = &w.vision.as_deref().unwrap().stamps;
    assert_eq!(st.len(), n as usize);
    assert!(!st[n as usize - 1].unwrap().disc);
    assert!(st[..n as usize - 1].iter().all(|s| s.unwrap().disc));
    assert!(w.save_coverage().contains(&"vision.stamp.refused"));
    let bytes = w.save().unwrap();
    let mut back = World::restore(make(), &bytes).unwrap();
    assert_eq!(back.save().unwrap(), bytes);
    assert_eq!(settled(back.clone()), settled(w.clone()));
    back.vision_audit().unwrap();
    // One unit goes: the refused stamp gets its disc on the next pass, in both.
    for world in [&mut w, &mut back] {
        world
            .schedule(
                Tick(1),
                P0,
                Command::Despawn {
                    id: EntityId::new(0, 0),
                },
            )
            .unwrap();
    }
    for _ in 0..2 {
        assert_eq!(back.step().checksum, w.step().checksum);
    }
    assert_eq!(back.save_contents().stamps_refused, 0);
    assert_eq!(back.vision_discs(P0), MAX_DISCS_PER_PLAYER);
    // The bound of the disc count: at the cap restores (above), one more disc is refused.
    let mut bad = w.clone();
    let v = bad.vision.as_deref_mut().unwrap();
    // Slot 0 is free now; give it a stamp with a disc: one past the cap.
    v.stamps[0] = Some(Stamp {
        src: Source {
            owner: P0,
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            sight_milli: 1000,
        },
        disc: true,
    });
    assert_eq!(
        World::restore(make(), &bad.save().unwrap()),
        Err(SaveError::Corrupt(
            "more vision discs than a count can hold"
        ))
    );
}

// ---- the restore checks, at their bounds and one past -----------------------------------------

fn vis(w: &mut World) -> &mut Vision {
    w.vision.as_deref_mut().unwrap()
}

fn explore(w: &mut World, p: PlayerId, i: u32) {
    // A player without a picture has nothing to explore in (the owner case one past its bound).
    let Some(Some(pv)) = vis(w).players.get_mut(usize::from(p.0)) else {
        return;
    };
    pv.explored[(i / 64) as usize] |= 1u64 << (i % 64);
}

/// Slot 5's stamp (the walking rifleman of player 0) made a stamp of `sight` without a disc at
/// cell `(x, y)` of `owner`, its cell explored by `owner`.
fn restamp(w: &mut World, owner: PlayerId, x: u32, y: u32, sight: u32, disc: bool) {
    restamp_sized(w, owner, (x, y), (1, 1), sight, disc);
}

/// [`restamp`] with a footprint of `fw` by `fh`, the cells of it on the map explored by `owner`.
fn restamp_sized(
    w: &mut World,
    owner: PlayerId,
    (x, y): (u32, u32),
    (fw, fh): (u8, u8),
    sight: u32,
    disc: bool,
) {
    let (width, height) = (vis(w).width, vis(w).height);
    for dy in 0..u32::from(fh) {
        for dx in 0..u32::from(fw) {
            if x + dx < width && y + dy < height {
                explore(w, owner, (y + dy) * width + x + dx);
            }
        }
    }
    vis(w).stamps[5] = Some(Stamp {
        src: Source {
            owner,
            x,
            y,
            w: fw,
            h: fh,
            sight_milli: sight,
        },
        disc,
    });
}

/// Clears the first cell player 0 has explored and does not see and that is in no footprint:
/// legal (the check is on cells in sight and in footprints only).
fn clear_a_fog_cell(w: &mut World) {
    let v = vis(w);
    let width = v.width;
    let feet: Vec<u32> = v
        .stamps
        .iter()
        .flatten()
        .filter(|s| s.src.owner == P0)
        .flat_map(|s| {
            let s = s.src;
            (0..u32::from(s.h)).flat_map(move |dy| {
                (0..u32::from(s.w)).map(move |dx| (s.y + dy) * width + s.x + dx)
            })
        })
        .collect();
    let cells = v.cells() as u32;
    let pv = v.players[0].as_deref_mut().unwrap();
    let i = (0..cells)
        .find(|&i| bit(&pv.explored, i) && !bit(&pv.visible, i) && !feet.contains(&i))
        .expect("a fog cell of player 0");
    pv.explored[(i / 64) as usize] &= !(1u64 << (i % 64));
}

type Case = (&'static str, &'static str, fn(&mut World, bool));

/// One case per restore check of section `V`, on the vision world after tick 35 (the barn on sale,
/// the walking rifleman, players 0 and 2): the value at its bound restores and goes on, one past it
/// is refused with the message. Tag cases are made in the bytes (`bytes_of`).
fn cases() -> Vec<Case> {
    vec![
        (
            "the vision stamps are not one per slot",
            "stamps more",
            |w, at| {
                let slots = w.entities.len() + usize::from(!at);
                vis(w).stamps.resize(slots, None);
            },
        ),
        (
            "the vision stamps are not one per slot",
            "stamps fewer",
            |w, at| {
                // The last slot is the walking rifleman (5); its stamp goes with the shorter column.
                let slots = w.entities.len() - usize::from(!at);
                vis(w).stamps.truncate(slots);
            },
        ),
        // A tag is 0 or 1; the save has 1 where the case writes 2, which is the bound (the 0 side
        // is every free slot and the gap at index 1, in the save as it is).
        ("unknown vision stamp tag", "stamp tag", |_, _| {}),
        ("a vision player out of range", "player count", |w, at| {
            let copy = vis(w).players[0].clone();
            let v = vis(w);
            v.players.resize(255 + usize::from(!at), None);
            v.players.push(copy);
        }),
        // As the stamp tag.
        ("unknown vision player tag", "player tag", |_, _| {}),
        (
            "a vision version the pass never reaches",
            "version high",
            |w, at| {
                let now = w.tick().0;
                vis(w).players[0].as_deref_mut().unwrap().version = now + u64::from(!at);
            },
        ),
        (
            "a vision version the pass never reaches",
            "version zero",
            |w, at| {
                vis(w).players[2].as_deref_mut().unwrap().version = u64::from(at);
            },
        ),
        (
            "explored words of the wrong length",
            "explored words",
            // A check of equality: the bound is the length the save has, one more is past it.
            |w, at| {
                if !at {
                    let pv = vis(w).players[0].as_deref_mut().unwrap();
                    let mut words = pv.explored.to_vec();
                    words.push(0);
                    pv.explored = words.into_boxed_slice();
                }
            },
        ),
        (
            "the vision players end with a gap",
            "trailing gap",
            |w, at| {
                let copy = vis(w).players[2].clone();
                vis(w).players.push(if at { copy } else { None });
            },
        ),
        ("an explored cell off the map", "explored tail", |w, at| {
            let cells = vis(w).cells() as u32;
            explore(w, P0, cells - u32::from(at));
        }),
        ("a vision stamp off the map", "stamp x", |w, at| {
            let width = vis(w).width;
            restamp(w, P0, width - u32::from(at), 3, 0, false);
        }),
        ("a vision stamp off the map", "stamp y", |w, at| {
            let height = vis(w).height;
            restamp(w, P0, 3, height - u32::from(at), 0, false);
        }),
        ("a vision stamp off the map", "stamp footprint", |w, at| {
            restamp(w, P0, 3, 3, 0, false);
            vis(w).stamps[5].as_mut().unwrap().src.w = u8::from(at);
        }),
        ("a vision stamp off the map", "stamp height", |w, at| {
            restamp(w, P0, 3, 3, 0, false);
            vis(w).stamps[5].as_mut().unwrap().src.h = u8::from(at);
        }),
        // A footprint of 2 by 2 at the right and the bottom edge: its last column (row) on the
        // last cell of the map passes, one further is refused (not only its anchor on the map).
        ("a vision stamp off the map", "footprint right", |w, at| {
            let width = vis(w).width;
            restamp_sized(w, P0, (width - 2 + u32::from(!at), 3), (2, 2), 0, false);
        }),
        ("a vision stamp off the map", "footprint bottom", |w, at| {
            let height = vis(w).height;
            restamp_sized(w, P0, (3, height - 2 + u32::from(!at)), (2, 2), 0, false);
        }),
        (
            "a vision stamp with a sight the rules do not give",
            "stamp sight",
            |w, at| restamp(w, P0, 3, 3, LEGACY + u32::from(!at), false),
        ),
        (
            "a vision disc without a sight",
            "disc of nothing",
            |w, at| {
                restamp(w, P0, 3, 3, 0, !at);
            },
        ),
        (
            "a vision stamp of a player without a picture",
            "stamp owner",
            |w, at| restamp(w, if at { P2 } else { P1 }, 3, 3, 0, false),
        ),
        (
            "a cell in sight that was never explored",
            "visible cell",
            |w, at| {
                // Slot 0's construction centre (player 0, 4 by 30) sees its own cell; at the bound a
                // cell out of sight is cleared instead.
                if at {
                    clear_a_fog_cell(w);
                } else {
                    let i = 30 * vis(w).width + 4;
                    let pv = vis(w).players[0].as_deref_mut().unwrap();
                    pv.explored[(i / 64) as usize] &= !(1u64 << (i % 64));
                }
            },
        ),
        (
            "a cell in sight that was never explored",
            "blind footprint",
            |w, at| {
                // The barn on sale (slot 3, at 20 by 20) sees nothing; its footprint is explored.
                let i = 20 * vis(w).width + 20;
                // Nothing else sees it: barn 3 stamps sight 0 and nothing of player 0 is near. At the
                // bound a cell in no footprint and out of sight is cleared instead.
                if at {
                    clear_a_fog_cell(w);
                } else {
                    let pv = vis(w).players[0].as_deref_mut().unwrap();
                    pv.explored[(i / 64) as usize] &= !(1u64 << (i % 64));
                }
            },
        ),
    ]
}

fn mid() -> World {
    let mut w = vision_world();
    for _ in 0..35 {
        w.step();
    }
    let v = w.vision.as_deref().unwrap();
    assert_eq!(v.stamps.len(), w.entities.len());
    assert_eq!(
        v.stamps[3].map(|s| s.src.sight_milli),
        Some(0),
        "barn 3 on sale"
    );
    assert!(v.players[1].is_none() && v.players.len() == 3);
    w
}

/// The bytes of `w`; for the tag cases the first stamp's or the first player's tag made 2 (unknown).
fn bytes_of(name: &str, w: &World, at: bool) -> Vec<u8> {
    let mut bytes = w.save().unwrap();
    if at {
        return bytes;
    }
    let (_, start, _) = *sections(&bytes).iter().find(|s| s.0 == b'V').unwrap();
    let v = w.vision.as_deref().unwrap();
    let tag = match name {
        "stamp tag" => start + 4,
        "player tag" => {
            let stamps: usize = v
                .stamps
                .iter()
                .map(|s| if s.is_some() { 17 } else { 1 })
                .sum();
            start + 4 + stamps + 4
        }
        _ => return bytes,
    };
    assert_eq!(bytes[tag], 1, "{name}");
    bytes[tag] = 2;
    bytes
}

#[test]
fn every_vision_bound_restores_and_one_past_it_is_refused() {
    let base = mid();
    for (what, name, change) in cases() {
        let mut ok = base.clone();
        change(&mut ok, true);
        let mut back = World::restore(fresh(), &bytes_of(name, &ok, true))
            .unwrap_or_else(|e| panic!("{name} at its bound: {e}"));
        for _ in 0..30 {
            back.step();
        }
        let mut bad = base.clone();
        change(&mut bad, false);
        assert_eq!(
            World::restore(fresh(), &bytes_of(name, &bad, false)),
            Err(SaveError::Corrupt(what)),
            "{name}"
        );
    }
}

/// Vision in a save is refused onto a world without vision rules (another digest), and the save of
/// a world without them has an empty section.
#[test]
fn vision_needs_the_same_rules_on_both_sides() {
    let w = mid();
    let bare = bare();
    assert_eq!(
        World::restore(bare.clone(), &w.save().unwrap()),
        Err(SaveError::Rules)
    );
    let bytes = bare.save().unwrap();
    let v = sections(&bytes).into_iter().find(|s| s.0 == b'V').unwrap();
    assert_eq!(v.2, 0);
}

/// Every vision rule enters the rules digest by number: each of the four fields on its own (the
/// legacy sight, a sight, a kind, a kind less, the gate, a shared pair, the pairs' order), and
/// whether the world has vision rules at all.
#[test]
fn every_vision_rule_is_in_the_digest() {
    let base = fresh().rules_digest();
    let with = |r: VisionRules| bare().with_vision(r).unwrap();
    let r = vision_rules;
    let others = [
        (
            "legacy sight",
            VisionRules {
                legacy_sight_milli: LEGACY + 1,
                ..r()
            },
        ),
        (
            "a sight",
            VisionRules {
                sights: vec![(RIFLEMAN, 5001), (BARN, 3000), (HQ, 4000)],
                ..r()
            },
        ),
        (
            "a kind",
            VisionRules {
                sights: vec![(RIFLEMAN, 5000), (HQ, 3000)],
                ..r()
            },
        ),
        ("the gate", r().with_acquire_needs_sight(false)),
        ("no pair", r().with_shared(Vec::new())),
        ("another pair", r().with_shared(vec![(P2, P0)])),
        ("one pair more", r().with_shared(vec![(P0, P1), (P0, P2)])),
    ];
    for (name, rules) in others {
        assert_ne!(with(rules).rules_digest(), base, "{name}");
    }
    // A sight more for a kind that had none: the kind list grows.
    let fewer = VisionRules {
        sights: vec![(RIFLEMAN, 5000), (BARN, 3000)],
        ..r()
    };
    assert_ne!(with(fewer).rules_digest(), base, "a kind less");
    // The same sights under other kinds: only the kinds tell them apart.
    let kinds = |k| VisionRules {
        sights: vec![(RIFLEMAN, 5000), (k, 3000)],
        ..r()
    };
    assert_ne!(
        with(kinds(BARN)).rules_digest(),
        with(kinds(HQ)).rules_digest(),
        "the kinds of the same sights"
    );
    // Gate off and no pairs (the checksum hashes them only when set; the digest always).
    let plain = with(VisionRules::new(LEGACY, r().sights));
    assert_ne!(plain.rules_digest(), base, "gate and pairs");
    assert_ne!(bare().rules_digest(), base, "no vision rules");
    let bytes = mid().save().unwrap();
    assert_eq!(
        World::restore(with(r().with_shared(Vec::new())), &bytes),
        Err(SaveError::Rules)
    );
    assert!(World::restore(fresh(), &bytes).is_ok());
}
