# Target selection and return fire (WP5.4)

Until WP5.4 a unit fired only at the victim of an `Attack` order ([`weapons.md`](weapons.md)). Since WP5.4 a
world that is given the **targeting settings** lets its armed units choose a target on their own and turn on
whoever shoots at them. This file is the rules; the idea (a unit chooses a target, the game checks that it
can attack it, an attack follows) is section 8 of `docs/red-alert-analysis.md`, which does not say how the
choice is made, so every rule and number here is ours (clean-room, B5).

**It is opt-in per world.** `World::with_targeting(config)` turns it on, `sf_bridge::targeting_config` makes
the config from `sim.ron`, and a world that is not given one behaves exactly as it did: an idle unit next to an
enemy fires nothing, nothing about targeting is hashed, and every checksum that was pinned before is what it
was (below). The headless runner gives it to a scenario that says `targeting: true` (which needs `combat: true`).
Since WP5.5 `sf_game` gives its world the weapon table, these settings and the destruction settings
([`destruction.md`](destruction.md)), so the units in the viewer choose targets, fire back and fall.

## The settings (data)

`assets/config/sim.ron`, read by `sf_data::sim_settings`:

| Field | Meaning | Bounds | Shipped |
|---|---|---|---|
| `scan_cooldown` | ticks between two looks round for a target of one unit (20 ticks are a second) | 1 to 1000 | 10 (half a second) |
| `pursuit_cells` | how far beyond its weapon's reach a unit that is shot at follows its attacker, and the farthest it ever strays: a guard from its post, an idle unit from where its first chase began; 0 means nobody follows | 0 to 32 | 6 (24 m) |
| `pursuit_reset_ticks` | how long an idle unit must be back at rest (no target, not hurt) before a new chase measures from where it then stands | 1 to 2000 | 100 (5 s) |

`TargetingConfig::DEFAULT` is the same two numbers; a test keeps them equal. Worlds, tests and scenarios that are
not given settings use the defaults. What a unit *prefers* is not a setting of its own: it is the armor table
(below), which is data too.

## A chosen target is an order

A unit that picks a target does not get a firing mechanism of its own. It puts itself on an
**`Order::Pursue(target)`**, which is an `Attack` that the unit gave itself and a player cannot give (the
command is ignored), and it remembers the order it had (`Engage::resume`: idle, hold, guard or attack-move).
Everything an attack already does then works as it is written: the unit turns toward the target (the hull of a
unit without a turret, the turret of a tank), fires when its weapon is ready, the target is in reach and the
weapon is aimed, and walks into reach if it may. When the engagement ends the old order is **begun again**:
a guard walks back to its post, an attack-move marches on, a hold holds, and an idle unit stays where it ended
(and starts the first order queued behind it, if a player queued one meanwhile).

**A player's order always wins.** Every command that gives a unit an order, or stops it, replaces the engagement
(`orders::begin` and `orders::clear` forget it): an explicit `Attack` overrides what the unit would have
chosen, and a unit that has an attack order is not distracted by an enemy next to it. A unit that is *moving*
(`Move`, and the legacy walks) never chooses a target and **does not stop to return fire**: a move is a
decision to go somewhere; attack-move is the order for fighting on the way.

### What each stance does

| The unit has | It picks a target in reach | It follows an attacker that is beyond its reach |
|---|---|---|
| no order (idle) | yes | yes, within the pursuit distance |
| `HoldPosition` | yes, from where it stands | never (a hold never moves, and is never pushed while it fires) |
| `Guard` (a point or an entity) | yes | yes, within the pursuit distance |
| `AttackMove` | yes: it stops to fight and marches on after | no: it fires back only from where it is |
| `Move`, `Attack`, or already engaged | no | no |

Only mobile units choose. Structures take part when they can hold a target without an order, in the defence
work package (WP6.7): a structure takes no order, and an `Order::Pursue` on one would make it walk.

## A tick of choosing

`World::step` runs, after the crowd pass and before the turrets: **`Targeting::update`**; then the turrets;
then the weapons (the two passes of [`weapons.md`](weapons.md#a-tick-of-shooting-in-two-passes)); then
**`Targeting::return_fire`**.

* `update` visits the units in slot order. A unit that has chosen checks its target (below); a unit that may
  choose and whose cooldown has run out looks round. Units that are at zero hit points are dealt with first
  (below).
* `return_fire` turns the units that were hurt in this tick on their attackers.

Both read the positions and the hit points of the end of the movement stage and of the weapon stage and write
only the engagement of the unit they are looking at, so the order in which the units are visited cannot
matter. The chosen target is therefore known to the turret and the weapons of the **same tick**: a unit that
picks at the look of tick N can fire at tick N.

## What a unit picks

**Candidates** are the units and structures of **another player** (there are no alliances yet: every other
owner is an enemy, and every owner a friend of itself) that have hit points left (an object that cannot be
damaged has none, and a unit shot down has none), that some weapon of the unit **hurts** (the armor table gives
the weapon's damage type more than zero percent against the candidate's armor type: a sniper does not pick a
tank) and that the weapon could **fire at now**: not farther than its range and not nearer than its minimum
range, both measured to the edge of the candidate, the test a shot is checked with (`weapons::in_reach`). A
unit does not back off from a target that is inside its minimum range; it does not pick it.

**The score of a candidate** is the score of the unit's best weapon against it, `weapons::weapon_score`:

```
damage * burst * percent * 1000 / reload ticks
```

where `percent` is the armor table's number for the weapon's damage type against the candidate's armor type: the
damage of one second of reloads, in percent thousandths, as one integer. Nothing is rounded to a hit point; it
only has to order candidates. So the preference of a weapon is the row of its damage type in the armor table:

| Weapon (damage type) | Prefers, best first, with the shipped table |
|---|---|
| rifle, pistol, carbine, the tower and automated guns (`bullet`) | infantry (100), light vehicles (40), structures (20), heavy vehicles (10) |
| long rifle (`precision`) | infantry only (zero against everything else, so nothing else is picked) |
| rocket (`rocket`) | light vehicles (130), heavy vehicles (100), structures (70), infantry (40) |
| tank guns, anti-tank gun, naval cannon, emplacement gun (`cannon`) | light vehicles (100), heavy vehicles (85), structures (60), infantry (40) |
| howitzer, battery (`shell`) | structures (150), infantry (100), light (75), heavy (50) |
| satchel (`demolition`) | everything alike (nearest) |

With several weapons the best one chooses and the others fire if they can; a weapon that does nothing to the
chosen target (zero percent) **does not fire** at a target the unit chose (it would be a wasted reload; an
explicit attack order still fires all of them, as before).

**The key** by which candidates are ranked, smallest first, is `(mode, score descending, distance squared,
y, x, slot, generation)`. It is a total order, so the choice is the same whatever order the candidates are
found in. Equal scores: the **nearer** wins. Equal distances too: the one with the **smaller y, then the smaller
x** wins, and only a tie on the position itself (two bodies on one point) falls to the slot. So two candidates
that cannot be told apart by what they are or where they stand are not told apart by when they were spawned
either; `tests/targeting.rs` spawns the same cast in every order to show it. The slot only decides between two
bodies on one point, and those the crowd pass already splits by slot (`crowd.rs`, module notes: units on exactly the
same point are split by slot index), so the slot-ordered tie is no new dependence.

**Finding them.** A scan uses the crowd pass's spatial index (`crowd::Index`, buckets of four cells), built
once in a tick in which some unit looks round, from everything that can be shot at, and queries the rectangle
that holds the unit's longest range plus the widest body. A tick without a scanner builds nothing.

## The cooldown, and keeping a target

A unit looks round only when its cooldown has run out; a look sets it to `scan_cooldown` again, found or not.
It counts down every tick of the unit's life, so units spawned together look on the same ticks (0, 10, 20 ...).
Staggering them would need a number that depends on the slot (the spawn order) or on something else that is not
state; they look on the same tick instead, which costs one index build and a few buckets each on those ticks. The
cooldown is **not** reset by a new order (a unit is not more alert for having been given an order) and it does
not delay return fire.

A unit **keeps its target** while the target is valid; it never switches to a better one. The target is dropped,
and the old order begun again, when:

* it is gone (despawned, a stale id), or at zero hit points (it is not picked again);
* for a unit that may not follow (everything but a unit following an attacker): it is no longer in reach of a
  weapon that hurts it (it walked out of range, or inside the minimum range). **A target that leaves its range
  is dropped, not followed**: a unit that chose in reach has no reason to walk;
* for a unit that follows: the target is farther than its reach plus the pursuit distance, the unit has
  strayed farther than the pursuit distance from its anchor (below), or the target is in reach but inside the
  minimum range of every weapon that hurts it (walking closer will not help; the unit goes back to its order).

When an engagement ends and the cooldown has already run out, the unit looks round on that same tick.

## Return fire

A unit that is **hurt by an enemy** in a tick (a shot or a splash; damage of zero does not count, since an
armor that is immune shrugs it off; a splash of its own side does not count) turns on its attacker at the end
of that tick if its stance lets it (the table above), **at once and not on the cooldown**.

* An attacker in reach of a weapon of the unit is engaged where the unit stands.
* An attacker beyond its reach but not beyond reach plus `pursuit` is **followed** (idle and guard only): the
  unit walks toward the place the target stood at the end of the last tick, into the range of the weapon that
  chose it, and fires. It goes no farther than `pursuit` cells from its **anchor**, and lets go of an attacker
  that gets away (the leash above). The anchor of a guard is its **post** (the guard point, or where the guarded
  entity stands when the chase begins). A guard of an entity measures its leash from where the guarded unit
  stood when the chase began, not from where it walks to during the chase. The anchor of an idle unit is where
  its **first** chase began, and it is kept across chases that follow straight after each other: only after
  `pursuit_reset_ticks` back at rest (no target, and not hurt: every hit starts the count again) does a new
  chase measure from where the unit then stands. So an attacker that hits, steps back and hits again cannot lure
  a unit away chase by chase, however long it keeps hitting a unit that stands at the end of its leash. A unit already at the end of its leash does not set out again,
  and an attacker inside the minimum range of every weapon (in reach, but nothing can fire) is not followed. It does not go back: an idle unit stays where the
  chase ended; a guard walks back to its post because that is what a guard does.
* An attacker beyond that is not answered.
* When several enemies hurt the unit in one tick it answers **one**, by the key above, and **one it can answer
  in reach before one it would have to follow**. The attackers are the set of the tick's hits (not `Shot::hp_after`
  or the order of the shots, which follow the slots).
* A unit that has chosen already keeps its target; a unit on `Move` or with an attack order does not answer.

A hold fires back only from where it stands: it answers an attacker in reach and ignores one that is not.

## Range check, and the duplicated range

The existing rules are kept: a weapon hits what is within `range` and not within `min_range`, both to the edge
of the target, and only when it points at it within its firing arc (hull or turret). A unit with a minimum
range does not back off.

**A hull weapon on a unit with a turret is refused** (`WeaponsError::HullWeaponOnTurret`, when the world is given
the table): the turret aims, and the hull does not follow it for a weapon, so such a weapon would never be aimed.
No shipped unit has one. This settles the open question of WP5.1 (item 3) in the cheapest way; if a design ever
wants a hull gun on a tank, lift the refusal and make the hull turn for it in `orders::run`. It is the world's
check and not the loader's: the frozen copies of the definitions that predate `mount` (`turning_defs` and
others, never edited) have their tank cannons on the default, `Hull`, and must keep loading for the scenarios
that never fire.

**`UnitStats::weapon_range` stays.** WP5.1's follow-up asked for one of the two copies of the first weapon's
range to go. It cannot, without moving a legacy golden: `weapon_range` is hashed by the pinned scenarios
that have a unit table and no weapon table (`orders`, `units_spawn`, `turning`, `replan` and the rest, in the
reach digest of the orders section), it is what an `Attack` order walks to in a world without a weapon table
(the game today), and `sf_game`'s cursor reads it to tell an armed unit from an unarmed one. What WP5.4 does
instead is stop adding readers: **nothing in the targeting reads it**. A chosen target is walked to with the
range of the weapon that chose it, from the weapon table, and `World::with_weapons` still refuses a table
whose first weapon's range is not the kind's `weapon_range`, so the two cannot disagree. When the game has a
weapon table and the legacy scenarios have been re-pinned for good, `weapon_range` can go in one change.

## Simultaneity

The two-pass rule of [`weapons.md`](weapons.md#a-tick-of-shooting-in-two-passes) still binds, and WP5.4 adds to
it:

* **Target decisions are made from the state at the end of the movement stage** (before the weapons change a
  hit point), by a function of the unit's own engagement and of the positions, owners and hit points of the
  others, none of which the stage changes.
* **Return fire is decided from the end of the weapon stage**, after all damage of the tick has been summed per
  victim and taken off once; a unit that was killed in this tick does not turn on anyone, and an attacker that
  was killed in this tick (a mutual kill) is not answered.
* **What a unit walks to is a snapshot.** A unit that follows an attacker heads for the place the target stood
  at the end of the last tick (`Engage::aim`), not for its live position, which a unit with a lower slot may
  already have changed in this tick's walk. (The `Attack` order reads the live position; its slot-order
  dependence is older than this work package.)
* A unit's cooldown and engagement are its own state, hashed.

Tests spawn the same units in every order and compare **by position** (`crates/sf_sim/tests/targeting.rs`):
idle units that pick each other, two on two on attack-move who meet, stop, fight and go on (24 orders, 400
ticks, with the outcome per starting place), units that return fire and follow, a follower whose target walks away while it is followed (all six orders, positions compared
**on every tick**), and several attackers hitting one unit in the same tick (the two attackers swapped). One thing had to be arranged for them: a mobile unit is born with a **random facing
drawn from the order it was spawned in**, and a unit that has to turn before it drives would set off at a different
tick. The test units therefore turn to any heading in a tick; the one test that is about turning uses a slow unit
and checks the facing, not the timing.

## Zero hit points (in a world without destruction)

Since WP5.5 a world with destruction settings removes a unit the moment it reaches zero
([`destruction.md`](destruction.md)), and none of this applies there. In a world **without** them nothing is
removed, and **in a world with targeting** a unit at zero:

* is not a candidate and is not answered, and picks nothing, fires nothing (as before);
* **loses its orders and stays where it fell**: `HoldPosition`, which nobody pushes, from the end of the tick it
  fell, so a wreck does not march on to its destination (a unit that is shot to zero while it walks stops that
  tick);
* **takes no orders** any more (a command to it is ignored).

A kind that cannot be damaged at all (hit points zero in the table: an ore field) is not mobile and not a
candidate either. `Shot::hp_after` is **not** used for any of this, nor for credit: WP5.5's kill credit must be
settled from the set of the tick's hits, as `weapons.md` says; the attackers of the return fire are exactly that
set.

## The checksum

* `Order::Pursue(target)` has the tag 8 in the orders section (`D`), which a world without targeting never has.
* Each unit's engagement (`Engage`: the order to go back to, whether it follows, where it began, where it last
  saw the target, the range it walks to, the ticks to its next look) is hashed in a new section **`A`**, written
  after the projectiles (`J`), only in a world that has a weapon table **and** targeting settings: the settings
  first (so peers with different settings differ from the first tick), then each live unit whose engagement is
  not at rest.
* **No pinned checksum moved.** `wander_5000` `0x38d5e3c43358a889`, `command_mix` `0x43f6583004258ceb`,
  `terrain_256` `0xb577112845e0097e`, `units_spawn` `0x6c369f42be150d6e`, `bridge_200` `0x03971fcc3548fc0c`, `orders`,
  `terrain_costs`, `pathing`, `pathing_256`, `crowd_jam`, `turning`, `replan` and `formation_move` have no weapon table;
  `weapons_reload`, `projectiles` and `armour_splash` have one but no targeting settings (their victims stand
  still on purpose, and the hand calculations of their tests would not survive units that shoot back), so
  they run exactly as before.

## The `tanks_vs_infantry` scenario

`assets/scenarios/tanks_vs_infantry.ron`, on a frozen copy of the shipped definitions
(`tanks_vs_infantry_defs/`), 64 by 64 cells, 1500 ticks, `combat: true`, `targeting: true`: **ten light tanks**
(400 hit points, a cannon of 35 every two seconds) against **twenty infantry** (fourteen riflemen and six rocket
troopers) forty cells apart. Nobody is given an attack order: at tick 1 each side is told to attack-move onto the
other's ground. Measured:

| | |
|---|---|
| first shot | tick 270 (the tanks and the infantry have closed from forty cells to five) |
| last shot | tick 787; the last shell lands and the last infantryman falls at the end of tick 791 |
| shots | 228: 87 cannon shells from the tanks, 97 rifle shots and 44 rockets at the tanks |
| outcome | the tanks win with **4 of 10** left (ids 1, 2, 3 and 9, with 282, 400, 400 and 400 hit points: 1482 of 4000); all **20** infantrymen are down |
| after the fight | the four tanks march on and stand within four cells of their goal; the 26 units at zero are still where they fell, on hold |
| final checksum | `0x53d3bcb0110a9479` |

`tools/sf_headless/tests/tanks_vs_infantry.rs` runs it **twice** and compares the checksum after every tick, all
shots, and the hit points and places of all thirty at every tick; checks the numbers above and the checksum
committed in `tanks_vs_infantry.expected` (run by CI on Linux and Windows); and checks along the run that nobody
fires at its own side, that nobody at zero fires or is fired at, and that a unit at zero never moves again.
This is the headless half of the phase-5 done-when; that it looks that way in the game comes after WP5.5 and
WP5.6.

## Tests

* `crates/sf_sim/tests/targeting.rs`: the settings and what refuses them; a world without settings; what an idle
  unit picks and fires at, and what it does not (own side, out of reach, cannot be hurt, hurts-nothing); the
  armor table's preferences (rifle, rocket, sniper); the key (nearer, then the position, in both spawn orders);
  the minimum range; two weapons; the cooldown and that it is data; letting go (out of reach, at zero,
  despawned) and the next look; hold, guard, attack-move and move; explicit orders, `Stop` and a queued order;
  return fire (in reach, followed, bounded, a pursuit of zero, hold, guard, attack-move, several attackers);
  the hull turning to a narrow arc and the turret aiming; a unit at zero; the checksum; the permutation tests.
* `crates/sf_sim/src/targeting/tests.rs`: the key, the score and the engagement's reset.
* `crates/sf_data` (`sim_settings`) and `crates/sf_bridge`: the two settings, their defaults and bounds, and that
  the shipped file is the simulation's default.
* `tools/sf_headless/tests/tanks_vs_infantry.rs` and `pinned.rs`: the scenario.

## Not here (and where it goes)

* **Structures choosing targets** (defence towers, emplacements): WP6.7. A structure takes no order, so it
  needs a target without one; the scan and the key are written so that it can use them.
* **Firing at the ground**, and backing away from a target inside the minimum range.
* **Threat-based priorities** (shoot what is shooting at you first, focus fire, retreat): the AI of WP9.3 gives
  commands for those; here a unit prefers what its weapon does most to, and the nearest.
* **Alliances and neutrals**: every other player is an enemy; `friendly_fire` and the choice of targets both
  mean "the same player".
* **Switching to a better target**, and **returning to where an idle unit stood** after a chase.
* A unit that resumes an attack-move or a guard is **routed again on its own**: it does not rejoin the flowfield
  group of the order that started it.
* The effects (WP5.6). Kill credit, removing the dead and `sf_game` giving its world the weapon table and the
  settings are done in WP5.5 ([`destruction.md`](destruction.md)).
* The cost of a tick with many scanners is not benchmarked on its own (see "The cooldown"): every unit looks on
  the same tick of ten.
