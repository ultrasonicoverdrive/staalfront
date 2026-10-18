# Movement classes and terrain costs

Work package WP4.1 (PLAN.md phase 4). Code: `crates/sf_data/src/units/classes.rs` and
`movement.rs` (the data and its checks), `crates/sf_bridge` (the copy into the simulation),
`crates/sf_sim/src/movement.rs` (the table and the queries), `behavior.rs` (`walk`), `orders.rs`
(`go_to`) and `world.rs` (`cost`, `passable`, the spawn rule). Scenario:
`assets/scenarios/terrain_costs.ron`. Idea: `docs/red-alert-analysis.md` section 6 (a moving unit
checks whether the next cell is accessible; the destination is not the route) and section 7 (per
unit class: permitted terrain types).

A **movement class** says which terrain a unit can use and how fast it moves on it. The terrain
says what a cell *is* (`docs/rules/terrain.md`); the class says what a unit can do there. Every
class has a **cost for every terrain type**: it cannot enter ("impassable"), or it moves at a
fraction of its own speed, in thousandths ("speed multiplier": `1000` is normal, `1200` a fifth
faster, `500` half).

## The table

The shipped values (`assets/config/unit_classes.ron`). They are placeholders for the balance
rounds (phase 15) in the sense that a number may change; the **shape** of the table (which pairs
are impassable) is decided here. A test pins the numbers, so a change is deliberate.

| Class (kind) | grass | road | water | rock | ore | bridge | Roles today |
|---|---|---|---|---|---|---|---|
| `foot` (0) | 1000 | 1100 | impassable | impassable | 800 | 1100 | infantry |
| `wheeled` (1) | 800 | 1200 | impassable | impassable | 600 | 1200 | trucks, light transport, repair truck |
| `tracked` (2) | 1000 | 1200 | impassable | impassable | 800 | 1200 | tanks, artillery, harvester |
| `hover` (3) | 1000 | 1100 | 800 | impassable | 900 | 1100 | amphibious transport |
| `water` (4) | impassable | impassable | 1000 | impassable | impassable | 1000 | gunboat, ferry |
| `air` (5) | 1000 | 1000 | 1000 | 1000 | 1000 | 1000 | none yet |

Why these shapes:

* **Rock** is "steep or high ground" (`terrain.md`); the generator paints it where the rock
  material is. Nothing on the ground crosses it, not even infantry, and neither does a hover
  craft. Only aircraft pass. (A map that wants infantry-only passes can use a terrain type
  added later: a new type is data.)
* **Water** stops everything that cannot swim: foot, wheeled and tracked. Hover crosses it, at
  800: the amphibious transport's `water_speed` is 1.2 cells per second against 1.5 on land, and
  1.5 * 800 / 1000 is 1.2. A test keeps the two in step; the simulation reads the cost, not the
  unit's `water_speed` field (see "Not here").
* **Road** is quicker than grass for the three ground classes, **ore** slower; wheeled vehicles
  gain the most on a road and lose the most on rough ground, tracked ones are steady, infantry
  barely care. Hover is nearly indifferent.
* **Vessels** (`water`) move only on water and on a bridge cell (below).
* **Air ignores terrain**: normal speed on every type, written out as 1000 in each column so
  that the "every pair is defined" rule has no exception. A flyer is still bound by the map edge.

## Where it lives, and why

The costs are a `terrain_costs` list **inside each movement class** of
`assets/config/unit_classes.ron`, not a new `movement_costs.ron`:

* adding a movement class is one record and no other edit (quality rule 6; a test appends a class
  as data only), while a separate matrix file would have to be kept in step with the class list;
* the class `kind` number is permanent (`units.md`, "Numbers are permanent") and the costs ride
  with it: a class that is retired takes its costs with it;
* the **frozen copies** of the definitions that pinned scenarios run on (`*_defs/`) already hold
  `unit_classes.ron`, so freezing the costs there needs no new mechanism, and a balance change
  cannot move a pinned checksum. (`terrain_costs_defs/` is such a copy for the new scenario.)

The terrain side is `assets/config/terrain_types.ron`: a type has an `id` (what map files store,
never renumbered) and a unique name, plus only the two flags that are really the type's own,
`buildable` and `harvestable`. The three `passable` flags of WP2.1 are gone; the file refuses
them. A class names a terrain type by its **name** in `unit_classes.ron`; `sf_data` turns names
into ids and `sf_bridge` copies ids and numbers into the simulation, which knows neither names
nor files.

### Validation

Two levels, because the two files are read apart:

* `sf_data::units` checks one class on its own, with file and line: terrain names are snake_case
  and listed once, a speed is between 100 and 2000 thousandths (0 is not a speed: write
  `Impassable`), and the class `kind` is unique, not retired and at most 255, as before.
* `sf_data::movement::MovementCosts::new(classes, terrain_types)` puts the files together:
  **every class has a cost for every terrain type** (a new terrain type has to be thought about
  for every class, there is no default), and no class names a terrain type that does not exist
  (a typo would be a cost nothing reads). Both game and headless runner build it, so a mistake
  is found when the game starts, not when a unit meets the cell.
* `sf_sim::World::with_movement` checks once more, on the terrain it was made with: every
  terrain type of the map must be listed by every class (`ConfigError::MovementGap`).

## The bridge

A bridge is a **road over water**, and the generator makes one only where the road crosses the
river. One terrain type, one cell, two things that can use it, so the rule is about *which class
uses it how*:

* **land crosses it**: foot, wheeled and tracked move on it like on a road (1100 to 1200), and
  so does hover;
* **water passes under it**: a vessel may enter a bridge cell, at normal speed (1000). Picking
  "blocked" would let a single bridge dam a whole river for ships, and the river is what the
  bridge crosses;
* air flies over, as over everything.

A unit on a bridge cell is just in the cell: there is no layering, so a vessel and a tank can be
in the same cell at the same time (units occupy no cell until phase 4's collision, WP4.4).
Because "under" only means anything over water, **a map must put a bridge cell over water**; a
bridge painted over dry land would let a vessel cross the land through it. The generator does
this; the editor (phase 10) is where it gets checked. A bridge that can be destroyed (missions
do this) is a later change of the cell's terrain type and a change of the digest; the table does
not need to know.

## What a cost does

The multiplier of the cell **the unit stands in when the step starts** scales its speed per tick
(`sf_sim::scale_step`: `step * permille / 1000`, rounded once to the nearest raw fixed-point
unit, never below one raw unit, never above `MAX_STEP`). At 1000 the step is the unit's own,
bit for bit. A unit never moves more than `MAX_STEP`, seven eighths of a cell per tick (17.5
cells per second; no shipped unit is anywhere near), so a step reaches at most the next cell and
no cell can be jumped over.

A step is **refused** when it would end in another cell that the class cannot enter. The unit
then keeps its position, it has still turned toward its waypoint, and **its order is kept**: it
stands at the edge (less than one step short of it) and tries again on the next tick, forever.
Nothing ends the order and the queue behind it waits; a new order replaces it. Going around the
obstacle is pathfinding (WP4.2), noticing that the route is blocked is the replan (WP4.7); until
then a straight line into a river simply ends at the bank. Details:

* **Corners.** A step that changes both column and row (a diagonal one) needs the two cells it
  cuts across to be passable as well, so a unit cannot slip through the corner where two blocked
  cells touch.
* **Snapping.** A unit within one step of its waypoint lands on it. That is a step like any other
  and is refused if the cell it lands in is blocked, so a destination inside water or rock is
  approached and never reached. (Within the arrival tolerance of an eighth of a cell a unit has
  arrived without a step, see `orders.md`; that is why only a unit faster than 2.5 cells per
  second can show the refusal at all.)
* **Standing in a cell it could not enter** (nothing puts a unit there today): it moves at normal
  speed and may step to a passable cell; it may not step to another blocked one.
* **Spawning.** `Command::SpawnUnit` of a mobile unit on a cell its class cannot enter is
  refused, like a spawn inside a structure: a rifleman is not put into the river, a gunboat not
  onto grass. Objects (no class) stand where they are put.
* **Wander and `MoveTo`.** The legacy behaviours of a unit made from the unit table obey the same
  rules. A wandering unit whose step is refused draws a new target; a `MoveTo` keeps its target
  and stands at the edge like an order.

### What ignores terrain

* Wanderers made by the legacy `Command::Spawn`: they have no unit table entry and no class
  (`wander_5000`, `command_mix`, `terrain_256` keep walking over everything).
* A world **without a terrain layer** is open ground for every class, whatever table it has.
* A world **with a layer but without a movement table** is open ground too.
* A class that the table does not have is never blocked (the same rule the cursor always had);
  `sf_bridge` has a test that every role's class is in the shipped table.
* A cell **outside the map** is impassable for everyone, aircraft included.

## The queries

`World::cost(class, cell) -> Cost` (`Cost::Impassable` or `Cost::Speed(permille)`) and
`World::passable(class, cell) -> bool` are read-only and answer by the rules above. The movement
itself asks the same function (`movement::Ground`), so what the cursor shows is what a unit will
do. `class` is the permanent kind number a `UnitStats::movement_class` carries; `UnitTable`
already carried it (checked: the bridge test compares every role's class with the one its
definition names).

## The cursor

`Passability` of WP3.5, which read the old flags by class *name* in `sf_game`, is gone. The
cursor's no-go test is `World::passable(class, cell)` for each selected unit that can take an
order: no-go when none of them can enter the cell, as before (`cursor.md`). One table decides the
cursor, the right-click and the walk.

## Slope: later

The plan says height is visual plus "a few gameplay effects (vision from a hill, slower uphill)".
Decision: **uphill slowing is not in this work package**, and `sf_sim` gets no per-cell height
yet. Reasons:

* steep ground already is a terrain type: the generator marks the steep and high ground `rock`,
  which every ground class refuses, so the case that matters most (a cliff) is covered;
* a coarse height per cell needs a definition (the map has a vertex height every metre, 5 by 5
  samples touch a 4 m cell: which one, or an integer average?), a new layer handed over by
  `sf_bridge`, a digest and a checksum section that exists only for maps with it, and a
  guarantee that it is bit-exact. That is a design of its own, and vision (phase 8) is the other
  reader and wants a say;
* slowing is a property of a **step** (from one cell to the next), not of a cell, so it is not
  `cost(class, cell)`. Pathfinding (WP4.2) and flowfields (WP4.3) are the readers that decide
  how it enters a route's cost; the cost model should be designed once, with them.

The seam is in place: `behavior::walk` is the one place that scales a step, `Ground::may_step`
the one place that judges it, and a class record can gain an optional field (with a serde
default, so existing files keep loading) without touching the others. Maps without a height
layer would have no slope, so no golden would move.

## The checksum

The terrain digest is what it was: the three passable bits it used to hash are now the single
derived `TerrainFlags::ground` (the three ground classes can all enter the type), set by
`sf_bridge::terrain_defs` from the shipped costs, never by hand. It is also the helper a
placement routine wants ("a cell any ground unit may be put on"). The movement table has a
digest of its own that joins the world checksum as section `M`, **written only on a map with a
layer, with a table, while the orders section is written** (it rides with the orders the way the
weapon reach does, see `sf_sim::checksum`). That is what keeps `terrain_256` (terrain, no table),
`orders` (a table, no terrain) and `units_spawn` (terrain and a table, legacy `MoveTo` walks, no
orders) at the values they were pinned with. Nothing is hidden by leaving it out: a different
table changes what a spawn accepts or where the first unit that moves ends up, and so the state
itself.

### The pinned scenarios and their frozen costs

`units_spawn` and `orders` run on frozen copies of the unit definitions, and the copies now
carry costs: **every class at 1000 on every type**, which is the world as it was when they were
pinned, because terrain did not exist for a unit then. This is not cosmetic for `units_spawn`: its
two walkers (a rifleman and a light tank sent by the old `MoveTo` to the cell (136, 90)) go
across a patch of rock and the target is itself a rock cell, so under the shipped costs they
would stop at the rock and the checksum would move. With the frozen costs it is byte-identical.
What the shipped costs do to units is pinned by `terrain_costs` instead.

## The `terrain_costs` scenario

`assets/scenarios/terrain_costs.ron` on the map `assets/maps/terrain_costs` (40 by 20 cells,
straight lanes, made by `cargo run -p sf_data --example gen_costs_map`): a rifleman, a light
transport, a light tank and an amphibious transport each cross 15 cells of grass, of road and of
ore, cross the river on the bridge, walk into the river (the first three stop at the bank, the
hover craft crosses at four fifths of its speed) and into a rock wall (all four stop); a gunboat
goes down the river and under the bridge, another is stopped by the bank; a survey drone (the
only unit of the air class, present in the frozen copy only: no shipped role flies yet) crosses
river and rock. `tools/sf_headless/tests/terrain_costs.rs` asserts the arrival ticks, against the
cost table and as pinned numbers, the positions, the blocked states and that no unit ever ends a
tick on ground its class cannot enter. The checksum is in `terrain_costs.expected` and runs on
Linux and Windows in CI.

## Not here

* **Pathfinding** (WP4.2 and WP4.3): a blocked straight line is a unit standing at the edge.
* **Collision and a stuck unit** (WP4.4), **replanning** a blocked route (WP4.7).
* **Slope**, above.
* **`water_speed` of a role** is not read by the simulation; the hover class's cost on water
  says it.
* **Vessels in the `sf_game` sandbox**: the sim can now say where water is
  (`World::passable`), so a gunboat could be placed; it is a change of the sandbox of its own.
* **Terrain that changes** (a destroyed bridge): a command that rewrites a cell and the digest.

## Where it is tested

* `crates/sf_sim/src/movement/tests.rs`: the table (order independence, digest, refusals), the
  step scaling, the cost rules in order, the corner rule.
* `crates/sf_sim/tests/movement.rs`: the queries, the speed of each cell, standing at the edge
  with the order and the queue kept, the snap refusal, who crosses what, the spawn rule,
  wanderers that still cross everything, a wanderer that never ends a tick on a blocked cell,
  and what is and is not in the checksum.
* `crates/sf_data/tests/movement.rs`: the shipped table complete and pinned, the rules it stands
  for, permanent kind numbers, refusals with file and line, a class and a terrain type added as
  data only. `crates/sf_data/src/movement.rs`: the combining, in unit tests.
* `crates/sf_bridge`: every class arrives with its costs, every role's class is in the table.
* `tools/sf_headless/tests/terrain_costs.rs` and `pinned.rs`: the scenario.
* `crates/sf_game/src/cursor/tests.rs`: the cursor says what `World::passable` says.
