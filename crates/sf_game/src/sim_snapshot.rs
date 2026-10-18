//! What the presentation reads of the simulation (WP1.15).
//!
//! The simulation runs on its own thread ([`crate::sim_thread`]) and owns the
//! [`World`]. Nothing on the main thread may touch the world; it reads a
//! [`SimSnapshot`] instead: an immutable copy of what the picture, the
//! selection, the cursor, the minimap and the F1 panel ask of the world,
//! published by the sim thread after every tick.
//!
//! # What is in it
//!
//! * every live entity as an [`EntityView`] (id and generation, kind, owner,
//!   previous and current position and facing, hit points), in ascending slot
//!   order, so the previous tick and the current one are both in the snapshot
//!   and the renderer interpolates inside it;
//! * the current [`Order`] of each entity (the F1 panel names the hovered one's);
//! * the unit table, shared and never changing, so that an entity's
//!   [`UnitStats`] (class, movement class, weapon range, full hit points) are
//!   one lookup by kind, exactly what [`World::stats_of`] answers;
//! * the passability of every cell for every movement class, which is all the
//!   cursor asks of the terrain ([`Statics`]: shared, read once);
//! * the tick number, the checksum of the world after that tick and how the
//!   tick went ([`Pace`]).
//!
//! The methods carry the names of the [`World`] methods they stand in for
//! (`get`, `is_alive`, `stats_of`, `order_of`, `cell_at`, `passable`), so the
//! selection and cursor code reads the same as when it held the world.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sf_math::Vec2;
use sf_sim::{Cell, EntityId, EntityView, Order, Tick, UnitStats, UnitTable, World};

/// How a tick went, for the interpolation and the F1 panel.
#[derive(Copy, Clone, Debug)]
pub struct Pace {
    /// When the last tick of this snapshot was due on the sim thread's clock.
    /// The picture shows that tick's result [`crate::sim_thread::INTERP_DELAY`]
    /// later ([`crate::sim_thread::interpolation_alpha`]).
    pub due: Instant,
    /// How long the last tick took (the step, plus an artificial delay when
    /// `--sim-delay-ms` asked for one).
    pub step: Duration,
    /// How far behind its clock the sim thread was when it published this
    /// snapshot; zero when it keeps up.
    pub lag: Duration,
}

impl Pace {
    /// A pace for a snapshot taken outside the sim thread: due now, instant.
    pub fn now() -> Pace {
        Pace {
            due: Instant::now(),
            step: Duration::ZERO,
            lag: Duration::ZERO,
        }
    }
}

/// An immutable copy of the world for the presentation. See the module
/// documentation.
pub struct SimSnapshot {
    tick: Tick,
    checksum: Option<u64>,
    map_size: (u32, u32),
    views: Vec<EntityView>,
    orders: Vec<Order>,
    units: Arc<UnitTable>,
    ground: Arc<GroundMap>,
    pace: Pace,
}

impl SimSnapshot {
    /// An empty snapshot of nothing; the sim thread refills such a buffer
    /// with [`SimSnapshot::fill`].
    pub(crate) fn blank() -> SimSnapshot {
        SimSnapshot {
            tick: Tick::ZERO,
            checksum: None,
            map_size: (0, 0),
            views: Vec::new(),
            orders: Vec::new(),
            units: Arc::new(UnitTable::empty()),
            ground: Arc::new(GroundMap::open(0, 0)),
            pace: Pace::now(),
        }
    }

    /// A snapshot of `world` as it is. Used for the first snapshot, in tests
    /// and wherever there is no sim thread; the sim thread itself reuses its
    /// buffers with [`SimSnapshot::fill`].
    pub fn capture(world: &World) -> SimSnapshot {
        let mut snap = SimSnapshot::blank();
        snap.fill(world, &Statics::new(world), Pace::now());
        snap
    }

    /// Replaces the contents with the state of `world`, reusing the
    /// allocations. `statics` is what the world's snapshots share.
    pub(crate) fn fill(&mut self, world: &World, statics: &Statics, pace: Pace) {
        self.tick = world.tick();
        self.checksum = world.checksum_history().last().copied();
        self.map_size = world.map_size();
        self.views.clear();
        self.views.extend(world.snapshot().iter());
        self.orders.clear();
        self.orders.extend(
            self.views
                .iter()
                .map(|v| world.order_of(v.id).unwrap_or(Order::Idle)),
        );
        // The table never changes after the world is made: share one copy.
        self.units = Arc::clone(&statics.units);
        self.ground = Arc::clone(&statics.ground);
        self.pace = pace;
    }

    /// Ticks executed so far: also the number of the next tick, as
    /// [`World::tick`]. `pos` in the views is the state after tick
    /// `tick() - 1`.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The checksum of the world right after the last tick; `None` before
    /// the first.
    pub fn checksum(&self) -> Option<u64> {
        self.checksum
    }

    /// The map size in cells, `(width, height)`.
    pub fn map_size(&self) -> (u32, u32) {
        self.map_size
    }

    /// How the last tick went.
    pub fn pace(&self) -> &Pace {
        &self.pace
    }

    /// The number of live entities.
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// True if there are no live entities.
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// Every live entity, in ascending slot order.
    pub fn views(&self) -> &[EntityView] {
        &self.views
    }

    /// The position of `id` in [`SimSnapshot::views`]; `None` for a stale id.
    fn find(&self, id: EntityId) -> Option<usize> {
        let at = self
            .views
            .binary_search_by_key(&id.index(), |v| v.id.index())
            .ok()?;
        (self.views[at].id == id).then_some(at)
    }

    /// The entity `id` names, or `None` if it is gone or the id is stale
    /// (the generation check).
    pub fn get(&self, id: EntityId) -> Option<EntityView> {
        self.find(id).map(|i| self.views[i])
    }

    /// True if `id` names a live entity.
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.find(id).is_some()
    }

    /// The unit table entry an entity was spawned from; `None` for a stale id
    /// and for an entity made by the legacy `Spawn` or `Place`.
    pub fn stats_of(&self, id: EntityId) -> Option<&UnitStats> {
        let view = self.get(id)?;
        if view.has_stats {
            self.units.get(view.kind)
        } else {
            None
        }
    }

    /// The current order of an entity; `None` for a stale id.
    pub fn order_of(&self, id: EntityId) -> Option<Order> {
        self.find(id).map(|i| self.orders[i])
    }

    /// The cell a position lies in; `None` outside the map.
    pub fn cell_at(&self, pos: Vec2) -> Option<Cell> {
        let (x, y) = pos.cell();
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        (x < self.map_size.0 && y < self.map_size.1).then_some(Cell::new(x, y))
    }

    /// Whether a unit of movement class `class` may enter `cell`, as
    /// [`World::passable`] answered when this snapshot was taken.
    pub fn passable(&self, class: u16, cell: Cell) -> bool {
        self.ground.passable(class, cell)
    }
}

/// Which cells each movement class may enter, one bit per cell.
///
/// A class the unit table does not use is not stored: the world treats an
/// unknown class as open ground, and so does [`GroundMap::passable`].
#[derive(Clone)]
pub struct GroundMap {
    width: u32,
    height: u32,
    /// Ascending class numbers.
    classes: Vec<u16>,
    /// One bit set per class, row by row: set = passable.
    bits: Vec<Vec<u64>>,
}

impl GroundMap {
    /// A map on which every cell is passable for no class at all.
    fn open(width: u32, height: u32) -> GroundMap {
        GroundMap {
            width,
            height,
            classes: Vec::new(),
            bits: Vec::new(),
        }
    }

    fn index(&self, cell: Cell) -> Option<usize> {
        (cell.x < self.width && cell.y < self.height)
            .then(|| cell.y as usize * self.width as usize + cell.x as usize)
    }

    /// See [`SimSnapshot::passable`].
    pub fn passable(&self, class: u16, cell: Cell) -> bool {
        let Some(i) = self.index(cell) else {
            return false;
        };
        match self.classes.binary_search(&class) {
            Ok(k) => self.bits[k][i / 64] >> (i % 64) & 1 == 1,
            Err(_) => true,
        }
    }

    fn set(&mut self, k: usize, i: usize, passable: bool) {
        let word = &mut self.bits[k][i / 64];
        let bit = 1_u64 << (i % 64);
        if passable {
            *word |= bit;
        } else {
            *word &= !bit;
        }
    }
}

/// What every snapshot of one world shares and no tick changes: the unit table
/// and the ground. Made once, when the world moves to the sim thread.
///
/// The ground is the terrain's: [`World::passable`] answers from the terrain
/// and the movement table alone (occupancy is not part of it; the path
/// service asks about structures itself), and neither changes after the world
/// is made. If `passable` ever starts to depend on what stands on a cell, this
/// is the place to update the map per tick;
/// `passable_does_not_depend_on_structures` in the tests fails when it does.
pub(crate) struct Statics {
    units: Arc<UnitTable>,
    ground: Arc<GroundMap>,
}

impl Statics {
    /// Reads the unit table and the whole ground of `world` once.
    pub(crate) fn new(world: &World) -> Statics {
        let (width, height) = world.map_size();
        let mut classes: Vec<u16> = world
            .unit_table()
            .iter()
            .filter_map(|s| s.movement_class)
            .collect();
        classes.sort_unstable();
        classes.dedup();
        let words = (width as usize * height as usize).div_ceil(64);
        let mut map = GroundMap {
            width,
            height,
            bits: vec![vec![0; words]; classes.len()],
            classes,
        };
        for y in 0..height {
            for x in 0..width {
                let i = y as usize * width as usize + x as usize;
                for k in 0..map.classes.len() {
                    map.set(k, i, world.passable(map.classes[k], Cell::new(x, y)));
                }
            }
        }
        Statics {
            units: Arc::new(world.unit_table().clone()),
            ground: Arc::new(map),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_data::movement::MovementCosts;
    use sf_data::terrain::TerrainTypes;
    use sf_data::units::UnitDefs;
    use sf_sim::{Command, Kind, PlayerId, Terrain, WorldConfig};

    const SIZE: u32 = 32;

    fn world() -> (World, UnitDefs) {
        let defs = UnitDefs::embedded();
        let types = TerrainTypes::embedded();
        let costs = MovementCosts::embedded();
        // Grass, with a river (water) down columns 10 to 12.
        let layer: Vec<u8> = (0..SIZE * SIZE)
            .map(|i| {
                if (10..=12).contains(&(i % SIZE)) {
                    2
                } else {
                    0
                }
            })
            .collect();
        let terrain =
            Terrain::from_layer(SIZE, SIZE, &layer, &sf_bridge::terrain_defs(&types, &costs))
                .expect("the layer is valid");
        let world = World::with_terrain(WorldConfig::new(3).with_map_size(SIZE, SIZE), terrain)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        (crate::playfield::with_movement(world, &defs), defs)
    }

    fn schedule(world: &mut World, command: Command) {
        world.schedule(world.tick(), PlayerId(0), command).unwrap();
        world.step();
    }

    fn same_ground(snap: &SimSnapshot, world: &World) {
        for class in 0..40_u16 {
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let cell = Cell::new(x, y);
                    assert_eq!(
                        snap.passable(class, cell),
                        world.passable(class, cell),
                        "class {class} at {cell}"
                    );
                }
            }
        }
        // Off the map nothing is passable, and the snapshot does not guess.
        assert!(!snap.passable(1, Cell::new(SIZE, 0)));
        assert!(snap.cell_at(Vec2::from_ints(-1, 3)).is_none());
        assert_eq!(snap.cell_at(Vec2::from_ints(5, 7)), Some(Cell::new(5, 7)));
        assert!(snap.cell_at(Vec2::from_ints(SIZE as i32, 0)).is_none());
    }

    /// The snapshot's ground is the world's, cell by cell and class by class.
    /// And the world's `passable` is terrain only: a structure on a cell does
    /// not change it, which is why the ground is read once ([`Statics`]). If
    /// this test fails because `passable` now looks at occupancy, the ground
    /// has to be refreshed per tick.
    #[test]
    fn passable_does_not_depend_on_structures() {
        let (mut world, defs) = world();
        let statics = Statics::new(&world);
        let mut snap = SimSnapshot::blank();
        snap.fill(&world, &statics, Pace::now());
        same_ground(&snap, &world);
        // The river is impassable on foot and open to boats.
        let classes: Vec<u16> = world
            .unit_table()
            .iter()
            .filter_map(|s| s.movement_class)
            .collect();
        assert!(
            classes.iter().any(
                |&c| world.passable(c, Cell::new(5, 5)) && !world.passable(c, Cell::new(11, 5))
            ),
            "some class crosses grass and not water"
        );

        let kind = Kind(defs.kind_of("barracks").unwrap());
        schedule(
            &mut world,
            Command::PlaceStructure {
                kind,
                cell: Cell::new(4, 4),
                owner: PlayerId(0),
            },
        );
        assert!(world.is_occupied(Cell::new(4, 4)));
        snap.fill(&world, &statics, Pace::now());
        same_ground(&snap, &world);
    }

    /// Lookups by id are the world's: a stale id is nothing.
    #[test]
    fn stale_ids_are_not_found() {
        let (mut world, defs) = world();
        let kind = Kind(defs.kind_of("rifleman").unwrap());
        schedule(
            &mut world,
            Command::SpawnUnit {
                kind,
                pos: Vec2::from_ints(3, 3),
                owner: PlayerId(0),
            },
        );
        let id = world.snapshot().iter().next().unwrap().id;
        let snap = SimSnapshot::capture(&world);
        assert!(snap.is_alive(id));
        assert_eq!(snap.stats_of(id).map(|s| s.kind), Some(kind));
        assert_eq!(snap.order_of(id), Some(Order::Idle));
        let stale = EntityId::new(id.index(), id.generation() + 1);
        assert!(!snap.is_alive(stale));
        assert!(snap.get(stale).is_none() && snap.stats_of(stale).is_none());
        assert!(snap.order_of(stale).is_none());
        assert!(!snap.is_alive(EntityId::new(900, 0)));
    }
}
