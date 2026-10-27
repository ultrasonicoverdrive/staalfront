//! What the picture shows of the ore (WP7.1, `docs/rules/economy.md`).
//!
//! The sim keeps an amount per ore cell; the picture shows it as one of a few fill levels
//! of the ore look (`terrain_look.ron`, `resource`), on the terrain and on the minimap.
//! [`OreView`] remembers the level each cell is drawn with and, for every new snapshot,
//! hands back only the cells whose level changed, so a field that grows by one unit at a
//! time costs nothing until a cell crosses a level boundary.

use sf_render::terrain_look::{ResourceLooks, TerrainLooks};

use crate::sim_snapshot::SimSnapshot;

/// A level no cell has: nothing has been drawn yet.
const NOT_DRAWN: u8 = u8::MAX;

/// The fill levels the picture shows, and the ones it has drawn.
pub struct OreView {
    resource: Option<ResourceLooks>,
    /// The version of the ore state last compared; `None` before the first.
    version: Option<u64>,
    /// The level each field cell is drawn with, in the snapshot's order.
    drawn: Vec<u8>,
}

impl OreView {
    /// A view with the shipped looks' levels. Without a `resource` block in the looks it
    /// never reports a change.
    pub fn new(looks: &TerrainLooks) -> OreView {
        OreView {
            resource: looks.resource().cloned(),
            version: None,
            drawn: Vec::new(),
        }
    }

    /// The minimap colour of a level (sRGB), or `None` for a level that does not exist.
    pub fn minimap_color(&self, level: u8) -> Option<[u8; 3]> {
        self.resource
            .as_ref()?
            .levels
            .get(usize::from(level))
            .map(|l| l.minimap)
    }

    /// The cells `(x, y, level)` whose drawn level differs from the level their amount
    /// has now. Empty if the snapshot has no ore, the looks have no levels, or the ore state
    /// is the one compared last. The drawn levels are updated: report the changes to the
    /// renderer, or call [`OreView::forget`] to have them reported again.
    pub fn changes(&mut self, snap: &SimSnapshot) -> Vec<(u32, u32, u8)> {
        let (Some(resource), Some(version)) = (&self.resource, snap.ore_version()) else {
            return Vec::new();
        };
        if self.version == Some(version) {
            return Vec::new();
        }
        self.version = Some(version);
        let cells = snap.ore_cells();
        if self.drawn.len() != cells.len() {
            self.drawn = vec![NOT_DRAWN; cells.len()];
        }
        let max = snap.ore_max();
        let mut out = Vec::new();
        for (drawn, &(x, y, amount)) in self.drawn.iter_mut().zip(cells) {
            let level = resource.level_of(amount, max) as u8;
            if *drawn != level {
                *drawn = level;
                out.push((x, y, level));
            }
        }
        out
    }

    /// Draw everything again at the next [`OreView::changes`] (a new renderer, a new map).
    pub fn forget(&mut self) {
        self.version = None;
        self.drawn.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_sim::{Cell, Command, OreRules, PlayerId, Terrain, TerrainDef, TerrainFlags};
    use sf_sim::{TerrainId, World, WorldConfig};

    const RULES: OreRules = OreRules {
        start: 160,
        max: 160,
        regrow_amount: 0,
        regrow_period: 100,
    };

    fn world() -> World {
        let defs = [
            TerrainDef {
                id: TerrainId(0),
                flags: TerrainFlags::OPEN,
            },
            TerrainDef {
                id: TerrainId(4),
                flags: TerrainFlags {
                    buildable: false,
                    harvestable: true,
                    ground: true,
                },
            },
        ];
        let ids: Vec<u8> = (0..16).map(|i| if i % 4 == 1 { 4 } else { 0 }).collect();
        let terrain = Terrain::from_layer(4, 4, &ids, &defs).unwrap();
        World::with_terrain(WorldConfig::new(1).with_map_size(4, 4), terrain)
            .unwrap()
            .with_ore(RULES)
            .unwrap()
    }

    fn take(world: &mut World, x: u32, amount: u16) {
        world
            .schedule(
                world.tick(),
                PlayerId(0),
                Command::TakeOre {
                    cell: Cell::new(x, 2),
                    amount,
                },
            )
            .unwrap();
        world.step();
    }

    #[test]
    fn the_first_look_reports_every_cell_then_only_level_changes() {
        let looks = TerrainLooks::embedded();
        let mut view = OreView::new(&looks);
        let mut w = world();
        let snap = SimSnapshot::capture(&w);
        let first = view.changes(&snap);
        // Four rows, one ore column: 4 cells, all full (the top level, 3).
        assert_eq!(first.len(), 4);
        assert!(first.iter().all(|&(x, _, level)| x == 1 && level == 3));
        assert!(view.changes(&snap).is_empty(), "the same state again");
        // A small take stays within the top level: the state changed, the picture did not.
        take(&mut w, 1, 10);
        assert!(view.changes(&SimSnapshot::capture(&w)).is_empty());
        // Down to empty: one cell, level 0.
        take(&mut w, 1, 1000);
        assert_eq!(view.changes(&SimSnapshot::capture(&w)), vec![(1, 2, 0)]);
        // Forgetting draws it all again.
        view.forget();
        assert_eq!(view.changes(&SimSnapshot::capture(&w)).len(), 4);
    }

    #[test]
    fn a_world_without_ore_reports_nothing() {
        let looks = TerrainLooks::embedded();
        let mut view = OreView::new(&looks);
        let w = World::new(WorldConfig::new(1).with_map_size(4, 4)).unwrap();
        assert!(view.changes(&SimSnapshot::capture(&w)).is_empty());
    }

    #[test]
    fn every_level_has_a_minimap_colour() {
        let view = OreView::new(&TerrainLooks::embedded());
        assert!((0..4).all(|l| view.minimap_color(l).is_some()));
        assert!(view.minimap_color(4).is_none());
    }
}
