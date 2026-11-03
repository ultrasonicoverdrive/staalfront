# Power

Work package WP6.4 (PLAN.md phase 6). Built after production (WP6.5) and stacked on the first match (WP7.6) by the
orchestrator's decision of 02-11-2026. Rules in our own words (clean-room, B5; `docs/red-alert-analysis.md` was used as a
functional analysis only). Code: `crates/sf_sim/src/power.rs`, with the hooks in `construction.rs` (the slower build),
`production.rs` (`production_speed_permille`) and `checksum.rs` (section `I`). Data: `assets/config/power.ron`
(`sf_data::power`, `sf_bridge::power_rules`) and, for the numbers of each structure, `power` in `unit_defs.ron`
(unchanged: the approved tech tree, power plant +100, refinery -30, barracks -20, vehicle factory -40, radar -50, ...).
Tests are named below.

## Made and used

Each structure kind has a `power` number in the building table: positive makes power, negative uses it. For each player:

* **made** is the sum of what its **standing** producers make, **used** the sum (as a positive number) of what its
  **standing** consumers use. *Stands* is the simulation's word (`World::stands`, `construction.md`): complete, with hit
  points left or a kind that cannot be damaged.
* **A structure under construction makes and uses nothing.** It draws power from the tick it stands, not while it goes up
  (decided in WP6.4; the simplest rule, and the one that does not punish the player twice for starting a building in a
  shortage). So a refinery that completes raises `used` by 30 from the next tick on
  (`a_structure_under_construction_makes_and_uses_nothing_until_it_stands`).
* **Damage lowers generation.** A producer with `hp` of `max` hit points makes `power * hp / max`, rounded down (integer,
  no number to tune: a plant at 599 of 600 makes 99, at 1 hit point 0; a structure that cannot be damaged, 0 hit points in
  the table, makes all of it). A consumer uses its full number whatever its health: a damaged barracks is not cheaper to run
  (`damage_lowers_what_a_producer_makes_by_hit_points_rounded_down`, which uses five kinds with different numbers and
  hit points). A structure at zero hit points is not standing and counts for neither. A destroyed structure leaves the
  totals at the next recount (`a_destroyed_structure_leaves_the_totals_at_the_next_step`).

Players are counted separately: one player's shortage never slows another (`a_shortage_slows_a_factory_and_the_price_is_still_paid_exactly`
runs a player with enough power beside one without).

## When it is counted

The totals are recounted **once at the very start of every step**, from the state the previous step left, and kept
(and hashed). What happens during a tick (a plant completes, a shell hits it, it is destroyed, a command places a
structure) counts from the next tick on. So the snapshot's numbers are exactly what that tick's production and
construction used (`the_totals_are_those_counted_at_the_start_of_the_last_step`), and nothing depends on the order of
the passes inside a tick. The recount is one pass over the slots in ascending order; each player's totals are kept
in an ascending list, so no hash map is iterated.

**Cost** (release, measured on this container, which had other load): 4 to 6 microseconds per recount with 40 structures
among 5,040 entities (`cost_of_the_recount`, `--ignored`); a headless run of 10,000 ticks with the same entities, six
alternating runs of three variants (base without the code, this branch with no power rules, this branch with them):
medians 899, 878 and 891 ticks per second, all inside the noise of the machine (single runs 857 to 965). A world
without power rules pays one `Option` test per tick.

## The shortage

With `used <= made` (or nothing used at all) the speed is 1000 thousandths. With `used > made` it is
`made * 1000 / used`, rounded down, **but not below `shortage_floor_permille`** (shipped 250: a base that lost its last plant
still builds at a quarter of the pace, so it can always build a new one; the simulation accepts 0, which stops
everything, `at_speed_zero_everything_stops_and_nothing_is_paid_then_it_goes_on`, but the data file refuses it).
`the_speed_rule_with_distinct_numbers` pins the rule on thirteen (made, used, floor) triples, among them 99 of 100 (990),
1 of 3 (333, rounded down), the floor winning over the ratio and floor 0.

**One speed for everything the player builds**, two mechanisms:

* **Production** (`production.md`): `World::production_speed_permille(player)` is this speed. The factory's progress counts
  in thousandths of a tick and a tick adds the speed, so a unit of 14 build ticks at speed 384 takes 37 ticks (14,000 / 384, rounded up) and the price
  follows the progress (`paid_at`): the sum is exactly the price whatever the speed
  (`a_shortage_slows_a_factory_and_the_price_is_still_paid_exactly`: 70 credits over 14 ticks, 37 ticks at speed 384; the
  other player, with enough power, 14). At speed 0 the status is `Stopped`, nothing is paid and no progress is lost.
* **Construction** (`construction.md`): a structure under construction **advances on `speed` ticks of every thousand**,
  those where `tick * speed / 1000` steps up (`power::advances_on`, no state of its own, so nothing new in the checksum):
  at 500 every other tick, at 333 about every third, at 1000 every tick, at 0 never. Over any 1,000 consecutive ticks it
  advances exactly `speed` times (`a_construction_advances_on_exactly_speed_of_every_thousand_ticks`). The money rule is
  unchanged: the tick that takes it from `k` to `k + 1` ticks done costs `C * (k + 1) / T - C * k / T` and a tick on which it
  does not advance costs nothing, so **the whole price is paid exactly, at any speed**: the test builds a 97-credit hut over
  20 ticks at speeds 1000, 625, 555, 384, 277, 1 and 333 and checks the charge of every one of the 20 advances, that nothing is
  paid on the ticks it waits, and the total (`a_structure_under_construction_slows_by_the_speed_and_the_price_adds_up_exactly`).
  A power plant under construction in a shortage is slowed like any other structure: the way out of a shortage is the
  floor, not an exemption.

## Defences (the hook; WP6.7 uses it)

`World::defences_powered(player)` is true while the speed is at least `defence_cutoff_permille` (shipped 1000: any shortage
switches defences off; 0 never does; 733 would keep them on down to a quarter missing,
`the_defence_cutoff_is_a_number_of_the_rules`), and always in a world without power rules. **Nothing reads it yet**:
the defences are WP6.7. It is in the snapshot's reach (`SimSnapshot::power_speed`) for the picture to use.

## The checksum (section `I`)

Written **last, after `Z`, only in a world given power rules** (`World::with_power`), so every earlier world hashes as
before: `golden.rs` and every `.expected` of an earlier phase stayed (checked: the section written unconditionally moves
`tiny_scenario_history`, `small_scenario_checkpoints` and twenty pinned scenarios; and
`a_world_without_power_rules_hashes_to_this_value` pins a world's checksum measured on the base, where no power code
exists). The section holds the two tuning numbers, then for each player that makes or uses anything, ascending: made and
used as counted at the start of the last step (`both_tuning_numbers_and_every_total_are_in_the_checksum` changes each
number in turn). Integers only, slot order. `with_power` is refused (`ConfigError::Power`) after the start, twice, before
the unit and building tables, after the match rules (they stay last) and for numbers above 1000.

## The data (`power.ron`)

| Number | Shipped | Meaning |
|---|---|---|
| `shortage_floor_permille` | 250 | The lowest speed a shortage brings things down to (the file takes 1 to 1000). |
| `defence_cutoff_permille` | 1000 | Defences work at or above this speed (0 to 1000). |

Both are placeholders for the balance rounds of phase 15 and the playtest of M1. Validated on load
(`numbers_out_of_bounds_are_refused`) and pinned with literals (`the_shipped_numbers_are_pinned`); the bridge test
`each_power_number_arrives_as_itself` uses distinct values so a swap shows.

## The picture

The sidebar's power bar (`sidebar.md`) fills from the snapshot: `sidebar::power_reading` returns
`PowerReading { made, used }` for the local player, the bar shows used against the larger of the two and turns to the low
colour when more is used than made. An item that is being built and advances more slowly says **"low power"** in the
warning colour in the place where "no credits" would stand, and its progress bar is the amber `bar_low` (a bar that waits for
credits stays the red `bar_waiting`): `an_item_that_is_being_built_in_a_shortage_says_low_power`,
`a_structure_going_up_in_a_shortage_says_low_power_and_one_with_enough_does_not`,
`an_item_in_a_power_shortage_has_its_own_bar_colour_and_says_so`. All colours and words are in `sidebar.ron`.
Screenshots from the real renderer: `docs/screenshots/wp6.4/` (`power_screenshot.rs`).

## Where power is on

| World | Power rules |
|---|---|
| the match (`--match`, `matchplay::configure`, the pinned scenario `match`) | on. The script of the match never has a shortage, so the game is the same game: **`match.expected` moved** from `0x54a140fc12273e31` to `0x49f207148bde095c` for the section alone (measured: with the `power` line taken out of `match.ron` the old value comes back, and tick by tick the entities, hit points and credits of the two worlds are equal: `power_in_the_match_moves_the_pin_by_its_section_only`). One effect to know: the opponent's plant is shot at from the first fight on, so its output falls and its (idle) factories slow down; the opponent never builds, so nothing changes. |
| the sandbox with `--harvest` | on. Its demo base has one of every structure for each side, so it stands in a permanent shortage (270 used of 100 made, speed 370; shown in `docs/screenshots/wp6.4/game_t00065_sandbox.png`): what it builds is slow, which is what the shortage looks like in the real HUD |
| the pinned scenario `power` | on (its own numbers written out) |
| every other scenario and the benchmarks | off; every pin stayed |

## Defences and structures that use no power

The hook is per player. WP6.7 must also key on the structure's own number: the fortified bunker is a Defence with `power` 0 and
the decoy a Decoy with 0 (the follow-up noted in `docs/progress.md` since WP6.1), so a shortage switches off a defence only
when `power < 0`.

## Pinned scenario `power`

`assets/scenarios/power.ron` (the frozen `match_defs`, m1 map, the numbers written out): a plant, a barracks and a refinery
(50 used of 100), four riflemen queued; at tick 150 a vehicle factory and a radar (140 used, speed 714: the riflemen slow
down); a Veyran light tank shoots the plant from tick 160 (its output falls with its hit points, the speed falls to the
floor of 400, the plant is destroyed at tick 1451); at tick 1600 two plants are built at the floor pace (750 ticks per 300),
complete at 2350, and everything is back at full speed. `tools/sf_headless/tests/power.rs` counts the totals again from the
public API at **every tick** and compares, asserts the story in numbers (the totals and the speed at named ticks, the tick
each rifleman came out: 101, 222, 364, 523, 2600, 2700), the price of everything, and pins the run's checksum after eight
ticks as well as the final one (the final value alone cannot see *when* things happened, which the mutation
"shortage ignored" showed). Checksum in `power.expected`, in the CI list.

## Mutations

Each applied to a scratch copy of the tree (the file restored afterwards, the working tree clean), then the named tests run. A mutation
that no test caught is a missing test: the first run of "shortage ignored" was not caught by the pinned final checksum of the
scenario (every unit stands still at the end, however fast it was made), which is why the run is also pinned along the way.

| # | Mutation | Tests that fail |
|---|---|---|
| 1 | damage ignored (a producer makes all of its output) | `damage_lowers_what_a_producer_makes_by_hit_points_rounded_down` (sf_sim); `at_every_tick_the_totals_and_the_speed_follow_the_rules`, `the_story_in_numbers`, `the_riflemen_come_out_slower_in_the_shortage_and_at_the_normal_pace_after_it` (headless `power`) |
| 2 | shortage ignored (the speed is always full) | `the_speed_rule_with_distinct_numbers`, `the_speed_and_the_defence_hook_follow_the_totals`, `the_defence_cutoff_is_a_number_of_the_rules`; in `tests/power.rs` all four tests of a slowed factory or structure; in the headless `power` the per-tick, story, riflemen and along-the-run tests (the final pin alone did not fail) |
| 3 | a structure under construction counted as standing | `a_structure_under_construction_makes_and_uses_nothing_until_it_stands`; `a_structure_under_construction_slows_by_the_speed_and_the_price_adds_up_exactly` |
| 4 | the section written without power rules | `tiny_scenario_history` and `small_scenario_checkpoints` (`golden.rs`), twenty tests of `pinned.rs`, `a_world_without_power_rules_hashes_to_this_value` |
| 5 | construction price scaled by the speed (rounding loses credits) | `a_structure_under_construction_slows_by_the_speed_and_the_price_adds_up_exactly` |
| 6 | production price scaled by the speed (rounding loses credits) | `a_shortage_slows_a_factory_and_the_price_is_still_paid_exactly` |
| 7 | the total `made` left out of the hash | `both_tuning_numbers_and_every_total_are_in_the_checksum` |
| 8 | construction not slowed | `a_structure_under_construction_slows_by_the_speed_and_the_price_adds_up_exactly`, `at_speed_zero_everything_stops_and_nothing_is_paid_then_it_goes_on`, `the_story_in_numbers`, `the_checksums_along_the_run_are_pinned` |
| 9 | consumers scaled by their damage too | `damage_lowers_what_a_producer_makes_by_hit_points_rounded_down` |
| 10 | the defence hook `>` instead of `>=` | `the_defence_cutoff_is_a_number_of_the_rules`, `the_speed_and_the_defence_hook_follow_the_totals` |
| 11 | the sidebar's low-power sign ignores the shortage | `an_item_that_is_being_built_in_a_shortage_says_low_power`, `a_structure_going_up_in_a_shortage_says_low_power_and_one_with_enough_does_not` |
| 12 | the bar reading with made and used swapped | `the_power_bar_reads_what_the_simulation_counted` |
| 13 | the low-power bar colour never used | `an_item_in_a_power_shortage_has_its_own_bar_colour_and_says_so` |

## Not done, on purpose

Defences that read the hook (WP6.7); repair and sell (WP6.6); the AI's response to a shortage (phase 9); a power-plant-first
build order or a "low power" announcement in sound (audio is another work package); a power cost for a structure while it
goes up; a separate rule for the structures of the harvester dock or the radar (a radar that switches off in a shortage is a
presentation of phase 11); damage stages by state rather than by hit points; counting power in the same tick the structure
completes.
