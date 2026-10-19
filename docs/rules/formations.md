# Formations

Work package WP4.6 (PLAN.md phase 4: "group keeps shape, slowest sets the pace"). Code:
`crates/sf_sim/src/formation.rs` (the formations, the lattice, the tick), with small parts in
`orders.rs` (a unit in a formation walks to its slot instead of the order's route),
`world.rs` (`apply` makes the formations, `step` updates them), `path.rs` and `path/slots.rs`
(the arrival lattice turned across the way the group came), `movement.rs` (which classes cross
the same ground) and `checksum.rs` (section `G`). Data: `assets/config/sim.ron`. Scenario:
`assets/scenarios/formation_move.ron` (map `assets/maps/formation`). Idea: the ordinary one of an
RTS group move: the player selects a handful of units of mixed speed, gives one move order, and
they arrive together and in the shape they set off in, not strung out by speed.

## What it does in one paragraph

One `Move` order that sends enough units that cross the same ground to one point makes them a
**formation**. The group's current layout becomes a **lattice** with room for the widest body;
each unit gets an **offset** in it and so a **slot** that hangs from the **leader** (the unit
nearest the middle) and turns with the way the leader goes. The leader follows the order's route
or flowfield at the pace of the slowest member; the others walk straight to their slots. Where
the ground is narrow the shape lets go and the units file through; on open ground the leader waits
for stragglers and the shape forms up again. Near the goal the formation is let go and the
members take the arrival slots of the goal, as the units of every group order do.

## Who forms a formation

| | |
|---|---|
| Order | `Order::Move` to a point, **not** attack-move, guard, hold, stop, scatter or an attack (they keep their behaviour). A single unit is no group. The units must start the order now (a queued order behind another does not start one). |
| Ground | Units of the **same family**: classes that may enter the same terrain types. Foot, wheeled and tracked units are one family (so a mixed ground group is one formation); a hovercraft, a vessel are each a family of their own; aircraft (no terrain stops them) form none. A vessel and a land unit sent together do **not** form one formation: the family that can reach the goal forms its own (the goal must be a cell the class may enter), the other follows the order as it always did. |
| Number | At least `formation_min_units` (3 as shipped) and at most `formation_max_units` (48). |
| World | `formation: true` and the world has pathfinding (terrain and a movement table). |

**Decision: formation-move is the default for a group move order** (`sim.ron`: `formation: true`),
**for groups of up to 48**. The 200-unit crowd of `bridge_200` is *not* a formation: it is a
crowd that moves, as every group order did before, with flowfields and arrival slots. Reasons, measured
(not assumed): with the limit lifted, one formation of 195 (a lattice 31 cells across on a map where the
whole block is 20 wide and the first rock outcrop is 14 cells away) had its leader at x = 26.6 after
4,600 ticks, from 17.7, against a crowd that has crossed the river by tick 2,225: the shape cannot be
held when most of its slots are inside rock, behind the bridge or in each other's way, and nobody can
wait for 195 stragglers. What a player means by a group of that size is "all of you, go there", which is
the group order. The limit is data (`formation_max_units`, at most 256) and `bridge_200` is its test:
the criterion of phase 4 holds with it and the sweep of 112 variants is clean (below). The five hover
craft of that scenario (a family of five, below the flowfield threshold) *are* a formation, which
crosses the river and the wall with the crowd.

## The shape

When the order is given, from the positions the units stand in:

1. **The leader** is the unit nearest the middle of the group (ties to the lower slot). It is the
   origin of the lattice: its offset is zero. The leader changes when it dies, gets another order or
   gives its order up (the member nearest the middle of those left; the offsets stay, so the shape
   does not move).
2. **The frame** points the way to the goal from the leader: `x` forward, `y` to the right. Every
   unit's position is turned into it.
3. **The lattice** has a cell of `formation_spacing_permille` (1250 as shipped: a quarter more) of the
   widest body's spacing (`slots::spacing_for`: the body, three quarters of it for infantry-sized
   bodies, and a sixteenth of a cell). One cell for all, so that any unit may stand on any point and
   two bodies never overlap on their points.
4. **Pulled in**: if the farthest unit is farther from the leader than `formation_spread_permille`
   (1500) of the radius of a disc that holds all the bodies (`pi r^2` = the sum of the spacings
   squared), every offset is scaled so that it is; a group scattered over 30 cells does not keep a
   30-cell shape.
5. **Snapped**: in the order of distance from the leader (ties by slot), each unit takes the lattice
   point nearest its own spot, or the nearest free one (rings outwards, ties to the lower point).
   A line stays a line, a block a block, and the order of the units along each row is the order they
   stood in.

The offsets are state (hashed). At every tick the **slot** of a member is the leader's position plus
its offset (minus the leader's) turned by the **heading** of the formation.

### The heading

It starts as the way from the leader to the goal. It follows where the leader actually goes (its last
step, when that is at least a quarter of the pace), **one facing step at a time** and no faster than
the outermost slot can be followed at the pace of the formation: one step is `2 pi / 256` of a turn,
so a step is made every `ceil(extent * 0.025 / pace)` ticks (1 to 40) and the outermost slot moves
at most as fast as the formation does. The offsets turn by it with the fixed-point rotation of
`sf_math` (`Vec2::rotate`); there are no floats.

## The pace

`pace = formation_pace_permille` (900) of the speed, **now**, of the slowest member: each member's
speed per tick scaled by the terrain it stands on (`movement::scale_step`). The **leader never steps
more than the pace** (its step is held to it before the terrain scales it). The others walk to their
slots at up to `formation_catchup_permille` (1000) of their **own** speed, so:

* a unit behind its slot catches up, at its own speed at most, never faster;
* a unit on its slot follows it: the slot comes at the pace of the formation, which is below every
  member's speed, so each reaches it and waits on it;
* the slowest has a tenth of its speed to catch up with when terrain or a crowd has held it up.

A formation sets the pace of the *order*: it takes the riflemen about as long as they need alone
(`formation_move`: 0.0314 cells a tick for the leader, 0.9 of the rifleman's 0.035) and not a tick
less, however fast the tanks are.

## What a member does in a tick (`MemberMode`)

| Mode | When | What |
|---|---|---|
| `Lead` | the leader | follows the order's flowfield or route (`path/`), at the pace of the formation, or at its own speed while the formation is loose |
| `Direct` | the member sees its slot (the thick line of sight of `path/los.rs`, one sector at most, no rock, water or structure within a quarter of a cell) and the formation is not loose | walks straight to the slot (`behavior::walk`, so the turning of its class applies), snaps onto it when within a step; on its slot it turns to the heading the way its class turns on the spot. It asks for no route. |
| `Flow` | the member cannot see its slot, or the formation is loose | follows the order's flowfield or route as every unit of a group does, at its own speed. When it was walking to a slot before, it chooses its way again from where it stands (a flowfield member from its cell; a route of its own is kept if its first waypoint is still ahead, within 12 cells, otherwise asked for again). |
| `Wait` | the leader waiting for stragglers; or a `Flow` member that is ahead of its slot (nearer the goal than the slot, by more than the re-form distance) on open ground | stands |

### Narrow places and re-forming

The ground round the leader is **open** when no rock, water or structure (by the class's own costs)
lies within three cells in any of eight directions. When it is not, the formation is **loose** for at
least 20 ticks more: everybody is `Flow`, the leader goes at its own speed, nobody waits for anybody.
So a bridge, a gap in a wall, a funnel between outcrops lets the units file through without a formation
standing in the doorway, and without a leader at a snail's pace holding up the units behind it.

On open ground the formation is whole again:

* a member farther than `formation_reform_tenths` (3.0 cells) from its slot is a **straggler**, and
  the leader **waits** (`Wait`) for it, if the leader itself is on open ground. The leader's
  `formation_patience_ticks` (300) run down one a tick while some straggler is **stalled**: has not
  come closer to its slot in a window of 20 ticks by at least a 32nd of a cell (compared with the
  nearest it has been since it fell off its slot, so a unit jostled by a crowd does not look as if it
  were on its way). While every straggler is coming closer they run down at a third of that rate: a
  straggler that is on its way is waited for three times as long, not for ever (without the third
  the five hover craft of `bridge_200` waited 1,336 ticks for each other in one variant of the sweep
  and arrived 112 ticks after the limit).
* When the patience is gone the formation is **broken**: the leader goes on whoever is behind, until
  at most a tenth of the members are stragglers again (`reforms` in the stats).
* A member that is a straggler and stalled for `formation_patience_ticks` **leaves** the formation and goes on
  as a unit of its own (`expelled`). Nothing waits for ever: a stuck member can hold the leader for 300
  ticks, then neither.
* When the formation forms up again after being loose, and every 100 ticks while some member is stalled,
  the slots are **handed out again** (`reassign`): the pairs of unit and slot nearest first, each used once,
  the leader keeping its place. Units that came through a gap in another order than they went in would
  otherwise have to pass each other to reach their slots.
* A member **stalled** behind another that stands on or in the way to its slot swaps places with that
  one (`swap_blocked`, once per window per member): the pair walks on without passing.

### The end

The formation is let go (`released`) when the leader is within `max(extent, d) + 2` cells of the
goal, `extent` being the farthest a member's offset lies from the leader's and `d` the radius of the
disc the bodies of the **whole order** fill (`CrowdState::disc`: the units of other families sent with
it stand there). Or when the leader has come to the end of the way to a goal it cannot reach.
After that every member is an ordinary unit of a group order: it follows its field or route
and **takes an arrival slot** when it comes near (`path/claim.rs`, WP4.4b): the first slot it can
walk to that no slot already taken lies too near, so a slot that is blocked (rock, a structure, a
crowd) is not taken. The lattice of those slots is the class rings of `path/slots.rs`, **turned about
the goal by the heading of the formation** (`Layout::turn`; nothing for a group that did not form up,
so the slots of every other order are exactly as they were): the rows of the arrived group stand across
the way it came. Arrival quality is the one of WP4.4b because the logic is: the claims, the bounds on
the crowd that counts as arrived, the closing up of a settled crowd.

A formation also ends when fewer than two members are left (`dissolved`): a new order, a stop, a
death takes a unit out at once (`orders::begin` and `clear` reset its column).

## Interplay with the rest

* **Collision** (WP4.4): bodies are as they were. The lattice leaves room (a quarter more than the
  widest body) so that members on their slots do not touch; the crowd pass pushes apart what does
  touch. Direct members do not set `walking`, so the stuck-unit machinery does not apply to them: the
  stall rule above does (they are expelled after the patience, and then it does).
* **Replanning** (WP4.7): the leader is an ordinary unit of its order: if it is stuck it sidesteps, asks
  for a new route round the bodies that stand still, and gives its order up after `replan_max` of them;
  it then leaves the formation (the order ended) and the next unit leads. A member that is expelled
  or released asks for routes as any unit.
* **Flowfields** (WP4.3): the fields are made as before. A formation that marches without its members
  asking the field for a direction would let it expire after `FIELD_IDLE_TICKS`; the formation touches the
  fields of its members every tick (`Pathing::keep_field`), so they are there when the members go back
  to them.
* **Turning** (WP4.5): a member walks to its slot with the turning of its class. A unit that snaps
  onto a slot faces the heading afterwards by the class's turn on the spot.

## Data

`assets/config/sim.ron`, read by `sf_data::sim_settings`, handed to the world by
`sf_bridge::formation_config` and `World::with_formation`; the simulation's own defaults
(`FormationConfig::DEFAULT`) are the same numbers, which is what tests and frozen scenarios use. A
scenario file can say `formation: Some(false)`.

| Setting | Shipped | Range | |
|---|---|---|---|
| `formation` | true | | false moves a group as a group order did before WP4.6 |
| `formation_min_units` | 3 | 2 to `max` | units of one family one order must send |
| `formation_max_units` | 48 | `min` to 256 | a bigger group is a crowd |
| `formation_spacing_permille` | 1250 | 1000 to 3000 | lattice cell, of the widest body's spacing |
| `formation_spread_permille` | 1500 | 1000 to 4000 | widest the group may be, of the radius of its disc |
| `formation_pace_permille` | 900 | 500 to 1000 | of the slowest member's speed now |
| `formation_catchup_permille` | 1000 | pace to 1000 | of its own speed, for a member getting back to its slot |
| `formation_reform_tenths` | 30 | 5 to 100 | how far from its slot a member may be (tenths of a cell) |
| `formation_patience_ticks` | 300 | 20 to 2000 | the leader's patience; a stalled straggler's too |

Constants that are not data (`formation.rs`): the open-ground probes (three cells, eight ways),
how long a formation stays loose (20 ticks), the window of the progress check (20 ticks, a 32nd of a
cell), how close counts as on the slot (half a cell), how often the slots are handed out again
(100 ticks), the release margin (2 cells), the size of the pool (64 formations, 256 members).

## State and checksum

Section `G` of the checksum, written only while a formation exists (every world whose move orders
never formed one hashes as before): the serial counter, and for each formation its goal, leader,
heading, the ticks to the next heading step, whole or broken, the leader's patience, how loose it
is, its age, and for each member its slot index, offset, stall ticks, nearest approach and what it did
last tick. What a tick works out from those (slots, the pace, what each member does) is derived and
not hashed. `Layout::turn` of a field is hashed after the rest of the layout and only when it is not
zero. A group order that forms no formation has exactly the checksum it had.

## Measured

`formation_move` (map `assets/maps/formation`, 112 by 48 cells; 20 units: 10 riflemen, 5 light tanks,
5 light transports in a block of 4 by 5, one order to a point 91 cells east and behind a 3-cell wide,
8-cell long passage in a wall):

| | |
|---|---|
| ticks on open ground (formation whole, leader outside 8 cells of the wall) | 1,993 |
| of them with every member within 3.0 cells of its slot | 1,993 (100 %) |
| farthest any member was from its slot there | 2.55 cells (the lattice is 4.46 cells from the leader) |
| leader's speed in the west field | 0.0314 cells a tick (the rifleman's top speed 0.035) |
| longest the group was along its heading | 7.2 cells |
| loose at the passage | 153 ticks, up to 19 members following the order's way at once |
| whole again behind the passage | tick 2,117 (every member on its slot to within a cell) |
| leader's waits (ticks) / breaks / expelled | 526 / 0 / 0 |
| formation let go | tick 2,967 |
| everybody arrived | tick 3,227; worst overlap during the run 0.088 cells, at the end 0.026 |

`bridge_200` still meets the done-when of phase 4 (the numbers, with the sweep of 112 variants, are
in `docs/progress.md`).

## Not here

* **Attack-move in formation, guard, queued moves**: they keep their behaviour. A formation made from
  an attack-move would have to fight as a group (phase 5).
* **Formations the player chooses** (line, wedge, column): the shape is the group's own, kept. A named
  shape would be a different offset generator; the rest would not change.
* **A formation of more than `formation_max_units`**: a crowd.
* **Aircraft and mixed ground**: aircraft do not form; hovercraft and vessels form their own families but
  their formations have been tested only with five hover craft (`bridge_200`) and three boats
  (`tests/formation.rs`).
* **Rejoining**: a member that left (expelled) does not rejoin; a member whose order was replaced takes
  the new order.
* **Reasons for the speed of the formation in the UI**, a drawn outline of the slots: presentation, not
  here.
