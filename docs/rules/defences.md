# Defensive buildings (WP6.7)

A standing structure that has a weapon picks its own targets, turns a turret toward them and fires; without
power it is silent; a structure with no weapon (the fortified bunker) does nothing. This file is the rules; the
idea (a defence shoots what comes within reach) is plain, so every rule and number is ours (clean-room, B5).

Code: `crates/sf_sim/src/targeting.rs` (`defence_check`, the structure branch of `Targeting::update`,
`write_defence_hash`), `weapons.rs` (the victim of a structure), `world.rs` (`AimTurret` refused). Data:
`unit_defs.ron` (`turret_rate` on four structures), `weapon_defs.ron` (their weapons on the turret).
Scenario `assets/scenarios/defences.ron` on the frozen copy `defences_defs/`.

It needs a world with weapons **and** targeting settings, like the units of [`targeting.md`](targeting.md). A world
without them behaves exactly as it did: no structure fires. Power rules are optional (below).

## A structure chooses, and takes no order

Candidates, score and key are the ones of [`targeting.md`](targeting.md#what-a-unit-picks), unchanged: the units and
structures of another player with hit points left, that some weapon of the structure hurts (armor table) and could
fire at now (range and minimum range, to the edge of the candidate, from the **middle** of the structure's
footprint), ranked by score, then distance, then place, then slot. The structure scans through the same spatial
index and on the same cooldown (`scan_cooldown`, 10 ticks), counted down on every tick of the structure's life
(a new structure looks on the tick after it stands). It **keeps** its target while the target is a live enemy with
hit points left that a weapon of the structure can fire at now, and drops it otherwise (gone, dead, out of reach, inside
the minimum range, no longer an enemy); it never switches to a better one, and it looks again at once if the cooldown
has run out. A weapon that does nothing to the chosen target (zero percent) does not fire, as for a unit.

**No order.** A structure is immobile, so every command that gives an order is ignored for it (`accepts_order`), and
`Order::Pursue` is something a unit gives itself; a structure never has an order and `Order::Pursue` cannot make one
walk. Its target is stored in `turret_target` (the column that holds what a turret aims at), and `Command::AimTurret`
is refused for a structure so that a player cannot make it shoot at something the rules would not (a test aims it at its
own side's unit and nothing fires). The weapons pass reads `turret_target` as the victim of a structure
(`weapons::update_all`) and fires through the ordinary weapon and projectile path: reload, burst, projectile,
damage, return fire of the victim, kill credit.

**It does not answer fire.** A structure that is hit by something out of its reach does nothing about it (return
fire is for mobile units). Alliances do not exist: every other owner is an enemy, a neutral structure of the shipped
data (`automated_gun`, `unmanned_artillery`) with a different owner shoots every player.

**When it may not fire**, `defence_check` clears its target and it does not look, on every tick:

| Condition | Why |
|---|---|
| under construction (`build_left` above zero) | it does not stand (`World::is_standing`, `construction.md`) |
| zero hit points, with a kind that has some | the same; in a world with destruction it is removed in that tick anyway |
| it uses power (its **own** `power` is below zero in the building table) and its owner's defences have none | below |

## Power

`World::defences_powered(player)` ([`power.md`](power.md)) is true while the owner's speed is at least
`defence_cutoff_permille` (shipped **1000**: any shortage switches the defences off) and always in a world without
power rules. A defence reads it **only if its own `power` is below zero**: the fortified bunker (no weapon, `power`
0) and any structure that makes or uses no power are never switched off by their owner's shortage, so a mission
structure with a weapon and no power number (the neutral `automated_gun`, `unmanned_artillery`) shoots whatever
happens to the player it is given to. The check keys on the kind's number in the building table, not on the function:
`Defence` is also the function of the bunker. The totals are those of the start of the step (`power.md`, "When it is
worked out"), so a plant that falls in tick N silences the defences from tick N+1; they lose their target in that
tick and fire again after the cooldown (at the latest `scan_cooldown` ticks after power returns, and the turret turns
first).

**Consequence for the match (M1).** The opponent's tower (`power` -20) shares its plant (+100) with the refinery
(-30) and the barracks (-20): 70 used. The plant makes `100 * hp / 600`, so the tower is dark as soon as the plant
has **less than 420 of its 600 hit points** (70 * 6 = 420). Measured in the `match` scenario with a Meridian light tank
sent at the plant (`the_opponents_tower_goes_dark_when_its_plant_is_below_420_of_600`): the last tick counted lit had the
plant at 432 hit points, the first dark one at 411 (a shell takes 21). The scripted army never gets there (it kills the
tower first, tick 4509, and the plant falls at 5400 to 5500), so in the script the power is never a factor; an army that
goes for the plant first silences the tower before it needs to fight it.

## The turret

A structure with a turret has a `turret_rate` in `unit_defs.ron` (`UnitStats::turret_per_tick`), facing steps (of 256)
per tick, and its weapons are `mount: Turret`: exactly the model of units ([`turning.md`](turning.md)). The turret
turns toward the target by at most its rate, the short way round (the tie at half a turn as there), and a weapon
fires only when the turret is within its `fire_arc` of the bearing to the target. With no target the turret turns
back to the facing of the hull, which for a structure is the facing it was placed with (0); it is an absolute facing
and is hashed in the turning section (`R`) like a tank's. The loader allows `turret_rate` on a **Structure that has a
weapon** (it stays refused on anything else without a movement class), and a weapon on the turret still needs the rate.

| Structure | `turret_rate` | Weapon (mount Turret, `fire_arc` 8) |
|---|---|---|
| `defence_tower` | 12 | `tower_gun` |
| `advanced_defences` | 8 | `emplacement_gun` |
| `automated_gun` (neutral) | 16 | `automated_gun` |
| `unmanned_artillery` (neutral) | 6 | `battery_shell` |

The numbers are distinct so a test shows a swap (`the_armed_structures_have_a_turret_each_with_its_own_rate...`, and
the turret of each tested at its own rate, `the_turret_turns_at_its_own_rate_and_fires_only_within_its_arc`). They are
placeholders for the balance rounds (phase 15), like the guns.

**The arc.** The four structure weapons had `fire_arc: 128` (all round) "until the defensive buildings get turrets"
([`weapons.md`](weapons.md#mount-and-arc)). Now they turn, they use the **default arc of every turret weapon, 8**
(about 11 degrees either side), and `fire_arc: 128` is gone from them. Why that: it is one rule for tanks and defences
(a turret weapon fires when its turret points at the target); an all-round arc would make the turret decorative and the
rule untestable. What it costs: the first shot waits for the turret, at most 128 steps (half a turn) minus the arc:
10 ticks of turning for the tower, 15 for the emplacement, 8 for the machine gun, 20 for the battery. Pins: only a world that has
a structure with one of these weapons in its definitions moves, and the headless scenarios run on frozen copies (their
structures have **no turret** and a hull weapon with the arc 128 they were pinned with: a structure without a turret
fires all round, as before the turret existed, and the loader and `Arsenal::new` check the two kinds apart: a hull weapon
on a structure without a turret, a turret weapon on one with).

## The bunker, and the gap

`fortified_bunker` has no weapon, so no loadout: a **weaponless structure** takes one class and one `is_armed` test in
the loop `Targeting::update` runs over all slots anyway, then continues: no cooldown, no scan, nothing hashed
(`a_weaponless_structure_is_left_alone_by_the_update`, `the_bunker_does_nothing`, and the scenario). Its `garrison: 5`
flag is data nothing reads.

**Gap: garrisoning (infantry inside a bunker, shelter, fire from the inside) is not done.** It needs a unit entering a
structure, taking it out of the world's bodies and putting its weapon in the structure's: that is a design of its own
(owner: phase 9 with the AI that would use it, or a work package WP6.7b if the playtest of M1 wants the bunker to do
something). Until then the bunker is a tough 1200-hit-point wall. Also not done: the decoy (`decoy_emplacement`,
`disguise_as`) has no behaviour, the neutral structures have no mission logic (`fires_at_all`, "fires when the uplink is
alive": mission logic of phase 10 reads them), and there is no fog: a defence sees as far as its weapon reaches
and the sight in the data is unused.

## The checksum

* The structure's scan cooldown is its `Engage::cool`; section `A` ([`targeting.md`](targeting.md#the-checksum)) lists
  every live entity whose engagement is not at rest, so a structure with a running cooldown is in it, as in any world
  with targeting.
* Its target is in `turret_target`, hashed in the turning section `R` only for a structure that has a turret. A hull
  structure (a frozen copy) would hide it, so a new section **`d`** (the tag byte `b'd'`, after `A`, inside the
  condition of `A`: weapons and targeting settings) lists every live armed structure in slot order with its target (id
  and generation), written **only while the world has at least one armed structure**: a world without one hashes
  exactly as before (`golden.rs`, every `.expected` without armed structures, the benchmark's `PLAIN`; mutation 6 below).
* Pins that moved, and why (measured):
  * **`match`**: `0x49f207148bde095c` to `0x164403a92986654f`. Its opponent has a defence tower on the frozen copy
    (no turret, hull weapon, all round). Tower first shot at tick 3830, 40 shots, destroyed at 4509; the human still
    wins, at **5967 instead of 5687**, with 3 of its 9 units left (before: 9 of 9) and 6 losses where there were none.
    Not the script (it is unchanged), the rules. Measured cause (review of WP6.7): with the structure branch of targeting
    and section `d` both switched off, `match` gives exactly the WP6.4 value `0x49f207148bde095c` again; switching only
    the tower's weapon off does not, since section `d` alone also moves the value. Besides: the world without the power line equals the
    world with it tick by tick (`power_in_the_match_moves_the_pin_by_its_section_only`, now without the WP7.6 literal that
    is no longer reachable). `match_m1.rs` is updated (`DECIDED`, six kills, `the_opponents_tower_fires_in_the_match`).
  * **The fight benchmark `FIGHT`** (`bench_scene.rs`): moved by two digests, the weapon table's (section `W`) and the unit table's turret rates
    (section `R`), both changed by the new data: the same run on the
    base's shipped `unit_defs.ron` and `weapon_defs.ron` with this code gives the old list; on the new ones every shot, hit and death per hundred
    ticks and the position and hit points of all entities at tick 1500 are equal and only the checksum differs. Details
    in `docs/bench/README.md`. `PLAIN` did not move.
  * No other pin moved: `golden.rs`, `wander_5000`, `command_mix`, `terrain_256`, `units_spawn`, the whole list of
    `.expected` files except `match` (and the new `defences`).

## The `defences` scenario

`assets/scenarios/defences.ron` (64 by 64 cells, 1500 ticks, frozen copy of the shipped definitions with the turrets in
`defences_defs/`): Meridian has a tower (-20) and an advanced defence (-40) on a plant (+100), a bunker and nothing
else; Veyran sends units in waves. Pinned in `defences.expected` (`0xb31bf0125145ac15`; CI runs it on Linux and Windows)
and asserted in `tools/sf_headless/tests/defences.rs`. Measured:

| Tick | What |
|---|---|
| 0 | the emplacement fires at the tank (its turret is within the arc of it from the start); the tower's first shot is at 7 (the turret has to come round) |
| 103 | the tower has shot a rifleman down; 119 its last shot before the dark |
| 134 | the three tanks have the plant at 348 of 600: it makes 58 of the 60 the two defences use, the speed falls to the floor 250, `defences_powered` is false; the emplacement's last shot was at 100 |
| 134 to 900 | neither defence fires, while the first wave's second rifleman and (from 700) a second wave stand in reach of the tower and hurt it (600 to a lower value at 900) |
| 371 | the plant is destroyed |
| 901 | the new plant (placed on 900) is counted: powered again; the emplacement fires at once, the tower at 907 |
| 954 to 1323 | the tank falls to the emplacement (954), the rifleman and the three of the second wave fall to the tower (987, 1099, 1211, 1323); the one at the bunker is out of the tower's reach and stays |
| 1500 | tower 498 of 600, emplacement 296 of 800, bunker 1128 of 1200 (it was hurt by the rifleman beside it, and never shot) |

## Presentation

The snapshot gives a structure with a turret its `TurretView` (as a unit's), and the runner drives the model's `turret`
part with it (`Motion::with_turret`), so the tower and the advanced defences of art set v4 (which have a `turret`, a
`barrel` and a `muzzle` node) turn on their own. The muzzle flash and tracer go through the existing event path: a `Fire`
event for a structure's shot uses the turret's facing and the footprint's size (`fx_events::record`, tested on the
`defences` scenario: `a_defence_structures_flash_points_along_its_turret`). Nothing new is drawn and nothing new is
configured; the models are the placeholders of the art set. Screenshots, from the real renderer on a software adapter
(`crates/sf_game/tests/defences_screenshot.rs`), in `docs/screenshots/wp6.7/`, with the tick in the name:
`tower_t0001_standing.png` and `tower_t0002_firing.png` (the tower with a Veyran tank 5 cells away: before and at the first
shot, the turret has come round); `tower_t0122_no_power_silent.png` (no plant, 120 ticks, the tank in reach, the turret at
rest and no shot) and `tower_t0125_power_back_firing.png` (a plant stands, the turret has come round and fires);
`emplacement_t0001_turning_0.png`, `_t0005_turning_1.png`, `_t0010_turning_2.png` (the slow turret of the advanced
defences at turret facings 8, 40, 80 steps). The checks look at pixels in the turret's place between the pictures
(731, 731, 2669 and 2803 pixels differ).

## Tests and mutations

| Where | What |
|---|---|
| `crates/sf_sim/tests/defences.rs` (11) | a tower picks, fires and keeps no order; out of reach not picked and a target that leaves dropped; each turret at its own rate and every shot within the arc; a target outside the arc waits for the turret to come round (the first shot on exactly the tick the turret is within the arc); a hull weapon fires all round; orders and `AimTurret` refused; a defence that uses power is silent in a shortage, one that does not is not, a plant restores them; no power rules, always fires; under construction nothing; the bunker does nothing; a replay gives the same checksums |
| `crates/sf_sim/src/targeting/tests.rs` (3 new) | `defence_check` for each way it may not fire, with the power list of another player changing nothing and a power of 0 or above not being switched off; the update leaves a weaponless structure at rest and a neighbour armed structure does pick it; the section `d` is written for an armed structure only and its target and generation are in it |
| `crates/sf_data/tests/units.rs` (2 new, 1 changed), `weapons.rs` (1 changed) | the shipped rates and mounts, a turret on a structure needs a weapon |
| `tools/sf_headless/tests/defences.rs` (5), `match_m1.rs` (+2) | the scenario above; the tower fires in the match; the 420 of 600 threshold |
| `crates/sf_game/tests/fx_events.rs` (+1), `defences_screenshot.rs` (3) | flash along the turret; the pictures |
| `crates/sf_game/tests/damage_screenshot.rs` (changed) | the measure of fire light also counts orange (see `progress.md`, WP6.7 notes): the sandbox's neutral gun now shoots in that scene |

Mutations, each applied to a scratch edit of the file and reverted (`git status` clean afterwards):

| # | Mutation | Tests that fail |
|---|---|---|
| 1 | the power check removed (`dark` always false) | `a_defence_that_uses_power_is_silent_in_a_shortage_and_one_that_does_not_is_not`, `a_structure_keeps_a_target_in_reach_and_loses_it_when_it_may_not_fire`, the scenario tests |
| 2 | the `power < 0` key replaced by "any defence" | the same two (the station that uses no power goes silent) |
| 3a | `AimTurret` accepted by a structure | `orders_and_aim_commands_to_a_structure_are_refused` |
| 3b | the orders accepted by a structure (the immobile and the speed guards of `accepts_order` removed) | `orders_and_aim_commands_to_a_structure_are_refused` |
| 4 | the turret arc ignored | `a_target_outside_the_arc_waits_for_the_turret`, `the_defences_shoot_while_they_have_power` and four more of the scenario's |
| 5 | an under-construction structure fires | `a_structure_under_construction_does_nothing_until_it_stands`, `a_structure_keeps_a_target_in_reach...` |
| 5b | a structure at zero hit points fires | `a_structure_keeps_a_target_in_reach...` (a structure cannot be brought to zero in a world without destruction by the public API of the tests) |
| 6 | the section `d` written without armed structures | `the_defences_section_is_written_only_for_a_world_with_an_armed_structure`, `tanks_vs_infantry_matches_the_pinned_checksum` and two more |
| 7 | a weaponless structure looks round (the `is_armed` test removed) | `a_weaponless_structure_is_left_alone_by_the_update`, `it_replays_the_same_and_matches_the_pinned_checksum` |

## Cost

Release build, same machine (4 cores shared with other agents, load average 2.1 to 2.8), alternating runs of the two
binaries, microseconds per tick, medians of 5 runs of each, 4 pairs (`crates/sf_sim/tests/defence_cost.rs`, `--ignored`; the same file
runs on the base, where the towers take no part):

| | base | this branch |
|---|---|---|
| 40 towers, 200 units in reach (3.8 shots a tick), 2000 ticks | 48.1 / 49.3 / 49.0 / 42.5 | 56.9 / 60.6 / 59.2 / 59.1 |
| no structure at all, 5,000 idle units, targeting on, 500 ticks | 806 / 765 / 781 / 765 | 794 / 786 / 772 / 757 |

The first is about **10 microseconds a tick more** for the forty armed structures with their shots (a 20 percent rise of a small
world's 49); the second shows **no difference** for a world with none (the new code in such a world is a class test per
slot in the update loop and one pass over the slots for the section `d`). Relative numbers only: the container cannot
measure the frame rate (PLAN section 13).

## Not here (and where it goes)

* Garrisoning the bunker, the decoy's disguise, mission logic of the neutral structures, fog and sight: see above.
* Repair and sell of structures: WP6.6, done (`repair_sell.md`; a tower being sold fires nothing, `a_tower_being_sold_fires_nothing_and_drops_its_target`). The AI that builds defences (phase 9), balance of ranges, damage and costs (phase 15).
* A structure answering fire from out of its reach, switching to a better target, preferring threats: as for units, not here.
* Structures that rotate their hull; the barrel's pitch (still a hook of the renderer).
