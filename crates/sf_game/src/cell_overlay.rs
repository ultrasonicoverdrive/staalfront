//! What the cell grid overlay (WP2.4, [`sf_render::cell_overlay`]) is fed
//! from the simulation: the terrain once, the occupancy per tick.
//!
//! * Terrain type per cell and whether ground units can enter it: the sim's
//!   own terrain (`World::terrain`, `TerrainFlags::ground`), read once before
//!   the world moves to the sim thread.
//! * Structures: the cells the sim's occupancy holds, from the snapshot
//!   ([`SimSnapshot::structure_cells`]).
//! * Units: the cell of every entity's current position, from the snapshot.
//!   The sim keeps no such record; it is a presentation view only.

use sf_data::terrain::TerrainTypes;
use sf_render::cell_overlay::{CellLayer, CellOverlayStyle};
use sf_sim::{TerrainId, World};

use crate::sim_snapshot::SimSnapshot;

/// The overlay's map for `world`, with the shipped colours.
pub fn layer_for(world: &World) -> Result<CellLayer, String> {
    let palette = CellOverlayStyle::embedded().palette(&TerrainTypes::embedded())?;
    let terrain = world.terrain();
    let impassable = |id: u8| {
        terrain
            .flags_of(TerrainId(id))
            .is_some_and(|flags| !flags.ground)
    };
    Ok(CellLayer::new(
        terrain.width(),
        terrain.height(),
        terrain.ids().to_vec(),
        impassable,
        palette,
    ))
}

/// The cells that hold an entity's current position (structures included:
/// their footprint wins in the overlay anyway).
pub fn unit_cells(snap: &SimSnapshot) -> impl Iterator<Item = (u32, u32)> + '_ {
    snap.views()
        .iter()
        .filter_map(|v| snap.cell_at(v.pos))
        .map(|c| (c.x, c.y))
}
