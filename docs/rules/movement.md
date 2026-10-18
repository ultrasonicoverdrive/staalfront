# Movement classes and terrain costs

Work package WP4.1 (PLAN.md phase 4). Code: `crates/sf_data/src/units/classes.rs` and
`movement.rs` (the data and its checks), `crates/sf_bridge` (the copy into the simulation),
`crates/sf_sim/src/movement.rs` (the table and the queries), `behavior.rs` (`walk`), `orders.rs`
(`go_to`) and `world.rs` (`cost`, `passable`, the spawn rule). Scenario:
`assets/scenarios/terrain_costs.ron`. Idea: `docs/red-alert-analysis.md` section 6 (a moving unit
checks whether the next cell is accessible; the destination is not the route) and section 7 (per
unit class: permitted terrain types).

Since WP4.2 an order to move **finds its way** round what the class cannot cross
(`docs/rules/pathfinding.md`); this page is about what the terrain allows and what a step costs, and
about the legacy walks (`MoveTo`, wandering) that still go in a straight line and stop at the edge.

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
  of the shipped costs cannot move a pinned checksum. (`terrain_costs_defs/` is such a copy for the
  new scenario.)

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
in the same cell at the same time. (Units occupy no cell; since WP4.4 they have **bodies**, and a
vessel passing under a bridge collides with what crosses it: layers are not decided yet,
`collision.md`, "Not here".)
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
Nothing ends the order and the queue behind it waits; a new order replaces it. A unit that walks an
**order** (`Move`, `AttackMove`, `Guard`, `Attack`) is routed round the obstacle (WP4.2) and is
refused a step only when its smoothed route grazed a corner, which makes it ask for a cell-by-cell
route; the straight line into a river that ends at the bank is now the **legacy** `MoveTo` and the
wanderers, and a destination that cannot be reached: the route then ends in the middle of the
nearest cell it can enter (`pathfinding.md`, "Unreachable goals"). Noticing that a route is blocked
later is the replan (WP4.7). Details:

* **Structures.** A cell a structure occupies is blocked for every class that terrain restricts (the
  classes the path finder serves; aircraft fly over). A step into it is refused like a step into
  rock, so a unit on a route that a structure was placed across stops at its edge, with its order
  kept, even if its route has not noticed yet. A unit standing in such a cell leaves it at normal
  speed.
* **Corners.** A step that changes both column and row (a diagonal one) needs the two cells it
  cuts across to be passable as well, so a unit cannot slip through the corner where two blocked
  cells touch.
* **Snapping.** A unit within one step of its waypoint lands on it. That is a step like any other
  and is refused if the cell it lands in is blocked, so a destination inside water or rock is
  approached and never reached (for the legacy `MoveTo`; a routed unit never has such a waypoint:
  its route ends in the nearest cell it can enter). (Within the arrival tolerance of an eighth of a cell a unit has
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
  `cost(class, cell)`. Pathfinding (WP4.2) did not need it and did not take it: the cost of a
  step there is the average of the two cells' costs (`pathfinding.md`, "Costs"), the one place being
  `Grid::step_cost` in `path/grid.rs`. Flowfields (WP4.3) are the other reader and took that function as
  it is, so a slope would be one change that routes and fields both pick up.

The seam is in place: `behavior::walk` is the one place that scales a step, `Ground::may_step`
the one place that judges it, and a class record can gain an optional field (with a serde
default, so existing files keep loading) without touching the others. Maps without a height
layer would have no slope, so no golden would move.

## The checksum

The terrain digest is what it was: the three passable bits it used to hash are now the single
derived `TerrainFlags::ground` (every ground class can enter the type), set by
`sf_bridge::terrain_defs` from the movement costs, never by hand. Which classes are the ground
classes is data: `ground: true` on the class in `unit_classes.ron` (foot, wheeled and tracked as
shipped; false when left out; no class marked means no restriction). The flag is also the helper a
placement routine wants ("a cell any ground unit may be put on").

**Whose costs.** The headless runner derives the bits from the costs of the scenario's *own*
definitions: the frozen copy named by `unit_defs`, or the shipped file for a scenario without
one. So editing the shipped `unit_classes.ron` moves the scenarios that run on the shipped
definitions (`terrain_256`) and none that run on a frozen copy (`units_spawn`, `orders`,
`terrain_costs`); a test (`a_change_to_the_shipped_costs_leaves_the_frozen_scenarios_alone`) closes
a cost in the shipped data and checks exactly that.

The movement table has a digest of its own that joins the world checksum as section `M`, **written
only on a map with a layer, with a table, while some live entity made from the unit table has a
movement class**: whatever drives it (an order, the legacy `MoveTo`, a wander, standing still).
`terrain_256` (terrain, no table), `orders` (a table, no terrain) and the wanderer scenarios have
no such section and are unchanged.

### The pinned scenarios, the frozen costs and the one re-pin

**Terrain costs are a new rule**, and a new rule can move a pinned checksum. Frozen definitions
protect a scenario against *balance tweaks* of the shipped files, not against new rules.
`units_spawn` and `orders` run on frozen copies of the unit definitions; at WP4.1 their
`unit_classes.ron` got the real costs, exactly as shipped then, frozen at those values. What that
did:

* `units_spawn` **moved once, on purpose**: `0x0576f4e3b9fcff04` became `0x6c369f42be150d6e`. Its two
  legacy `MoveTo` walkers (a rifleman and a light tank sent to the cell (136, 90)) cross a rock
  patch, and the target is itself a rock cell; they now stop at the rock instead of walking
  through it (`tools/sf_headless/tests/units.rs` asserts where), and the world has the movement
  section. Nothing else in the scenario changed.
* `orders` did **not** move: it has no terrain layer, so every cell is open ground and the costs
  cannot show.
* `wander_5000`, `command_mix` and `terrain_256` did not move either: their wanderers are legacy
  entities without a movement class, and `terrain_256` has no unit table.

What the real costs do to units across all classes is pinned by `terrain_costs`.

**WP4.2 moved `terrain_costs` once, on purpose**: `0x8c43b875a877488c` became
`0xe98b2eed90616d3f`. Its units are given `Order::Move`, which now finds its way: the land classes
sent into the river take the bridge, everybody goes round the rock wall, and the gunboat sent onto
the bank ends in the middle of its nearest water cell instead of standing at the edge of the
bank; every unit of a terrain-bound class waits one tick for its route, so the arrival ticks of the
lanes in the open grew by one (the drone, which asks for no route, is unchanged). The scenario
file's commands are the same and the frozen definitions are the same; the old checksum was a
different behaviour, not a different scenario. `units_spawn`, `orders`, `wander_5000`,
`command_mix` and `terrain_256` did not move: `units_spawn` has only legacy `MoveTo` walkers and
`orders` has no terrain.

## The `terrain_costs` scenario

`assets/scenarios/terrain_costs.ron` on the map `assets/maps/terrain_costs` (40 by 20 cells,
straight lanes, made by `cargo run -p sf_data --example gen_costs_map`): a rifleman, a light
transport, a light tank and an amphibious transport each cross 15 cells of grass, of road and of
ore, cross the river on the bridge, are sent to the far bank along a lane that runs into the river
(the first three go up to the bridge and come back down, the hover craft crosses the water at four
fifths of its speed) and are sent through a rock wall (all four go round it); a gunboat goes down
the river and under the bridge, another is sent onto the bank and ends in the middle of the water
cell nearest to it; a survey drone (the only unit of the air class, present in the frozen copy
only: no shipped role flies yet) crosses river and rock. Until WP4.2 the units sent at the water
and the wall stopped at the edge. `tools/sf_headless/tests/terrain_costs.rs` asserts the arrival
ticks, against the cost table and as pinned numbers, the positions, the routes and that no unit
ever ends a tick on ground its class cannot enter. The checksum is in `terrain_costs.expected` and
runs on Linux and Windows in CI.

## Not here

* **Replanning** a blocked route (WP4.7). Collision and a stuck unit are WP4.4
  (`docs/rules/collision.md`: bodies, pushing, a unit that steps to a free neighbouring spot and asks
  for a new route; a step the terrain refuses now **slides** along what it grazed for a unit that walks
  an order, so a unit the crowd pushed off its route is not refused; the legacy walks still stand at
  the edge). Single units are routed since WP4.2 and groups sent to one place follow a flowfield since
  WP4.3 (`docs/rules/pathfinding.md`); the legacy `MoveTo` and the wanderers are not routed.
* **Slope**, above.
* **`water_speed` of a role** is not read by the simulation; the hover class's cost on water
  says it.
* **Vessels in the `sf_game` sandbox**: the sim can now say where water is
  (`World::passable`), so a gunboat could be placed; it is a change of the sandbox of its own.
* **Terrain that changes** (a destroyed bridge): a command that rewrites a cell and the digest.

## Where it is tested

* `crates/sf_sim/src/movement/tests.rs`: the table (order independence, digest, refusals), the
  step scaling, the cost rules in order, the corner rule.
* `crates/sf_sim/tests/movement.rs`: the queries, the speed of each cell, a move that goes over
  the bridge, a destination that cannot be reached (the order and the queue kept), who crosses
  what, the corner rule for the legacy walk and for a route, the spawn rule, wanderers that still
  cross everything, a wanderer that never ends a tick on a blocked cell, and what is and is not in
  the checksum. `tests/pathing.rs` has the rest of WP4.2.
* `crates/sf_data/tests/movement.rs`: the shipped table complete and pinned, the rules it stands
  for, permanent kind numbers, refusals with file and line, a class and a terrain type added as
  data only. `crates/sf_data/src/movement.rs`: the combining, in unit tests.
* `crates/sf_bridge`: every class arrives with its costs, every role's class is in the table.
* `tools/sf_headless/tests/terrain_costs.rs` and `pinned.rs`: the scenario.
* `crates/sf_game/src/cursor/tests.rs`: the cursor says what `World::passable` says.
