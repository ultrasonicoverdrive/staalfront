//! What the AI remembers of the structures it has seen.
//!
//! A human sees a structure once and then remembers where it stands, also when the fog closes over it again.
//! The AI does the same, on its own side: [`update`] writes every visible structure of another player into
//! the map, with the tick, and drops a structure the observation says is gone (one of its cells is visible
//! and it is not seen there). A structure that was visible only between two thinks is missed; that is
//! acceptable at half a second.

use std::collections::BTreeMap;

use sf_sim::EntityId;

use crate::observe::Observation;
use crate::state::Known;

/// Brings `known` up to date with `obs`: forgets what is gone, then remembers what is seen.
pub fn update(known: &mut BTreeMap<EntityId, Known>, obs: &Observation) {
    for id in &obs.gone {
        known.remove(id);
    }
    for s in &obs.visible {
        let Some(fp) = s.footprint else {
            continue;
        };
        known.insert(
            s.id,
            Known {
                kind: s.kind,
                owner: s.owner,
                anchor: s.cell,
                width: fp.width,
                height: fp.height,
                seen_tick: obs.tick,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::Seen;
    use sf_sim::{Cell, Footprint, Kind, PlayerId, Tick};

    fn obs(tick: u64, visible: Vec<Seen>, gone: Vec<EntityId>) -> Observation {
        Observation {
            tick: Tick(tick),
            player: PlayerId(0),
            map: (64, 64),
            credits: 0,
            ore_stored: 0,
            storage_capacity: 0,
            power: None,
            own: Vec::new(),
            visible,
            gone,
            out_of_play: false,
        }
    }

    fn structure(index: u32, x: u32, y: u32) -> Seen {
        Seen {
            id: EntityId::new(index, 0),
            kind: Kind(1),
            owner: PlayerId(1),
            cell: Cell::new(x, y),
            footprint: Some(Footprint {
                width: 2,
                height: 3,
            }),
            hp: 100,
            armed: false,
        }
    }

    fn unit(index: u32) -> Seen {
        Seen {
            id: EntityId::new(index, 0),
            kind: Kind(12),
            owner: PlayerId(1),
            cell: Cell::new(5, 5),
            footprint: None,
            hp: 50,
            armed: false,
        }
    }

    #[test]
    fn a_seen_structure_is_remembered_with_the_tick() {
        let mut known = BTreeMap::new();
        update(&mut known, &obs(30, vec![structure(3, 10, 12)], vec![]));
        let k = known[&EntityId::new(3, 0)];
        assert_eq!((k.anchor, k.width, k.height), (Cell::new(10, 12), 2, 3));
        assert_eq!(
            (k.kind, k.owner, k.seen_tick),
            (Kind(1), PlayerId(1), Tick(30))
        );
    }

    #[test]
    fn a_structure_stays_in_memory_when_it_is_no_longer_seen() {
        let mut known = BTreeMap::new();
        update(&mut known, &obs(30, vec![structure(3, 10, 12)], vec![]));
        // The fog closes: it is neither visible nor gone.
        update(&mut known, &obs(40, vec![], vec![]));
        update(&mut known, &obs(50, vec![], vec![]));
        assert_eq!(known[&EntityId::new(3, 0)].seen_tick, Tick(30));
    }

    #[test]
    fn seeing_it_again_updates_the_tick() {
        let mut known = BTreeMap::new();
        update(&mut known, &obs(30, vec![structure(3, 10, 12)], vec![]));
        update(&mut known, &obs(90, vec![structure(3, 10, 12)], vec![]));
        assert_eq!(known[&EntityId::new(3, 0)].seen_tick, Tick(90));
        assert_eq!(known.len(), 1);
    }

    #[test]
    fn a_structure_seen_gone_is_forgotten() {
        let mut known = BTreeMap::new();
        update(
            &mut known,
            &obs(30, vec![structure(3, 10, 12), structure(4, 20, 20)], vec![]),
        );
        update(&mut known, &obs(40, vec![], vec![EntityId::new(3, 0)]));
        assert!(!known.contains_key(&EntityId::new(3, 0)));
        assert!(known.contains_key(&EntityId::new(4, 0)));
    }

    #[test]
    fn units_are_not_remembered() {
        let mut known = BTreeMap::new();
        update(&mut known, &obs(30, vec![unit(7)], vec![]));
        assert!(known.is_empty());
    }
}
