# The mission file (WP10.1a)

The mission file is the match file of WP7.6 (`match.md`) with what a mission needs on top of it: **goals** per
player, **zones**, **variables**, **teams** and **triggers**. Code: `crates/sf_data/src/mission.rs`
(`MissionDef`). Tests: `crates/sf_data/tests/mission_rules.rs` (every rule), `mission_files.rs` (the files that
exist, and the example), fixture `crates/sf_data/tests/fixtures/mission_example.ron`.

**Where the line falls.** WP10.1a is the format and its validation in `sf_data` and nothing else: **no consumer
reads a `MissionDef` yet**. The simulation, the bridge, the game, the headless tool and the AI still read
`MatchDef` (`match_def.rs`, untouched). WP10.1b puts the goals into `sf_sim::outcome` and makes the bridge build
a world from a mission (and refuse a mission with triggers or teams until WP10.2: no silently dead triggers);
WP10.1c moves the other consumers over and removes `MatchDef`; WP10.2 runs events, conditions and variables;
WP10.3 runs actions (including `Win`/`Lose` into the same outcome). Until then the example mission is only
parsed and checked.

**Provisional.** The vocabularies of `Event`, `Condition`, `Action` and `TeamOrder` are the lists PLAN names for
WP10.2 and WP10.3, fixed now so that the editor (WP10.4) has a target. The runtime may change them. Everything
else (goals, zones, variables, names, the match-file part) is meant to stay.

## A superset of the match file

Every field of a match file keeps its name and meaning; every new field is `#[serde(default)]`. So every file
in `assets/matches/` and every frozen `assets/scenarios/*_defs/match.ron` loads as a mission with the defaults
and `MissionDef::as_match_def()` gives back exactly the `MatchDef` (`every_shipped_and_frozen_match_file_loads_as_a_mission_and_equals_its_match_def`,
field by field: 3 shipped and 11 frozen files, counted so that a vanished file fails the test). No file moves
in this WP. Added: `name: Some("hq")` on an object (an `Option`, so RON wants `Some(..)`), and per player
`goals`; top level `zones`, `variables`, `teams`, `triggers`.

## Goals

Per player, `goals: (defeat: [...], victory: [...])`; a list left out is its default. The default is today's
rule: `defeat: [NoStanding(function: Anchor)]`, `victory: [LastInMatch]` (`Goals::default()`, pinned by
`a_bare_match_file_gets_the_default_goals_and_nothing_else`).

| Goal | Means |
|---|---|
| `NoStanding(function: F)` | the player owns no standing structure with the building function `F` (`Anchor` is the construction centre) |
| `NoStandingRoles(roles: [..])` | the player owns no standing structure of any of these roles |
| `NothingLeft` | no standing structure and no armed unit (the rule before WP7.7) |
| `LastInMatch` | every other player is defeated; a victory goal only |

The memo wrote `NoStanding(function)` and `NoStanding(roles)` as one variant; serde cannot have one variant
with two shapes, so the roles form is `NoStandingRoles`.

* **`defeat`: any goal holding defeats the player. `victory`: all must hold**, and an empty `victory` never
  wins by the state of the world (it is not vacuously true); an empty `defeat` never loses by it (a trigger can
  still end a player). Pinned by `defeat_is_any_and_victory_is_all` (`Goals::is_defeated`, `Goals::has_won`,
  which WP10.1b will call).
* `LastInMatch` in `defeat` is refused (it would defeat the winner); `NoStandingRoles` needs at least one
  role, each a plain name.
* **Canonical form.** The order of a goal list means nothing, so on reading both lists are sorted and
  de-duplicated (roles inside `NoStandingRoles` too); this is the form a hash or a comparison sees
  (`goal_lists_are_sorted_and_without_doubles_and_triggers_keep_their_order`). Players, objects, teams and
  triggers keep **file order**: players and objects give slots, triggers are evaluated in that order.
* `check` (with the unit definitions): a role in a goal must exist and be a structure; and **a player must
  start with what its own defeat goals ask for**: `NoStanding(function: F)` needs a start structure with that
  function, `NoStandingRoles` needs one of the roles, else the player would be defeated before it began. With
  the default goals that is the old rule "every player starts with a construction centre", with the old
  message. A player whose defeat goals are something else, or empty, needs no construction centre
  (`a_player_must_start_with_what_its_defeat_goal_asks_for`).

## Zones, variables, teams, triggers

* `zones: [(name, min: (x, y), max: (x, y))]`: integer cells, both corners included, `min` not above `max`;
  `check` puts `max` on the map.
* `variables: [(name, start)]`: integers from -1,000,000 to 1,000,000 (`start` defaults to 0).
* `teams: [(name, owner, members: [(role, count)], orders: [..])]`: a **scripted group**, not an alliance
  (nothing changes who is hostile to whom; alliances would be their own sim work package). 1 to 32 member
  lines of 1 to 64 each, at most 32 orders: `MoveTo(zone)`, `AttackMove(zone)`, `Guard(ticks)`,
  `Loop(to)` (to a step at or before itself), `SetVariable(variable, value)`. `check`: each role is infantry
  or a vehicle and open to the owner's faction.
* `triggers: [(name, owner, lifetime: Once | Persistent, events, conditions, actions)]`: it fires when **any**
  event happens and **all** conditions hold. 1 to 16 events, 0 to 16 conditions, 1 to 16 actions.
  Events: `Always`, `Time(ticks)`, `EnterZone(zone)`, `Destroyed(object)`, `AllDestroyed(player)`,
  `MoneyAbove(player, credits)`, `Built(player, role)`, `PowerShortage(player)`. Conditions: `VariableIs`,
  `VariableAtLeast`, `ObjectStanding(object)`, `TeamAlive(team)`, `MoneyAtLeast(player, credits)`. Actions:
  `Win`, `Lose`, `Message(text)`, `SpawnTeam(team, zone)`, `DisbandTeam`, `SetVariable`, `AddVariable`,
  `RevealZone`, `CameraTo`, `Destroy(object)`, `ForceTrigger(trigger)`. All provisional.
* Counts: at most 64 zones, 64 variables, 64 teams, 256 triggers; ticks 1 to 1,000,000; messages 1 to 200
  characters. No floats anywhere: cells, ticks and counts are integers.

## Validation

`MissionDef::from_ron` checks what the file alone can say; the error names the place and the field
(`triggers[0] "tr" events[0]: zone "nowhere" is not among the zones`). Every rule has its own test:

| Rule | Test |
|---|---|
| a zone / team / variable / object / trigger / player that is named but does not exist | `a_dangling_{zone,team,variable,object,trigger,player}_reference_is_refused` |
| duplicate names (zones, variables, teams, triggers, object names across all players, player ids) | `duplicate_names_are_refused` |
| a name that is not 1 to 32 lower-case letters, digits or `_` | `a_name_must_be_a_plain_name` |
| a range outside its bound (zone corners, variable and value, ticks, credits, counts, message length, loop target, list sizes) | `ranges_outside_their_bounds_are_refused`; the edges are accepted in `the_edges_of_every_range_are_accepted` |
| `LastInMatch` as a defeat goal, empty or bad roles | `last_in_match_is_a_victory_goal_only`, `a_goal_with_empty_or_badly_named_roles_is_refused` |
| the match-file rules (name, map, 2 to 8 players, no Neutral, credits, AI profile, unknown fields) | `unknown_fields_and_the_match_file_checks_still_apply` |

`MissionDef::check(defs, map_cells)` is `MatchDef::check` plus: roles in goals, teams and `Built` events exist
(`an_unknown_role_is_refused_by_check_naming_the_field`), zones lie on the map
(`a_zone_outside_the_map_is_refused_by_check`), and the goal-based start rule above. The three shipped match
files pass both checks (`the_shipped_match_files_pass_the_mission_check_like_the_match_check`).

## Not done

No runtime of any kind (see the line above). `match_def.rs`, `sf_sim`, `sf_bridge`, `sf_game`, `sf_ai`,
`sf_headless`, CI and every `.expected` are untouched. Start objects have no extras (hit points, facing: they
need new sim commands). The vocabulary is untested against a runtime. A trigger's `owner` is checked as a
player but its meaning ("units of the owner" in `EnterZone`) is WP10.2's to fix.
