//! Who is on the demo map: which sim entity kinds exist, what they look like
//! and where they start.
//!
//! The sim knows an entity kind only as a number. This module is the
//! stand-in for data-driven units (phase 2): it maps a [`Kind`] to a *shape*
//! of the instancer (a placeholder cube, cylinder or cone, or one model of
//! the glTF pack) and says whether it moves. Everything is derived from the
//! pack, nothing from model names: a model with a square strategic icon is a
//! building and stands still, the others are vehicles and wander.
//!
//! Placement is in sim **cells** (B12, 4 m each) and comes from a seeded
//! `sf_math::Rng`, so the same options always give the same world and the
//! same checksums.

use sf_math::{Fx, Rng, Vec2};
use sf_render::IconShape;
use sf_render::glb::LoadedPack;
use sf_sim::{Command, EntityView, Kind, PlayerId, Tick, World};

/// Placeholder shapes of `units.ron` (cube, cylinder, cone).
pub const PLACEHOLDER_SHAPES: u32 = 3;

/// Buildings of each building model that the demo puts up.
pub const BUILDINGS_PER_MODEL: u32 = 6;

/// Spacing of the base's building grid, in cells (24 m).
const BASE_SPACING: i32 = 6;

/// How often a placeholder shape is drawn compared with one vehicle model.
const PLACEHOLDER_WEIGHT: u32 = 10;

/// What one entity kind is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindSpec {
    /// Shape index in the instancer's style.
    pub shape: u8,
    /// Wanders (units) or stands still (buildings).
    pub mobile: bool,
    /// Relative share among the mobile units.
    pub weight: u32,
}

/// The kinds of a run and the rule that spawns them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Population {
    kinds: Vec<KindSpec>,
}

impl Population {
    /// The placeholder shapes plus, with a pack, one kind per model after
    /// them (kind `PLACEHOLDER_SHAPES + i` is model `i`, shape the same).
    pub fn new(pack: Option<&LoadedPack>) -> Self {
        let mut kinds: Vec<KindSpec> = (0..PLACEHOLDER_SHAPES)
            .map(|i| KindSpec {
                shape: i as u8,
                mobile: true,
                weight: PLACEHOLDER_WEIGHT,
            })
            .collect();
        if let Some(pack) = pack {
            for (i, icon) in pack.icons.iter().enumerate() {
                let building = *icon == IconShape::Square;
                kinds.push(KindSpec {
                    shape: (PLACEHOLDER_SHAPES as usize + i).min(255) as u8,
                    mobile: !building,
                    weight: 1,
                });
            }
        }
        Self { kinds }
    }

    pub fn kinds(&self) -> &[KindSpec] {
        &self.kinds
    }

    /// The shape an entity kind is drawn as.
    pub fn shape_of(&self, kind: Kind) -> u8 {
        self.kinds
            .get(usize::from(kind.0))
            .map_or(kind.0 as u8, |k| k.shape)
    }

    /// Number of buildings for `units` entities: every building model
    /// [`BUILDINGS_PER_MODEL`] times, but never more than a tenth of the
    /// units.
    pub fn building_count(&self, units: u32) -> u32 {
        let models = self.kinds.iter().filter(|k| !k.mobile).count() as u32;
        (models * BUILDINGS_PER_MODEL).min(units / 10)
    }

    /// Schedule the spawn of `units` entities for tick 0 on a map of
    /// `cells` (width, height): the buildings in a base around the map
    /// centre (kinds in turn, owners in turn), the rest wandering units
    /// spread over the whole map. Returns the number of buildings.
    pub fn spawn(&self, world: &mut World, units: u32, cells: (u32, u32)) -> u32 {
        let buildings = self.building_count(units);
        let building_kinds: Vec<usize> = (0..self.kinds.len())
            .filter(|&k| !self.kinds[k].mobile)
            .collect();
        let mobile_kinds: Vec<usize> = (0..self.kinds.len())
            .flat_map(|k| {
                let spec = &self.kinds[k];
                std::iter::repeat_n(k, if spec.mobile { spec.weight as usize } else { 0 })
            })
            .collect();
        let mut schedule = |kind: usize, pos: Vec2, owner: u32| {
            let command = Command::Spawn {
                kind: Kind(kind as u16),
                pos,
                owner: PlayerId((owner % 4) as u8),
            };
            world
                .schedule(Tick(0), PlayerId(0), command)
                .expect("tick 0 is not in the past");
        };

        // The base: a square grid of buildings around the centre of the map.
        let side = (f64::from(buildings).sqrt().ceil() as i32).max(1);
        let centre = (cells.0 as i32 / 2, cells.1 as i32 / 2);
        for i in 0..buildings {
            let (gx, gz) = ((i as i32) % side, (i as i32) / side);
            let at = |g: i32, c: i32| c + (g - side / 2) * BASE_SPACING;
            let pos = Vec2::from_ints(at(gx, centre.0), at(gz, centre.1));
            schedule(
                building_kinds[(i as usize) % building_kinds.len().max(1)],
                pos,
                i / 3,
            );
        }

        // The wanderers.
        let mut rng = Rng::new(0x5742_0106);
        let (w, h) = (Fx::from_int(cells.0 as i32), Fx::from_int(cells.1 as i32));
        for i in 0..units - buildings {
            let pos = Vec2::new(rng.fx_range(Fx::ZERO, w), rng.fx_range(Fx::ZERO, h));
            schedule(mobile_kinds[(i as usize) % mobile_kinds.len()], pos, i / 3);
        }
        buildings
    }

    /// After tick 0 has run: make the buildings stand still. The sim only
    /// knows `MoveTo` (walk to a target and idle there), so each building is
    /// told to move to where it is. Scheduled for the next tick.
    pub fn plant_buildings(&self, world: &mut World, views: &[EntityView]) {
        for view in views {
            let spec = self.kinds.get(usize::from(view.kind.0));
            if spec.is_some_and(|k| !k.mobile) {
                let command = Command::MoveTo {
                    ids: vec![view.id],
                    target: view.pos,
                };
                let tick = world.tick();
                world
                    .schedule(tick, PlayerId(0), command)
                    .expect("the next tick is not in the past");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_sim::WorldConfig;

    fn world(cells: u32) -> World {
        World::new(WorldConfig::new(1).with_map_size(cells, cells)).unwrap()
    }

    fn placeholders_and_two_buildings() -> Population {
        let mut p = Population::new(None);
        p.kinds.push(KindSpec {
            shape: 3,
            mobile: true,
            weight: 1,
        });
        for shape in [4, 5] {
            p.kinds.push(KindSpec {
                shape,
                mobile: false,
                weight: 1,
            });
        }
        p
    }

    #[test]
    fn placeholders_only_has_three_wandering_kinds() {
        let p = Population::new(None);
        assert_eq!(p.kinds().len(), 3);
        assert!(p.kinds().iter().all(|k| k.mobile));
        assert_eq!(p.building_count(5000), 0);
        assert_eq!(p.shape_of(Kind(2)), 2);
    }

    #[test]
    fn buildings_are_few_and_never_more_than_a_tenth() {
        let p = placeholders_and_two_buildings();
        assert_eq!(p.building_count(5000), 2 * BUILDINGS_PER_MODEL);
        assert_eq!(p.building_count(50), 5);
        assert_eq!(p.building_count(5), 0);
    }

    #[test]
    fn spawn_makes_the_requested_number_of_entities_in_the_map() {
        let p = placeholders_and_two_buildings();
        let mut w = world(64);
        let buildings = p.spawn(&mut w, 400, (64, 64));
        w.step();
        assert_eq!((buildings, w.len()), (2 * BUILDINGS_PER_MODEL, 400));
        let mut views = Vec::new();
        w.snapshot().collect_into(&mut views);
        let static_ones = views.iter().filter(|v| v.kind.0 >= 4).count();
        assert_eq!(static_ones as u32, buildings);
        assert!(
            views
                .iter()
                .all(|v| v.pos.x >= Fx::ZERO && v.pos.x <= Fx::from_int(64))
        );
    }

    #[test]
    fn planted_buildings_stand_still_and_the_rest_moves() {
        let p = placeholders_and_two_buildings();
        let mut w = world(64);
        p.spawn(&mut w, 400, (64, 64));
        w.step();
        let mut views = Vec::new();
        w.snapshot().collect_into(&mut views);
        p.plant_buildings(&mut w, &views);
        let start: Vec<_> = views.iter().map(|v| (v.id, v.kind, v.pos)).collect();
        for _ in 0..40 {
            w.step();
        }
        w.snapshot().collect_into(&mut views);
        let moved = |id, kind: Kind, pos| {
            views
                .iter()
                .find(|v| v.id == id && v.kind == kind)
                .is_some_and(|v| v.pos != pos)
        };
        for (id, kind, pos) in start {
            let building = p.kinds()[usize::from(kind.0)].mobile;
            assert_eq!(moved(id, kind, pos), building, "kind {}", kind.0);
        }
    }

    #[test]
    fn the_same_options_give_the_same_world() {
        let p = placeholders_and_two_buildings();
        let run = || {
            let mut w = world(64);
            p.spawn(&mut w, 300, (64, 64));
            for _ in 0..30 {
                w.step();
            }
            w.checksum_history().last().copied()
        };
        assert_eq!(run(), run());
    }
}
