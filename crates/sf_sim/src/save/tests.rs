//! Tests of the save file (WP13.1a, `docs/rules/save.md`). Inside the crate: they compare whole
//! worlds by `PartialEq`, reach the gated columns and build worlds the public API cannot.

use sf_math::bytes::ByteWriter;
use sf_math::{Facing, Fx, Vec2};

use super::*;
use crate::crowd::CollisionConfig;
use crate::formation::Member;
use crate::orders::{GuardTarget, Order};
use crate::projectiles::{Projectile, Target};
use crate::terrain::Cell;
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::world::{World, WorldConfig};

const MOBILE: Kind = Kind(10);
const TURRET: Kind = Kind(11);
const HQ: Kind = Kind(20);
const LEGACY: Kind = Kind(3);

fn mobile(kind: Kind, turret: u8) -> UnitStats {
    UnitStats {
        kind,
        hp: 120,
        movement_class: Some(0),
        speed_per_tick: Fx::from_ratio(1, 4),
        turn_per_tick: 16,
        size: Fx::from_ratio(1, 2),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: turret,
    }
}

fn table(hq_hp: u32) -> UnitTable {
    UnitTable::new(vec![
        mobile(MOBILE, 0),
        mobile(TURRET, 8),
        UnitStats {
            kind: HQ,
            hp: hq_hp,
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

/// The tick-0 world the host builds for the match: the same every time.
fn fresh_with(hq_hp: u32, crowd: bool) -> World {
    let w = World::new(WorldConfig::new(41).with_map_size(64, 48))
        .unwrap()
        .with_units(table(hq_hp))
        .unwrap();
    if crowd {
        w
    } else {
        w.with_collision(CollisionConfig {
            enabled: false,
            ..CollisionConfig::DEFAULT
        })
        .unwrap()
    }
}

fn fresh() -> World {
    fresh_with(500, false)
}

fn p(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

/// One command of every variant, for a tick that has not come yet. They are never run: they wait
/// in the queue (most would be ignored in this world anyway).
fn every_command() -> Vec<Command> {
    let id = EntityId::new(2, 0);
    let ids = vec![EntityId::new(0, 0), EntityId::new(1, 3)];
    vec![
        Command::Spawn {
            kind: LEGACY,
            pos: p(3, 4),
            owner: PlayerId(1),
        },
        Command::Place {
            kind: LEGACY,
            cell: Cell::new(9, 9),
            owner: PlayerId(0),
        },
        Command::SpawnUnit {
            kind: MOBILE,
            pos: Vec2::new(Fx::from_raw(-7), Fx::from_ratio(5, 3)),
            owner: PlayerId(2),
        },
        Command::PlaceStructure {
            kind: HQ,
            cell: Cell::new(40, 30),
            owner: PlayerId(1),
        },
        Command::BuildStructure {
            kind: HQ,
            cell: Cell::new(1, 2),
        },
        Command::QueueUnit {
            factory: id,
            kind: MOBILE,
        },
        Command::CancelUnit {
            factory: id,
            index: 3,
        },
        Command::SetRally {
            factory: id,
            cell: Some(Cell::new(5, 6)),
        },
        Command::SetRally {
            factory: id,
            cell: None,
        },
        Command::SetRepeat {
            factory: id,
            on: true,
        },
        Command::Repair {
            structure: id,
            on: false,
        },
        Command::Sell { structure: id },
        Command::Order {
            ids: ids.clone(),
            order: Order::Guard(GuardTarget::Entity(id)),
            queued: true,
        },
        Command::Order {
            ids: Vec::new(),
            order: Order::Pursue(id),
            queued: false,
        },
        Command::Stop { ids: ids.clone() },
        Command::AimTurret {
            ids: ids.clone(),
            target: Some(id),
        },
        Command::AimTurret {
            ids: ids.clone(),
            target: None,
        },
        Command::TakeOre {
            cell: Cell::new(7, 8),
            amount: 900,
        },
        Command::Despawn { id },
        Command::SetWanderArea {
            ids: ids.clone(),
            area: Area {
                min: p(9, 9),
                max: p(2, 2),
            },
        },
        Command::MoveTo {
            ids,
            target: p(30, 20),
        },
    ]
}

/// A world with wanderers, units walking orders with a queue behind them, structures of both kinds,
/// deaths with slot reuse (so the free list has an order that matters) and pending commands of
/// every variant, after `ticks` ticks.
fn busy(mut w: World, ticks: u64) -> World {
    let s =
        |w: &mut World, t: u64, pl: u8, c: Command| w.schedule(Tick(t), PlayerId(pl), c).unwrap();
    for n in 0..5 {
        s(
            &mut w,
            0,
            n % 2,
            Command::Spawn {
                kind: LEGACY,
                pos: p(4 + 9 * i32::from(n), 5 + 7 * i32::from(n)),
                owner: PlayerId(n % 2),
            },
        );
    }
    for n in 0..4 {
        let kind = if n % 2 == 0 { MOBILE } else { TURRET };
        s(
            &mut w,
            1,
            0,
            Command::SpawnUnit {
                kind,
                pos: p(10 + 4 * n, 30),
                owner: PlayerId(0),
            },
        );
    }
    s(
        &mut w,
        1,
        1,
        Command::Place {
            kind: LEGACY,
            cell: Cell::new(50, 10),
            owner: PlayerId(1),
        },
    );
    s(
        &mut w,
        1,
        1,
        Command::PlaceStructure {
            kind: HQ,
            cell: Cell::new(55, 40),
            owner: PlayerId(1),
        },
    );
    // Slots: wanderers 0 to 4, units 5 to 8, the legacy structure 9, the HQ in slot 10 (last).
    let unit = |i: u32| EntityId::new(i, 0);
    s(
        &mut w,
        2,
        0,
        Command::Order {
            ids: vec![unit(5), unit(6)],
            order: Order::Move(p(40, 10)),
            queued: false,
        },
    );
    s(
        &mut w,
        2,
        0,
        Command::Order {
            ids: vec![unit(5)],
            order: Order::Move(p(20, 40)),
            queued: true,
        },
    );
    s(
        &mut w,
        2,
        0,
        Command::Order {
            ids: vec![unit(5)],
            order: Order::HoldPosition,
            queued: true,
        },
    );
    s(
        &mut w,
        2,
        0,
        Command::Order {
            ids: vec![unit(7)],
            order: Order::Guard(GuardTarget::Entity(unit(8))),
            queued: false,
        },
    );
    s(
        &mut w,
        2,
        0,
        Command::AimTurret {
            ids: vec![unit(6), unit(8)],
            target: Some(unit(10)),
        },
    );
    s(
        &mut w,
        3,
        1,
        Command::MoveTo {
            ids: vec![unit(1)],
            target: p(60, 44),
        },
    );
    s(
        &mut w,
        3,
        1,
        Command::SetWanderArea {
            ids: vec![unit(2)],
            area: Area::from_cells(0, 0, 20, 20),
        },
    );
    // Deaths: 3 then 0, so the free list is [3, 0]; a later spawn takes 0 again.
    s(&mut w, 4, 0, Command::Despawn { id: unit(3) });
    s(&mut w, 4, 0, Command::Despawn { id: unit(0) });
    s(
        &mut w,
        6,
        0,
        Command::Spawn {
            kind: LEGACY,
            pos: p(33, 33),
            owner: PlayerId(0),
        },
    );
    s(&mut w, 6, 1, Command::Despawn { id: unit(4) });
    for _ in 0..ticks {
        w.step();
    }
    // Pending: every variant, for later ticks, from two players, one with a sender's number.
    let later = w.tick().0 + 50;
    for (k, c) in every_command().into_iter().enumerate() {
        s(&mut w, later + k as u64 % 3, (k % 2) as u8, c);
    }
    w.schedule_sequenced(
        Tick(later),
        PlayerId(3),
        900,
        Command::Stop { ids: vec![unit(5)] },
    )
    .unwrap();
    w
}

/// The world with what a restore does not bring back put equal on both sides: the reports of the
/// last tick, the stats, the interpolation columns and the checksum history.
fn settled(mut w: World) -> World {
    let e = &mut w.entities;
    e.prev_pos.clone_from(&e.pos);
    e.prev_facing.clone_from(&e.facing);
    e.prev_turret.clone_from(&e.turret);
    w.stalled.clear();
    w.completions.clear();
    w.placement.rejections.clear();
    w.arsenal.shots.clear();
    w.arsenal.impacts.clear();
    w.arsenal.attacks.clear();
    w.destruction.deaths.clear();
    w.pathing.clear_stats();
    w.crowd.stats = Default::default();
    w.formations.stats = Default::default();
    w.history.clear();
    w.history_base = w.tick;
    w
}

fn round_trip(w: &World) -> World {
    World::restore(fresh(), &w.save().unwrap()).unwrap()
}

#[test]
fn a_restored_world_equals_the_saved_one() {
    for ticks in [0, 1, 5, 17, 60] {
        let w = busy(fresh(), ticks);
        let back = round_trip(&w);
        assert_eq!(settled(back), settled(w.clone()), "after {ticks} ticks");
        assert_eq!(round_trip(&w).checksum(), w.checksum());
    }
    // The world really is busy: orders, a queue, structures, deaths, reuse, pending commands.
    let w = busy(fresh(), 17);
    assert!(w.entities.order.iter().any(|o| matches!(o, Order::Move(_))));
    assert!(w.entities.queue.iter().any(|q| q.len() == 2));
    assert!(w.occupancy.count() == 5);
    assert_eq!(w.free, vec![3, 4]);
    assert_eq!(w.entities.generation[0], 1);
    assert_eq!(w.queue.len(), every_command().len() + 1);
}

#[test]
fn a_resave_is_byte_equal() {
    for ticks in [0, 3, 40] {
        let w = busy(fresh(), ticks);
        let bytes = w.save().unwrap();
        let again = World::restore(fresh(), &bytes).unwrap().save().unwrap();
        assert_eq!(again, bytes, "after {ticks} ticks");
    }
}

/// Run N ticks, save and restore, run M more: every checksum equals the uninterrupted run. No CI
/// scenario uses only the sections of this WP (every one has a unit table with crowd, pathing or
/// weapons state), so the world is built here.
#[test]
fn a_restored_world_continues_like_the_original() {
    for crowd in [false, true] {
        continues_like_the_original(crowd);
    }
}

fn continues_like_the_original(crowd: bool) {
    let total = 200;
    let mut straight = busy(fresh_with(500, crowd), 0);
    for _ in 0..total {
        straight.step();
    }
    for n in [1u64, 7, 33, 97] {
        let mut w = busy(fresh_with(500, crowd), 0);
        for _ in 0..n {
            w.step();
        }
        let mut back = World::restore(fresh_with(500, crowd), &w.save().unwrap()).unwrap();
        assert_eq!(back.checksum_history_start(), Tick(n));
        assert!(back.checksum_history().is_empty());
        for _ in n..total {
            back.step();
        }
        let start = n as usize;
        assert_eq!(
            back.checksum_history(),
            &straight.checksum_history()[start..],
            "saved after {n} ticks, crowd {crowd}"
        );
        assert_eq!(
            back.checksum_after(Tick(total - 1)),
            straight.checksum_after(Tick(total - 1))
        );
        assert_eq!(back.checksum_after(Tick(n - 1)), None);
    }
}

#[test]
fn the_free_list_keeps_its_stack_order() {
    let w = busy(fresh(), 10);
    assert_eq!(w.free, vec![3, 4]);
    let mut back = round_trip(&w);
    assert_eq!(back.free, vec![3, 4]);
    let mut original = w.clone();
    let id = back.spawn(LEGACY, p(1, 1), PlayerId(0)).unwrap();
    assert_eq!(original.spawn(LEGACY, p(1, 1), PlayerId(0)), Some(id));
    assert_eq!(id.index(), 4, "the last freed slot is reused first");
}

#[test]
fn command_tags_are_pinned() {
    let tags: Vec<u8> = every_command().iter().map(command_tag).collect();
    assert_eq!(
        tags,
        vec![
            0, 1, 2, 3, 4, 5, 6, 7, 7, 8, 9, 10, 11, 11, 12, 13, 13, 14, 15, 16, 17
        ]
    );
    for c in every_command() {
        let mut w = ByteWriter::new();
        write_command(&mut w, &c).unwrap();
        let bytes = w.finish();
        let mut r = ByteReader::new(&bytes);
        assert_eq!(read_command(&mut r).unwrap(), c);
        assert!(r.is_empty());
    }
    // One encoding pinned byte for byte.
    let mut w = ByteWriter::new();
    write_command(
        &mut w,
        &Command::Despawn {
            id: EntityId::new(0x0102, 7),
        },
    )
    .unwrap();
    assert_eq!(w.finish(), vec![15, 0x02, 0x01, 0, 0, 7, 0, 0, 0]);
}

#[test]
fn the_header_is_pinned() {
    let w = busy(fresh(), 2);
    let bytes = w.save().unwrap();
    assert_eq!(&bytes[..4], b"SFSV");
    assert_eq!(&bytes[4..8], &[1, 0, 0, 0]);
    assert_eq!(&bytes[8..16], &w.rules_digest().to_le_bytes());
    assert_eq!(&bytes[16..24], &2u64.to_le_bytes());
    assert_eq!(&bytes[24..28], &[3, 0, 0, 0]);
    assert_eq!(bytes[28], b'W');
    assert_eq!(FORMAT_VERSION, 1);
}

#[test]
fn a_wrong_header_is_refused() {
    let bytes = busy(fresh(), 5).save().unwrap();
    let mut bad = bytes.clone();
    bad[0] = b'X';
    assert_eq!(World::restore(fresh(), &bad), Err(SaveError::BadMagic));
    assert_eq!(World::restore(fresh(), b"SF"), Err(SaveError::BadMagic));
    let mut bad = bytes.clone();
    bad[4] = 2;
    assert_eq!(World::restore(fresh(), &bad), Err(SaveError::Version(2)));
    let mut bad = bytes.clone();
    bad[9] ^= 0x40;
    assert_eq!(World::restore(fresh(), &bad), Err(SaveError::Rules));
}

#[test]
fn other_assets_or_settings_are_refused() {
    let bytes = busy(fresh(), 5).save().unwrap();
    // A structure with other hit points in the unit table.
    assert_eq!(
        World::restore(fresh_with(501, false), &bytes),
        Err(SaveError::Rules)
    );
    // The collision setting.
    assert_eq!(
        World::restore(fresh_with(500, true), &bytes),
        Err(SaveError::Rules)
    );
    // Another seed.
    let other = World::new(WorldConfig::new(42).with_map_size(64, 48))
        .unwrap()
        .with_units(table(500))
        .unwrap()
        .with_collision(CollisionConfig {
            enabled: false,
            ..CollisionConfig::DEFAULT
        })
        .unwrap();
    assert_eq!(World::restore(other, &bytes), Err(SaveError::Rules));
    assert_ne!(
        fresh().rules_digest(),
        fresh_with(501, false).rules_digest()
    );
    assert_eq!(fresh().rules_digest(), fresh().rules_digest());
}

#[test]
fn a_started_world_is_no_fresh_world() {
    let bytes = busy(fresh(), 5).save().unwrap();
    let mut started = fresh();
    started.step();
    assert_eq!(World::restore(started, &bytes), Err(SaveError::NotFresh));
    assert_eq!(
        World::restore(busy(fresh(), 0), &bytes),
        Err(SaveError::NotFresh)
    );
}

#[test]
fn every_truncation_is_an_error_and_no_flip_panics() {
    let bytes = busy(fresh(), 12).save().unwrap();
    for cut in 0..bytes.len() {
        assert!(
            World::restore(fresh(), &bytes[..cut]).is_err(),
            "cut at {cut}"
        );
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(World::restore(fresh(), &extra).is_err());
    // Every bit of every byte. A world that still restores is stepped a few ticks: whatever the
    // file says, nothing may panic afterwards either.
    let template = fresh();
    let mut restored = 0;
    for byte in 0..bytes.len() {
        for bit in 0..8 {
            let mut d = bytes.clone();
            d[byte] ^= 1 << bit;
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
                Err(_) => panic!("flip {byte}:{bit} panicked"),
            }
        }
    }
    // Most flips land in values with no invariant (a random stream, a position inside the map).
    assert!(restored > bytes.len(), "{restored} flipped saves restored");
}

/// The body of every section: `(tag, start, length)`.
fn sections(bytes: &[u8]) -> Vec<(u8, usize, usize)> {
    let mut out = Vec::new();
    let mut at = 28;
    while at < bytes.len() {
        let len = u32::from_le_bytes(bytes[at + 1..at + 5].try_into().unwrap()) as usize;
        out.push((bytes[at], at + 5, len));
        at += 5 + len;
    }
    out
}

/// The byte of slot 5's `guard_walking` flag, found as the one byte two saves differ in.
fn guard_walking_byte(w: &World) -> usize {
    let a = w.save().unwrap();
    let mut v = w.clone();
    v.entities.guard_walking[5] = !v.entities.guard_walking[5];
    let b = v.save().unwrap();
    let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    assert_eq!(diff.len(), 1);
    diff[0]
}

type Corruption = (&'static str, fn(&mut World), fn(&mut Vec<u8>));

fn no_edit(_: &mut Vec<u8>) {}
fn no_change(_: &mut World) {}

/// A queue of two despawns of player 0 for tick 900, numbered 0 and 1 (in that order).
fn two_despawns(w: &mut World) {
    let mut pending = BTreeMap::new();
    for seq in 0..2 {
        pending.insert(
            (Tick(900), PlayerId(0), seq),
            Command::Despawn {
                id: EntityId::new(1, 0),
            },
        );
    }
    w.queue = CommandQueue::from_parts(pending, BTreeMap::from([(PlayerId(0), 2)]));
}

/// One impossible world per check of `restore`, made by changing a valid world before it is saved
/// (`save` does not judge what it writes) or by editing the bytes, with the exact error.
fn corruptions() -> Vec<Corruption> {
    vec![
        (
            "an entity stands outside the map",
            |w| w.entities.pos[5].x = Fx::from_int(65),
            no_edit,
        ),
        (
            "a target or wander area lies outside the map",
            |w| w.entities.target[2] = Some(p(99, 1)),
            no_edit,
        ),
        (
            "a target or wander area lies outside the map",
            |w| w.entities.area[2].max = p(1, 49),
            no_edit,
        ),
        (
            "an order point lies outside the map",
            |w| w.entities.order[5] = Order::Move(p(-1, 3)),
            no_edit,
        ),
        (
            "an order point lies outside the map",
            |w| {
                w.entities.queue[5].push_back(Order::AttackMove(p(70, 3)));
            },
            no_edit,
        ),
        (
            "an entity occupies a cell outside the map",
            |w| w.entities.cell[9] = 64 * 48,
            no_edit,
        ),
        (
            "an entity has no unit table entry for its kind",
            |w| w.entities.def[5] = 3,
            no_edit,
        ),
        (
            "an entity has no unit table entry for its kind",
            |w| w.entities.def[5] = 1,
            no_edit,
        ),
        (
            "an entity has more hit points than its kind",
            |w| w.entities.hp[5] = 121,
            no_edit,
        ),
        (
            "an entity has more hit points than its kind",
            |w| w.entities.hp[1] = 1,
            no_edit,
        ),
        (
            "a structure without a cell, or a cell without a structure",
            |w| w.entities.cell[10] = NO_CELL,
            no_edit,
        ),
        (
            "a structure without a cell, or a cell without a structure",
            |w| w.entities.cell[5] = 0,
            no_edit,
        ),
        (
            "an entity that cannot move has a behaviour or an order",
            |w| w.entities.behavior[9] = Behavior::Wander,
            no_edit,
        ),
        (
            "an entity that cannot move has a behaviour or an order",
            |w| w.entities.order[10] = Order::HoldPosition,
            no_edit,
        ),
        ("a random stream has an even increment", no_change, |b| {
            // `Rng` keeps its increment odd, so the even one is written into the bytes: the
            // low byte of slot 5's increment is the one byte a save with `inc + 2` differs in.
            let w = busy(fresh(), 8);
            let a = w.save().unwrap();
            let mut v = w.clone();
            let s = v.entities.rng[5].state();
            v.entities.rng[5] = Rng::from_state(RngState {
                inc: s.inc.wrapping_add(2),
                ..s
            });
            let c = v.save().unwrap();
            let at = (0..a.len()).find(|&i| a[i] != c[i]).unwrap();
            b[at] ^= 1;
        }),
        (
            "an order queue is longer than the ring",
            |w| {
                w.entities.order[5] = Order::Idle;
                w.entities.queue[5] = OrderQueue::EMPTY;
                w.entities.behavior[5] = Behavior::Idle;
            },
            |b| {
                // Slot 5 has no order and an empty queue: its queue length is the byte before its
                // `guard_walking` flag.
                let mut w = busy(fresh(), 8);
                w.entities.order[5] = Order::Idle;
                w.entities.queue[5] = OrderQueue::EMPTY;
                w.entities.behavior[5] = Behavior::Idle;
                let at = guard_walking_byte(&w) - 1;
                assert_eq!(b[at], 0);
                b[at] = ORDER_QUEUE_CAP as u8 + 1;
            },
        ),
        (
            "unknown order",
            |w| {
                w.entities.order[5] = Order::Idle;
                w.entities.queue[5] = OrderQueue::EMPTY;
                w.entities.behavior[5] = Behavior::Idle;
            },
            |b| {
                // The idle order of slot 5 is two bytes before its `guard_walking` flag.
                let mut w = busy(fresh(), 8);
                w.entities.order[5] = Order::Idle;
                w.entities.queue[5] = OrderQueue::EMPTY;
                w.entities.behavior[5] = Behavior::Idle;
                let at = guard_walking_byte(&w) - 2;
                assert_eq!(b[at], 0);
                b[at] = 9;
            },
        ),
        ("the free list", |w| w.free.push(5), no_edit),
        (
            "a free slot is missing from the free list",
            |w| {
                w.free.pop();
            },
            no_edit,
        ),
        (
            "two structures on one cell",
            |w| w.entities.cell[10] = w.entities.cell[9],
            no_edit,
        ),
        (
            "a footprint leaves the map",
            |w| w.entities.cell[10] = 64 * 48 - 1,
            no_edit,
        ),
        ("the occupied cells do not add up", no_change, |b| {
            let at = occupancy_offset(&busy(fresh(), 8));
            b[at] = 4;
        }),
        (
            "a pending command is for a past tick",
            |w| {
                let pending = BTreeMap::from([(
                    (Tick(2), PlayerId(0), 0),
                    Command::Stop { ids: Vec::new() },
                )]);
                w.queue = CommandQueue::from_parts(pending, BTreeMap::from([(PlayerId(0), 1)]));
            },
            no_edit,
        ),
        (
            "a pending command has the reserved number",
            |w| {
                let pending = BTreeMap::from([(
                    (Tick(900), PlayerId(0), u64::MAX),
                    Command::Stop { ids: Vec::new() },
                )]);
                w.queue = CommandQueue::from_parts(pending, BTreeMap::from([(PlayerId(0), 1)]));
            },
            no_edit,
        ),
        (
            "a command is numbered past its counter",
            |w| {
                let pending = BTreeMap::from([(
                    (Tick(900), PlayerId(0), 5),
                    Command::Stop { ids: Vec::new() },
                )]);
                w.queue = CommandQueue::from_parts(pending, BTreeMap::from([(PlayerId(0), 5)]));
            },
            no_edit,
        ),
        (
            "a command is numbered past its counter",
            |w| {
                let pending = BTreeMap::from([(
                    (Tick(900), PlayerId(1), 0),
                    Command::Stop { ids: Vec::new() },
                )]);
                w.queue = CommandQueue::from_parts(pending, BTreeMap::new());
            },
            no_edit,
        ),
        ("the command queue is out of order", two_despawns, |b| {
            // The second entry gets the first one's number: a duplicate key.
            let (_, body, _) = sections(b)[2];
            let entry = 8 + 1 + 8 + 9;
            let seq = body + 4 + entry + 9;
            assert_eq!(b[seq], 1);
            b[seq] = 0;
        }),
        (
            "the sequence counters are out of order",
            |w| {
                two_despawns(w);
                let (p, _) = w.queue.parts();
                w.queue = CommandQueue::from_parts(
                    p.clone(),
                    BTreeMap::from([(PlayerId(0), 2), (PlayerId(1), 7)]),
                );
            },
            |b| {
                let (_, body, len) = sections(b)[2];
                // The two counters are the last 18 bytes: swap the players.
                let end = body + len;
                assert_eq!((b[end - 18], b[end - 9]), (0, 1));
                b[end - 18] = 1;
                b[end - 9] = 0;
            },
        ),
        ("unknown command", no_change, |b| {
            // The first pending command's tag.
            let (_, body, _) = sections(b)[2];
            b[body + 4 + 17] = 99;
        }),
        (
            "unknown behaviour",
            |w| w.entities.target[2] = None,
            |b| {
                let mut w = busy(fresh(), 8);
                w.entities.target[2] = None;
                let _ = &mut w;
                // Slot 0's behaviour: after its generation, flag, kind, owner, position and facing.
                let (_, body, _) = sections(b)[1];
                let at = body + 4 + 4 + 1 + 2 + 1 + 16 + 1;
                assert!(b[at] <= 3);
                b[at] = 9;
            },
        ),
        ("wrong number of sections", no_change, |b| b[24] = 4),
        ("the tick cannot advance", no_change, |b| {
            b[16..24].copy_from_slice(&u64::MAX.to_le_bytes())
        }),
        ("a section has the wrong tag or length", no_change, |b| {
            b[28] = b'X'
        }),
        ("a section has the wrong tag or length", no_change, |b| {
            // The world section one byte longer: it then ends with a byte left over.
            let len = u32::from_le_bytes(b[29..33].try_into().unwrap());
            b[29..33].copy_from_slice(&(len + 1).to_le_bytes());
        }),
    ]
}

#[test]
fn every_impossible_world_is_corrupt() {
    for (k, (what, change, edit)) in corruptions().into_iter().enumerate() {
        let mut w = busy(fresh(), 8);
        change(&mut w);
        let mut bytes = w.save().expect("save does not judge");
        edit(&mut bytes);
        assert_eq!(
            World::restore(fresh(), &bytes),
            Err(SaveError::Corrupt(what)),
            "case {k}"
        );
    }
}

/// Offset of the occupancy count in the world section: header, tag and length, spawn serial, free
/// list.
fn occupancy_offset(w: &World) -> usize {
    28 + 1 + 4 + 8 + 4 + 4 * w.free.len()
}

#[test]
fn a_wrong_occupancy_count_is_corruption() {
    let w = busy(fresh(), 8);
    let mut bytes = w.save().unwrap();
    let at = occupancy_offset(&w);
    assert_eq!(&bytes[at..at + 4], &5u32.to_le_bytes());
    bytes[at] = 4;
    assert_eq!(
        World::restore(fresh(), &bytes),
        Err(SaveError::Corrupt("the occupied cells do not add up"))
    );
}

#[test]
fn a_broken_free_list_is_corruption() {
    let w = busy(fresh(), 8);
    let bytes = w.save().unwrap();
    // The free list is [3, 4]: make it [3, 3].
    let first = 28 + 1 + 4 + 8 + 4;
    let mut bad = bytes.clone();
    bad[first + 4] = 3;
    assert_eq!(
        World::restore(fresh(), &bad),
        Err(SaveError::Corrupt("the free list"))
    );
}

type Gate = (&'static str, fn(&mut World));

/// One change per section that this format does not save yet, each the smallest that leaves the
/// tick-0 state.
fn gates() -> Vec<Gate> {
    vec![
        ("pathing", |w| {
            w.entities.route[5].state = crate::path::RouteState::Wanted
        }),
        ("turning", |w| w.entities.turn_carry[6] = 1),
        ("formations", |w| {
            w.entities.form[5] = Member {
                serial: 1,
                ..Member::NONE
            }
        }),
        ("weapons", |w| w.entities.weapon[5][0].wait = 1),
        ("weapons", |w| {
            w.arsenal.attacks.push((5, EntityId::new(6, 0)));
        }),
        ("targeting", |w| w.entities.eng[5].cool = 1),
        ("buildings", |w| w.entities.build_left[10] = 1),
        ("repair", |w| w.entities.repairing[10] = true),
        ("repair", |w| w.entities.sell_left[10] = 3),
        ("destruction", |w| w.entities.kills[5] = 1),
        ("destruction", |w| {
            w.destruction.tally.push((PlayerId(0), Default::default()));
        }),
        ("pathing", |w| w.pathing.dirty_for_test(0)),
        ("pathing", |w| w.pathing.dirty_for_test(1)),
        ("pathing", |w| w.pathing.dirty_for_test(2)),
        ("pathing", |w| w.pathing.dirty_for_test(3)),
        ("pathing", |w| w.pathing.dirty_for_test(4)),
        ("flowfields", |w| w.pathing.dirty_for_test(5)),
        ("formations", |w| w.formations.bump_serial_for_test()),
        ("economy", |w| {
            w.harvest = Some(Box::new(crate::harvest::Harvest::new(
                crate::harvest::tests::RULES,
                Vec::new(),
                &w.units,
                &w.buildings,
            )));
        }),
        ("economy", |w| {
            w.money = Some(crate::money::Money {
                rules: crate::money::MoneyRules { credits_per_ore: 5 },
                spent: vec![(PlayerId(0), 0), (PlayerId(1), 30)],
            });
        }),
        ("production", |w| {
            w.production = Some(Box::new(crate::production::Production {
                rules: crate::production::ProductionRules {
                    queue_limit: 3,
                    exit_radius: 2,
                    build_speed: crate::production::BuildSpeed::FULL,
                },
                table: Default::default(),
                factories: Vec::new(),
                rejections: Vec::new(),
                produced: Vec::new(),
            }));
        }),
        ("vision", |w| {
            w.vision = Some(Box::new(crate::vision::Vision::new(
                64,
                48,
                Default::default(),
                Vec::new(),
            )));
        }),
        ("outcome", |w| {
            let rules = crate::outcome::MatchRules::new(&[PlayerId(0), PlayerId(1)]).unwrap();
            w.outcome = Some(Box::new(crate::outcome::Outcome::new(rules)));
        }),
        ("projectiles", |w| {
            w.arsenal.projectiles.insert(Projectile {
                weapon: 0,
                owner: EntityId::new(5, 0),
                player: PlayerId(0),
                target: Target::Point,
                aim: p(1, 1),
                pos: p(2, 2),
                prev_pos: p(2, 2),
                height: Fx::ZERO,
                prev_height: Fx::ZERO,
                heading: Facing::PLUS_X,
                span: Fx::ONE,
                fuse: 9,
            });
        }),
    ]
}

#[test]
fn a_section_that_is_not_saved_yet_refuses_the_save() {
    let base = busy(fresh(), 4);
    base.save().expect("the base world saves");
    for (k, (section, change)) in gates().into_iter().enumerate() {
        let mut w = base.clone();
        change(&mut w);
        assert_eq!(w.save(), Err(SaveError::Unsupported(section)), "gate {k}");
    }
}

#[test]
fn a_world_with_optional_rules_refuses_the_save_once_they_hold_state() {
    use crate::ore::OreRules;
    use crate::power::PowerRules;
    let rules = OreRules {
        start: 100,
        max: 200,
        regrow_amount: 1,
        regrow_period: 10,
    };
    let mut w = World::new(WorldConfig::new(3).with_map_size(16, 16))
        .unwrap()
        .with_ore(rules)
        .unwrap();
    // Ore at its start saves; once an amount changed it is economy state.
    w.save().unwrap();
    w.ore.as_mut().unwrap().version_bump_for_test();
    assert_eq!(w.save(), Err(SaveError::Unsupported("economy")));
    // Power is refused as soon as the world has the rules (WP13.1c saves the totals).
    let mut w = World::new(WorldConfig::new(3).with_map_size(16, 16)).unwrap();
    w.power = Some(crate::power::Power {
        rules: PowerRules {
            shortage_floor_permille: 250,
            defence_cutoff_permille: 500,
        },
        totals: Vec::new(),
    });
    assert_eq!(w.save(), Err(SaveError::Unsupported("power")));
}

#[test]
fn the_crowd_memory_round_trips_with_the_crowd_pass_on() {
    // With the crowd pass on, units bump into each other: their crowd memory is saved whole.
    for ticks in [3, 40, 120] {
        let w = busy(fresh_with(500, true), ticks);
        let back = World::restore(fresh_with(500, true), &w.save().unwrap()).unwrap();
        assert_eq!(settled(back), settled(w.clone()), "after {ticks} ticks");
    }
    let mut w = busy(fresh_with(500, true), 5);
    let c = &mut w.entities.crowd[5];
    c.side = Some((p(3, 4), 2));
    c.gave_up_at = Some(Tick(4));
    c.claim = Claim { serial: 9, slot: 3 };
    c.home = Some(p(1, 2));
    c.stranded = true;
    let back = World::restore(fresh_with(500, true), &w.save().unwrap()).unwrap();
    assert_eq!(back.entities.crowd[5], w.entities.crowd[5]);
}

#[test]
fn money_with_nothing_spent_is_no_economy_state() {
    let mut w = busy(fresh(), 4);
    w.money = Some(crate::money::Money {
        rules: crate::money::MoneyRules { credits_per_ore: 5 },
        spent: vec![(PlayerId(0), 0)],
    });
    w.save().expect("nothing spent saves");
}

/// The numbers of every optional rule set that a world which saves may have are in the digest.
#[test]
fn other_ore_money_or_repair_numbers_are_other_rules() {
    use crate::ore::OreRules;
    use crate::repair::{RepairRules, RepairSell};
    let ore = |start: u16| {
        World::new(WorldConfig::new(3).with_map_size(16, 16))
            .unwrap()
            .with_ore(OreRules {
                start,
                max: 200,
                regrow_amount: 1,
                regrow_period: 10,
            })
            .unwrap()
    };
    let bytes = ore(100).save().unwrap();
    assert!(World::restore(ore(100), &bytes).is_ok());
    assert_eq!(World::restore(ore(101), &bytes), Err(SaveError::Rules));

    let money = |credits_per_ore: u32| {
        let mut w = fresh();
        w.money = Some(crate::money::Money {
            rules: crate::money::MoneyRules { credits_per_ore },
            spent: Vec::new(),
        });
        w
    };
    let bytes = money(5).save().unwrap();
    assert!(World::restore(money(5), &bytes).is_ok());
    assert_eq!(World::restore(money(6), &bytes), Err(SaveError::Rules));

    let repair = |sell_ticks: u32| {
        let mut w = fresh();
        w.repair = Some(Box::new(RepairSell {
            rules: RepairRules {
                hp_per_tick: 2,
                full_cost_permille: 500,
                sell_ticks,
                refund_permille: 500,
            },
            rejections: Vec::new(),
            waiting: Vec::new(),
            sold: Vec::new(),
        }));
        w
    };
    let bytes = repair(20).save().unwrap();
    assert!(World::restore(repair(20), &bytes).is_ok());
    assert_eq!(World::restore(repair(21), &bytes), Err(SaveError::Rules));
}
