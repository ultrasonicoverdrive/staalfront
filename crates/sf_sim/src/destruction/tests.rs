//! What the destruction section of the checksum covers: each part of the state changes it, and the
//! report of the last tick does not. (The behaviour is tested through the public API in
//! `tests/destruction.rs`.)

use super::*;
use crate::world::WorldConfig;

fn world() -> World {
    World::new(WorldConfig::new(1).with_map_size(16, 16))
        .unwrap()
        .with_destruction(DestructionConfig::DEFAULT)
        .unwrap()
}

fn wreck() -> Wreck {
    Wreck {
        kind: WreckKind::Vehicle,
        unit: Kind(3),
        owner: PlayerId(1),
        pos: Vec2::from_ints(4, 5),
        facing: Facing::new(40),
        size: Fx::from_ratio(3, 2),
        footprint: None,
        anchor: None,
        born: Tick(7),
        expires: Tick(607),
    }
}

#[test]
fn every_part_of_the_state_is_in_the_checksum() {
    let base = world();
    let before = base.checksum();

    let mut with_wreck = base.clone();
    with_wreck.destruction.add_wreck(wreck());
    assert_ne!(with_wreck.checksum(), before, "a wreck");

    let changes: [fn(&mut Wreck); 8] = [
        |w| w.pos = Vec2::from_ints(4, 6),
        |w| w.facing = Facing::new(41),
        |w| w.size = Fx::from_int(2),
        |w| w.unit = Kind(4),
        |w| w.owner = PlayerId(2),
        |w| w.expires = Tick(608),
        |w| w.born = Tick(8),
        |w| {
            w.kind = WreckKind::Rubble;
            w.footprint = Some(Footprint {
                width: 2,
                height: 3,
            });
            w.anchor = Some(Cell::new(4, 5));
        },
    ];
    for change in changes {
        let mut other = base.clone();
        let mut w = wreck();
        change(&mut w);
        other.destruction.add_wreck(w);
        assert_ne!(other.checksum(), with_wreck.checksum(), "{w:?}");
    }

    let mut with_tally = base.clone();
    with_tally.destruction.tally_of(PlayerId(1)).kills = 1;
    assert_ne!(with_tally.checksum(), before, "a kill");
    let mut with_assist = base.clone();
    with_assist.destruction.tally_of(PlayerId(1)).assists = 1;
    assert_ne!(with_assist.checksum(), with_tally.checksum(), "an assist");
    let mut with_loss = base.clone();
    with_loss.destruction.tally_of(PlayerId(1)).losses = 1;
    assert_ne!(with_loss.checksum(), with_tally.checksum(), "a loss");
    let mut other_player = base.clone();
    other_player.destruction.tally_of(PlayerId(2)).kills = 1;
    assert_ne!(other_player.checksum(), with_tally.checksum(), "whose kill");

    let mut with_kills = base.clone();
    let id = with_kills
        .spawn(Kind(0), Vec2::from_ints(3, 3), PlayerId(0))
        .unwrap();
    let spawned = with_kills.checksum();
    with_kills.entities.kills[id.index() as usize] = 2;
    assert_ne!(with_kills.checksum(), spawned, "the kills of a unit");
}

#[test]
fn the_report_of_the_last_tick_is_not_in_the_checksum() {
    let base = world();
    let mut other = base.clone();
    other.destruction.deaths.push(Death {
        tick: Tick(3),
        victim: EntityId::new(1, 0),
        kind: Kind(1),
        owner: PlayerId(0),
        class: UnitClass::Mobile,
        pos: Vec2::from_ints(1, 1),
        facing: Facing::default(),
        size: Fx::ONE,
        footprint: None,
        wreck: None,
        killer: None,
        assists: Vec::new(),
    });
    assert_eq!(other.checksum(), base.checksum());
}

#[test]
fn wrecks_are_kept_in_a_total_order_whatever_the_order_they_come_in() {
    let mut a = Destruction::default();
    let mut b = Destruction::default();
    let list: Vec<Wreck> = (0..6u16)
        .map(|i| Wreck {
            pos: Vec2::from_ints(i32::from(i % 3), i32::from(5 - i)),
            unit: Kind(i),
            ..wreck()
        })
        .collect();
    for w in &list {
        a.add_wreck(*w);
    }
    for w in list.iter().rev() {
        b.add_wreck(*w);
    }
    assert_eq!(a.wrecks, b.wrecks);
    assert!(a.wrecks.windows(2).all(|p| p[0].key() <= p[1].key()));
}
