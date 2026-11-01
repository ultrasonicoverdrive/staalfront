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
once) asks one question of every player at the end of every tick in which something left the world:

> Does this player own **a standing structure** or **an armed unit**?

A player for whom the answer is no is **defeated**, in that tick, for good. When all players but one are
defeated, the last one **wins**, in that tick, for good. If the last players are defeated in the *same*
tick nobody wins: the result is a **draw**. Once the result is decided nothing in the outcome changes any
more, whatever happens in the world afterwards (the defeated player's harvester may still be driving, a
loser may even make a new unit: the result stays).

| What the player owns | Counts? | Why |
|---|---|---|
| a structure that stands (`World::stands`: complete, hit points left) | yes | it is a base. |
| a structure under construction | no | there is nothing yet to lose; and it cannot be built without a standing one (`construction.md`). |
| a mobile unit whose kind carries a weapon in the weapon table, with hit points left | yes | it can still fight. |
| a mobile unit without a weapon (a **harvester**, a truck, an engineer) | **no** | it cannot fight, and it cannot build: building needs a standing structure of one's own, so a player with only harvesters can do nothing that changes the match. Counting it would let a player hide a harvester in a corner and keep a lost match going. |
| an object (a crate, an ore field) | no | not a thing a player owns in play. |
| anything of a player who does not take part | not looked at | the neutral roles of a scenario are nobody's business. |

**The match begins** at the first check at which *every* player owns something that counts. Until then
nobody can be defeated. The starting objects of a match enter the world through commands, and a world
whose second player appears at tick 5 must not end at tick 0. (A world in which some player never owns
anything has no result: `a_world_in_which_a_player_never_owns_anything_has_no_result`.)

**When it is decided.** After the weapon stage and the removal of the dead (`destruction.md`), in the same
tick: the result carries the tick of the last death (`MatchResult::tick`, equal to the defeat tick of the
last defeated player; they are recorded separately, `World::defeat_tick` and `World::match_result`).
Defeats are kept in the order they happened: ascending tick, ascending player within a tick.

**What a world needs.** `with_match` must come last, after the unit table, the weapon table and the
destruction settings (`ConfigError::Match` otherwise, also after the start and twice). Without weapons
nobody is armed; without destruction nothing is ever removed, so the question would never be asked.

## Cost

The question is not asked every tick. `World::despawn`, the one place an entity leaves, sets a flag;
the end of the tick looks at the flag. Once the match has begun only a removal can change the answer: a
structure that stops standing and a unit that stops having hit points are removed in the same tick
(destruction is required), and the weapon table never changes. A tick without a removal costs one flag
test. A tick with one scans the slots in ascending order and **stops as soon as every player still in the
match has been found once** (usually within the first slots: the starting objects of both sides are the
lowest slots), so the scan is not a pass over all entities per tick. Before the match has begun the scan
runs every tick (for the first tick or two). Measured: see "Measured" below.

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
that lies (with its footprint) outside the map and two structures that overlap. Whether the ground is
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
| 1, Veyran (the opponent) | a power plant, a refinery, a barracks, a defence tower, a harvester, four riflemen and a light tank around (62..82, 14..31); no credits. |

## The static opponent

The opponent **does not build, produce or attack**. There is no AI before phase 9, and nothing in this WP
gives the opponent a command. Its units stand where they were put and fire back on their own: the
return fire and target choice of WP5.4 (`targeting.md`), with its pursuit distance of 6 cells, so a unit
goes no farther from where it stood (`the_opponent_stays_put_fires_back_and_never_builds_or_produces`
measures it). Its **harvester does mine**: the harvest loop of WP7.2 is the unit's own state machine, not
an order, and a base without it would look dead; the credits it brings (the refinery fills to its capacity
and the harvester then waits) are never spent. To win, the human destroys its four structures *and* its
four riflemen and tank; the harvester does not count.

## The game mode

`sf_game --match [FILE]` (default `assets/matches/m1.ron`) starts the match scene: the map, the economy
(`--harvest`'s rules: harvest, storage, money, production), combat and the match rules, the starting
objects of the file, the camera on the human's first object, and the sandbox's controls: build with B
and the mouse, produce with the keys of `production.md`, order, attack. `--map` is ignored (the file names
the map). A file the game cannot play (a role that is not there, a seat whose faction is not the one the
game draws that seat as, a missing local player, a refused object) stops the start with the reason.

**The result banner** (`match_view.rs`, `match_look.ron`): at the top middle, "Victory" when the local
player won, "Defeat" when someone else did, "Draw" when nobody, and under it the game time and the
tick ("decided after 4:44 (tick 5687)"). **The game keeps running behind it**: the result never changes,
there is no menu to go to before the sidebar and menus of phase 11, and a player may carry on, for
example to destroy the rest. Every word, number and colour is in `match_look.ron`.

## The headless scenario `match`

`assets/scenarios/match.ron`, on a frozen copy of the definitions and of the match file
(`assets/scenarios/match_defs/`; the economy numbers are written out in the scenario), scripts the human:
a power plant (done at tick 301), a barracks (705) and a refinery (1105; it brings a free harvester),
four riflemen and five rocket troopers (the credits and then the ore pay for them), an attack-move at tick
2400 that arrives at the opponent's base, and two attack-moves deeper into it. **The first fight is at
tick 3689; the opponent's last structure falls at tick 5687, and that is the defeat tick of player 1 and
the win tick of player 0.** The scenario runs to tick 7000. `tools/sf_headless/tests/match_m1.rs` pins the
checksum and asserts all of the above (see "Tests").

A scenario can name a `match_file` (a file in its `unit_defs` directory): it then gets the match rules,
the factions, the credits and the starting objects from the file, and `factions` and `money.starting`
must be left out.

## Tests and mutations

| Test | What it shows |
|---|---|
| `crates/sf_sim/tests/outcome.rs` (14) | the rules are refused when wrong; a structure or an armed unit keeps a player in, a harvester and an object do not; the last player standing wins in the tick of the last defeat; a simultaneous end is a draw; three players; nobody is defeated before everyone has something; a world with a player who never has anything has no result; once decided never changes; a structure shot down defeats its owner in the tick it died; independent of spawn order; the defeats and result are in the checksum; a world without match rules has no section. |
| `tools/sf_headless/tests/match_m1.rs` (9) | the pinned checksum; the starting objects in the order of the file; the win tick 5687 and nothing decided before it; nothing changes in the 1,313 ticks after; the same game without the rules differs only by the section; the opponent stays put, fires back and never gains an entity; the harvest pays; the committed map is what the generator draws, and its places and counts. |
| `crates/sf_data` (`match_def`, `match_look`), `crates/sf_bridge/tests/match_def.rs` | the file's shape and checks; the shipped file pinned as literals; each number arrives in the world as itself (distinct values). |
| `crates/sf_game` (`match_view`, `args`, `tests/match_screenshot.rs`) | the standing from the winner, the clock, the words from the look; the flag; the real renderer. |

MUTATIONS_PLACEHOLDER

## Measured

MEASURED_PLACEHOLDER

## What is not done

* No AI (phase 9) and no triggers or scenario format (phase 10): the match file and the fixed rule are
  absorbed by WP10.1 to WP10.3.
* No fog (WP8.1), no sidebar (WP11.1), no menu: the banner is all the game shows, and there is no restart,
  no pause and no way back to a menu; close the window.
* No surrender and no timeout: a match that nobody can win (the human has no way to hurt the opponent) goes
  on.
* One human; the local player is fixed at player 0 (`commands::LOCAL_PLAYER`), as in the sandbox.
* The opponent's harvester gathers credits it never spends.
* The Windows and the framerate check of M1 on the reference laptop (PLAN §13) are not made here.
