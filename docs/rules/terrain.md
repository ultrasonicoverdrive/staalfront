# Terrain types and occupancy

Work package WP2.1 (PLAN.md phase 2); WP4.1 moved the movement half of the types out
(`movement.md`). Rules in our own words.

## The grid

The playfield is a grid of **gameplay cells of 4 m** (decision B12), 64 x 64 up to
256 x 256 cells. The visual terrain keeps its 1 m resolution (four samples per cell
side); the game rules in this page are per cell. A sim position is in cells
(`Fx::ONE` is one cell), so the cell of a position is its floor
(`World::cell_at`). The middle of cell `(x, y)` is `(x + 1/2, y + 1/2)`.

## Terrain types

A terrain type is data: `assets/config/terrain_types.ron` lists each type with an
`id` (the number a map file stores, never renumbered once a map uses it), a unique
`name` and two flags of its own. Adding a type is a data change; nothing in Rust names a type except
the map generator, which looks the six below up by name. **Who can cross a type, and how fast,
is not a flag of the type**: it is the cost table of the movement classes, with one entry per
class for every type here (`docs/rules/movement.md`; since WP4.1 it replaced the three
`passable` flags this file used to have).

| id | name | buildable | harvestable | Meaning |
|---|---|---|---|---|
| 0 | grass | **yes** | no | Open ground; the only terrain a structure may stand on. |
| 1 | road | no | no | Open ground that is kept free of buildings so that it stays a road. Quicker to cross than grass for the ground classes. |
| 2 | water | no | no | River and lake: foot, wheeled and tracked units cannot cross it; hover craft and vessels can. |
| 3 | rock | no | no | Steep or high ground: nothing on the ground or the water crosses it, only aircraft. |
| 4 | ore | no | **yes** | Open ground with resources on it; kept free of buildings so that harvesters can reach it. Slower to cross than grass. |
| 5 | bridge | no | no | A road over water: land crosses it, vessels pass under it, not buildable (`movement.md`, "The bridge"). |

What the flags mean, and who reads them:

* `buildable`: whether a structure may stand on the cell. Read by `Command::Place`
  (below) and `Command::PlaceStructure`; WP6.x building placement will read it too.
* `harvestable`: whether harvesters can take resources from the cell. Nothing reads it
  yet (phase 7).

Which cells a unit may enter and its speed there: `World::cost(class, cell)` and
`World::passable(class, cell)` (`movement.md`). The sim's `TerrainFlags` also has a derived
`ground` summary (all three ground classes can enter the type), computed from the costs and
not written by hand; it is in the terrain digest in the bits the old passable flags had, so
the checksum of every map that existed before WP4.1 is unchanged.

The values are our placeholders, not taken from any other game.

## A map's terrain layer

A map directory may hold `terrain.png`, named by `terrain_map` in `map.ron`: an 8-bit
grayscale PNG with **one pixel per gameplay cell** whose value is the type id. (A
PNG, not run-length RON: the other two layers are PNGs, the decoder and the checks
are shared, it is a few kilobytes, and an image editor can paint it; see
`sf_data::map`.) The loader refuses a layer of the wrong size, an id the table does not
define, and a file name that could leave the map directory (the same rules as the
height and material files). A map without the line has no layer and loads as before.

A world made on a map without a layer has **open ground** everywhere: every cell is
type 0, buildable, not harvestable, and every movement class moves on it at normal speed.

## Generated maps

`sf_data::map::generate` gives every map a layer (integer arithmetic only, so it is
bit-exact on every platform; pinned by `crates/sf_data/tests/golden.rs`):

1. **water** where the river is drawn (the riverbed material `sand`), **rock** where
   the rock material is (steep or high ground); a cell takes the material that
   covers at least half of its 16 samples, water first; everything else is grass;
2. **ore**: one field per 8,192 cells, at least two, each a disc of 2 to 4 cells
   radius around a grass cell, painted on grass only;
3. **road**: one cell wide, a staircase from the west edge (at a fifth of the map
   height) to the east edge (four fifths), so it always crosses the river; every
   road cell that lies on water is a **bridge**. A map under 5 cells high has no road.

## How it is drawn

Work package WP2.2. The presentation shows the terrain type of every cell, so what you
see is what units can do: the water is where vessels go, the rock is where nothing
passes, the road is where units are fast, the ore is where harvesters work, and the
bridge is a deck over the river. This is presentation only; `sf_sim` and its checksums
are not involved.

* **A map with a terrain layer** is drawn from it. The material map of the map file
  (one material per 1 m sample) is then **not used**: the terrain type decides. A map
  without a layer is drawn from its material map as before (WP1.7).
* **The mapping is data**: `assets/config/terrain_look.ron`, one entry per terrain
  type, matched by the type's **name** (ids are what map files store, names are what the
  tools use). An entry has a base colour (sRGB), a `contrast`, a procedural `pattern`
  (`Noise`, `Cracks`, `Ripples`, `Planks(count)`, `Flecks(color)`) and two style flags:
  `flat` (drawn level, no hill shading) and `liquid` (gets a shore line). A type
  without an entry is still drawn, in a colour derived from its name, so adding a type
  to `terrain_types.ron` never breaks the picture; the test
  `every_shipped_type_has_a_look_and_every_look_a_type` fails until somebody has chosen
  its colours. There are no third-party textures: every look is a 128 x 128 tileable
  texture generated in code (one repeat per 4 m, so it is aligned to the cells).
* **The colours are invented** to read from 45 to 80 degrees and to stay apart from the
  unit colours of the factions (sand, olive, weathered blue; slate, charcoal, muted
  crimson, `content/world.md`): fresh yellow-green grass, a light tan road of packed
  earth (clearly apart from the grey rock), saturated blue
  water, mid grey rock, dark gold ore with bright flecks and a light wooden
  bridge with planks.

| type | look | style |
|---|---|---|
| grass | yellow-green patches | |
| road | packed earth: light tan, low contrast | |
| water | blue with thin light wave crests | flat, liquid |
| rock | mid grey with dark cracks | |
| ore | dark ochre with gold flecks | |
| bridge | light wood, 8 planks per 4 m with dark gaps | flat |

* **The minimap** (WP2.3) draws every cell in one flat colour of its type, taken from the
  same look: `minimap: (r, g, b)` if the entry names one, otherwise its base `color`. The
  look is the one source of a type's colour; the gameplay table `terrain_types.ron` has
  none. Only ore names a minimap colour (138, 100, 39): its base is the dark ground under
  the gold flecks, the minimap colour is the mean of the texture. Tests in
  `sf_render::terrain_look` hold that every minimap colour lies within 40 (RGB distance)
  of the mean of its look's texture, that every pair is at least 50 apart, and that each
  says what its type is (grass green, water blue, rock a mid grey of low saturation, ore
  warm and darker than the road, road light tan, bridge light wood). The minimap shades
  the relief lightly over the colour (at most 18 % either way).

* **Soft borders.** The terrain shader reads one index texel per gameplay cell (the
  texture array layer and the style bits) and blends the four nearest cell centres.
  The blend is a ramp of `edge.ramp_m` (2 m) centred on the cell border, so the middle
  of every cell stays pure and a border is two metres soft (the ramp is never narrower
  than a pixel, so a far zoom does not alias). The border **wobbles** by up to
  `edge.wobble_m` (0.7 m, value noise in world space, fading to nothing at the cell
  centres so the picture stays continuous), which turns the corners of a staircase
  of cells (the generated road) into rounded, irregular shapes instead of a
  chessboard. **Cost:** the four index `textureLoad`s run for every pixel. Where the
  four cells agree the shader takes one texture sample; in a border it takes four,
  plus two `value_noise` calls, an `fwidth` and a `smoothstep`. A GPU shades in 2x2
  quads, so in a quad that straddles a border both paths run. No extra pass, no extra
  vertex data.
* **Water.** A `flat` style is lit as level ground: the slope of the river bed does
  not shade the water (the mesh is not changed, there is no level offset). A `liquid`
  gets a light line along its shore (`edge.shore`, `edge.shore_amount`), strongest where
  water and the other type weigh the same. The bridge is flat as well, and so lies like
  a deck over water. Water is not animated (later polish).

## Occupancy

Every cell has at most one **occupant**, an entity. Only structures occupy cells;
wanderers and units do not (thousands share a cell, and collision is phase 4). A
structure from the unit definitions (WP3.2, `Command::PlaceStructure`) occupies every cell
of its footprint; `docs/rules/units.md` has those rules. What follows is the one-cell
`Command::Place` of WP2.1, which the scenarios still use.

`Command::Place { kind, cell, owner }` (scenario: `Place(kind: .., owner: .., cell: (x, y))`)
creates a structure. In this order, the command is **ignored** (it counts as a
command that changed nothing, no entity is created) if

1. the cell is outside the map;
2. the terrain of the cell is not `buildable`;
3. the cell is already occupied;
4. the entity limit is reached.

Otherwise a new entity of `kind` for `owner` stands in the middle of the cell, with
the idle behaviour, and the cell is its. The entity takes the next free slot like a
spawn. Two placements on one cell in the same tick: commands run by player number, then
in scheduling order, so the lower player gets it.

A structure **does not move**: `MoveTo` and `SetWanderArea` skip it. `Despawn` removes
it and frees its cell in the same command; the slot comes back later with a higher
generation, so the old id never names the new structure.

Occupancy changes **only in the tick**, inside commands (quality rule 1). Everything
public on the world is a read-only query: `terrain_at`, `occupant_at`, `is_occupied`,
`cell_of`, `cell_at`, `occupied_cells`, `terrain`.

## Checksum

The terrain and the occupancy are part of the per-tick checksum, in two sections that
are written **only when they have content** (a terrain layer; at least one occupied
cell), after everything else. For every world without a layer and without structures
the hashed bytes are exactly what they were before WP2.1, so the existing golden
checksums and the `wander_5000` and `command_mix` values did not change. The terrain
is hashed through a digest computed once at creation (it never changes during a game),
the occupancy cell by cell in ascending order. See `sf_sim::checksum`.

## Where it is tested

* `crates/sf_data` (`map/terrain_tests.rs`, `tests/golden.rs`): loading, validation,
  file name safety, the generator's layer and its pinned hashes.
* `crates/sf_sim/tests/terrain.rs`: queries, `Place` rules, despawn and slot reuse,
  wanderers unchanged by terrain, the checksum sections.
* `crates/sf_render` (`terrain_look/tests.rs`, `tests/terrain_types_screenshot.rs`): the
  looks, the index built from a map, and the picture (each type in its own colours, the
  material map does not matter, soft borders, level water, the shore line).
* `assets/scenarios/terrain_256.ron` with `tools/sf_headless/tests/terrain.rs`: the
  256 x 256 cell playfield, terrain type and occupancy asserted per cell, and the
  pinned checksum on Linux and Windows (CI job `headless`).
