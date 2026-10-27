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
`sf_data`: the small crate `sf_bridge` copies the fields the rules read into a
`sf_sim::UnitTable` (see "Spawning"), for both the game and the headless runner.

## Units of measure

| Quantity | Unit | Notes |
|---|---|---|
| distance, range, radius, size, sight | **cells** (1 cell = 4 m, B12) | `Fx::ONE` is one cell |
| speed | **cells per game second** | 1 cell/s = 4 m/s; per tick it is 1/20 of that |
| turn rate, turret rate | **facing steps per tick** | 256 steps per full turn, so 8 per tick is 225 degrees per second |
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
  water (the amphibious transport); no role uses air yet. Since WP4.1 every entry has a
  `terrain_costs` list: for **every** terrain type of `terrain_types.ron`, by name, either
  `Impassable` or `Speed(n)`, the speed on it in thousandths of the unit's own (100 to 2000).
  `terrain_costs: [("grass", Speed(1000)), ("water", Impassable), ...]`. A class that leaves a
  type out, or names one that does not exist, is refused when the files are combined
  (`sf_data::movement::MovementCosts`); the table, the bridge rule and the reasons are in
  [`movement.md`](movement.md). It replaced the three `passable` flags of `terrain_types.ron`.
  A class may carry `ground: true` (foot, wheeled and tracked do): the ground classes, whose
  ability to enter a terrain type makes it open ground (`movement.md`, "The checksum"). Since
  WP4.5 a class may also carry a `turning` block: how its units turn the hull
  (`turning: (mode: Arc, drive_tolerance: 16, min_radius: 1.5, pivot_permille: 250)`; mode `Pivot`,
  `Arc` or `Instant`; every field defaults to the old walk, `Pivot` and 16 steps); the modes, the
  numbers and what is checked are in [`turning.md`](turning.md).
* `armor_types`: none, infantry, light, heavy, structure (WP5.3 kept the set; the reasons and how to add
  a sixth are in [`armour.md`](armour.md)).
* `damage_types`: bullet, precision, rocket, cannon, shell, demolition. "Infantry only" for the sniper is the
  armor table's job (precision does nothing to non-infantry armor), not a weapon field.
* `armor_table` (WP5.3): one row for every damage type with a percentage for every armor type, `(damage:
  "bullet", percent: [("none", 0), ("infantry", 100), ...])`. Optional in the file (the frozen copies of
  the scenarios have none: a hit then does the weapon's damage), complete and 0 to 400 percent when it is
  there. A new damage type or armor type needs its row or cell. The numbers, the rounding and everything
  refused are in [`armour.md`](armour.md).

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
| `projectile` | `Instant`, `Straight`, `Arc`, `Guided` or `Falling` (the flights: [`projectiles.md`](projectiles.md)) |
| `splash_radius`, `splash_inner`, `friendly_fire` | the splash (WP5.3): cells from the burst to the edge of a victim in which a hit also damages, the cells inside which it does its full damage, and whether it spares the shooter's own side (default: hurts everyone); [`armour.md`](armour.md) |
| `projectile_speed`, `flight_height`, `guide_turn` | the parameters of the projectile (WP5.1; read by WP5.2); optional for the loader, required by the simulation for a projectile that flies (`projectiles.md`) |
| `burst`, `burst_delay` | shots per trigger pull and the time between them (WP5.1); default one shot |
| `mount`, `fire_arc` | `Hull` or `Turret`, and how far from the target the weapon may point and still fire (WP5.1) |

Since WP5.1 `reload` counts from the first shot of a burst to the first of the next, and the
sixteen fields and their rules, the rounding, the shipped set and what each field means for
WP5.3 and WP5.4 are in [`weapons.md`](weapons.md) and [`targeting.md`](targeting.md).

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
| `speed` | cells per game second, above 0 up to 20; only with a `movement`. On terrain a unit never moves more than 17.5 cells per second (`movement.md`), so a speed above that is capped; no role comes near |
| `water_speed` | cells per second on water, if different from `speed` (the amphibious transport) |
| `turn_rate` | facing steps per tick, at least 1; only with a `movement`. How the hull turns with it depends on the class (`turning.md`): a class that turns at once does not use it |
| `turret_rate` | facing steps per tick the **turret** turns, for a role with a turret that turns apart from the hull (WP4.5; the glTF `turret` part's yaw joint); zero or absent means none; only with a `movement`. Shipped for the light tank (12), the heavy tank (8) and the siege artillery (6) |
| `size` | diameter in cells for collision and selection (not for structures). Since WP4.4 the body of a unit is a disc of half of it for a radius; two bodies of 0.75 cells or less (infantry) keep three quarters of the sum of their radii, all others the whole sum (`collision.md`). Aircraft (a class that no terrain stops) have no body |
| `footprint` | `(width: w, height: h)` in cells, 1 to 16; **required for and only for a Structure** |
| `sight` | cells; above 0 except for an Object |
| `weapons` | up to four ids from `weapon_defs.ron` |
| `model` | `Shape(Cube \| Cylinder \| Cone)`, a placeholder primitive of `units.ron`, or `Glb("<path under assets/>")`, a glTF file that must exist and lie in a pack of `config/models.ron`. The generic look of the role (see "Models per faction") |
| `models` | optional map from a faction to a model, `models: { Meridian: Glb("set_v4/meridian_rifleman.glb"), Veyran: Glb("set_v4/veyran_rifleman.glb") }`; the keys are factions of the role; each value is a model like `model` (see "Models per faction") |
| `strategic_icon` | `Square`, `Circle`, `Triangle` or `HalfCircle`: the base shape at strategic zoom, one per kind of role (structure, infantry, vehicle, vessel); what the role is drawn as when it has no `icon` (WP1.9, WP3.6) |
| `icon` | optional name of the role's strategic icon (WP3.6): a picture of the icon set in `assets/icons`, `a-z 0-9 _`, at most 48 characters; see [`icons.md`](icons.md). Only the spelling is checked in `sf_data`; a name the set does not have is a warning when the game loads and the role is drawn as its `strategic_icon`. (The build menu picture of WP11.1 will be a field of its own.) |
| `flags` | abilities, below; every flag defaults to off |
| `building` | structures only, optional (WP6.1): `(function: ..., exit_side: ..., requires: [...])`, what a structure is for and which structures must stand before it can be built; [`buildings.md`](buildings.md) |

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

What the file does **not** carry: the campaign mission that unlocks a role (the campaign
unlock order is in `content/units.md`; since WP6.1 the building prerequisites are data, the `requires` of the
`building` block, see [`buildings.md`](buildings.md)),
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
  `comes_with` reference resolves; model files exist and stay inside `assets/`
  (no `..`, no absolute path); an `icon` is a plain name (not a path);
* numbers are in range, durations are whole ticks, a Structure has a footprint and a
  mobile unit a movement class, a speed and a turn rate;
* tests check both ways that the ids of the defs are exactly the roles of the tables in
  `content/units.md` (a role deleted from that file fails too), and that cost, power,
  hit points, speed, weapon range, minimum range, capacities, radii and the relations
  "looks like", "comes with" and "build ... in the field" match what its notes say;
  sight where it states one.

## Spawning (WP3.2)

`sf_sim` knows a unit kind only through its **unit table**, `sf_sim::UnitTable`: for each
kind number its hit points, movement class, speed per tick, turn rate, size, (for a
structure) footprint, since WP4.5 the turn rate of its turret (`turret_per_tick`, zero for none;
hashed in the turning section, not in the table's digest) and, since WP3.4, the range of its first weapon (`weapon_range`, zero
without a weapon; what an attack order walks to, `docs/rules/orders.md`). `sf_bridge::unit_table(&UnitDefs)` builds it from `UnitDefs::sim_units()` and the caller hands it to
the world once, right after the world is made (`World::with_units`, as the terrain is
handed over); a world cannot take it after it has started. The table is fixed for the
whole game and a digest of it is in the checksum.

What a kind is follows from its data: **mobile** (it has a movement class: infantry, a
vehicle, a vessel), **structure** (it has a footprint) or **object** (neither: a crate,
an ore field). Three commands create them, through the command queue like everything else:

| Command | Makes | Where | Ignored (counts as a command that changed nothing) when |
|---|---|---|---|
| `SpawnUnit { kind, pos, owner }` | a mobile unit or an object | at `pos`, in cells | the table has no such kind; the kind is a structure; `pos` is outside the map or on a cell a structure occupies; a mobile unit's movement class cannot enter the cell (`movement.md`); the entity limit is reached |
| `PlaceStructure { kind, cell, owner }` | a structure | its footprint **anchored** on `cell`: the cell with the smallest x and y, the footprint extending east and south | the table has no such kind; the kind is not a structure; any cell of the footprint is outside the map, not `buildable` (`docs/rules/terrain.md`) or already occupied; the entity limit is reached |
| `BuildStructure { kind, cell }` (WP6.2) | a structure for the player who sent it | as `PlaceStructure` | any placement rule of `docs/rules/buildings.md` fails; the refusal is reported in `World::placement_rejections()` |

A refused command creates nothing: no entity, no occupied cell, no slot. A structure is
placed **all or nothing**. Unknown kinds are refused, never guessed at.

**What the new entity has.** Hit points from the table (`World::hp_of`; zero for an object
with `hp: 0`, which cannot be damaged; damage is phase 5), the owner, and the table entry
(`World::stats_of`). A unit starts **idle** with a random facing from its own random
stream; it moves only when ordered (`docs/rules/orders.md`, WP3.4; the legacy `MoveTo` too) and then by **its own**
speed per tick and turn rate, not the world's placeholder speed. A structure and an
object start with facing zero and never move: orders, `MoveTo` and `SetWanderArea` skip them.
Structures stand in the middle of their footprint (the anchor corner plus half the
width and height).

**Occupancy.** A structure occupies **every cell of its footprint** in the occupancy grid
of `docs/rules/terrain.md`; `World::occupant_at` answers with the structure for each of
them, `World::cell_of` with the anchor and `World::footprint_of` with the size. Despawning
it frees all its cells in the same command. **A mobile unit (and an object) occupies no
cell**, the same as the wanderers of WP2.1: units share cells (since WP4.4 they have bodies that
push each other apart instead, `collision.md`). The one rule that touches them is that a unit never **appears** inside a
structure (`SpawnUnit` on an occupied cell is refused). Since WP4.1 it is also refused on terrain its movement class cannot
enter (a rifleman is not put into the river, a gunboat not onto grass; `movement.md`); on a
world without a terrain layer or without a movement table every cell is open ground and only the
map edge and structures stop a spawn.

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

`sf_game::roster::Roster` is the data mapping from a kind number (and, for a role with
`models`, the faction of the unit's owner: `Roster::shape_for`) to what the instancer
draws (it replaces the stand-in `kind % shapes` of the first demo). Built from the
definitions:

* `model: Glb(path)` is loaded with the axes and level-switch sizes of the pack in
  `assets/config/models.ron` whose directory the path lies in; a structure's model is
  scaled, the same in every direction, until it just fits its footprint of 4 m cells.
* `model: Shape(...)` is that placeholder primitive, drawn **at the size of the role**: a
  structure is 4 m times its footprint on each side (and 3 m plus one metre per cell of
  its shorter side high), a unit or object as wide as its `size` in cells times 4 m.
* `strategic_icon` is the base shape at strategic zoom, the shape of the role's category
  (a test keeps the shipped file to that); `icon` is the picture inside it (WP3.6, see
  [`icons.md`](icons.md)). Roles that draw the same glTF file with different icons share
  the loaded model (one shape each, one chain and one set of meshes).
* A role whose model cannot be loaded, and every role when models are off
  (`sf_game --models none`), falls back to a primitive chosen by what it is: infantry a
  cylinder, a vehicle or vessel a cone, a structure or object a cube.

The F1 panel names the hovered unit by its role id and shows its hit points (`light_tank
18v0 (red) by model, 83.1 m, HP 400/400`); an entity that is not a role is `placeholder <kind>`
without hit points.

## Models per faction

One role can look different for each faction: art set v4 has `meridian_rifleman.glb`
and `veyran_rifleman.glb` for the single role `rifleman`. The role keeps one **kind**
and one record; only how a unit is *drawn* depends on who owns it.

* `model` is required and is the **generic** look.
* `models` is optional and gives a faction its own look. It is a map from a faction
  (`Meridian`, `Veyran`, `Neutral`) to a model (`Shape(..)` or `Glb("..")`, the same
  as `model`). A key must be one of the role's `factions`, and a faction can be listed
  only once.
* **Fallback order** when a unit is drawn: the entry for the faction of its owner, then
  the generic `model`, then the placeholder primitive of the role's category (infantry a
  cylinder, a vehicle or vessel a cone, a structure or object a cube). A model file that
  fails to load, and every `Glb` when models are turned off (`--models none`), drops to the
  next step (with a warning for the first).
* **Which faction owns a unit** is the game shell's business, not the simulation's:
  `sf_game::roster::faction_of_owner` maps the sandbox's owner seats (0 Meridian, 1 Veyran,
  2 Neutral); any other owner is drawn with the generic model.
* Roles of one faction (the sniper, the medic, the civilian) simply name their file in
  `model`; `models` is for roles that two factions share.
* **Presentation only.** The simulation never sees a model: the unit table, the checksums
  and every headless scenario (also the frozen `*_defs` copies in `assets/scenarios`) are
  the same with or without `models`.

**Validated when the definitions load:** every `Glb` (generic or per faction) is a plain
relative path to an existing `.glb` under `assets/` that lies in the directory of a pack of
`assets/config/models.ron` (the pack gives its axes and level-of-detail switches); a faction
key that is not a faction, a faction listed twice, and a faction the role does not have are
refused with the file and line. A `models.ron` that is not there (a tool with a partial assets
directory) skips only the pack check.

What is wired (art set v4, `assets/set_v4`, which replaced set v3 as the placeholder pack):

* **A model for Meridian and one for Veyran** (`models`; the generic `model` stays as it was and
  is what an owner that is neither gets): the structures construction_center, power_plant,
  refinery, barracks, vehicle_factory, radar, defence_tower, advanced_defences and shipyard; the
  vehicles harvester, light_tank, heavy_tank, siege_artillery and repair_truck; the infantry
  rifleman, rocket_trooper, engineer and scout.
* **One file in `model`** (roles of one faction, and neutral ones): decoy_emplacement
  (Meridian), fortified_bunker (Veyran), sniper (Meridian), medic and combat_engineer (Veyran),
  transport_truck (Veyran), light_transport and amphibious_transport (Meridian), gunboat
  (Veyran), civilian, technician, relay_transmitter, fuel_depot, archive_building,
  evidence_crate, ferry, bridge_segment, rail_spur, automated_gun and unmanned_artillery (the
  files of these two are `network_*.glb`), generator and ore_field.
* **Not on set v4:** `demolition_team` keeps the set v3 file (the v4 file fails `sf_asset_check`:
  five parts in one level, no nodes for the second figure); `heavy_gun_team` is a primitive (same
  reason, no v3 model either); `supply_truck` keeps its `testpack_v1` model (v4 has a
  supply truck per faction, but the role is Neutral's alone).

The file names of v4 are `<faction>_<role>.glb`, so a role of both factions has two. The
damage and construction states of a structure (`_damaged`, `_heavily_damaged`, `_foundation`,
`_scaffolding`, `_rubble`, `_no_uplink`, ...) and the shared rubble heaps are in the pack
`set_v4` but are not roles, so no definition names them: they wait for the building and map
work. Decor (ore stages, rocks, trees) and wrecks are not unit roles and are not in
`unit_defs.ron`; placing them on maps is later work (phase 2.2/2.3, wrecks 5.5; the four v4
wrecks fail the checker and are not in the repo, the v3 wrecks are, but they fail it
too: their LOD2 lacks `debris` and `scorch`).

A structure's model is scaled, the same in every direction, to fit its footprint (at most
four times bigger or smaller, `sf_render::glb::load_pack_model`). The footprints of the mission
structures are placeholders: the v4 relay transmitter is a tall mast (3.8 x 25 x 3.8 m) and
stands 52 m high on its 2 x 2 cell footprint until that footprint is decided.

## Conversion for the simulation

`UnitDefs::sim_units()` and `sim_weapons()` give `SimUnitDef` and `SimWeaponDef`:
integers and `sf_math::Fx` in the sim's own units (distances in cells, speed in cells
per second and per tick, times in ticks, names as kind numbers, a faction mask). No
float type crosses (a test checks the conversion module). They stay in `sf_data`;
`sf_sim` takes its own `UnitStats` (see "Spawning"). The 20 Hz constants (`TICKS_PER_SECOND`,
`TICK_MILLIS`) are in `sf_math`, which `sf_sim` re-exports and `sf_data` uses, so the
two crates need no dependency on each other. Since WP6.1 every Structure also converts to a
`SimBuildingDef` (`UnitDefs::sim_buildings()`: footprint, function, exit side, build ticks, price, power,
factions, prerequisites), which `sf_bridge::building_table` hands to the world ([`buildings.md`](buildings.md)).

## Where it is tested

WP3.2: `crates/sf_sim/tests/units.rs` (stats at birth, footprints, refusals, per-unit speed,
legacy commands, checksum), `assets/scenarios/units_spawn.ron` with
`tools/sf_headless/tests/units.rs` (every role of the shipped file spawned on the 256 x 256
playfield, hit points, footprint occupancy and refusals asserted, the checksum pinned on
Linux and Windows by CI; it reads a **frozen copy** of the definitions,
`assets/scenarios/units_spawn_defs/`, so a balance change does not force a re-pin),
`tools/sf_headless/tests/shipped_roles.rs` (every role of the shipped files placed
automatically and accepted, no checksum), and in `sf_game` the roster (`roster.rs`), the sandbox
(`sandbox.rs`) and the scenes (`runner.rs`).

WP3.1:

`crates/sf_data/tests/units.rs`: the shipped data loads and every reference resolves;
the roster (both ways), stats and factions against `content/units.md`; the structure
footprints of the art brief; the shipped kind numbers; a shuffled file giving identical
definitions; duplicate and retired numbers; appending a unit, a weapon, a damage type
and a movement class as data only; refusals with file and line, including numbers with
too many decimals; the conversion, and the speed per tick against the exact rounding
for every speed from 0.001 to 20.000 cells per second.
