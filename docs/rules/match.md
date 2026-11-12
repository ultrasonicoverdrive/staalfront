# The match: defeat, win and the first map (WP7.6)

Milestone M1 is "build a base, harvest, make units, attack, win". Until WP7.6 nothing in the game said
who had won. This page is the minimum that makes M1 playable: a **fixed win and loss rule in the
simulation**, a **match file** (one map, two players, what each starts with), a **static opponent** and a
**result banner**. It is not the scenario format: that is phase 10 (WP10.1 to WP10.3), which takes the
match file over (starting objects and players move into the scenario, and the fixed rule becomes one goal
among others). It is not an AI either: that is phase 9.

Code: `crates/sf_sim/src/outcome.rs` (the rule), `crates/sf_data/src/match_def.rs` (the file),
`crates/sf_bridge/src/lib.rs` (`match_rules`, `match_commands`, ...), `crates/sf_game/src/matchplay.rs`
and `match_view.rs` (the game mode and the banner). Data: `assets/matches/m1.ron`, `assets/maps/m1`,
`assets/config/match_look.ron`. Scenario: `assets/scenarios/match.ron`.

## The rule

A world given match rules (`World::with_match(MatchRules)`: the players that take part, two to eight, each
once) asks one question of every player at the end of every tick in which something changed the answer:

> Does this player own **a standing construction centre**?

*Since WP7.7 (Tijmen, 08-11-2026, after he played M1: hunting the last unit was tedious). The rule before was "a standing
structure or an armed unit". It is not kept anywhere and has no flag.* The construction centre is the structure whose
building function is `Anchor` (`construction_center` in the shipped data; `buildings.md`).

A player for whom the answer is no is **defeated**, in that tick, for good. When all players but one are
defeated, the last one **wins**, in that tick, for good. If the last players are defeated in the *same*
tick nobody wins: the result is a **draw**. Once the result is decided nothing in the outcome changes any
more, whatever happens in the world afterwards (the defeated player's harvester may still be driving, a
loser may even build a new construction centre: the result stays).

| What the player owns | Counts? | Why |
|---|---|---|
| a construction centre that stands (`World::stands`: complete, not being sold, hit points left) | **yes, the only thing** | it is the base. |
| a construction centre under construction | no | nothing yet to lose (`a_construction_centre_under_construction_does_not_count_until_it_is_complete`). |
| a construction centre being sold | no, from the first tick of the sale | selling it defeats the player at once (`selling_the_construction_centre_defeats_the_player_at_once`, `repair_sell.md`). |
| any other structure, any unit (armed or not), an object | no | the base is gone with its construction centre; a player who only has units or other structures can do nothing that changes the match. |
| anything of a player who does not take part | not looked at | the neutral roles of a scenario are nobody's business. |

**The match begins** at the first check at which *every* player owns a standing construction centre. Until then
nobody can be defeated. The starting objects of a match enter the world through commands, and a world
whose second player appears at tick 5 must not end at tick 0. (A world in which some player never owns
a construction centre has no result: `a_world_in_which_a_player_never_owns_a_construction_centre_has_no_result`. The match
file refuses such a player when it is loaded, see "The match file".)

**When it is decided.** After the weapon stage and the removal of the dead (`destruction.md`), in the same
tick: the result carries the tick of the last death (`MatchResult::tick`, equal to the defeat tick of the
last defeated player; they are recorded separately, `World::defeat_tick` and `World::match_result`).
Defeats are kept in the order they happened: ascending tick, ascending player within a tick.

**What a world needs.** `with_match` must come last, after the unit table, the building table, the weapon table and the
destruction settings (`ConfigError::Match` otherwise, also after the start and twice; `with_units`, `with_buildings`,
`with_weapons` and `with_destruction` refuse with `ConfigError::Match` once the match rules are in). The building table
must have a kind of the function `Anchor` (`match_rules_need_a_construction_centre_kind_in_the_building_table`): without
one nobody could stay in. Without destruction nothing is ever removed, so the question would never be asked; without weapons
the world has no fight (the weapon table is still required, as before).

## Cost

The question is not asked every tick. `World::despawn`, the one place an entity leaves, sets a flag;
the end of the tick looks at the flag (the start of a sale sets it too). Once the match has begun only a removal
or a sale can change the answer: a construction centre that stops standing is removed in the same tick
(destruction is required) or starts its sale, and the building table never changes. A tick without a removal costs one flag
test. A tick with one scans the slots in ascending order and **stops as soon as every player still in the
match has been found once** (usually within the first slots: the starting objects of both sides are the
lowest slots), so the scan is not a pass over all entities per tick. Before the match has begun the scan
runs every tick (for the first tick or two). Measured: see "Measured" below (the worst case adds under one percent).

## The checksum

Section `Z`, written **last** (after `Y`, production) and **only in a world that has match rules**: the
players, whether the match began, the defeats (player and tick) and the result. The flag of the scan is
not state (a step ends with it clear). A world without match rules hashes exactly as before: every pin of
the earlier phases is unchanged (`crates/sf_sim/tests/golden.rs`, every `.expected`, the benchmark's PLAIN
and FIGHT checksums); `a_world_without_match_rules_has_no_outcome_and_no_section` pins the value of a small
such world as a literal.

## The match file

`assets/matches/m1.ron` (`sf_data::match_def::MatchDef`, checked when it is read and against the unit
definitions and the map):

| Field | Meaning |
|---|---|
| `name`, `seed` | name for the console; the world seed. |
| `map` | the name of a directory under `assets/maps` (no `/`, no path out of it). |
| `players` | two to eight; the first is the human (the game's local player is player 0). |
| per player: `id`, `faction` | the player number (once) and its faction (not Neutral): what it may build. |
| per player: `credits` | the credits it starts with, 0 to a billion (the money rules' limit). |
| per player: `objects` | `(role, cell)` in the order they enter the world; a structure's footprint is anchored on `cell` (smallest x and y), a unit stands in the middle of it. Objects, neutrals and vessels cannot start a match. |

`MatchDef::check` refuses a role that does not exist, a role not open to the player's faction, an object
that lies (with its footprint) outside the map, two structures that overlap and (WP7.7) **a player that starts
without a construction centre** (a structure of the function `Anchor`): the defeat rule would defeat it before it
began, so the file is wrong (`a_player_without_a_construction_center_is_a_load_error`). Every loader calls
`check` (`sf_game --match`, `sf_headless`), so the error is raised when the file is loaded. A player that is to get
its first construction centre later is not something the file can say. Whether the ground is
buildable or passable is the world's to say: the objects enter on tick 0 through `PlaceStructure` and
`SpawnUnit` (`sf_bridge::match_commands`; nothing writes the world directly), and a command the world
refuses is an error for the game (`sf_game --match` stops: "the world refused N of the starting objects")
and a test failure for the scenario (`commands_ignored == 0`).

## The first match: map `m1`

A flat 96 by 96 cell map made by `cargo run -p sf_headless --example gen_match_map -- assets/maps/m1`
(the layout is written out in the example; `the_committed_map_is_what_the_generator_draws` draws it again
and compares). The human (Meridian) is in the south-west, the opponent (Veyran) in the north-east, 80 cells
apart. Between them a lake and two rock ridges leave two ways round it; each base has two ore fields
nearby and each way has a neutral one.

| Player | Starts with |
|---|---|
| 0, Meridian | a construction centre on (12, 68); 4000 credits (a power plant 300, a refinery 1200 and a barracks 400 leave 2100 for the first units). |
| 1, Veyran (the opponent) | a power plant, a refinery, a barracks, a defence tower, a harvester, four riflemen and a light tank around (62..82, 14..31), and since WP7.7 a **construction centre on (72, 14)** (4 by 4: x 72..75, y 14..17) in the middle of the north of its base, behind the tower and the plant as seen from the human's side, clear of every other object and of ore; no credits. It is the last object of the list, so that the slots of the others (1 to 10) stay as they were and what the human builds comes one slot later. |

## The static opponent

The opponent **does not build, produce or attack**. There is no AI before phase 9, and nothing in this WP
gives the opponent a command. Its units stand where they were put and fire back on their own: the
return fire and target choice of WP5.4 (`targeting.md`), with its pursuit distance of 6 cells, so a unit
goes no farther from where it stood (`the_opponent_stays_put_fires_back_and_never_builds_or_produces`
measures it). Its **harvester does mine**: the harvest loop of WP7.2 is the unit's own state machine, not
an order, and a base without it would look dead; the credits it brings (the refinery fills to its capacity
and the harvester then waits) are never spent. To win (WP7.7) the human destroys its construction centre; before it had to destroy its four structures *and* its
four riflemen and tank (the harvester did not count).

## Controllers (WP9.1d)

Each player of a match file has a `controller` (`sf_data::match_def::Controller`). Left out it is `Standard`, which is what every file meant
before: the player with the id of the game's local player (0) is the human at the keyboard and every other player is **static** (above). `Ai(profile:
"normal")` makes the player a computer opponent (`sf_ai`) with the profile `assets/ai/<profile>.ron`; the game's sim thread thinks for it
(`docs/rules/ai.md`, "In the game"). Checked when the file is read: the profile is a plain name (lower-case letters, digits and `_`);
`matchplay::check`: the local player cannot be an AI; `matchplay::ais`: the profile file exists and fits the player's faction, the error names the
player and the profile. An AI player needs a construction centre like every player (`MatchDef::check`). `m1.ron` and every frozen copy of it have no
`controller` and are unchanged; `assets/matches/m1_ai.ron` is the first match with an AI, started with `sf_game --match m1_ai`. Pinned in
`match_def.rs` (`the_controller_defaults_to_standard_...`, `the_shipped_match_with_an_ai_is_pinned`, `an_ai_controller_is_read_and_its_profile_name_is_checked`)
and `matchplay.rs` (`the_local_player_cannot_be_an_ai`, `the_ais_come_from_the_controllers_and_a_missing_profile_is_named`).

## The game mode

`sf_game --match [FILE]` (default `assets/matches/m1.ron`) starts the match scene: the map, the economy
(`--harvest`'s rules: harvest, storage, money, production), combat and the match rules, the starting
objects of the file, the camera on the human's first object, and the sandbox's controls: build with B
and the mouse, produce with the keys of `production.md`, order, attack. `--map` is ignored (the file names
the map). A file the game cannot play (a role that is not there, a seat whose faction is not the one the
game draws that seat as, a missing local player, a refused object) stops the start with the reason.

**The result banner** (`match_view.rs`, `match_look.ron`): at the top middle, "Victory" when the local
player won, "Defeat" when someone else did, "Draw" when nobody, and under it the game time and the
tick ("decided after 4:44 (tick 5687; since WP6.7 the scripted match is decided at tick 5967, see below)"). **The game keeps running behind it**: the result never changes,
and a player may carry on, for example to destroy the rest. Since WP11.5a two buttons, **Restart** and **Main menu**, stand under
the banner, and Esc opens the pause menu (`menus.md`); neither changes the result. Every word, number and colour of the banner is
in `match_look.ron`, those of the buttons in `menu_look.ron`.

## The headless scenario `match`

`assets/scenarios/match.ron`, on a frozen copy of the definitions and of the match file
(`assets/scenarios/match_defs/`; the economy numbers are written out in the scenario), scripts the human:
a power plant (done at tick 301), a barracks (705) and a refinery (1105; it brings a free harvester),
four riflemen and five rocket troopers (the credits and then the ore pay for them), an attack-move at tick
2400 that arrives at the opponent's base, and two attack-moves deeper into it. **The first fight is at
tick 3689; the opponent's construction centre falls at tick 6238 (WP7.7; its last structure and armed unit fell at 5967 until then, 5687 until WP6.7), and that is the defeat tick of player 1 and
the win tick of player 0.** The scenario runs to tick 7000. **At the shipped build speed (WP6.11, 3 / 2) the same scripted match is decided
at tick 5656, not 6238** (final checksum `0xda53debc239be030`): the scenario `match` is frozen at 1 / 1, so its pin and the
ticks of this page did not move; `match_m1.rs` (`the_match_at_the_shipped_build_speed_is_won_sooner_and_one_over_one_is_the_old_match`)
runs it with `build_speed: (3, 2)` written into its production block and pins 5656 and that checksum, and shows that written as
1 / 1 the old pin comes back. (The script's commands stay on their old ticks, so what changes is that everything they ask for is ready sooner.) `tools/sf_headless/tests/match_m1.rs` pins the
checksum and asserts all of the above (see "Tests").

**Power (WP6.4).** The match is played with power rules (`matchplay::configure`, and the `power` line of the scenario):
what the structures make and use is counted, a shortage slows building and production, damage lowers a plant's output
(`power.md`). The script never has a shortage (the human's plant makes 100 and its refinery and barracks use 50), so the game
is the same game, tick by tick; **`match.expected` moved** from `0x54a140fc12273e31` to `0x49f207148bde095c` for the new
checksum section `I` alone. Measured: with the `power` line taken out of `match.ron` the old value comes back, and with
it in, the entities, their hit points and the credits equal those of the world without it at every checked tick
(`power_in_the_match_moves_the_pin_by_its_section_only`). The one visible effect is the opponent's: once the army shoots at
its plant, its output falls with the hit points and its (idle) factories slow down; it never builds, so nothing changes.

**The tower fires (WP6.7).** The opponent's defence tower (`power` -20, on its plant with the refinery and barracks: 70 used of
100 made) now shoots, since defences choose targets ([`defences.md`](defences.md)). `match.expected` moved from
`0x49f207148bde095c` to `0x164403a92986654f`: the script is unchanged and the rules moved it. The tower fires its first shot at
tick 3830 (140 ticks after the first death, the army must come within its reach of 6 cells), 40 shots in all, and is destroyed at
tick 4509; the human still wins, at **tick 5967 instead of 5687**, with 3 of its 9 units left and 6 lost (before WP6.7: none lost). The frozen
copy of the definitions has the tower without a turret and with the weapon all round, as when it was made, so the match is
not a test of the turret. Because the opponent's power use is 70, the tower goes dark when its plant has **less than 420 of its
600 hit points**; the script never takes the plant before the tower falls, so it never happens in the script
(`the_opponents_tower_goes_dark_when_its_plant_is_below_420_of_600` makes it happen with a tank sent at the plant: lit at 432 hit
points, dark at 411). *(Ticks 5967 and 5687 in this paragraph and the next are history: see "The construction-centre rule and the gate" for today's.)* The ticks above in "The headless scenario" and the banner text "decided after 4:44 (tick 5687)" describe the
first version; the game's banner shows whatever tick the game decides.

**Vision (WP8.1, WP8.2a).** The game turns vision on in a match (`matchplay::configure`); since WP8.2a the scenario has the same
rules (`vision: Some((legacy_sight_milli: 5000))`), so there is no longer a difference between the pinned match and the played one.
`match.expected` moved from `0x164403a92986654f` to `0x771de0f816275bbc` for the section `v` alone (measured: with the line taken
out the old value comes back, `vision_in_the_match_moves_the_pin_by_its_section_only`; the game is the same game tick by tick,
`the_first_match_plays_the_same_with_vision_on`). The targeting gate of WP8.2a is off in the match until the match has been played
and decided (`vision.md`). *(Shipped on since WP7.7, see below.)* Measured by WP8.2a: with the gate on the match is still won, at tick 8067 instead of 5967, with 8 units
lost instead of 6 (a rocket trooper whose sight equals its reach does not pick the opponent's tower at the limit of its reach; the
worlds part at tick 4282, `what_the_gate_would_do_to_the_first_match`).

**The construction-centre rule and the gate (WP7.7).** Tijmen played M1 on 08-11-2026 and won; two decisions came out of it
and are built together because both move the pinned `match` scenario. (1) The loss rule above. (2) `acquire_needs_sight` is on
in the shipped `vision.ron` and in the scenario (`vision.md`), with the sight of a target measured to its edge. The scenario's
frozen match file got the opponent's construction centre (the same line as `m1.ron`; a deliberate change of a frozen copy, for
the rule this WP changes), and the script's slot numbers moved up by one (factory 13 to 14, army 14..23 to 15..24) because the
human's things now come after eleven opponent objects, not ten.

*Measured* (the scenario run to its end, release; the world is the same game whatever the rule, so the old rule's decision
can be read off the same run):

| | match decided at | units of the human lost | note |
|---|---|---|---|
| main before this WP (old rule, gate off, 10 opponent objects) | 5967 | 6 | `match.expected` `0x771de0f816275bbc` |
| main before this WP, gate on, old cell test (`#105`) | 8067 | 8 | measured by WP8.2a, not shipped |
| **(i)** new rule, new base, gate **off** | 6228 | 6 | pin `0x4d2b34f903b93f78` |
| **(ii)** new rule, new base, gate on, **old cell test** | 9189 | 8 | measured while building this WP, the test replaced by the old one in a scratch copy |
| **(iii)** new rule, new base, gate on, **edge test** (shipped) | **6238** | 6 | `match.expected` `0xb709cd3110af3c0f` |
| for comparison: the old rule on the new base, gate off | 6717 | 6 | the tick the opponent has no structure and no armed unit left |

So the edge test brings the gated match back to within 10 ticks of the ungated one (the gate still changes something: the
worlds part at tick 4692), where the cell test cost 3000 ticks and two units. The win tick is *later* than before (6238, not
5967) because the construction centre stands at the back of the base, behind the tower and the plant: the tower falls at
4509 and the centre only at 6228 (gate off); with the old rule the human needed 6717 on this base. At the deciding tick the
opponent still owns its barracks (`the_deciding_blow_is_the_construction_centre_and_the_opponent_still_has_other_things`).
The scripted human still wins and **the script needed no change beyond the slot numbers**: the attack-moves at 2400, 4700 and 5400 reach the
construction centre on their own.

*Why `match.expected` moved* (`0x771de0f816275bbc` to `0xb709cd3110af3c0f`), one part at a time: (a) the opponent's extra
object (and with it the slots) changes the world from tick 0: entities, vision stamps (the centre sees 10 cells), kills
(11 instead of 10); (b) the rule changes section `Z` (the defeat and result ticks: 6238 instead of 5967) and nothing else, the
game does not read the outcome (checked: the game with and without match rules is the same game tick by tick,
`a_world_without_match_rules_plays_the_same_game_and_has_no_outcome`); (c) the gate (section `v` carries it, and it changes the
game from tick 4692). Pins of the same scenario with the parts switched off, measured: gate off `0x4d2b34f903b93f78`, without
the vision line `0xdccf390f64809d17` (`the_pins_of_the_match_with_the_gate_on_off_and_without_vision`). No other pin moved:
`repair_sell` (its match file has one construction centre per player, nobody builds), `silo`, `vision`, `vision_targeting`
and the earlier phases are what they were.

A scenario can name a `match_file` (a file in its `unit_defs` directory): it then gets the match rules,
the factions, the credits and the starting objects from the file, and `factions` and `money.starting`
must be left out.

## Tests and mutations

| Test | What it shows |
|---|---|
| `crates/sf_sim/tests/outcome.rs` (18) | the rules are refused when wrong, and without a construction centre kind in the building table; only a standing construction centre keeps a player in (a barn, armed units, a harvester and an object do not); the last player standing wins in the tick of the last defeat; a simultaneous end is a draw; three players; nobody is defeated before everyone has a construction centre; a world with a player who never has one has no result; once decided never changes; a construction centre shot down defeats its owner in the tick it died; **selling it defeats the player at once, selling another structure does not**; **a construction centre under construction does not count until it is complete**; independent of spawn order; the defeats and result are in the checksum; a world without match rules has no section (literal pinned, as before WP7.7). |
| `crates/sf_sim/tests/sight_gate.rs` (6), `vision/tests.rs` (3 new) | the edge test of the gate: a tower at squared distance 26 (sight 5, reach 5) is picked, a two-cell structure too, what is out of reach is not; a unit is seen through a neighbour cell its body reaches and not otherwise; the sight alone (side yes, corner no, clipped at the map border). |
| `tools/sf_headless/tests/match_m1.rs` (13) | the pinned checksum and the pins of the gate off and without vision; the starting objects in the order of the file (12, the construction centre last); the win tick 6238, nothing decided before it; the deciding blow is the construction centre while the opponent still has its barracks; nothing changes in the 762 ticks after; the gate parts the worlds at 4692; the same game without the rules differs only by the section; the opponent stays put, fires back and never gains an entity; the tower fires (WP6.7); the harvest pays; the committed map is what the generator draws. |
| `crates/sf_data` (`match_def`, `match_look`), `crates/sf_bridge/tests/match_def.rs` | the file's shape and checks; the shipped file pinned as literals; each number arrives in the world as itself (distinct values). |
| `crates/sf_game` (`match_view`, `args`, `tests/match_screenshot.rs`) | the standing from the winner, the clock, the words from the look; the flag; the real renderer. |

Twelve mutations of WP7.6 (rows 1, 4, 5 and 7 are about the old rule and are superseded by the table after this one), each applied in a scratch copy of the tree and reverted:

| # | Mutation (scratch copy, reverted) | Tests that fail |
|---|---|---|
| 1 | the armed check removed (any mobile unit with hit points counts) | `an_unarmed_unit_and_an_object_do_not_keep_a_player_in`, `the_result_does_not_depend_on_the_order_things_were_made_in` (sf_sim). The pinned scenario does **not** catch it: the script kills the opponent's harvester too. |
| 2 | the result may change after it is decided | `once_decided_the_result_never_changes` |
| 3 | section `Z` written without match rules | `a_world_without_match_rules_has_no_outcome_and_no_section`, `golden.rs` `small_scenario_checkpoints` and `tiny_scenario_history`, 20 of 21 tests of `pinned.rs` |
| 4 | `despawn` does not flag the match | `the_human_wins_in_the_tick_the_last_of_the_opponent_falls`, `nothing_changes_after_the_outcome`, `the_final_checksum_is_the_pinned_value`, `a_structure_still_going_up_does_not_keep_its_owner_in_the_match` |
| 5 | the match begins before everyone owns something | `nobody_is_defeated_before_every_player_has_something`, `a_world_in_which_a_player_never_owns_anything_has_no_result` |
| 6 | a structure under construction counts | `a_structure_still_going_up_does_not_keep_its_owner_in_the_match` (`match_m1.rs`) |
| 7 | the win is recorded one tick late | `the_human_wins_in_the_tick_the_last_of_the_opponent_falls`, `the_final_checksum_is_the_pinned_value` |
| 8 | faction bits swapped in `match_factions` | `each_faction_and_credit_arrives_with_its_own_player` |
| 9 | every starting object sent for player 0 | `a_structure_is_placed_and_a_unit_is_spawned_for_its_owner_in_the_order_of_the_file` |
| 10 | winner and loser swapped on the banner | `the_standing_is_the_local_players`, `there_is_no_banner_before_the_result` |
| 11 | the snapshot does not carry the result | `the_match_is_won_in_the_world_the_game_makes` |
| 12 | the clock counts ticks as tenths of a second | `the_clock_counts_game_seconds_at_twenty_ticks_to_the_second`, `the_words_come_from_the_look` |

**Mutations of WP7.7**, each applied in a scratch copy and reverted (the file restored from the commit):

| # | Mutation | Tests that fail |
|---|---|---|
| A | the outcome keeps the old idea: any standing structure keeps a player in | 11 of 18 in `outcome.rs`, among them `only_a_standing_construction_centre_keeps_a_player_in_the_match` |
| B | a construction centre under construction counts (`has_hit_points` for `is_standing`) | `a_construction_centre_under_construction_does_not_count_until_it_is_complete`, `selling_the_construction_centre_defeats_the_player_at_once` |
| C | a match file without a construction centre is accepted | `a_player_without_a_construction_center_is_a_load_error` |
| D | `acquire_needs_sight: false` in `vision.ron` (gate default off) | `the_shipped_number_is_pinned` (sf_data), `the_gate_flag_reaches_the_rules_and_is_on_in_the_shipped_file` (sf_bridge) |
| E | the edge test replaced by the old cell test | 4 of 6 in `sight_gate.rs`; in `match_m1.rs` 8 tests (pin, win tick 6238, tower fires at 3830, ...) |
| F | the start of a sale does not flag the match | `selling_the_construction_centre_defeats_the_player_at_once` |
| G | `with_match` accepts a building table without a construction centre | `match_rules_need_a_construction_centre_kind_in_the_building_table` |

Not covered by a test: the one line in `Hud::frame` that calls `match_view::draw_result` and the line in the
runner that copies the snapshot's result into `HudStats` (both need the window's `Context`); the screenshot
test calls the same `draw_result` and the snapshot test the same `match_result`.

## Measured

Release, same machine (4 cores, shared with other jobs: load average 1 to 4 during the runs), alternating runs.

* **A world without match rules** (the cost of the hook in every other world): the scenarios `production`, `harvest`
  and `wander_5000` on the base (`origin/wp/6.5-production`) and on this branch, 6 to 8 alternating runs each, ticks per second,
  medians: production 6817 / 7628, harvest 8600 / 8625, wander_5000 853 / 840 (a first series), and in a three-way
  series with the end-of-tick call switched off: 846 / 842 / 851. The spread between runs (about 3 percent) is as large as
  every difference: no cost is shown, and none is claimed. The hook is one test of an `Option` per tick
  (`evaluate_match` is `#[inline]` and the work is in a function of its own, which removed a 1.5 percent dip that an
  earlier layout of the function showed against the version with the call removed).
* **A world with match rules**: the `match` scenario (25 entities, 7000 ticks) with and without the rules, 7 alternating
  runs, median microseconds per tick: 27.18 with, 27.20 without (`cost_of_the_outcome`, `--ignored`).
* **The worst case of the scan**: 40,000 unarmed units, the only thing each player owns that counts at the highest slots,
  one unit removed in every tick, so every tick walks all slots (`worst_case_cost`, `--ignored`): median 7073 microseconds
  per tick with the rules, 7008 without (WP7.6). Re-measured in WP7.7 (the scan now looks up the building table per candidate): 5079 with, 4983 without, about 2 percent, within the spread of the runs (4817 to 5096 without, 5050 to 5359 with), load average 7 on a shared container.
* **WP7.7, the whole match:** `cost_of_the_outcome` 19.93 with, 20.11 microseconds per tick without (median of 7). The edge test of the gate, release, alternating, 7 rounds, ticks per second, `tanks_vs_infantry` and a copy of it with the gate on: main (cell test) 135,000 and 121,700 (medians), this branch 134,000 and 121,400; the spread (50,000 to 138,000, load average 8 on a shared container) is much larger than any difference. No cost is shown and none is claimed.


## What is not done

* No AI (phase 9) and no triggers or scenario format (phase 10): the match file and the fixed rule are
  absorbed by WP10.1 to WP10.3.
* No fog in the rules: WP8.1 added what each player sees ([`vision.md`](vision.md); the game's match turns the rules on, the headless
  `match` scenario does not and keeps its pin) but nothing reads it; the gate on targeting is WP8.2a. *(The sidebar came with WP11.1; the start menu, the pause menu, restart and the way back to the main menu
  with WP11.5a, `menus.md`: "Match" on the start menu plays this file, Restart builds the same match again.)*
* No surrender and no timeout: a match that nobody can win (the human has no way to hurt the opponent) goes
  on.
* One human; the local player is fixed at player 0 (`commands::LOCAL_PLAYER`), as in the sandbox.
* The opponent's harvester gathers credits it never spends.
* A sale cannot be cancelled, so a player that sells its last structure is defeated at once and for good (WP6.6; `repair_sell.md`).
* The Windows and the framerate check of M1 on the reference laptop (PLAN §13) are not made here.
