# Repair and sell: two processes of a structure

Work package WP6.6 (PLAN.md phase 6), stacked on WP6.7. A player can **repair** a damaged structure of its own, which
costs money tick by tick and gives hit points back, and **sell** a structure, which takes it down over a number of ticks
and pays part of its price back. Both are processes with state and progress in the simulation, not buttons: the buttons of
the sidebar only send two commands. Rules in our own words (clean-room, B5; `docs/red-alert-analysis.md`, "Repair and
sale", was used as a functional analysis only: it says only that both are processes with progress). Code:
`crates/sf_sim/src/repair.rs` (the state, the commands, the tick), data `assets/config/repair.ron` (`sf_data::repair`,
`sf_bridge::repair_rules`) and `assets/config/repair_look.ron` (the picture); the game side is `repair_mode.rs`,
`repair_view.rs`, the buttons in `sidebar.rs`, `sidebar_layout.rs`, `sidebar_view.rs` and the hooks in `runner.rs`.
Tests are named below. All numbers are placeholders for the balance rounds of phase 15.

## The numbers

`repair.ron` (shipped values; whole numbers only):

| Field | Unit | Bounds | Shipped | Meaning |
|---|---|---|---|---|
| `hp_per_tick` | hit points | 1 to 1000 | 3 | what a repairing structure gains per tick at full speed |
| `full_cost_permille` | thousandths of the price | 0 to 1000 | 250 | what a repair from 0 to full hit points costs |
| `sell_ticks` | ticks | 1 to 6000 | 100 | how long a sale takes |
| `refund_permille` | thousandths of the price | 0 to 1000 | 500 | what a sale pays at full hit points |

A world is given them with `World::with_repair` (refused after the start, twice, without the unit and building tables or
with a number out of bounds: `the_rules_are_checked_and_given_once_before_the_start`). **A world without them refuses both
commands** (they count as ignored and are not listed as refusals) and hashes as before
(`a_world_without_the_rules_ignores_both_commands`). The data file refuses unknown fields and every out-of-bounds number
(`numbers_out_of_bounds_are_refused` in `sf_data`); the limits of the data and of the simulation are held equal by
`the_limits_of_the_data_are_those_of_the_simulation`; each number arrives in the world as itself, checked with seven
different values (`each_repair_number_arrives_as_itself` in `sf_bridge`).

## The two commands

`Command::Repair { structure, on }` (tag 16 in the pending-command hash) starts (`on`) or stops repairing. `Command::Sell {
structure }` (tag 17) starts a sale. Both are player commands, checked at their tick: the owner is the player who sent
them, and a refusal is listed (`World::repair_rejections`, reason `RepairError`) and counts as ignored. Never a panic.

| Refused with | When |
|---|---|
| `NoSuchStructure` | a stale id, a unit, an object |
| `NotYours` | an enemy's or a neutral structure (`only_the_owner_may_repair_or_sell_and_every_refusal_is_listed`; in the scenario the opponent is refused both commands on the plant, `the_tanks_hurt_the_plant_and_the_barracks_and_the_opponent_is_refused`) |
| `UnderConstruction` | the structure is still going up (`a_structure_under_construction_or_being_sold_cannot_be_repaired_or_sold`) |
| `BeingSold` | a sale has started; neither a second sale nor a repair |
| `NotStanding` | zero hit points (a wreck, which a world without destruction settings keeps) |
| `FullHitPoints` | a repair of a structure at full hit points, or of one that cannot be damaged (0 in the unit table) |
| `AlreadyRepairing` / `NotRepairing` | a start while repairing, a stop while not |

An anchor, a refinery, every other structure may be repaired and sold: there is no exception by kind. (Whether the player
should be allowed to sell the construction centre is a design question for the playtest; as the rule stands it can, and
selling the last structure defeats a player, below.)

## Repair

While the flag is set the structure gains `hp_per_tick` hit points per tick and its owner pays per tick.

**The price is one rule on the absolute hit points**, the construction rule (`C * (k + 1) / T - C * k / T`) applied to hit
points: the credits paid to bring a structure of price `C` and `M` maximum hit points to `h` are `cum(h) = C * full_cost_permille
* h / (M * 1000)` (whole credits, rounded down), and the tick that takes it from `a` to `b` hit points costs `cum(b) -
cum(a)`. No state is kept next to the hit points. So the ticks of any repair add up to exactly `cum(end) - cum(start)`
whatever the pace, and a repair from nothing to full costs exactly `C * full_cost_permille / 1000`
(`the_pay_rule_adds_up_to_the_computed_cost_for_every_gap`: 4 prices, 5 permilles, 3 maxima, 4 paces and 4 starts; and
`repair_cost` is hand-checked: 75 for a 600-hit-point, 300-credit plant from 100 hit points at 300 thousandths). In the
unit test the plant (300 credits, 600 hit points, 300 thousandths) goes from 100 to 600 in 72 steps (71 of 7 and one of 3)
and every step takes exactly `3 * h / 20 - 3 * 100 / 20` credits, in total 75
(`a_repair_gains_hit_points_and_pays_by_the_cumulative_rule_every_tick`). A tick that the rule prices at zero is free: hit
points still come.

**It never goes past full** (`a_repair_never_goes_past_full_hit_points_and_a_full_structure_refuses_it`: 3 missing, 7 per tick,
stops on the maximum). **It stops by itself** at full hit points, when the structure no longer stands (zero hit points),
when it is destroyed (the slot is cleared, so no state is left, `a_structure_removed_during_its_sale_pays_nothing_and_leaves_no_state`
shows the same for a sale), and when the sale starts (`a_sale_cancels_a_repair_in_progress`); and on the stop command.

**No money: it waits, it does not stop** (decision, the way production waits). A tick whose price the owner cannot pay takes
nothing and gains nothing; the flag stays (`RepairStatus::WaitingForMoney`) and the next tick that can be paid goes on
(`a_repair_waits_without_money_and_goes_on_when_there_is_some`: no hit points, no credits and the flag kept for 25 ticks, and the
whole run still costs 75). Why: a stop would need a click again after every income, and the owner cannot lose
anything by waiting. In a world without money rules a repair is free, like construction.

**Power slows it like it slows construction** (decision: one rule with construction): in a shortage the repair advances only
on the ticks `advances_on(tick, speed)` names (`power.md`) and costs nothing on the others, so the price is still exact
(`a_shortage_slows_a_repair_and_the_price_is_still_exact`: speed 740, a gain on exactly the ticks the function names, 58 gains
for 400 hit points and 104 credits, the cumulative difference). Production, construction and repair share the one speed.

**Restored hit points restore what hangs on them**: a producer makes `power * hp / max_hp` again from the next count
(`a_repaired_producer_makes_more_power_from_the_next_tick`: 50, rising, 100) and the damage stage of WP6.8 is read from the
hit points, so it follows with no rule of its own.

## Sale

`Sell` sets a countdown of `sell_ticks`. **A structure being sold counts as gone from the first tick**: `World::stands` and
the simulation's `is_standing` are false from the tick of the command, which is what these ask:

| Asker | Result during the sale |
|---|---|
| prerequisites, the base anchor of the build radius, production | it does not stand, so it is no prerequisite or anchor and a factory being sold accepts nothing (`a_structure_being_sold_gives_no_power_no_prerequisite_and_builds_nothing`) |
| power | it makes and uses nothing from the next count (the totals are counted at the start of a step; the scenario: used 50 to 30 at tick 481 for a sale begun at 480) |
| storage | it stores nothing from the next sync (`selling_a_refinery_lowers_the_capacity_and_the_store_keeps_what_it_holds`) |
| defences | a tower being sold fires nothing (`targeting` uses the same standing test) |
| the match | it is no structure that keeps its owner in |

It is still an entity: it occupies its cells and **can be shot** (a structure destroyed during its sale is removed by the
ordinary path and nothing is paid, `a_structure_removed_during_its_sale_pays_nothing_and_leaves_no_state`). It cannot be
repaired or sold again. **A sale cannot be cancelled** (gap).

**The end.** A sale started by a command in tick `t` takes `n = sell_ticks` ticks and ends in the step of tick `t + n`,
before the commands of that tick (`a_sale_takes_its_ticks_pays_the_refund_by_hit_points_and_removes_the_structure`: tick 40
after, `sales()` reports it). The structure is removed through the ordinary removal (`World::despawn`: its cells are
free, no wreck, no death in the tally, the outcome rule looks again). A `Sale` record (tick, id, kind, owner, refund) is
kept for that step (`World::sales`): the hook for a trigger.

**The refund** is `price * refund_permille * hp / (max_hp * 1000)`, rounded down, for the hit points it has **when the sale
ends**, paid once into the owner's store; a structure that cannot be damaged pays the full refund; a structure with no
price (the anchor) pays nothing (`the_refund_is_a_part_of_the_price_scaled_by_the_hit_points`: five structures with different
prices and hit points, e.g. 99 for the plant at 333 of 600 where a whole one would pay 180, 360 for a refinery at 450 of 900,
0 for a hut of 1 hit point; and `sale_refund` hand-checked). The refund is **income, not an undo of spending**: the statistic
of what was spent (`credits_spent`) is not reduced.

**Storage.** The sale lowers the owner's capacity from the first tick (above), and the store may then hold more than its
capacity: the existing rule (`economy.md`, "Storage") already says nothing is lost and nothing more goes in until it is below
again. The refund goes into the store after the removal, whatever the capacity, like the refund of a cancelled unit: with 5000
credits stored and one refinery (capacity 2000) the store is 5000 during the sale and 5720 after the refund of 720
(`selling_a_refinery_lowers_the_capacity_and_the_store_keeps_what_it_holds`). In a world without money rules a sale pays nothing.

**Match.** Selling the last structure of a player can defeat it. A sale marks the outcome rule to look again at the end of the
tick, and a structure being sold does not stand, so **the defeat is in the tick the sale starts**, not when the structure
is removed: in the scenario the opponent sells its only structure at tick 700, is defeated in tick 700 and the other player
wins in tick 700; the structure is gone at tick 760 (`the_opponent_sells_its_last_structure_and_is_defeated_in_the_tick_it_starts`).
A defeated player's sale still runs and still pays. A player with an armed unit left is not defeated by it
(`match.md`).

## When it happens in a tick

`World::step`: power is recounted, the structures under construction advance (`construction.md`), **then repair and sale
advance in ascending slot order** (`World::advance_repair_sell`), **then the commands of the tick run**, then production
and the rest. So the money order is: construction first, then repair, then the factories, each in slot order
(a structure under construction and a repair draw on the same store before a factory does, `production.md`). A repair
started by a command in tick `t` gains its first hit points in tick `t + 1`; the sale above ends in `t + n`. The two
processes of one structure never run together: starting a sale clears the repair flag.

## The state and the checksum

Two columns of the entity table, `repairing` (bool) and `sell_left` (ticks, zero when not being sold), reset with the slot
(`Entities::fill` and `clear`). **Section `r`** of the checksum is the last one, **written only while some structure repairs or
is being sold** (and only in a world with the rules): the four numbers of the rules, then the count and for each such structure in
slot order the slot, the flag and the ticks of its sale still to go. So a world where nobody repairs or sells hashes exactly as
before, whether or not it has the rules (`the_checksum_is_untouched_until_somebody_repairs_or_sells`: the same 20 steps in a
world with the rules and in one without give the same 20 checksums; with a repair from step 5 they differ from step 5 on), and
every number of the state and of the rules is in it (`every_number_of_the_state_and_the_rules_is_in_the_checksum`). The commands
have their own tags 16 and 17 in the pending-command hash (`the_two_commands_have_their_own_numbers_in_the_pending_queue_hash`).
The waiting list and the sales of the last step are derived and not hashed. Integers only, no map is iterated, the pass is
the slot order. Re-pins: see "What moved" below.

## The picture and the player's way in (presentation only)

* **Sidebar.** Under the power bar two buttons, **Repair** and **Sell** (`sidebar.ron`: `tool_height_px`, `tool_labels`,
  the colours `tool`, `tool_hover`, `tool_active`; all words, sizes and colours are data). A left click switches the mode on, or
  off when it is the one that is on; a right click leaves it. The lit button is the mode that is on
  (`the_button_of_the_mode_that_is_on_is_lit_and_the_others_are_not`, `a_left_click_on_a_tool_button_switches_its_mode_and_a_right_click_leaves_it`).
  In a world without repair rules the buttons are drawn dim and do nothing (`the_buttons_do_nothing_in_a_world_without_repair_rules`).
  The layout keeps the row between the power bar and the tabs, side by side (`the_two_tool_buttons_stand_side_by_side_between_the_power_bar_and_the_tabs`).
* **The mode.** In a mode a **left click on the player's own structure** sends the command (`repair_mode::click`); a **right
  click or Esc leaves the mode** (a right click orders nothing, `the_right_click_that_leaves_a_repair_or_sell_mode_orders_nothing`;
  Esc leaves the mode first and the next Esc opens the pause menu, `menus.md`). The mode stays on after a click so that several
  structures can be done one after the other. A chosen structure to build and a mode do not go together: choosing one ends the other.
  A click in repair mode on a hurt structure starts the repair, on a repairing one stops it, on a whole one does nothing; in sell mode on
  any own structure that is not going up or being sold it sells (`repair_mode::target`; tests in `repair_mode/tests.rs`). The
  simulation checks everything again at its tick (`what_a_click_sends_the_simulation_accepts`).
* **Cursor** (`cursor.md`): in a mode the spanner (`repair`) or the coin (`sell`) of the placeholder set over a structure the click
  can act on, the forbidden sign over anything else, the scroll arrows of the screen edge still first. The line under the top bar
  says what the mode does (`repair_look.ron`).
* **On the structure** (`repair_look.ron`): a **repair sign** (a square badge with a plus, floating above the structure, in the
  warning colours while the owner cannot pay) and a **sale progress bar** under the front edge (lower than the construction and storage
  bars so they do not overlap), filled to `SaleProgress::permille`. Both are drawn from the snapshot (`SimSnapshot::repairs`,
  `sales`), nothing is counted in the presentation. The money readout of the sidebar changes as it is paid.
* **Screenshots** from the real renderer, `docs/screenshots/wp6.6/` (README there): the repair cursor with the button lit, a plant
  repaired under its sign (hit points and credits at the shot), a sale at 40 and at 90 per cent, the cursor in sell mode, and the
  sold structure gone. `crates/sf_game/tests/repair_screenshot.rs` checks pixels and numbers.

## The scenario `repair_sell`

`assets/scenarios/repair_sell.ron` (frozen definitions `repair_sell_defs`, a small match of its own; numbers written out so
editing the shipped data never moves it: 5 hit points per tick, 400 thousandths, 60 ticks, 700 thousandths). Two tanks shoot the
plant and the barracks of the player (each 168 hit points down) and are removed at tick 300. The opponent is refused a repair and a
sale of the plant (tick 400). The owner repairs the plant at tick 410 with 20 credits: a credit per five hit points, the money is gone at tick 430
(532 hit points) and the repair **waits** for 110 ticks; the sale of the damaged barracks (532 of 700 hit points) started at tick
480 ends at 540 and pays **212** (400 * 700 * 532 / (700 * 1000), where a whole one would pay 280); the repair goes on at 541 and ends at 554
on 600 hit points after costing 34 credits (20 and 14 of the refund); at tick 700 the opponent sells its only structure and is
defeated that tick. Checksum `0x963b3db14951cfa5` in `repair_sell.expected` (CI runs it on Linux and Windows); amounts and ticks in
`tools/sf_headless/tests/repair_sell.rs`, worked out by hand.

## What moved (re-pins)

Nothing. No `.expected` file and no constant of `golden.rs` differs from main (a diff against `origin/main` over
`assets/scenarios/*.expected` and `golden.rs` shows only the new `repair_sell.expected`); the 29 scenarios of the CI list pass
`sf_headless --expect` in release; the benchmark pins `PLAIN` and `FIGHT` (`crates/sf_game/tests/bench_scene.rs`) pass. Cause, by
construction: the new state is two columns that are zero for every existing world and the section `r` is written only while something
repairs or is sold (`the_checksum_is_untouched_until_somebody_repairs_or_sells`). The frozen copy `repair_sell_defs` is new; no existing
copy was touched.

## Cost

Measured 04-11-2026 in the container, a release build, on a machine that was busy with other agents' builds (load average 2.4 to 6.3), so
the numbers are relative and say nothing about the laptop (PLAN section 13). `cost_of_the_repair` (`--ignored`, five rounds each): the pass
`advance_repair_sell` over 5,040 entities (40 structures) takes 7 to 16 microseconds per call whether the 40 structures are idle or
repairing (the ranges overlap: the scan of the slots is the cost, not the repair); a whole step with 5,040 entities is 1.24 to 1.40 ms
without the rules, 1.25 to 1.38 ms with the rules and nobody repairing and 1.28 to 1.47 ms with 40 repairing (the three ranges overlap).
Base (`origin/main`, built in a copy) against this branch on `wander_5000` in release, six alternating pairs: base 783 to 851 ticks per
second, branch 816 to 844, the same final checksum `0x38d5e3c43358a889`. A world without the rules pays one `Option` test per tick.

## Mutations (each reverted, each caught)

Run on the committed code with one change at a time; the failing tests are named. Unit tests are
`crates/sf_sim/src/repair/tests.rs`, scenario tests `tools/sf_headless/tests/repair_sell.rs`.

| Mutation | Fails |
|---|---|
| the owner check removed | `only_the_owner_may_repair_or_sell_and_every_refusal_is_listed`, scenario (refused-opponent, repair, sale and pin tests) |
| a repair pays nothing | `a_repair_gains_hit_points_and_pays_by_the_cumulative_rule_every_tick`, `a_repair_waits_without_money_...`, `a_shortage_slows_a_repair_...`, scenario |
| a repair goes past the maximum | `a_repair_never_goes_past_full_hit_points_...`, `a_repair_gains_hit_points_...`, scenario |
| the refund ignores the hit points | `a_sale_takes_its_ticks_pays_the_refund_by_hit_points_...`, `the_refund_is_a_part_of_the_price_scaled_by_the_hit_points`, scenario |
| a structure being sold still stands | `a_structure_being_sold_gives_no_power_...`, `selling_a_refinery_lowers_the_capacity_...`, scenario (defeat test) |
| the checksum section written always | `the_checksum_is_untouched_until_somebody_repairs_or_sells`, the pin |
| power does not slow a repair | `a_shortage_slows_a_repair_and_the_price_is_still_exact` |
| a repair stops when it cannot pay | `a_repair_waits_without_money_and_goes_on_when_there_is_some`, scenario |
| a sale does not wake the match | scenario `the_opponent_sells_its_last_structure_and_is_defeated_in_the_tick_it_starts` |
| a tower being sold still fires | `a_tower_being_sold_fires_nothing_and_drops_its_target` (`tests/defences.rs`) |
| a sale one tick too long | `a_sale_takes_its_ticks_...`, the tower test, scenario |
| `sell_ticks` and `refund_permille` swapped in the bridge | `each_repair_number_arrives_as_itself` |
| a right click in a mode orders the selection | `the_right_click_that_leaves_a_repair_or_sell_mode_orders_nothing` |
| the sidebar counts a structure being sold as standing | `a_structure_being_sold_stands_no_more_and_unlocks_nothing` |
| the repair cursor over a whole structure | `a_hurt_own_structure_is_offered_a_repair_and_a_whole_one_is_not` |

## Not done, on purpose (gaps)

* **No cancelling a sale**, no pausing a repair other than by the stop command, no "repair all" or "sell all".
* **A sold factory's queue is lost**: what was paid for its items is not given back, the same as a destroyed factory
  (`production.md`). The units it was building do not come out. **No units come out of a sold structure** (no crew, no
  free harvester).
* **No sound** (audio is another work package) and no hotkeys for the two modes (the Esc key leaves a mode; the buttons are the
  way in: the keys are rebindable actions of `input.ron` and a new action is for the key work, WP11.5).
* Repair of vehicles, engineers, repair trucks, a repair facility (phase 15 or a later work package), and the AI's use of repair and
  sale (phase 9). No fog (WP8.1): a click acts on whatever is under the cursor.
* A structure under construction cannot be repaired or sold; nor does selling a structure refund what was paid while it went up.
* A structure at zero hit points in a world without destruction settings (a wreck that stays) cannot be repaired or sold.
* The balance (speed, cost, time and refund) is a placeholder of phase 15; selling the anchor is allowed.
