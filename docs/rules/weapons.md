# Weapons (WP5.1, projectiles WP5.2, armor and splash WP5.3)

A weapon is a row of numbers in `assets/config/weapon_defs.ron`: how far it shoots, how often,
how hard, with what kind of damage, and what the shot does on its way. Units carry up to four
weapons by id (`weapons: ["cannon"]` in `unit_defs.ron`). This work package makes the data
complete enough for the three that follow (projectiles WP5.2, armor and splash WP5.3, choosing
targets WP5.4, [`targeting.md`](targeting.md)), puts it into the simulation, and uses it in the smallest possible way: a unit
with an attack order fires when its weapon is reloaded and the victim is in reach, and the hit
takes hit points off the victim. Since WP5.2 a weapon that is not `Instant` launches a projectile
that flies and detonates ([`projectiles.md`](projectiles.md)). Since WP5.3 what a hit does is the
weapon's `damage` times the victim's armor percentage, and a weapon with a splash radius also hurts what
stands near the burst ([`armour.md`](armour.md)).

Everything is in our own words and numbers (clean-room, B5): invented weapon names, invented
values, nothing from another game's data. The ranges are the design targets of `content/units.md`;
the rest are guesses for the balance rounds of phase 15.

The general rules for the three definition files (permanent numbers, tombstones, what is
validated, how a file error reads) are in [`units.md`](units.md); this file is about weapons.

## Units of measure

| Quantity | In the file | In the simulation | Rounding |
|---|---|---|---|
| distance (`range`, `min_range`, `splash_radius`, `flight_height`) | cells, up to three decimals (1 cell = 4 m, B12) | `Fx` cells | to the nearest 1/65536, ties away from zero (`Decimal::to_fx`, `Fx::from_ratio`) |
| time (`reload`, `burst_delay`) | game seconds, up to three decimals | whole ticks (20 a second, B3) | **none**: a value that is not an exact multiple of 0.05 s is refused when the file loads |
| speed (`projectile_speed`) | cells per game second | `Fx` cells **per tick** (`projectile_speed_per_tick`, and the per-second value for display) | rounded **once** from the exact decimal: `milli / 20000` to the nearest raw unit, ties away from zero (`Decimal::to_fx_per_tick`). Dividing the fixed-point per-second value by 20 afterwards would round twice |
| angle (`fire_arc`, `guide_turn`) | facing steps, 256 to a full turn | the same integers | none |
| damage, `burst` | whole numbers | `u32`, `u8` | none |

A text with a fourth decimal (even a zero), an exponent or a `+` sign is refused, so no float
ever exists between the file and the simulation. The conversion module has no float type, and
`sf_sim` is checked by the determinism lint.

## The fields

| Field | Meaning | Bounds | Default |
|---|---|---|---|
| `kind` | the permanent number of the weapon (below) | 0 to 4095, unique | required |
| `id` | unique snake_case name that units refer to | | required |
| `range` | longest distance it can hit, measured to the **edge** of the target | above 0, at most 64 cells | required |
| `min_range` | shortest distance it can hit, measured the same way | 0 or more, below `range` | 0 (none) |
| `reload` | time from the first shot of a burst to the first shot of the next | at least one tick (0.05 s), at most 600 s, a multiple of 0.05 | required |
| `damage` | hit points per shot, before armor and falloff | 1 to 1,000,000 | required |
| `damage_type` | a name from `unit_classes.ron`; the armor table (WP5.3, [`armour.md`](armour.md)) keys on it | must exist | required |
| `projectile` | `Instant`, `Straight`, `Arc`, `Guided` or `Falling` | | required |
| `projectile_speed` | cells per second | 0 to 200 (ten cells a tick); not for `Instant` | 0 (not given) |
| `flight_height` | cells: the top of an `Arc`, the height a `Falling` shell drops from | 0 to 32; only `Arc` and `Falling` | 0 |
| `guide_turn` | facing steps per tick a `Guided` projectile steers toward its target | 0 to 64; only `Guided` | 0 |
| `burst` | shots per trigger pull | 1 to 16 | 1 |
| `burst_delay` | time between the shots of a burst | a multiple of 0.05 s; at least one tick when `burst` is above 1, **zero** when it is 1 | 0 |
| `mount` | `Hull` or `Turret` | `Turret` only on a unit with a `turret_rate` | `Hull` |
| `fire_arc` | half the width of the firing arc, in facing steps | 0 to 128 (128 is all round) | 8 (eleven degrees and a quarter) |
| `splash_radius` | cells from the burst, to the edge of a victim, in which a hit also damages, with falloff (WP5.3) | 0 to 64 | 0 (none) |
| `splash_inner` | cells inside which the splash does its full damage, then it falls in a straight line to nothing at `splash_radius` (WP5.3) | 0 or more and below `splash_radius`; only with a `splash_radius` | 0 (it falls from the start) |
| `friendly_fire` | whether the splash also hurts the shooter's own side (WP5.3) | only `false` is given, and only with a `splash_radius` | `true` |

None of the fields added in WP5.1 is required **by the loader**. The scenarios pin their checksums on
frozen copies of the definition files written before the fields existed, and those copies must keep
loading with the same meaning: every default above is "no change". The shipped file gives every
projectile that flies a speed (a test sees to that).

**WP5.2's decision: the simulation requires what it reads, the loader does not.** A weapon that flies
(any kind but `Instant`) needs a speed, and a `Falling` one a height to drop from: `WeaponTable::new`
refuses a table that lacks them (`WeaponsError::NoSpeed`, `NoHeight`), so no world is made with a
projectile that never moves. The five frozen copies that predate the fields are left as they are (comments
included): the headless runner, the only code that builds a table from a frozen copy, now builds it only for
a scenario with `combat: true`. See [`projectiles.md`](projectiles.md#data-is-projectile_speed-required).

### The reload and the burst

The reload counts from the **first** shot of a burst to the first shot of the next. A burst of
`n` shots `d` ticks apart takes `(n - 1) * d` ticks, and the rest, `reload - (n - 1) * d`, is the
wait after the last shot (`tail`). So the cadence is the reload whatever the burst, and a burst is
a way to deliver the damage of one reload in pieces. Loading refuses a burst that leaves no rest
(`(n - 1) * d` at or above the reload), a delay on a single shot and a burst without a delay.

```
reload 40 ticks, burst 2, burst_delay 5 ticks (the twin cannon)
tick:   0     5                   40    45
        shot  shot  . . . . . .   shot  shot
        |--5--|--------35---------|--5--|
              wait 35 = the tail
```

In the unit, per weapon slot, two numbers are kept: `wait`, the ticks until the weapon may fire
(it counts down every tick, whatever the unit is doing), and `burst_left`, the shots still to come in
a running burst. After a shot with shots to come, `wait` is the burst delay; after the last shot it
is the tail. **The reload belongs to the unit, not to its order**: stopping a unit, or giving it
another order, does not reload its weapons, so there is no trick in changing orders. A burst that
cannot go on when its next shot is due (the victim is gone or dead, out of reach, not aimed at, or
the order was taken away) is **dropped**: the weapon waits the tail, as after a whole burst, and the
shots that were left are not owed.

### Reach and minimum range

Both are measured from the middle of the shooter to the **edge** of the victim: its radius is half
the `size` of a unit and half the shorter side of the footprint of a structure, the same radius
`Order::Attack` walks to (`orders.md`). The condition is `distance <= range + radius`, which is
exactly the test by which an attack order counts as arrived, so a unit that has stopped in reach can
fire. The minimum range is `distance >= min_range + radius`. A unit that is too close to fire does
not back away (and since WP5.4 it does not pick a target that close either: [`targeting.md`](targeting.md)).

### Mount and arc

A weapon points where the hull points (`Hull`) or where the turret points (`Turret`; the turret has
its own facing since WP4.5, `turning.md`). It fires when that facing is within `fire_arc` steps of the
bearing to the victim, the shortest way round. Infantry and a gunboat carry hull weapons; the three
tanks and the siege artillery carry theirs on the turret. A unit with a turret does not turn its hull
toward its victim (`Order::Attack` leaves that to the turret), so a **hull** weapon on a unit with a
turret would never be aimed; no shipped unit has one, and WP5.4 decided that the world **refuses** such a
table (`WeaponsError::HullWeaponOnTurret`; the turret aims and the hull does not follow it for a weapon, see
[`targeting.md`](targeting.md#range-check-and-the-duplicated-range)). The structures' weapons are `fire_arc: 128` (all round) because a structure
has a fixed hull until the defensive buildings get turrets (WP6.7).

The data check: a weapon with `mount: Turret` on a unit without a `turret_rate` is refused when the
units are loaded, with the line of the weapon in the unit's record. The world checks the same when it
is given the table.

### The projectile

`projectile` says how the shot gets there; the parameters are what the flight needs. How each kind flies, what
makes it go off, what happens when the target moves or dies: [`projectiles.md`](projectiles.md).

| Kind | Meaning | Parameters |
|---|---|---|
| `Instant` | hits in the tick of the shot, no projectile exists (rifles, machine guns) | none |
| `Straight` | flies straight at where the target stood and goes off when its path touches the target | `projectile_speed` |
| `Arc` | lobbed over what is between (howitzers); goes off where it lands | `projectile_speed`, `flight_height` |
| `Guided` | steers toward its target in flight (rockets) | `projectile_speed`, `guide_turn` |
| `Falling` | drops from above onto the place where the target stood | `projectile_speed` (the fall), `flight_height` (where it starts) |

Height is presentation only (PLAN.md 1.3: no true 3D ballistics in v1), apart from the drop of a `Falling`
shell: the flight time of the others follows from the speed and the horizontal distance, not from the arc. A
parameter on a kind that does not use it (a `guide_turn` on a `Straight` weapon) is refused, so a typo cannot
hide.

The weapons of the shipped file at full range: a shell of the cannons takes 4 ticks (30 cells a second), a
rocket 8, a howitzer shell 15 (14 cells a second over ten cells), a bomb of the scenario's frozen
definitions 16. The fuse is not a field: it follows from the flight (`projectiles.md`).

### Damage type and splash (WP5.3)

`damage_type` is a name in `unit_classes.ron`, stored as its kind number; the armor table in that file is
damage type times armor type to a percentage ([`armour.md`](armour.md)). The sniper's "infantry only" is
that table's job (precision does nothing to non-infantry armor), not a field of the weapon.
`splash_radius`, `splash_inner` and `friendly_fire` make a burst hurt what stands near it; how the
falloff works, who is hurt (the direct victim once, the shooter and the shooter's side included unless
the weapon says otherwise) and how the number is rounded are in [`armour.md`](armour.md).

## Permanent numbers and tombstones

A weapon is named by its `kind` everywhere the simulation, scenarios, saves and replays look: never
changed, never reused, and the order of the records in the file does not matter. A removed weapon
leaves a tombstone in `retired_weapons: [(kind: 9, name: "old_gun")]`. Since WP5.1 a tombstone
protects the **id** as well as the number: a live weapon that takes the id of a tombstone under a new
number is refused, so an old scenario that names the removed weapon never finds a different one. (The
same rule now holds for every list that has tombstones, units and movement, armor and damage types
included.)

## What is refused when the files load

Each error names the file, the line of the weapon and the weapon, for instance
`assets/config/weapon_defs.ron:<line>:<column>: weapon "twin_cannon": reload 2 s is not longer than the
burst of 2 shots 2 s apart (40 ticks): the next burst would start before this one has ended`.

* ids: not snake_case, listed twice (the second is named), a tombstone's id;
* numbers: a `kind` above 4095, twice, missing or on a tombstone; an empty list of weapons;
* `reload`: zero, negative, not a multiple of 0.05 s, above 600 s;
* `range` out of bounds; `min_range` negative or not below `range`; `splash_radius` out of bounds;
  `splash_inner` negative, not below the radius, or given to a weapon without one; `friendly_fire: false`
  on a weapon without a splash;
* `damage` zero or above the limit; `damage_type` that does not exist;
* `burst` out of 1 to 16; `burst_delay` not whole ticks, missing for a burst, present for a single
  shot, or a burst that does not fit in the reload; `fire_arc` above 128;
* `projectile_speed`, `flight_height`, `guide_turn` out of bounds, or given to a kind that does not use
  them;
* a unit that lists an unknown weapon, the same weapon twice, or more than four; a turret mount on a
  unit without a turret;
* a syntax error, an unknown field or an unknown projectile name (with the line and the column).

`crates/sf_data/tests/weapons.rs` has one test per group, checking the message and the line.

## In the simulation

`sf_sim` does not read `sf_data`. `sf_bridge::weapon_table(&UnitDefs)` copies the converted weapons
into a `sf_sim::WeaponTable` (the numbers of every weapon, and for each unit kind which weapons it
carries in slot order), and the caller gives it to the world with `World::with_weapons`, **after**
the unit table and before anything is scheduled. The world refuses a table that does not fit its
units: a loadout for a kind it lacks, a first weapon whose range is not the kind's `weapon_range`
(the reach of an attack order, copied from the first weapon by the same bridge), a kind with a reach
and no weapon, a turret mount on a kind without a turret.

A world that is not given a table has no weapons: nothing fires, nothing about weapons or projectiles is
hashed. Since WP5.5 the game (`sf_game`) gives its world one, with the targeting settings
([`targeting.md`](targeting.md)) and the destruction settings ([`destruction.md`](destruction.md)); the sandbox's
sides are placed out of each other's reach so that the viewer does not start as a battle. The headless runner gives it when a scenario says `combat: true` (and has
`unit_defs`), and the targeting when it says `targeting: true`.

### The checksum

The weapon state of every entity is hashed in a new section, `W`, written **last and for as long as the
world has a weapon table** (`checksum.rs`): the table's digest, the number of entities that have a weapon
reloading or in a burst, and each of them in slot order with, for each of its four slots, `wait` and
`burst_left`. Peers with different tables therefore differ from the first tick, not from the first shot. A
world without a table hashes exactly as before (the section does not exist for it). That is why no pinned checksum of an earlier phase moved: `wander_5000`, `command_mix`, `terrain_256`,
`units_spawn`, `orders`, `terrain_costs`, `pathing`, `pathing_256`, `bridge_200`, `crowd_jam`,
`turning`, `replan` and `formation_move` are the values they were. The hit points the shots take are in
the units section already. The shots listed by `World::shots` and the impacts by `World::impacts` are not
state and not hashed.

Since WP5.2 the projectiles in flight are hashed in a section of their own, `J`, written after `W` for as long
as the world has a weapon table (and so never for a world without one): the slot count and free list of the
projectile store and every projectile in flight. That is why `weapons_reload`, the only pinned scenario with a
weapon table, was re-pinned in WP5.2: `0x5a1bca1ebf1efa16` to `0x20a067e16e098d0b`. The final value moved
**only** because of section `J`: with that section left out the branch ends on the WP5.1 value, because every
shell has landed by the last tick and the end state is the same. (The flight times show in the checkpoints in
between and in the asserts: the heavy tank D goes down at tick 427, not 423.) No other pinned checksum moved.

WP5.3 changed the **digest of the weapon table** (the first thing section `W` hashes): its tag is
`sf_sim/weapons/v2` and it covers the full-strength radius of a splash, the friendly-fire flag and the armor
table, the armor type of every unit kind included ([`armour.md`](armour.md#the-checksum)). Worlds without a
weapon table are still not affected; `weapons_reload` was re-pinned again (`0x20a067e16e098d0b` to
`0x59d08622efd30483`) and `projectiles` too (`0x770192e1d2d6da1c` to `0xd9912751868da230`), for the reasons
measured in `docs/progress.md`.

WP5.4 added a section `A` after `J`, written only when the world was also given targeting settings
([`targeting.md`](targeting.md#the-checksum)); `weapons_reload`, `projectiles` and `armour_splash` are given none and
did not move.

`UnitStats` and the digests of the unit table (`digest`, the weapon reach digest of the orders
section, the turret digest) are untouched: the weapons are a table of their own, so the sections that
the earlier scenarios pin do not change.

## A tick of shooting, in two passes

Since WP5.2 a weapon either hits at once (`Instant`) or launches a projectile that flies and detonates later.
Since WP5.3 what a hit does is `weapons::damage_of`: the weapon's `damage` through the armor table for the
victim it hit, and the falloff share for everyone else near the burst ([`armour.md`](armour.md)). Choosing
targets is WP5.4 ([`targeting.md`](targeting.md)).

After the units have moved and the turrets turned, the weapons of a tick run in **two passes, and no hit point
changes in the first**. This is the rule, and it is binding for every work package that changes what a hit
does (it was asked for in the review of WP5.1):

1. **Decide.** In ascending slot order the projectiles in flight move and decide whether they go off, and on
   whom ([`projectiles.md`](projectiles.md)); then, in ascending slot order, every weapon slot of every entity
   counts its wait down and decides whether it fires. A weapon fires when it is ready and the entity's
   **current order is `Attack`** (or, since WP5.4, `Pursue`: the same, on a target the unit chose itself, which
   fires only the weapons that do something to it) on a live victim other than itself, and
   1. the victim has hit points (zero means it cannot be hurt, or has been shot down: see below), and so has
      the shooter (a unit whose kind has hit points but has none left fires no more),
   2. it is in reach and not inside the minimum range, as above,
   3. the hull or the turret points at it within the arc.

   An `Instant` weapon decides a hit on the victim; any other kind decides a launch (the projectile enters the
   store in the second pass, and first moves in the next tick). **Every decision reads the hit points and
   positions of the start of the weapon stage and changes none.**
2. **Apply.** Every hit of the tick, the instant ones and the detonations, is turned into damage by
   `weapons::damage_of` (the direct victim, and since WP5.3 everyone in a splash too), **summed per victim,
   and taken off its hit points once, saturating at zero**. Projectiles that went off leave the store and
   the launched ones enter it.

So the outcome of a tick does not depend on the order of the entities or of the projectiles: two units that
kill each other both die; two shooters that overkill one victim both fire and both spend their reload; a unit
that is shot down on a tick still fires on it; a shell that lands on the tick another shell brings its victim to
zero counts, and one that lands on a victim already at zero at the start of its tick does not (a miss). Only the
**reports** follow the slots (`Shot`, `Impact`): see `hp_after` below. Tests spawn the units in every order
and compare by position (`crates/sf_sim/tests/weapons.rs`, `tests/projectiles.rs`) and create projectiles in
every order with every state of the free list (`src/projectiles/tests.rs`).

`Shot` (tick, attacker, target, weapon, slot, damage, hit points after) is recorded for every trigger pull.
**`hp_after` is the target's hit points at the end of the tick**, the same number for every shot and impact at
that target in that tick (in WP5.1 it was a running total in attacker-slot order, which was the one thing that
depended on the order). It is a report, not state, not hashed, and **not to be read as truth about who took the
hit points: WP5.5's kill credit must not use it** (nor the order of the shots in a tick). `damage` of a
projectile's shot is what it will do if it hits, **before the armor** (`Impact::damage` is what the burst did
to its direct victim after it, and `Impact::splash_victims` counts the others it hurt).

In a world without destruction settings nothing is removed when a victim reaches zero: it stays where it
is, is not fired at again, and the attacker keeps its order. With them (WP5.5, [`destruction.md`](destruction.md))
the victim is removed at the end of the tick it reaches zero, and an order against it ends like one against
any entity that is gone. Orders other than `Attack` and `Pursue` (move, and attack-move, guard, hold and idle in a world
without targeting) fire nothing. An attack order does not check whose unit the victim is; a unit that
chooses its target does (it never picks its own side).

## What WP5.3 and WP5.4 do with each field

(WP5.3 is done: [`armour.md`](armour.md). So is WP5.4: [`targeting.md`](targeting.md).)

| Work package | Replaces or reads |
|---|---|
| WP5.2 projectiles (done) | `projectile` and its parameters: an `Instant` weapon hits in its tick, the others launch a projectile that carries the weapon's `damage` (and, for WP5.3, `damage_type` and `splash_radius`) to its detonation. The reload, burst and aim rules stay |
| WP5.3 armor and splash (done) | `damage_type` times the victim's armor type; `splash_radius` and `splash_inner` with a falloff, `friendly_fire`; the saturating subtraction takes the real damage. It plugs into `weapons::damage_of`, the one function that turns a hit into damage |
| WP5.4 targets (done) | `range`, `min_range` and the armor table's percentage pick the target (the weapon's `damage`, `burst`, `reload` weigh the weapons of one unit); a weapon that does nothing to the chosen target does not fire; the choice by idle, hold, guard and attack-move units, and return fire; a hull weapon on a turret unit is refused. Not done: backing off inside `min_range`, firing at the ground |

## The shipped set (placeholders)

Ranges are design targets of `content/units.md`; everything else is a placeholder. Metres are cells times
four. Time is the reload in seconds and ticks.

| id | kind | range cells (m) | min | reload s (ticks) | damage | type | projectile | speed c/s | mount | arc |
|---|---|---|---|---|---|---|---|---|---|---|
| `rifle` | 0 | 4 (16) | | 1 (20) | 6 | bullet | Instant | | Hull | 8 |
| `rocket` | 1 | 5 (20) | | 2.5 (50) | 45 | rocket | Guided (turn 8, was 4 in WP5.1) | 12 | Hull | 8 |
| `long_rifle` | 2 | 8 (32) | | 3 (60) | 40 | precision | Instant | | Hull | 8 |
| `pistol` | 3 | 3 (12) | | 1 (20) | 4 | bullet | Instant | | Hull | 8 |
| `carbine` | 4 | 3 (12) | | 0.8 (16) | 5 | bullet | Instant | | Hull | 8 |
| `satchel_charge` | 5 | 0.5 (2) | | 1 (20) | 5000 | demolition | Instant | | Hull | 8 |
| `cannon` | 6 | 5 (20) | | 2 (40) | 35 | cannon | Straight | 30 | Turret | 8 |
| `twin_cannon` | 7 | 5 (20) | | 2 (40) | 2 x 30, 5 ticks apart | cannon | Straight | 30 | Turret | 8 |
| `howitzer` | 8 | 10 (40) | 3 | 5 (100) | 90, splash 1.5 (full to 0.5) | shell | Arc (height 3) | 14 | Turret | 8 |
| `anti_tank_gun` | 9 | 6 (24) | | 2.5 (50) | 70 | cannon | Straight | 36 | Hull | 8 |
| `naval_cannon` | 10 | 6 (24) | | 2 (40) | 45 | cannon | Straight | 30 | Hull | 8 |
| `tower_gun` | 11 | 6 (24) | | 0.8 (16) | 8 | bullet | Instant | | Hull | 128 |
| `emplacement_gun` | 12 | 7 (28) | | 2.5 (50) | 80 | cannon | Straight | 36 | Hull | 128 |
| `automated_gun` | 13 | 8 (32) | | 1 (20) | 4 x 5, 2 ticks apart | bullet | Instant | | Hull | 128 |
| `battery_shell` | 14 | 12 (48) | 3 | 6 (120) | 80, splash 1.5 (full to 0.5) | shell | Arc (height 3) | 14 | Hull | 128 |

The rocket's `guide_turn` went from 4 to 8 in WP5.2, so that its turning circle (about 3.1 cells at 12 cells a
second) is inside its range of 5 where it was larger (about 6.1); the numbers behind it are in
[`projectiles.md`](projectiles.md#the-rocket).

Two weapons changed their numbers in WP5.1 to make a burst natural without changing the damage of a
reload: the twin cannon fires two shells of 30 where it fired one of 60, and the automated gun four of
5 where it fired one of 20. The `rifle`, `long_rifle`, `pistol`, `carbine`, `satchel_charge`,
`tower_gun` and `automated_gun` are `Instant` (they were `Straight` placeholders).

## Tests

* `crates/sf_data/tests/weapons.rs`: the shipped set and its conversion, the rounding of the speed,
  every refusal above with its message and line, the permanent numbers and tombstones, a file written
  before WP5.1;
* `crates/sf_sim/tests/weapons.rs` and `crates/sf_sim/src/weapons/tests.rs`: the table and what it
  refuses, the cadence of a reload, a burst, a burst that is cut or loses its victim, reach, minimum
  range, aim by turret and by hull, a victim at zero, orders that do not fire, the checksum section and
  that a world with weapons that have not fired hashes as one without;
* the projectile tests are listed in [`projectiles.md`](projectiles.md#tests);
* the armor and splash tests are listed in [`armour.md`](armour.md#tests);
* `crates/sf_bridge`: every weapon and loadout arrives, and the shipped table fits the shipped units in
  a world;
* `assets/scenarios/weapons_reload.ron` with `tools/sf_headless/tests/weapons_reload.rs`: riflemen on a
  structure, two tanks on a tank, two howitzers (one too close), a victim shot down to zero, and a reload
  that an order change does not reset; the checksum is pinned in `weapons_reload.expected` (re-pinned in WP5.2 and WP5.3)
  and run by CI on Linux and Windows. It reads a **frozen copy** of the definitions,
  `assets/scenarios/weapons_defs/`.
