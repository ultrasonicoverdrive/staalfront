# Cell grid overlay (WP2.4)

A debug and planning view, off by default. The F1 panel's **Cell grid (4 m)** checkbox
(or `sf_game --debug-grid`) switches it on together with the grid lines. It is a view only:
it reads the simulation and changes nothing in it.

## What a cell shows

Every 4 m gameplay cell gets exactly one state. The highest one wins:

| State | Drawn as | Means |
|---|---|---|
| structure | `structure` colour, filled and outlined | The sim's occupancy holds a structure on the cell (`World::occupant_at`). A structure with a footprint marks every cell of it. |
| units | `units` colour, a disc in the cell | At least one entity's current position lies in the cell. |
| impassable | type colour with dark diagonal hatching | The terrain type is not open ground: no ground movement class (foot, wheeled, tracked) may enter it (`TerrainFlags::ground`). Water is hatched although boats sail on it. |
| open | type colour, translucent (`fill_percent`) | None of the above. |

## What "occupied" means

Only **structures occupy cells** in the simulation (WP2.1, `sf_sim::terrain`). Units have
bodies (WP4.4) and do not claim cells, so any number of them can share one; the sim keeps
no per-cell record of them. The "units" state is therefore derived by the presentation
from the snapshot's positions: it says where units are, not what blocks a route. A cell
with a unit is not blocked for anything.

## Data

`assets/config/cell_overlay.ron`: an sRGB colour for every terrain type (by name; a
missing or unknown name is refused), the `units` and `structure` colours and the opacity
of the tint. The type colours are meant to merge with WP2.3's `minimap_colour` once that
lands in `terrain_types.ron`.

## Cost

One texel per cell, rebuilt on the CPU at most once per sim tick while the overlay is on;
only the rows that changed are uploaded. Drawing repeats the terrain's level-of-detail
draws (one call per run of chunks) with a shader that reads one texel per fragment. When
off, nothing is built, uploaded or drawn (the snapshot only lists structure cells, and
only when the world has any).
