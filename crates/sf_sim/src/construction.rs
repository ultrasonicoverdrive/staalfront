//! A structure under construction (WP6.3, `docs/rules/construction.md`).
//!
//! A structure a player builds with [`Command::BuildStructure`](crate::Command::BuildStructure)
//! is placed **under construction**: the cells are taken at once, but it does not count for
//! anything until its `build_ticks` (from the building table) have run. The state is one integer
//! per entity, the ticks still to go (`Entities::build_left`); zero means complete. Everything
//! the presentation shows of a construction (the foundation, the scaffolding, a progress bar) is
//! worked out from that integer, never the other way round.
//!
//! # The order inside a tick
//!
//! **The state is the truth, the presentation follows it** (the "animation before mission logic"
//! rule of the analysis, section 10, read the way this project uses it: the picture may never run
//! ahead of the rule, and the rule never waits for the picture). In [`World::step`]:
//!
//! (1) the structures under construction advance by one tick ([`World::advance_construction`]),
//!    in ascending slot order; one that reaches zero is complete and a [`Completion`] is recorded
//!    for this tick ([`World::completions`], counted in `StepReport::structures_completed`);
//! (2) then the commands of the tick run: a structure that completed in step 1 already stands for
//!    a `BuildStructure` of the same tick (a prerequisite, a base anchor);
//! (3) the rest of the tick as before.
//!
//! A structure placed in the step of tick `t` with `n` build ticks has `n` ticks left after
//! `t`, and is complete after tick `t + n` (its [`Completion`] says `tick: t + n`). The
//! presentation reads the state after a tick; it can show the completed model only from a
//! snapshot in which the state is already complete, and the completion is in the same step's
//! report. A mission trigger "structure completed" listens to [`World::completions`], not to
//! anything drawn.
//!
//! # What counts only when complete
//!
//! [`World::stands`] is true for a structure that is complete and has hit points left (or cannot
//! be damaged). Placement uses it for prerequisites and for the base anchors; power (WP6.4) and
//! production (WP6.5) are to use it. A structure under construction still occupies its cells,
//! blocks ground units and can be hurt like any structure; destroying it drops the state with
//! the entity.

use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::world::World;

/// How far along a structure under construction is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Construction {
    /// Ticks still to go; at least 1 (a complete structure has no `Construction`).
    pub left: u32,
    /// Ticks in all (the structure's `build_ticks`).
    pub total: u32,
}

impl Construction {
    /// Progress in thousandths, rounded down: 0 on the tick it was placed, at most 999 while
    /// it is under construction.
    pub fn permille(self) -> u16 {
        if self.total == 0 {
            return 1000;
        }
        let done = u64::from(self.total.saturating_sub(self.left));
        (done * 1000 / u64::from(self.total)).min(999) as u16
    }
}

/// A structure that became complete in the last step. Kept until the next step; this is where a
/// mission or a rule hooks in for "structure completed".
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// The tick whose step completed it.
    pub tick: Tick,
    /// The structure.
    pub id: EntityId,
    /// Its kind.
    pub kind: Kind,
    /// Its owner.
    pub owner: PlayerId,
}

impl World {
    /// The construction of the structure `id`, or `None` if it is complete, is not a structure
    /// or is gone.
    pub fn construction_of(&self, id: EntityId) -> Option<Construction> {
        let i = self.live_index(id)?;
        let left = self.entities.build_left[i];
        (left > 0).then(|| Construction {
            left,
            total: self
                .buildings
                .get(self.entities.kind[i])
                .map_or(left, |b| b.build_ticks),
        })
    }

    /// Every structure under construction with its state, in ascending slot order.
    pub fn constructions(&self) -> impl Iterator<Item = (EntityId, Construction)> + '_ {
        (0..self.entities.len()).filter_map(move |i| {
            if !self.entities.alive[i] || self.entities.build_left[i] == 0 {
                return None;
            }
            let id = EntityId::new(i as u32, self.entities.generation[i]);
            self.construction_of(id).map(|c| (id, c))
        })
    }

    /// How many structures are under construction.
    pub fn under_construction(&self) -> usize {
        (0..self.entities.len())
            .filter(|&i| self.entities.alive[i] && self.entities.build_left[i] > 0)
            .count()
    }

    /// Whether the structure `id` stands: complete, and with hit points left (or one that cannot
    /// be damaged). What the tech tree, the base, power and production ask. False for an unknown
    /// or stale id.
    pub fn stands(&self, id: EntityId) -> bool {
        self.live_index(id).is_some_and(|i| self.is_standing(i))
    }

    /// The structures that became complete in the last step, in the order they did (ascending
    /// slot). Replaced by every step.
    pub fn completions(&self) -> &[Completion] {
        &self.completions
    }

    /// The first thing a step does: every structure under construction comes one tick closer.
    /// One at zero hit points is dead and does not advance (it would complete as a wreck); one
    /// that reaches zero is complete and recorded.
    pub(crate) fn advance_construction(&mut self) {
        self.completions.clear();
        for i in 0..self.entities.len() {
            if !self.entities.alive[i]
                || self.entities.build_left[i] == 0
                || !self.has_hit_points(i)
            {
                continue;
            }
            self.entities.build_left[i] -= 1;
            if self.entities.build_left[i] == 0 {
                self.completions.push(Completion {
                    tick: self.tick,
                    id: EntityId::new(i as u32, self.entities.generation[i]),
                    kind: self.entities.kind[i],
                    owner: self.entities.owner[i],
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings::{BuildingFunction, BuildingStats, BuildingTable};
    use crate::command::Command;
    use crate::terrain::Cell;
    use crate::units::{Footprint, UnitStats, UnitTable};
    use crate::world::WorldConfig;
    use sf_math::Fx;

    const ANCHOR: Kind = Kind(1);
    const HUT: Kind = Kind(2);

    fn world() -> World {
        let fp = Footprint {
            width: 2,
            height: 2,
        };
        let unit = |kind| UnitStats {
            kind,
            hp: 50,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(fp),
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        };
        let building = |kind, function, cost: bool, radius| BuildingStats {
            kind,
            footprint: fp,
            function,
            exit_side: None,
            build_ticks: if cost { 20 } else { 0 },
            cost: cost.then_some(10),
            power: 0,
            factions: 1,
            requires: Vec::new(),
            build_radius: radius,
        };
        World::new(WorldConfig::new(1).with_map_size(32, 32))
            .unwrap()
            .with_units(UnitTable::new(vec![unit(ANCHOR), unit(HUT)]).unwrap())
            .unwrap()
            .with_buildings(
                BuildingTable::new(vec![
                    building(ANCHOR, BuildingFunction::Anchor, false, 6),
                    building(HUT, BuildingFunction::None, true, 0),
                ])
                .unwrap(),
            )
            .unwrap()
            .with_player_factions(&[(PlayerId(0), 1)])
            .unwrap()
    }

    fn send(w: &mut World, command: Command) {
        let tick = w.tick();
        w.schedule(tick, PlayerId(0), command).unwrap();
        w.step();
    }

    #[test]
    fn a_structure_at_zero_hit_points_does_not_finish_building_and_fires_no_event() {
        let mut w = world();
        send(
            &mut w,
            Command::PlaceStructure {
                kind: ANCHOR,
                cell: Cell::new(10, 10),
                owner: PlayerId(0),
            },
        );
        send(
            &mut w,
            Command::BuildStructure {
                kind: HUT,
                cell: Cell::new(14, 10),
            },
        );
        let hut = w.occupant_at(Cell::new(14, 10)).unwrap();
        let slot = hut.index() as usize;
        // It is shot to pieces while it is going up (a later work package removes it at once; until
        // then it lies where it fell, and a wreck must not "complete").
        w.entities.hp[slot] = 0;
        let mut fired = 0;
        for _ in 0..60 {
            fired += w.step().structures_completed;
        }
        assert_eq!(fired, 0);
        assert_eq!(w.construction_of(hut).map(|c| c.left), Some(20));
        assert!(!w.stands(hut));
        // Repaired (hit points back), it carries on from where it stopped.
        w.entities.hp[slot] = 50;
        for _ in 0..19 {
            assert_eq!(w.step().structures_completed, 0);
        }
        assert_eq!(w.step().structures_completed, 1);
        assert!(w.stands(hut));
    }

    #[test]
    fn a_complete_structure_at_zero_hit_points_does_not_stand() {
        let mut w = world();
        send(
            &mut w,
            Command::PlaceStructure {
                kind: HUT,
                cell: Cell::new(10, 10),
                owner: PlayerId(0),
            },
        );
        let hut = w.occupant_at(Cell::new(10, 10)).unwrap();
        assert!(w.stands(hut));
        w.entities.hp[hut.index() as usize] = 0;
        assert!(!w.stands(hut));
    }
}
