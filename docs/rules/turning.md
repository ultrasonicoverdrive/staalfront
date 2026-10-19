# Turning: the hull before it drives, the turret apart from it

Work package WP4.5 (PLAN.md phase 4). Code: `crates/sf_sim/src/turning.rs` (the profiles and the
steering), `behavior.rs` (`walk`, the one step every walk takes), `orders.rs` (`go_to`, the attack
facing), `turret.rs` (the turret), `movement.rs` (the table that holds the profiles), `world.rs`
(`Command::AimTurret`, the tick) and `checksum.rs`; the data in `sf_data::units::classes`
(`turning`) and `unit.rs` (`turret_rate`), copied by `sf_bridge`; the presentation in
`sf_render::instancing` (`Motion::with_turret`) and `sf_game::runner`. Scenario:
`assets/scenarios/turning.ron`. Idea: `docs/red-alert-analysis.md` section 6 (a vehicle works out
which way it has to turn, turns, and goes on; it does not wait for the order to steer every
small piece), facts only.

A unit has two directions: where its **hull** points and, for a unit with a turret, where its
**turret** points. The hull is what drives; the turret is what will shoot (phase 5). Since WP4.5
the hull turns **before it drives**, in a way that depends on the movement class, and the turret
turns **on its own**, at its own rate.

All of it is integer. A direction is a `Facing`, one of 256 (`sf_math`, 256 steps to a full turn; 8
steps a tick is 225 degrees a second). The shortest way round is `Facing::turn_toward`, which
never overshoots and, when the target is **exactly opposite** (128 steps off), turns the
**decreasing** way. That tie-break is fixed and holds for every hull and every turret.

## The hull

Every movement class has a **turning profile** (`turning` in `unit_classes.ron`); the unit's own
`turn_rate` says how fast it can turn at best. Every walk (`walk`, the one step the orders, the
legacy `MoveTo` and a sidestep all take) works out the heading it wants (the facing closest to
the direction of its waypoint), turns by the profile, and drives or not:

| Mode | The hull | Shipped for |
|---|---|---|
| `Instant` | the heading **snaps** onto the wanted one in the tick it is wanted, and the unit drives in that same tick. `turn_rate` is not used | foot |
| `Pivot` | **turns in place** at the unit's turn rate; drives only when its heading, after this tick's turn, is within `drive_tolerance` steps of the wanted one, and steers on at the turn rate while it drives | tracked |
| `Arc` | **has to move to turn**: it turns only as fast as a circle of `min_radius` cells allows for the distance it covers in the tick, never faster than its turn rate. A target inside its two turning circles is reached by a **turn on the spot** at `pivot_permille` thousandths of the turn rate (the stand-in for backing and turning), then it drives | wheeled, hover, water |

The shipped numbers (`assets/config/unit_classes.ron`; placeholders for the balance rounds, but
the **shape** of the table is decided here, and a test pins the modes):

| Class | Mode | `drive_tolerance` | `min_radius` | `pivot_permille` | What it is |
|---|---|---|---|---|---|
| foot | `Instant` | (16) | | | a person turns at once |
| tracked | `Pivot` | 16 steps (22 degrees) | | | pivots on its tracks; the margin the route smoothing was built on |
| wheeled | `Arc` | 16 steps | 1.5 cells (6 m) | 250 | a truck's turn is a circle of 6 m; at a quarter of the turn rate it backs and turns |
| hover | `Arc` | 16 steps | 2 cells (8 m) | 500 | wider, and thrusters that turn it half as fast on the spot |
| water | `Arc` | 16 steps | 4 cells (16 m) | 125 | a long turn, and a rudder that barely turns the vessel when it stands |
| air | (not given) | | | | no role flies yet; a class that says nothing turns as the walk did before WP4.5: `Pivot`, 16 steps |

A class that leaves `turning` out, or one field of it, gets the old walk (`Turning::LEGACY`:
`Pivot`, 16), and so does every entity without a movement class (the legacy wanderers of the
`Spawn` command). Nothing in the table of the **tracked** class changed the way a tank behaves:
before WP4.5 every unit turned at its turn rate and drove within 16 steps of its heading; that
is now the tracked profile, and the number is data.

### Pivot

The first tick of an order to go behind itself: a light tank (turn rate 10) facing east and ordered
west turns 10 steps a tick the decreasing way (0, 246, 236, ... 128), stands still while it is more than
16 steps off, sets off on the twelfth tick, when it is 8 steps off, and then drives straight on. The
tie at 128 and the two sides of it are tests (`crates/sf_sim/tests/turning.rs`,
`tools/sf_headless/tests/turning.rs`).

### Arc, in numbers

A circle of radius `r` cells turns `256 / (2 pi)`, about 40.7, facing steps for every cell of path
per cell of radius. So in a tick in which a unit covers `d` cells its heading may change by
`d * 40.744 / r` steps (`40744` thousandths per radian, a table constant; the arithmetic is on the
raw fixed point numbers). A light transport at 2.5 cells a second (0.125 a tick) with a 1.5-cell
radius turns 3.4 steps a tick, a repair truck at 1.8 cells a second 2.4: a half turn is a half
circle of 4.7 cells and takes 38 ticks. **The fraction of a step is carried from tick to tick**
(`turn_carry`, thousandths of a step, in the entity columns and the checksum) so that the radius is
the radius and not what whole steps round it to; the carry is cleared when the heading is the
wanted one. `d` is the distance the unit really covers this tick, terrain included: slower on ore,
the circle is tighter for the same turn.

The turning rate is never above the unit's turn rate, which is a cap on the radius, too: a unit
that cannot turn as fast as its circle asks for turns on a larger circle.

**Inside the turning circles.** An arc cannot reach a point that lies inside one of the two
circles of the smallest turn that touch the unit on either side: it would circle it for ever. With `a`
along the heading and `b` across it, the point is inside when `a^2 + b^2 < 2 r |b|` (squared length
of the way to it against twice the radius times the sideways distance); `r` is the radius the unit
really turns at. Then, and only when the heading is more than `drive_tolerance` off, the unit
**turns on the spot** at `pivot_permille` of its turn rate until it is within the tolerance, and
drives. A unit whose step is **refused by terrain** while its heading is not yet on the wanted one
(it stands against rock with its nose off the way) does the same. `pivot_permille` is 1 to 1000 for an
arc: a class that could never turn at a standstill could be stuck for good.

**Waypoints are points to pass.** A route is a string of waypoints, and a unit that has to move to
turn cannot be told to land within one step of one that lies off its line. A unit of an `Arc` class
counts a waypoint as reached when it is within **a quarter of a cell** of it (`ARC_CAPTURE`: the margin
the routes are smoothed with), unless it is the last waypoint of the ring, which is steered at as
before. A turn on the spot (above) is what finishes the job when it still cannot.

### Why 16 steps for everyone

The routes are pulled taut with a **thick** line of sight: a segment is clear when no blocked cell
lies within a quarter of a cell of it (`pathfinding.md`, "Line of sight and string pulling"). That
margin is what a unit that is up to 16 steps off its heading strays by near a corner. A larger
tolerance (an earlier try gave the hover class 32) makes a unit drive into the rock it was routed
past; the data allows up to 64 but the shipped classes keep the number the margin stands on.

## Avoidance and the turn constraint

The crowd pass (`collision.md`) does two kinds of things to a unit, and they are treated differently
on purpose.

* **What the unit drives.** Following the unit in front, waiting, detouring, the **sidestep to a free
  neighbouring spot** of a stuck unit, and the steps of a new route are all `walk`: they go through the
  profile. A tank that has to sidestep a quarter turn pivots first; a truck arcs.
* **What happens to the unit.** The **pushes**: the overlap of bodies resolved apart in the passes, the
  shove of an idle friend, the slip to the right of two units that meet head-on (`keep_right`).
  They are corrections to the position, like a bump, **bypass the turn constraint and leave the
  heading alone**: a pushed unit slides, it does not turn. They are small by construction (a mover
  is moved at most a quarter of a cell a pass, an idle unit an eighth of a cell a tick and three
  cells in all) and never into rock.

The choice keeps the crowd pass order independent (it reads one snapshot and writes positions, it
has no headings to agree on) and keeps a unit from being unable to give way because it is facing
the wrong way. The price is a visible slide when a crowd shoves a tank sideways; the pushed units
are not animated (`collision.md`, "Not here").

Path following needs nothing else: a unit asks for waypoints and steers at them as before
(`orders.md`, "Destination and route"); `Pathing::steer` does not know how the hull turns.

## Orders: the facing of an attacker

An `Attack` order stops a unit within weapon reach and used to turn it toward the victim. Now the
**unit without a turret** turns its hull the way its class turns on the spot (`Instant`: at once;
`Pivot`: at the turn rate; `Arc`: at the pivot share), and a **unit with a turret leaves its hull as
it is** and lets the turret do the facing. Firing is phase 5.

## The turret

A kind with `turret_rate: n` in `unit_defs.ron` (facing steps a tick; vehicles and vessels, a
movement class is required, zero or absent means no turret) has a turret facing of its own, next to
its hull. The light tank turns its turret 12 steps a tick against 10 for the hull, the heavy tank 8
against 6, the siege artillery 6 against 5.

**State.** `turret` is the **absolute** facing in the world, not an angle relative to the hull, so a
turret that is tracking something keeps tracking while the hull turns under it. A new unit's turret
is on its hull. It is in the `EntityView` (`turret: Option<TurretView>`: the facing at the end of
the previous and of the current tick, for interpolation), in `World::turret_of` and, in the
checksum, with the target (below).

**Where it points.** Every tick, after the units have moved and the crowd has been resolved
(`turret::update_all`), a turret turns toward where it wants to point by at most its rate, the
short way round, with the fixed tie at 128:

* at the unit in its **turret target** column, while that unit is alive (set by `Command::AimTurret`;
  phase 5 target selection writes the same column from `turret::would_acquire_turret_target_here`,
  a hook that does nothing yet); a target that is gone, or the unit itself, clears the column;
* else at the victim of an **`Attack` order**, from the tick of the order, also while the hull is
  still on its way (the turret is on the target before the hull is);
* else **along the hull**: the turret returns to hull-forward, the short way, and follows the hull when
  it turns, with the lag of its own rate.

`Command::AimTurret { ids, target }` aims the turrets of the units at an entity and leaves the hull
and the order alone: a tank can drive one way and aim another. With `target: None` the turrets go
back to the hull. It skips what has no turret and counts as ignored when no unit took it. The
aim stays until it is replaced, until the target is gone, or until a `Stop` (which lets go).

**The barrel's pitch** is presentation only for now (the glTF `barrel` part's `Pitch` joint). The
renderer's hook is `Motion::with_barrel_pitch`; nothing in the simulation decides it, and phase 5's
ballistics will.

## The render bridge

The model's `turret` part has a `Yaw` joint (`docs/conventions.md`, section 12: positive turns the
front toward `+z`, the sense of the facings). `sf_game` gives the instancer `Motion::with_turret(prev,
current)` for every `EntityView` that has a turret; the instancer interpolates the **turret's**
facing and the **hull's** between the two ticks, the shortest way, and the joint's angle is their
difference. A unit whose model has a turret part but whose definition has no `turret_rate` is drawn
as before (the instancer's demo aim, if one is set; the sandbox sets it). The screenshot test
`crates/sf_game/tests/faction_models_screenshot.rs` (`a_tank_with_its_turret_turned_off_the_hull`) puts
two light tanks side by side, turns the turret of one a quarter turn off its hull with
`AimTurret`, and checks both what the simulation says (64 steps off, the hull untouched) and what
the picture does (3,000 pixels change where the first tank is and none where the second is);
`docs/screenshots/wp4.5/` has the two pictures. `crates/sf_render` has a CPU test of the angle,
interpolation and the wrap.

## Data

`unit_classes.ron`, a movement class:

```ron
(kind: 1, name: "wheeled", ground: true,
 turning: (mode: Arc, drive_tolerance: 16, min_radius: 1.5, pivot_permille: 250),
 terrain_costs: [ ... ]),
```

Every field of `turning` has a default (`Pivot`, `drive_tolerance: 16`, no radius,
`pivot_permille: 1000`), so the block, or any field of it, may be left out. Checked when the file
loads, with the file and the line: `drive_tolerance` at most 64; an `Arc` needs a `min_radius` of 0.25
to 32 cells and a `pivot_permille` of 1 to 1000; `min_radius` and `pivot_permille` are for `Arc` only.
`unit_defs.ron`, a role: `turret_rate: 12` (a movement class is required for it).

## The checksum

* **Nothing changes for a world that does not use any of it.** The movement table's digest adds the
  turning profile of a class **only when it is not the old walk**, so the pinned `units_spawn`
  (frozen definitions without the block) keeps its value, and so do `wander_5000`, `command_mix` and
  `terrain_256`. The unit table's digest does not hold the turret rate (as it does not hold the
  weapon range): `UnitTable::turret_digest` is hashed in the new section only.
* **The turning section** (`R`, last): written only while a live unit has a turret or an unfinished arc
  turn: the turret digest, the movement digest (the profiles are in it), then for each such unit in slot
  order the carry, and for a turret its facing and its target. `prev_turret` is derived and not
  hashed. `Command::AimTurret` has its own tag in the command hash.

## What changed, and which scenarios moved

The rule applies to every unit made from a unit table whose classes carry a profile: the frozen
definitions of `orders`, `terrain_costs`, `pathing` (also `pathing_256`, `bridge_200`, `crowd_jam`) were
given the shipped profiles, once, and those scenarios were re-pinned once. `units_spawn` has frozen
definitions **without** the block, its legacy walkers turn as before, and its checksum did not
move; the wanderers (`wander_5000`, `command_mix`) have no class and did not move either.

| Scenario | Before | After |
|---|---|---|
| `wander_5000`, `command_mix`, `terrain_256`, `units_spawn` | | unchanged |
| `orders` | `0x38b6882e777306bb` | `0x0dcb52425f1adc60` |
| `terrain_costs` | `0x2f74d916504df7cf` | `0xdc9d5c4a65a60508` |
| `pathing` | `0x09f819e7bfeb22a2` | `0x606eb6feeffa964c` |
| `pathing_256` | `0x78acbca87df9a5f0` | `0x7c4e31d024ed4cf9` |
| `bridge_200` | `0xa55348bf0b519c50` | `0x03b3145755a9081e` |
| `crowd_jam` | `0xd5e9d6e45a14b0ef` | `0x3d8fab0ab9a564e6` |
| `turning` (new) | | `0xb07ba8066dd59884` |

(WP4.4b then moved the wheeled radius to 1.5 cells and re-pinned the scenarios once more; the table of
that is in `collision.md`, "Data, scenarios and the checksum".)

Test changes that follow from the rule, in `tools/sf_headless/tests`: `terrain_costs.rs` (the
pinned arrival ticks, the bound on the ticks a unit spends turning by mode, the least time of a
unit that swings out of its lane, the ordering test without the hover craft, whose arrival ticks
include a swing of a length that depends on the heading it spawned with), `pathing.rs` (a unit that
has to move to turn may walk 1.25 times the best any-angle route, the others 1.10 as before).
`bridge_200.rs` needed no change (below).

### `bridge_200`, measured

The "done when" of phase 4 (PLAN.md) holds with the turning rules. At WP4.5 it held only at the
shipped radius, by the luck of the crowd, and the review of that package showed it: the table of the
radii it swept failed at 1.5 (4 riflemen never arrive), 1.6 (9 never arrive and 6 end outside 9.58
cells), 1.75 (worst overlap 0.286) and 2.0 (a hovercraft gets stuck), and passed at 1.25 and from 1.7
to 1.8. **The cause was not the turning rules** (bodies are discs, so a turn never changes an overlap)
**but the crowd and arrival logic of WP4.4**, which met the criterion by chaotic margins: the last
units took the slots on the far side of a crowd that had arrived and pushed through it, the crowd
spread out and was left that way, and two idle units that were pressed together stayed overlapped
because nobody was moving them. WP4.4b made the criterion hold by construction (the slot a unit takes
when it comes near, the bounds on the overlap, a settled crowd that closes up: `collision.md`,
"Arrival" and "Bounds that hold whatever presses on the crowd"), and **the radius table now passes
everywhere**:

| wheeled radius | result |
|---|---|
| 1.25 | passes; the last unit arrives on tick 3,703, the farthest ends 8.96 cells out (limit 9.58) |
| 1.5 (ships) | passes; the last unit arrives on tick 3,456, the farthest ends 8.29 cells out |
| 1.6 | passes; tick 3,302, the farthest 8.64 |
| 1.75 | passes; tick 3,491, the farthest 8.86 |
| 2.0 | passes; tick 3,239, the farthest 8.42 |

(Worst overlap in the run 0.19 at every radius; the first tick, on which the block of
units is placed, is clean since the second review round, which also added the variants: 90 tanks in
place of the riflemen at 1.25, 1.5 and 2.0, and the goal one cell west at 1.5.)

There is one test for each of those radii (`phase_four_holds_at_a_wheeled_radius_of_*` in
`tools/sf_headless/tests/bridge_200.rs`, which replaces the radius in the movement table with a
test-only knob and asks for every way the run misses the criterion at once); the full numbers of the
run and the wider sweeps (112 variants of the block: `sweep_of_variants`) are in `collision.md`,
"Measured".

**The shipped radius is now 1.5 cells (6 m)**, a realistic truck, and it is data, for the balance
rounds: since the criterion no longer depends on it, changing it is no longer a risk to phase 4. The
scenarios that run on the frozen copies of the definitions (`orders`, `terrain_costs`, `pathing`,
`pathing_256`, `turning`) were re-pinned for it, and `terrain_costs.rs` pins the new arrival ticks of
the wheeled units (152, 185, 183, 164 and 176 for grass, ore, bridge, water and rock; the rock lane
was 109 at 1.25 cells, the swing round the rock being longer).

## The `turning` scenario

`assets/scenarios/turning.ron` on a 64 by 40 cell open map (no terrain layer, so every route is the
straight line and the headings are plain geometry), frozen definitions in `turning_defs/`. Seven
units; `tools/sf_headless/tests/turning.rs` asserts their headings and positions tick by tick, and
CI runs it on Linux and Windows against `turning.expected`.

* **A tank ordered behind itself** (a light tank, facing east, ordered west): turns the decreasing way,
  10 steps a tick, stands still for 11 ticks, drives on the 12th, then straight at its speed.
* **A wheeled vehicle** (light transport, facing east, ordered to a spot behind and to the side): drives
  off on the very next tick and every tick after, turns 3 or 4 steps a tick (the circle allows 3.4,
  its turn rate is 10), swings on east by more than half its radius before it comes round, and arrives.
* **A rifleman**: faces its goal on the first tick it is ordered, and ordered behind itself it faces
  west on the first tick and has stepped off.
* **A turret on a moving target** (heavy tank driving east along a row, its turret told to follow a
  light transport that drives west along another): the hull stays at facing 0 and the turret sweeps
  more than a quarter turn off it, never more than two ticks of its rate behind the target; let go at tick
  600 it returns to the hull the shortest way and stays on it.
* **The turret of an attacker** (a light tank with an attack order on a rifleman): on the victim a tick
  before the hull is, at weapon reach at the end.

## Not here

* **Formations and the pace of a group** (WP4.6): a group still goes at the pace of its own units.
* **Replanning** (WP4.7 is done, `pathfinding.md`): a unit that circles because of a corner it cannot take
  is not replanned; a unit that is stuck is (`collision.md`).
* **Fog and visibility**: turret targeting has no fog or visibility check yet; a turret follows any
  target it is told to, seen or not.
* **Reversing.** A wheeled vehicle never drives backward; the turn on the spot is a stand-in.
* **Slope**, **crushing**, **turret limits** (a turret turns all the way round) and **the barrel's
  elevation** are not decided; phase 5 aims with the turret, not with the hull.
* **Pushed units are not animated**: a shoved vehicle slides without turning.
* **Hover drift**: the hover class is an arc with its own radius and its own pivot share, not a
  drifting craft that keeps its old velocity while it turns.
* `sf_game`'s hover highlight under the pointer in `--shots` (`docs/progress.md`, item 21, listed
  with "WP4.5" as owner) was not touched: it is not part of this work package.
