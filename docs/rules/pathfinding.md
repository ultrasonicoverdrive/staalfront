# Pathfinding for single units

Work package WP4.2 (PLAN.md phase 4). Code: `crates/sf_sim/src/path.rs` (the service and the
unit's side of it) and `crates/sf_sim/src/path/` (`grid`, `graph`, `local`, `abstract_search`, `los`,
`route`, `job`, `heap`). Scenarios: `assets/scenarios/pathing.ron` and `pathing_256.ron`. Idea:
`docs/red-alert-analysis.md` section 6 (the destination is where a unit wants to be, the route is
how it gets there, and the route can be made again). The algorithm is hierarchical A* from the
general literature (sectors, portals, an abstract graph, refinement inside sectors), written from
scratch.

A unit with an order to move **asks for a route** to the order's destination and **follows it**
waypoint by waypoint. The order keeps the destination; the route is a separate record of the unit
(`docs/rules/orders.md`, "Destination and route"). Everything here is integer or fixed point,
deterministic, and inside a fixed amount of work per tick.

## Who is routed

Routes are found for a **movement class that terrain restricts**: some terrain type is
`Impassable` for it (foot, wheeled, tracked, hover and water as shipped). The world builds a graph
for each such class when it is made (`World::with_movement`, on a map with a terrain layer).

* **Air ignores pathing.** A class that can enter every terrain type (the shipped `air`) has no
  graph. Its units steer straight at the destination and are not stopped by structures either.
* A unit without a class, the legacy wanderers and the legacy `MoveTo` are not routed: they walk
  the straight line, as before WP4.2, and `terrain_256`, `wander_5000`, `command_mix`,
  `units_spawn` and `orders` hash as they did.
* A world without a terrain layer or without a movement table is open ground: no graphs, no
  routes. That is why `orders` (no terrain) did not move.

**Mobile units are not obstacles here** (collision is WP4.4). **Structures are**: a cell a structure
occupies is blocked for every routed class (a structure stands on buildable ground, which a boat
never meets, but a hover craft does).

## Costs

A cost is the time a step takes at the unit's own speed, as an integer: **4096 per cell at a speed
multiplier of 1000**, `4096 * 1000 / multiplier` for another. A step from cell `a` to cell `b`
covers half its length in each (a unit walks from the middle of a cell to the middle of the next),
so it costs the average of the two cells' per-cell costs, a diagonal step that times the square root
of two (5793 over 4096, rounded down). The cost of a step does not depend on its direction. This is
the "distance divided by speed multiplier" of the movement table (`movement.md`), in fixed point.

A step is allowed exactly when `Ground::may_step` allows it (`movement.md`, "Corners"): the cell it
ends in can be entered and, for a diagonal step, so can the two cells it cuts across. A cell the
unit stands in may be blocked (a structure was placed on it); leaving it costs the cost of normal
speed.

**The heuristic is the octile distance** (straight steps for the longer axis minus the shorter,
diagonal steps for the shorter) at the **cheapest per-cell cost** the class has: the highest speed
multiplier among the cells in the area searched. For the abstract search that is the whole map; for a
search inside one or two sectors it is those sectors', which is exact on ground of one speed.
Never more than the real cost, so every A* here is exact.

**Ties are broken the same way everywhere**: lowest `f` (cost so far plus heuristic), then lowest
`h`, then the lowest index (cell number inside the area, or node number). The heap
(`path/heap.rs`) is an indexed binary heap with that total order; an element is in it once, a better
cost lowers it in place, and it never allocates after it is made. Nothing iterates a hash map.

## Sectors and portals

The map is cut into **sectors of 16 by 16 cells** (the last row and column smaller when the map side
is not a multiple of 16). Why sixteen:

* A sector is 256 cells (64 m by 64 m at B12's 4 m cells), so a search inside one is bounded and
  small (256 expansions, 512 for two sectors side by side), and a budget of a few thousand nodes
  holds several.
* A 256 by 256 map has 256 sectors (a 1,024 map 4,096): the abstract graph stays in the low
  thousands of nodes, searched in a few hundred expansions.
* A border of 16 cells has at most 8 separate openings, so a side of a sector needs 8 portal slots
  and a sector 32: fixed arrays, node numbers that are a plain `sector * 32 + slot`.
* A line of sight is capped at one sector (below), which is what lets a change of one sector find
  the routes it touches by looking at its eight neighbours.

On every border between two sectors a **run** is a maximal stretch of border positions where the
cell on both sides can be entered by the class. A run gets **portals**, pairs of cells facing each
other, one in each sector (two nodes of the graph):

| Length of the run | Portals |
|---|---|
| 1 to 5 | one, in the middle (the lower middle of an even run) |
| 6 to 11 | three: both ends and the middle |
| 12 or more | four, spread evenly from one end to the other |

One portal per run is enough for connectivity (the cells of a run are all open and next to each
other, and a diagonal step across a border needs its two orthogonal cells open, which is a run
position of its own); a test checks on random maps that two cells are connected in the abstract
graph exactly when they are on the grid. The extra portals on a long run are for quality: the
abstract search chooses the way round an obstacle from the cost of walking to the portals, so a
portal far from where the best route would cross makes the wrong way look cheaper, and the ends of
a run are where an optimal route hugs the corner of an obstacle. The counts were chosen by
measurement (below): two portals for a run of 6 to 11 and three from 12 on gave a worst case of
1.15 of the best route, these give 1.08. Never more than one portal for every two positions of a
run, so a side's 8 slots always suffice. A side's 8 slots are rewritten as a whole when its border changes, so a changed border
never renumbers the nodes of another one.

The **edges** of the graph:

* **crossing**: from a portal node to the one facing it, the cost of that one orthogonal step;
* **intra-sector**: from a portal node to each other portal node of the same sector, the cost of the
  cheapest walk inside the sector (a Dijkstra from every portal); none where there is no walk. A walk
  that leaves a sector and comes back is not an edge: the graph finds it through the portals.

Costs of a sector are computed once, when the world is made, for every class. For a 256 by 256 map
that takes about 55 ms per class in a release build (`pathing_256`).

## Changes of the map

When a structure is placed or despawned (the only things that change what a cell allows) the
world reads the sectors of its footprint again from the terrain and the structures, rebuilds the
portals on their borders (both sides) and the edges of those sectors and of their neighbours (whose
portals on a shared border may have changed), and nothing else. This happens inside the tick, in
the command that places or removes the structure, and nowhere else: there is no public call that
changes the graph, because the graph is part of the hashed state (§5.1). A later change of the
terrain (a destroyed bridge) gets its own command, which calls the same rebuild. A test builds the graph again from scratch after a change
and compares: they are equal.

The graph has an **epoch**, which a change counts up, and remembers the epoch of the last change of
each sector. A route is stamped with the epoch it was made in. When the epoch has moved on, the unit
looks (once) at whether any sector **within one of** a sector its route, its position or its goal is
in changed since: line-of-sight segments are at most a sector long, so that covers everything a
route crosses. If so, the unit **asks for a new route from where it stands** (and waits for it); if
not, it keeps its route. This holds for a route that waits for a new search as well: a unit
whose destination moved (a chase) keeps following the waypoints it has until the new route is
ready, and a map change that touches them drops them at once and the unit waits (a hunter used to
walk through a structure placed meanwhile). As a second line of defence a step into a cell the
unit's class cannot enter, **a cell a structure occupies included**, is refused like a step into
rock: the unit stops at the edge (`movement.md`). This is not WP4.7's replan: nothing notices a route made unwalkable by a
change this check did not cover, a target that disappears, or a better route that a removed
structure now offers. The graph itself is always right.

## One request, in stages

A request is served by a **job**, in these stages (`path/job.rs`). Each spends from the tick's
budget, and none allocates.

1. **Direct.** If the middle of the goal cell is within sight (a thick line of sight of at most
   sixteen cells) the route is that one waypoint.
2. **Connect start.** A Dijkstra inside the start sector gives the cost from the unit to each portal
   of its sector. (When the goal is in the same sector it is an A* straight to the goal, and the
   route is that path.)
3. **Connect goal.** The same from the goal, in the goal's sector, to its portals.
4. **Abstract.** A* over the portals, from the start's links to the goal's links: the start and the
   goal are two virtual nodes. **Resumable**: it keeps its heap between ticks and does at most the
   expansions it is given.
5. **Nearest.** Only when the goal cannot be reached (below).
6. **Refine, then smooth.** The next legs of the abstract path are searched cell by cell and pulled
   taut. A **leg** is one sector, up to a portal; or, where the path crosses a border, **two sectors
   side by side** as one search, from the unit to the portal after the crossing, of which only the
   cells up to the first one across the border are kept. So a route crosses a border where it suits
   it, not at the portal; the portal after it only shapes the search and is not committed to. A job
   refines three legs and stops; **a refill** (the ring ran low) is the same stages from here on.

A route found this way is not always the best: the graph is exact between portals, not between all
cells. **How good it is is measured against the right reference.** A unit walks at any angle (the
route is pulled taut), so the best walk along the grid, which cannot cut a corner, is the wrong
yardstick: a route that cuts corners is shorter than it, and a ratio under 1 against it says nothing
about the search. The reference is the best **any-angle route by the game's own line-of-sight rule**:
Theta* over the cells of the map with the same thick line of sight (`path/tests.rs`,
`any_angle_reference`; the headless test `tools/sf_headless/tests/pathing.rs` has its own copy on
`World::sight_clear`). On about 1,100 random routes of 20 cells or more over rock-strewn 70 by 55
maps (three seeds) the walked length is **1.006 of it at the median, 1.05 at the 99th percentile
and 1.075 at worst**; both tests assert at most 1.10. What is left is the abstract search choosing
the longer of two ways round an obstacle when the portals make them look alike. Taking the grid-best
path and pulling it taut with the same smoothing gives 1.00, so the loss is in the graph and not in
the smoothing.

### Line of sight and string pulling

A unit turns at its own rate and moves while up to 16 facing steps off its heading, so it strays
from the line to a waypoint by a fraction of a cell near a corner. A line of sight that only asks
"does the line touch a blocked cell" would let a route graze a corner and the unit would be refused a
step. So the test is **thick**: a segment is clear when no blocked cell (and no cell outside the map)
is within **a quarter of a cell** of it (`path/los.rs`; integer arithmetic on the raw numbers; a test
checks a fine sampling). Two waypoints that cannot see each other that way keep the cell-by-cell
route between them. A corridor one cell wide therefore stays cell by cell, and one two cells wide is
pulled straight.

Smoothing takes the raw cell route of a leg, starting from the unit's position (the ring's last
waypoint, for a later leg), and for each vertex tries the farthest point it can see, then a
binary search between the nearest and the farthest. A segment is at most sixteen cells long and the
reach in the raw route 24 points. The vertices are cell centres. The destination itself is the last
waypoint of a complete route that can be reached, after the centre of its cell.

### Unreachable goals

When the goal cell cannot be entered or is not connected to the unit, the abstract search runs out
of nodes after expanding all it can reach. The route then leads to the **reachable cell nearest to
the goal, by the heuristic**: the better, by `(heuristic, cost from the unit, cell number)`, of the
nearest cell of the start sector and of the nearest cell found in the sector of the expanded portal
that was nearest. The unit walks there, stands in the middle of that cell and **the route says
so**: `World::route_of(id)` has `unreachable: true`, `status: Stranded`, `goal: Some(that cell)`.
That is the mark in the order state: it lives in the unit's route record next to its order, and goes
when the order is replaced. **The order is kept**, with its queue, as WP4.1's stop at the edge kept it
(`movement.md`); ending it ("gives up", "stands as near as it gets") is WP4.4 and WP4.7. A search for
an unreachable goal explores the whole connected part of the map, so a crowd of such orders is
expensive (below, "Not here").

## The route a unit keeps

`path/route.rs`: fixed size, `Copy`, a column of its own (it is long: the vector grows to the
highest slot that has asked for a route, so a million wanderers cost nothing).

* a **ring of 8 waypoints**, cells already refined and pulled taut: the unit steers at the front one
  and takes it off when it arrives, and the ring is refilled (a job of the same kind) when it holds
  **2 or fewer**;
* the **abstract path still to refine**: up to **64 portal nodes**, from which the next legs are
  refined lazily. A plan with more (a very long way on a big map) keeps the first 64 and is planned
  again from the end of the ring when they are used up;
* the cell it was asked for, the cell it leads to, the epoch, and a few flags (complete,
  unreachable, cautious).

The states: **Idle** (no route), **Wanted** (asked for; the unit follows the waypoints it has and,
with none, waits), **Active** (a route exists; it may still be refined), **Stranded** (the route
ended short of the destination).

**What the unit does each tick** (`Pathing::steer`, called by `orders::go_to`):

* no route yet: it asks (the request is queued) and **stands still**;
* a destination that moves more than **3 cells** from the cell the route was asked for (a guard or an
  attack on a moving target): it asks again, and keeps following the old waypoints until they run
  out;
* a route with a waypoint: steer at its middle; the waypoint is taken off the ring when the walk
  reaches it;
* a complete route with no waypoint left: steer at the destination itself.

`World::waypoint_of` is the point the unit steers at: a waypoint, then the destination. While the
unit waits for a route it is `None`.

**A step refused on a smoothed route** (the margin was not enough for a fast unit that turns slowly)
makes the unit ask again for a **cautious** route, cell by cell with no smoothing, which cannot be
refused; a step refused on that stands the unit still, with its order kept.

## The budget and the queue

The path service runs once per tick after the commands and before the units move
(`Pathing::serve`). It has a **budget of nodes**: 4,096 by default (`DEFAULT_NODE_BUDGET`), at least
512 (`MIN_NODE_BUDGET`: a line of sight, 60 nodes at most, must fit with room to spare); `World::with_path_budget(n)` and the
scenario field `path_budget` set it, before the world starts. **The number is game data**:
`assets/config/sim.ron` (`path_node_budget`, read by `sf_data::sim_settings`, handed over by
`sf_bridge::path_budget`) is what the game gives its world. A world that is given none, which is what
the tests and the frozen headless scenarios have, uses the simulation's default, the same number, so
turning the knob in the file never moves their checksums. A node is one cell or portal
expanded in a search, or one cell looked at in a line of sight. Nothing else counts.

* Requests are served **going round the entity ids**, one job at a time: the job in flight finishes
  (over several ticks if it must) before the next starts, and the next is the first unit with work
  from the slot after the one the last job was started for (`next_slot`, hashed), once round at most
  per tick. Requests asked in the same tick are therefore served in the order of the ids; a request
  asked later waits for the lap, never for the whole queue twice, and the highest ids do not always
  come last (service used to restart from the lowest id every tick: with 30 units and the smallest
  budget the last waited 351 ticks, now 36). A unit whose request is not served yet **waits where it
  stands**, deterministically.
* **A tick spends its budget to the last node whenever a request waits, and never more.** Every
  stage stops where the budget runs out and goes on next tick: the abstract search between two
  expansions, a **local search between two cells** (`Local::begin` and `Local::advance`; a sector
  search is no longer all-or-nothing, which used to leave up to a whole search's worth of budget
  unspent while a later, smaller request could have used it), the smoothing and the direct sight
  between two lines of sight. A line of sight is the one atom, at most 60 nodes, so what a tick can
  leave unspent while someone waits is under that (an eighth of the smallest budget). A test
  asserts at least 80 % use on every tick of the crowd with requests waiting (measured at least 88 %).
  The state of the half-done search is the scratch space of the one job in flight, and the budget
  is the only thing that cuts a search, so the result is the same whatever the slices (tested on
  slices of 1 to 1,000 cells and on whole jobs at 61 and 512 nodes). **A tick never spends more than
  the budget** (`World::path_stats().nodes_last_tick`, asserted every tick by the headless tests).
* **The budget decides when a unit gets its route, not which**: a job finishes the same way on any
  budget (a test runs one on two budgets and compares the result; a scenario test runs it on two and
  compares where everyone ends). The budget is hashed with the routes.
* Measured, `pathing_256`: 200 tanks on the playfield, all ordered on tick 1 to a different cell
  some 300 cells away across two rivers: **all 200 had their first waypoints after 85 ticks** (4.3 s of
  game time, 603 jobs in all with the refills) at the default budget; the test limits it to 120.
  A first request costs about a thousand nodes, a refill a few hundred.
* **Large orders are not this service's job.** 200 units sent to 200 different cells is the worst
  case for single-unit searches and still takes five seconds for the last one; a group sent to *one*
  place is WP4.3's flowfield, one search for everybody, and that is how a group order is meant to be
  handled. A cheap improvement for the meantime, not done here: units that start in the same sector
  with the same goal cell can share one abstract path (only the first and last legs differ), which
  would cut the work of a group order by most of the abstract searches.

`World::route_of(id)` shows a unit's route: `None`, `Waiting` (asked, no waypoint yet), `Following`,
`Stranded`; `World::path_stats()` shows the budget, the nodes the last tick and the busiest tick
spent, jobs done, the units waiting and the sectors rebuilt; `World::plan_route(class, from, to)`
plans a whole route at once with no budget and changes nothing (tests and tools).

## The checksum

A section `P`, written only while the world has pathfinding **and** some live unit has a route: the
budget, the epoch, the job in flight (unit, ticket, stage, progress) and every route that exists
(state, flags, cells, waypoints and portal path, in ascending slot order). A world that never
routed a unit hashes as before: `wander_5000`, `command_mix`, `terrain_256`, `units_spawn` and
`orders` are unchanged.

`terrain_costs` moved, on purpose, because its orders now find their way round things instead of
stopping at them (see `progress.md`).

## Not here

* **Flowfields for groups** (WP4.3), **collision, pushing and a stuck unit** (WP4.4: a unit that a
  structure is placed on the middle of, with no free neighbour, can reach nothing and stands with
  its order), **turning before driving and turrets** (WP4.5), **formations** (WP4.6), **replanning**
  when a route is blocked or a target disappears (WP4.7).
* **Diagonal crossings of a border** (a corner) are not portals; every connection is found through
  an orthogonal one.
* **Slope** (`movement.md`, "Slope: later").
* **A cache for unreachable goals.** Each such order searches the whole connected part of the map
  (some thousand nodes). Fine for single orders, not for a crowd; WP4.3 gives a crowd one field.
* **Terrain that changes** needs a command that rebuilds the sector from inside the tick; none exists yet.

## Where it is tested

* `crates/sf_sim/src/path/`: the heap (against a sort), the cost and the heuristic, the sector
  searches (a wall, a closed room, early exit), the line of sight (against a sampling), the portals
  (their positions, symmetric edges, a rebuilt sector equal to a fresh build), the abstract search
  (resuming equals running whole; connectivity equals the grid's on random maps), the route's
  storage, and whole routes against a plain search of the grid (`path/tests.rs`: straight in
  open ground, through a gap, round an island, a bridge, random routes against the best any-angle
  route, a job on two budgets).
* `crates/sf_sim/tests/pathing.rs`: through the world: the tick of the request, an unreachable goal
  and its mark, stop and new orders, a queue, a structure placed on the way and taken away, one far
  away, one on the unit, the budget and the order of service, a unit removed while its route is made,
  a clone mid-run, a chase over a bridge, a fast unit that turns slowly, a boat under a bridge, and
  a hunter that waits for a new route while a structure is placed in its way.
* `tools/sf_headless/tests/pathing.rs` with `assets/scenarios/pathing.ron` (map
  `assets/maps/pathing`, made by `cargo run -p sf_headless --example gen_pathing_map`): a maze of
  rock and water, a river with one bridge, an island, a structure that closes a gap, 24 requests at
  once; arrival, no cell it cannot be in, the length against the best any-angle route, the nearest
  cell, the budget, the order of service. And `pathing_256.ron`, the load test. Both are pinned in
  `assets/scenarios/*.expected` and run on Linux and Windows in CI.
