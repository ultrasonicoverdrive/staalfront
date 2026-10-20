# Weapons (WP5.1)

A weapon is a row of numbers in `assets/config/weapon_defs.ron`: how far it shoots, how often,
how hard, with what kind of damage, and what the shot does on its way. Units carry up to four
weapons by id (`weapons: ["cannon"]` in `unit_defs.ron`). This work package makes the data
complete enough for the three that follow (projectiles WP5.2, armor and splash WP5.3, choosing
targets WP5.4), puts it into the simulation, and uses it in the smallest possible way: a unit
with an attack order fires when its weapon is reloaded and the victim is in reach, and the hit
takes hit points off the victim. That last part is a **placeholder** and says so below.

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
| `damage` | hit points per shot, before armor | 1 to 1,000,000 | required |
| `damage_type` | a name from `unit_classes.ron`; the armor table of WP5.3 keys on it | must exist | required |
| `projectile` | `Instant`, `Straight`, `Arc`, `Guided` or `Falling` | | required |
| `projectile_speed` | cells per second | 0 to 200 (ten cells a tick); not for `Instant` | 0 (not given) |
| `flight_height` | cells: the top of an `Arc`, the height a `Falling` shell drops from | 0 to 32; only `Arc` and `Falling` | 0 |
| `guide_turn` | facing steps per tick a `Guided` projectile steers toward its target | 0 to 64; only `Guided` | 0 |
| `burst` | shots per trigger pull | 1 to 16 | 1 |
| `burst_delay` | time between the shots of a burst | a multiple of 0.05 s; at least one tick when `burst` is above 1, **zero** when it is 1 | 0 |
| `mount` | `Hull` or `Turret` | `Turret` only on a unit with a `turret_rate` | `Hull` |
| `fire_arc` | half the width of the firing arc, in facing steps | 0 to 128 (128 is all round) | 8 (eleven degrees and a quarter) |
| `splash_radius` | cells in which a hit also damages, with falloff (WP5.3) | 0 to 64 | 0 (none) |

None of the fields added in WP5.1 is required. The scenarios pin their checksums on frozen copies
of the definition files written before the fields existed, and those copies must keep loading
with the same meaning: every default above is "no change". The shipped file gives every
projectile that flies a speed (a test sees to that); the loader does not insist, so that the
copies stay valid. WP5.2 is the first to read a speed and decides whether to require one then
(it would mean refreshing the frozen copies).

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
not back away (that is a matter of choosing targets, WP5.4).

### Mount and arc

A weapon points where the hull points (`Hull`) or where the turret points (`Turret`; the turret has
its own facing since WP4.5, `turning.md`). It fires when that facing is within `fire_arc` steps of the
bearing to the victim, the shortest way round. Infantry and a gunboat carry hull weapons; the three
tanks and the siege artillery carry theirs on the turret. A unit with a turret does not turn its hull
toward its victim (`Order::Attack` leaves that to the turret), so a **hull** weapon on a unit with a
turret would never be aimed; no shipped unit has one, and WP5.4 will have to decide whether the hull
turns for such a weapon. The structures' weapons are `fire_arc: 128` (all round) because a structure
has a fixed hull until the defensive buildings get turrets (WP6.7).

The data check: a weapon with `mount: Turret` on a unit without a `turret_rate` is refused when the
units are loaded, with the line of the weapon in the unit's record. The world checks the same when it
is given the table.

### The projectile, for WP5.2

`projectile` says how the shot gets there; the parameters are what the flight needs.

| Kind | Meaning | Parameters |
|---|---|---|
| `Instant` | hits in the tick of the shot, no projectile exists (rifles, machine guns) | none |
| `Straight` | flies straight at the target | `projectile_speed` |
| `Arc` | lobbed over what is between (howitzers) | `projectile_speed`, `flight_height` |
| `Guided` | steers toward its target in flight (rockets) | `projectile_speed`, `guide_turn` |
| `Falling` | drops from above onto the target cell | `projectile_speed` (the fall), `flight_height` (where it starts) |

Height is presentation only (PLAN.md 1.3: no true 3D ballistics in v1): the flight time follows from
the speed and the horizontal distance, not from the arc. A parameter on a kind that does not use it
(a `guide_turn` on a `Straight` weapon) is refused, so a typo cannot hide.

The weapons of the shipped file at full range: a light-tank shell takes about 3.3 ticks (30 cells a
second), a rocket about 8, a howitzer shell about 14. The fuse and the detonation (analysis section 8)
are WP5.2's; a fuse time, if it needs one, becomes another optional field.

### Damage type and splash, for WP5.3

`damage_type` is a name in `unit_classes.ron`, stored as its kind number; the table of WP5.3 is
damage type times armor type to a percentage, and it adds itself next to the lists in that file. The
sniper's "infantry only" is that table's job (precision does nothing to non-infantry armor), not a
field of the weapon. `splash_radius` is stored and converted; the falloff is WP5.3's. WP5.1 uses
neither.

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

A world that is not given a table has no weapons: nothing fires, nothing about weapons is hashed. The
game (`sf_game`) does not give one yet, because nothing there would show a shot and a destroyed unit
would stand where it was; it is one line when WP5.4 and WP5.5 land. The headless runner gives it when a
scenario says `combat: true` (and has `unit_defs`).

### The checksum

The weapon state of every entity is hashed in a new section, `W`, written **last and for as long as the
world has a weapon table** (`checksum.rs`): the table's digest, the number of entities that have a weapon
reloading or in a burst, and each of them in slot order with, for each of its four slots, `wait` and
`burst_left`. Peers with different tables therefore differ from the first tick, not from the first shot. A
world without a table hashes exactly as before (the section does not exist for it). That is why no pinned checksum of an earlier phase moved: `wander_5000`, `command_mix`, `terrain_256`,
`units_spawn`, `orders`, `terrain_costs`, `pathing`, `pathing_256`, `bridge_200`, `crowd_jam`,
`turning`, `replan` and `formation_move` are the values they were. The hit points the shots take are in
the units section already. The shots listed by `World::shots` are not state and not hashed.

`UnitStats` and the digests of the unit table (`digest`, the weapon reach digest of the orders
section, the turret digest) are untouched: the weapons are a table of their own, so the sections that
the earlier scenarios pin do not change.

## The placeholder shot

WP5.2 to WP5.4 replace this part, step by step; it exists so that the data and the reload are
exercised and a scenario can pin them.

After the units have moved and the turrets turned, in **two passes**.

The first goes through the entities in ascending slot order and every weapon slot of each in order: the
wait counts down; when it is zero and the entity's **current order is `Attack`** on a live victim other
than itself, the weapon fires if

1. the victim has hit points (zero means it cannot be hurt, or has been shot down: see below), and
   so has the shooter (a unit whose kind has hit points but has none left fires no more),
2. it is in reach and not inside the minimum range, as above,
3. the hull or the turret points at it within the arc.

Nothing in this pass changes a hit point, so **every shot of a tick is decided from the hit points at the
start of the tick**, whatever the slots: two units that kill each other both die, two shooters that overkill
one victim both fire and both spend their reload, and a unit that is shot down on a tick still fires on it.
The outcome of a fight does not depend on which unit was spawned first (tests spawn the units in every order).

The second pass lands the hits. A shot is an instant hit, whatever the `projectile` says: the victim's hit
points go down by `damage`, the sum of its hits of the tick saturating at zero, and a `Shot` (tick,
attacker, victim, weapon, slot, damage, hit points after) is recorded for the tick. `hp_after` is a running
total in the order of the attackers' slots (this hit and the ones recorded before it in the tick), so it is
the one thing that depends on the order; it is a report and is not hashed. The `damage_type`, the splash and the armor play no part. Nothing is
removed when a victim reaches zero (WP5.5): it stays where it is, is not fired at again, and the attacker
keeps its order. Orders other than `Attack` (attack-move, guard, hold, a unit that is idle) fire
nothing: target choice is WP5.4. There is no check of whose unit the victim is.

## What WP5.2 to WP5.4 do with each field

| Work package | Replaces or reads |
|---|---|
| WP5.2 projectiles | `projectile` and its parameters; the instant hit stays for `Instant` and the others spawn an entity at the shot, which carries `damage`, `damage_type` and `splash_radius` to the detonation. The reload, burst and aim rules stay |
| WP5.3 armor and splash | `damage_type` times the victim's armor type; `splash_radius` with a falloff; the saturating subtraction becomes the real damage |
| WP5.4 targets | which victim, which weapon at which target, the choice of order other than `Attack`, whether a hull weapon on a turret unit turns the hull, backing off inside `min_range` |

## The shipped set (placeholders)

Ranges are design targets of `content/units.md`; everything else is a placeholder. Metres are cells times
four. Time is the reload in seconds and ticks.

| id | kind | range cells (m) | min | reload s (ticks) | damage | type | projectile | speed c/s | mount | arc |
|---|---|---|---|---|---|---|---|---|---|---|
| `rifle` | 0 | 4 (16) | | 1 (20) | 6 | bullet | Instant | | Hull | 8 |
| `rocket` | 1 | 5 (20) | | 2.5 (50) | 45 | rocket | Guided (turn 4) | 12 | Hull | 8 |
| `long_rifle` | 2 | 8 (32) | | 3 (60) | 40 | precision | Instant | | Hull | 8 |
| `pistol` | 3 | 3 (12) | | 1 (20) | 4 | bullet | Instant | | Hull | 8 |
| `carbine` | 4 | 3 (12) | | 0.8 (16) | 5 | bullet | Instant | | Hull | 8 |
| `satchel_charge` | 5 | 0.5 (2) | | 1 (20) | 5000 | demolition | Instant | | Hull | 8 |
| `cannon` | 6 | 5 (20) | | 2 (40) | 35 | cannon | Straight | 30 | Turret | 8 |
| `twin_cannon` | 7 | 5 (20) | | 2 (40) | 2 x 30, 5 ticks apart | cannon | Straight | 30 | Turret | 8 |
| `howitzer` | 8 | 10 (40) | 3 | 5 (100) | 90, splash 1.5 | shell | Arc (height 3) | 14 | Turret | 8 |
| `anti_tank_gun` | 9 | 6 (24) | | 2.5 (50) | 70 | cannon | Straight | 36 | Hull | 8 |
| `naval_cannon` | 10 | 6 (24) | | 2 (40) | 45 | cannon | Straight | 30 | Hull | 8 |
| `tower_gun` | 11 | 6 (24) | | 0.8 (16) | 8 | bullet | Instant | | Hull | 128 |
| `emplacement_gun` | 12 | 7 (28) | | 2.5 (50) | 80 | cannon | Straight | 36 | Hull | 128 |
| `automated_gun` | 13 | 8 (32) | | 1 (20) | 4 x 5, 2 ticks apart | bullet | Instant | | Hull | 128 |
| `battery_shell` | 14 | 12 (48) | 3 | 6 (120) | 80, splash 1.5 | shell | Arc (height 3) | 14 | Hull | 128 |

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
* `crates/sf_bridge`: every weapon and loadout arrives, and the shipped table fits the shipped units in
  a world;
* `assets/scenarios/weapons_reload.ron` with `tools/sf_headless/tests/weapons_reload.rs`: riflemen on a
  structure, two tanks on a tank, two howitzers (one too close), a victim shot down to zero, and a reload
  that an order change does not reset; the checksum is pinned in `weapons_reload.expected` and run by CI
  on Linux and Windows. It reads a **frozen copy** of the definitions, `assets/scenarios/weapons_defs/`.
