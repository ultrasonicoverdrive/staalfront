# Unit, structure and weapon definitions

Work packages WP3.1 and WP3.2 (PLAN.md phase 3). The data is the design targets of
`content/units.md`, the canonical roster; the values it does not state are
**placeholders** until the balance rounds (phase 15), marked `placeholder` in the
RON files. WP3.1 made the definitions; WP3.2 spawns units and structures from them in
the simulation and draws them (see "Spawning" and "Drawing a role" below).

Quality rule 6 applies: game data is RON, and **adding a unit is a new record in
`assets/config/unit_defs.ron` and no Rust change.** A test appends a unit to the
shipped file and loads it (`crates/sf_data/tests/units.rs`).

## Files

| File | Holds | Read by |
|---|---|---|
| `assets/config/unit_classes.ron` | movement classes, armor types, damage types | `sf_data::units` |
| `assets/config/weapon_defs.ron` | weapons | `sf_data::units` |
| `assets/config/unit_defs.ron` | one record per role: units, structures, mission objects | `sf_data::units` |

(`assets/config/units.ron` is something else: the placeholder look of the first engine
demo, WP1.6.)

`UnitDefs::load(config_dir, assets_dir)` reads the three files and checks them;
`UnitDefs::embedded()` is the same data compiled in. `sf_sim` does not depend on
`sf_data`: the game and the headless runner copy the fields the rules read into a
`sf_sim::UnitTable` (see "Spawning").

## Units of measure

| Quantity | Unit | Notes |
|---|---|---|
| distance, range, radius, size, sight | **cells** (1 cell = 4 m, B12) | `Fx::ONE` is one cell |
| speed | **cells per game second** | 1 cell/s = 4 m/s; per tick it is 1/20 of that |
| turn rate | **facing steps per tick** | 256 steps per full turn, so 8 per tick is 225 degrees per second |
| `build_seconds`, `reload` | **game seconds** | must be a whole number of 20 Hz ticks (a multiple of 0.05) |
| cost | credits | |
| hit points, damage | abstract | |
| power | power units | positive produces, negative uses |

Numbers are written with at most three decimals (`speed: 1.2`) and are read as
thousandths, never as floats. The decimals are counted on the text of the file: a
number with a fourth decimal (`0.0004`, `1.0000000001`, even `1.0000`) or with an
exponent (`12e-1`) is refused with its line, and so is a duration that is not a whole
number of ticks (`reload: 0.12`). Distances and sizes become `sf_math::Fx` (so `1.2` is
`Fx::from_ratio(6, 5)`), durations become tick counts. A speed also becomes a distance
per tick, **rounded once** from the exact decimal (`speed / 20` to the nearest
1/65536): heavy_tank at 1.1 cells/s moves 3604 raw units per tick, where rounding 1.1
first and dividing by 20 afterwards gives 3605 (and is off for about 2.5 % of all
speeds). The simulation moves by `speed_per_tick`; `speed` (per second) is for tools
and display.

## `unit_classes.ron`

Three lists of entries, each with a permanent `kind` number (0 to 255, see "Numbers are
permanent" below) and a unique snake_case `name`.

* `movement_classes`: foot, wheeled, tracked, hover, water, air. Hover crosses land and
  water (the amphibious transport); no role uses air yet. **Terrain costs per movement
  class are not defined here**: WP4.1 adds a `terrain_costs` field to each entry (and
  replaces the three `passable` flags of `terrain_types.ron`, which has no hover, water
  or air). Until then the movement class is only a name that units carry.
* `armor_types`: none, infantry, light, heavy, structure. The set is a placeholder.
* `damage_types`: bullet, precision, rocket, cannon, shell, demolition. The set is a
  placeholder. "Infantry only" for the sniper is the future armor table's job
  (precision does nothing to non-infantry armor), not a weapon field.

WP5.3 adds the damage-times-armor table and the splash falloff next to these lists.

## `weapon_defs.ron`

| Field | Meaning |
|---|---|
| `kind` | the permanent number of the weapon, 0 to 4095, unique (see "Numbers are permanent") |
| `id` | unique snake_case name that units refer to |
| `range` | longest distance it can hit, cells, above 0 |
| `min_range` | shortest distance it can hit, cells, default 0, below `range` (the howitzer cannot hit what is next to it) |
| `reload` | game seconds between shots, at least one tick |
| `damage` | hit points per hit before armor, 1 or more |
| `damage_type` | a name from `unit_classes.ron` |
| `projectile` | `Straight`, `Arc`, `Guided` or `Falling` (the flight is WP5.2) |
| `splash_radius` | cells, default 0 (none); falloff is WP5.3 |

## `unit_defs.ron`

Every record carries its permanent number (`sf_sim::Kind`); the order of the records in
the file does not matter.

| Field | Meaning |
|---|---|
| `kind` | the permanent number of the role, 0 to 4095, unique (see "Numbers are permanent") |
| `id` | unique snake_case role name, exactly as in `content/units.md` |
| `factions` | `[Meridian]`, `[Veyran]`, `[Meridian, Veyran]` or `[Neutral]` (mission objects, civilians); Neutral cannot be combined |
| `category` | `Infantry`, `Vehicle`, `Vessel`, `Structure` or `Object` (a crate, an ore field) |
| `cost` | credits; absent means it cannot be bought or built |
| `build_seconds` | game seconds; required with a `cost`, absent without |
| `hp` | hit points, 1 or more; 0 only for an `Object` and means it cannot be damaged |
| `armor` | a name from `unit_classes.ron` |
| `power` | structures only; produced (+) or used (-) |
| `movement` | a movement class; required for Infantry, Vehicle and Vessel, absent for the rest |
| `speed` | cells per game second, above 0 up to 20; only with a `movement` |
| `water_speed` | cells per second on water, if different from `speed` (the amphibious transport) |
| `turn_rate` | facing steps per tick, at least 1; only with a `movement` |
| `size` | diameter in cells for collision and selection (not for structures) |
| `footprint` | `(width: w, height: h)` in cells, 1 to 16; **required for and only for a Structure** |
| `sight` | cells; above 0 except for an Object |
| `weapons` | up to four ids from `weapon_defs.ron` |
| `model` | `Shape(Cube \| Cylinder \| Cone)`, a placeholder primitive of `units.ron`, or `Glb("<path under assets/>")`, a glTF file that must exist |
| `strategic_icon` | `Square`, `Circle` or `Triangle`: the symbol at strategic zoom (WP1.9) |
| `icon` | optional picture for the build menu (WP11.1, WP3.6): a PNG path under `assets/` that must exist |
| `flags` | abilities, below; every flag defaults to off |

### Flags

Generic data fields that later work packages read. A flag that needs a moving unit
(harvests, deploys, can_capture, ...) on a role without a `movement` is refused.

| Flag | Meaning |
|---|---|
| `can_capture` | captures enemy structures by walking into them |
| `repairs_structures` | repairs friendly structures it touches |
| `places_charges` | places a demolition charge on a marked object |
| `consumed_on_use` | is used up when it destroys a structure on contact |
| `harvests` | collects resources and drops them at a refinery |
| `deploys` | immobile while it fires |
| `transports` | infantry it carries (0 = none) |
| `transports_vehicles` | vehicles it carries instead (0 = none); the amphibious transport carries 5 infantry **or** 1 vehicle |
| `garrison` | infantry that can garrison a structure |
| `heal_radius`, `repair_radius` | cells in which it heals infantry / repairs vehicles (0 = does not) |
| `stealth_in_cover` | invisible to the enemy while standing still in cover terrain |
| `builds` | structure ids it can build in the field |
| `disguise_as` | a role it looks like to the enemy AI (the decoy emplacement) |
| `comes_with` | a unit that comes free with it (the refinery's harvester) |
| `needs_water` | a structure that can only stand on the coast |
| `build_radius` | cells around it in which its owner may build (the base anchor) |
| `minimap` | enables the minimap for its owner |
| `death_blast` | `(radius: r, damage: d)`: explodes when destroyed |
| `pickup` | is picked up by infantry walking onto it |
| `fires_at_all` | fires at every faction, its own included |

What the file does **not** carry: the campaign mission that unlocks a role (the tech
tree is in `content/units.md`; it becomes data with building requirements in WP6.1),
and mission logic such as "fires while the uplink is alive". Which faction a role
belongs to is checked against the tech tree by a test.

## Numbers are permanent

The simulation, scenario files, saves and replays name a unit, a weapon, a movement
class, an armor type or a damage type by the **`kind` number written in its record**,
not by its name and not by its place in the file. Rules:

* A number is chosen by hand when a record is written (the next free one) and **never
  changes**.
* Two live records of one list never share a number (the load is refused).
* The order of the records in the file is irrelevant: sorting, grouping or inserting
  records changes nothing. A test loads a shuffled copy of all three files and checks
  that the definitions and the simulation definitions are identical.
* To remove a record, delete it and add a **tombstone** with its number to the
  `retired_*` list of that file: `retired_units: [(kind: 12, name: "old_role")]`
  (`retired_weapons`, `retired_movement_classes`, `retired_armor_types`,
  `retired_damage_types` likewise). The number is then never handed out again: a live
  record on a retired number is refused, and so is a number retired twice.
* The numbers as shipped in WP3.1 (the position each record had in the file then) are
  pinned by a test.

Because numbers can have gaps, look one up with `UnitDefs::sim_unit(kind)` and
`sim_weapon(kind)`; the `sim_units()` and `sim_weapons()` slices are sorted by kind, not
indexed by it.

## What is validated

Loading reports the first problem as `file:line:column: message` (the line comes from
the RON parser for syntax and type errors, and from a search for the record's id for
the checks after parsing):

* ids are unique snake_case names (a test keeps the role names of other games out of
  the shipped file; roles are named only as in `content/units.md`), and every
  `kind` is unique within its list and not retired;
* every weapon, movement class, armor type, damage type, `builds`, `disguise_as` and
  `comes_with` reference resolves; model and icon files exist and stay inside `assets/`
  (no `..`, no absolute path);
* numbers are in range, durations are whole ticks, a Structure has a footprint and a
  mobile unit a movement class, a speed and a turn rate;
* tests check both ways that the ids of the defs are exactly the roles of the tables in
  `content/units.md` (a role deleted from that file fails too), and that cost, power,
  hit points, speed, weapon range, minimum range, capacities, radii and the relations
  "looks like", "comes with" and "build ... in the field" match what its notes say;
  sight where it states one.

## Spawning (WP3.2)

`sf_sim` knows a unit kind only through its **unit table**, `sf_sim::UnitTable`: for each
kind number its hit points, movement class, speed per tick, turn rate, size and (for a
structure) footprint. The caller builds it from `UnitDefs::sim_units()` and hands it to
the world once, right after the world is made (`World::with_units`, as the terrain is
handed over); a world cannot take it after it has started. The table is fixed for the
whole game and a digest of it is in the checksum.

What a kind is follows from its data: **mobile** (it has a movement class: infantry, a
vehicle, a vessel), **structure** (it has a footprint) or **object** (neither: a crate,
an ore field). Two commands create them, through the command queue like everything else:

| Command | Makes | Where | Ignored (counts as a command that changed nothing) when |
|---|---|---|---|
| `SpawnUnit { kind, pos, owner }` | a mobile unit or an object | at `pos`, in cells | the table has no such kind; the kind is a structure; `pos` is outside the map or on a cell a structure occupies; the entity limit is reached |
| `PlaceStructure { kind, cell, owner }` | a structure | its footprint **anchored** on `cell`: the cell with the smallest x and y, the footprint extending east and south | the table has no such kind; the kind is not a structure; any cell of the footprint is outside the map, not `buildable` (`docs/rules/terrain.md`) or already occupied; the entity limit is reached |

A refused command creates nothing: no entity, no occupied cell, no slot. A structure is
placed **all or nothing**. Unknown kinds are refused, never guessed at.

**What the new entity has.** Hit points from the table (`World::hp_of`; zero for an object
with `hp: 0`, which cannot be damaged; damage is phase 5), the owner, and the table entry
(`World::stats_of`). A unit starts **idle** with a random facing from its own random
stream; it moves only when ordered (`MoveTo`, WP3.4 adds the rest) and then by **its own**
speed per tick and turn rate, not the world's placeholder speed. A structure and an
object start with facing zero and never move: `MoveTo` and `SetWanderArea` skip them.
Structures stand in the middle of their footprint (the anchor corner plus half the
width and height).

**Occupancy.** A structure occupies **every cell of its footprint** in the occupancy grid
of `docs/rules/terrain.md`; `World::occupant_at` answers with the structure for each of
them, `World::cell_of` with the anchor and `World::footprint_of` with the size. Despawning
it frees all its cells in the same command. **A mobile unit (and an object) occupies no
cell**, the same as the wanderers of WP2.1: units share cells until collision arrives in
phase 4. The one rule that touches them is that a unit never **appears** inside a
structure (`SpawnUnit` on an occupied cell is refused). A unit is also not refused on terrain it
cannot cross (a rifleman may be spawned into water): which terrain each movement class
crosses is WP4.1's, and until then only the map edge and structures stop a spawn.

**The older commands stay.** `Spawn` (a wandering placeholder, its kind an opaque number)
and `Place` (a one-cell structure of an opaque kind) do not look at the table: they have
no hit points, no footprint and the world's own speed, and they are what the scenarios
`wander_5000`, `command_mix` and `terrain_256` use, so those checksums did not change.
A kind number can therefore mean two things in one world (a legacy wanderer of kind 1
next to a `power_plant`, kind 1): the simulation tells them apart (`EntityView::has_stats`)
and the presentation does the same. They go when scenarios are converted to roles.

**Checksum.** With a table the checksum gets one more section after the occupancy: the
table's digest, then for every entity made from it the slot and its hit points. A world
without a table hashes exactly as before. See `sf_sim::checksum`.

## Drawing a role (WP3.2)

`sf_game::roster::Roster` is the data mapping from a kind number to what the instancer
draws (it replaces the stand-in `kind % shapes` of the first demo). Built from the
definitions:

* `model: Glb(path)` is loaded with the axes and level-switch sizes of the pack in
  `assets/config/models.ron` whose directory the path lies in; a structure's model is
  scaled, the same in every direction, until it just fits its footprint of 4 m cells.
* `model: Shape(...)` is that placeholder primitive, drawn **at the size of the role**: a
  structure is 4 m times its footprint on each side (and 3 m plus one metre per cell of
  its shorter side high), a unit or object as wide as its `size` in cells times 4 m.
* `strategic_icon` is the symbol at strategic zoom. (`icon`, the build menu picture, is
  WP3.6 and WP11.1.)
* A role whose model cannot be loaded, and every role when models are off
  (`sf_game --models none`), falls back to a primitive chosen by what it is: infantry a
  cylinder, a vehicle or vessel a cone, a structure or object a cube.

The F1 panel names the hovered unit by its role id and shows its hit points (`light_tank
18v0 (red) by model, 83.1 m, HP 400/400`); an entity that is not a role is `placeholder <kind>`
without hit points.

## Conversion for the simulation

`UnitDefs::sim_units()` and `sim_weapons()` give `SimUnitDef` and `SimWeaponDef`:
integers and `sf_math::Fx` in the sim's own units (distances in cells, speed in cells
per second and per tick, times in ticks, names as kind numbers, a faction mask). No
float type crosses (a test checks the conversion module). They stay in `sf_data`;
`sf_sim` takes its own `UnitStats` (see "Spawning"). The 20 Hz constants (`TICKS_PER_SECOND`,
`TICK_MILLIS`) are in `sf_math`, which `sf_sim` re-exports and `sf_data` uses, so the
two crates need no dependency on each other.

## Where it is tested

WP3.2: `crates/sf_sim/tests/units.rs` (stats at birth, footprints, refusals, per-unit speed,
legacy commands, checksum), `assets/scenarios/units_spawn.ron` with
`tools/sf_headless/tests/units.rs` (every role of the shipped file spawned on the 256 x 256
playfield, hit points, footprint occupancy and refusals asserted, the checksum pinned on
Linux and Windows by CI), and in `sf_game` the roster (`roster.rs`), the sandbox
(`sandbox.rs`) and the scenes (`runner.rs`).

WP3.1:

`crates/sf_data/tests/units.rs`: the shipped data loads and every reference resolves;
the roster (both ways), stats and factions against `content/units.md`; the structure
footprints of the art brief; the shipped kind numbers; a shuffled file giving identical
definitions; duplicate and retired numbers; appending a unit, a weapon, a damage type
and a movement class as data only; refusals with file and line, including numbers with
too many decimals; the conversion, and the speed per tick against the exact rounding
for every speed from 0.001 to 20.000 cells per second.
