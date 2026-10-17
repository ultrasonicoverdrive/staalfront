# Unit, structure and weapon definitions

Work package WP3.1 (PLAN.md phase 3). The data is the design targets of
`content/units.md`, the canonical roster; the values it does not state are
**placeholders** until the balance rounds (phase 15), marked `placeholder` in the
RON files.

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
`UnitDefs::embedded()` is the same data compiled in. Nothing in `sf_sim` reads the
definitions yet: WP3.2 spawns units from them.

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

## Conversion for the simulation

`UnitDefs::sim_units()` and `sim_weapons()` give `SimUnitDef` and `SimWeaponDef`:
integers and `sf_math::Fx` in the sim's own units (distances in cells, speed in cells
per second and per tick, times in ticks, names as kind numbers, a faction mask). No
float type crosses (a test checks the conversion module). They live in `sf_data` until
WP3.2 settles how `sf_sim` takes them. The 20 Hz constants (`TICKS_PER_SECOND`,
`TICK_MILLIS`) are in `sf_math`, which `sf_sim` re-exports and `sf_data` uses, so the
two crates need no dependency on each other.

## Where it is tested

`crates/sf_data/tests/units.rs`: the shipped data loads and every reference resolves;
the roster (both ways), stats and factions against `content/units.md`; the structure
footprints of the art brief; the shipped kind numbers; a shuffled file giving identical
definitions; duplicate and retired numbers; appending a unit, a weapon, a damage type
and a movement class as data only; refusals with file and line, including numbers with
too many decimals; the conversion, and the speed per tick against the exact rounding
for every speed from 0.001 to 20.000 cells per second.
