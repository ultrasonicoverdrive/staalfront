# The computer opponent

WP9.1a (the frame). Design memo of 10-11-2026 (orchestrator); Tijmen: "fase 9 mag eerder beginnen". WP9.1a builds what every later
part stands on: the AI **observes through the fog, remembers what it saw and keeps its schedule, and sends no command yet**. The
rules (build order, placement, economy, production, attack timing, defence) are WP9.1b and WP9.1c and are **not built**; this page
says what exists and what is a stub.

Code: `crates/sf_ai/src/` (`observe.rs`, `memory.rs`, `state.rs`, `schedule.rs`, `profile.rs`, `sites.rs`, `ai.rs`), the profile format
`crates/sf_data/src/ai_profile.rs`, the data `assets/ai/normal.ron`, the driver `tools/sf_headless/src/run.rs` (`run_full`), the scenario
`assets/scenarios/ai_fog.ron`. Tests: `crates/sf_ai/tests/fence.rs`, the unit tests in each module, `tools/sf_headless/tests/ai_fog.rs`,
`ai_perf.rs`.

## Where it lives: outside the simulation, to the simulation's rules

The AI is a **player outside `sf_sim`**: a pure function of (the observation at tick `t`, its own state, its seed). It acts only by
scheduling the commands a human can send, so its commands are in the command queue and the world checksum like any other, and it can do
nothing a human cannot. No file of `crates/sf_sim/` changed for it. It follows the simulation's determinism rules: no floats, no
`HashMap`/`HashSet`, no clock, no I/O, randomness only from `sf_math::Rng` (stream `AI_STREAM + player`, seeded by the `seed` of its
`ai` entry). Enforced three ways: `scripts/check-sim-determinism.sh` scans `crates/sf_ai/src` for float and hash-collection tokens and
also for `Instant`, `SystemTime` and `std::fs/io/env/time/thread/process` (the host reads the profile file and hands it over); the float
lints of `crates/sf_ai/Cargo.toml` (`float_arithmetic`, `float_cmp`, `float_cmp_const`, `cast_precision_loss`); `crates/sf_ai/clippy.toml`
(a copy of the one of `sf_sim`: `HashMap` and `HashSet` are disallowed types).

The AI state is **not** in the world checksum, so `Ai::digest()` hashes it (player, random state, profile, start sites, think count, last
think, manager counts, every remembered structure in id order) and `sf_headless` prints it beside the checksum (`ai 0: 0x...`) and writes
it into the log (`"ai": [{"player": 0, "digest": "0x..."}]`, only for a scenario with `ai`). CI compares the logs of Linux and Windows
except the `host` block, so the digest is compared too. `--expect` checks only the world checksum; the AI digest is pinned in
`tests/ai_fog.rs` and compared across platforms by that log comparison. The state is plain data (integers, vectors, `BTreeMap`), ready to be saved
(WP13.1).

## The fence: no cheating on information

`sf_ai::observe` is the **only file of the crate that names `sf_sim::World`**. It builds an `Observation` and everything else takes only
that. `crates/sf_ai/tests/fence.rs` reads the files of `src/` and fails when another file contains the word `World`, the path
`sf_sim::world` or a glob import of `sf_sim`.

| In the observation | Rule |
|---|---|
| own entities | in full: id, kind, cell (a unit: the cell its position is in; a structure: its anchor), footprint, hit points, order, `standing` (a structure complete and with hit points left) |
| credits, ore store and capacity, power made and used | a player knows its own |
| entities of others | **only when their cell is `Visible`** for this player (`World::visibility`, which includes shared vision): a unit by the cell its position is in, a structure by **any** cell of its footprint; the same cell test as the snapshot's `seen` (`vision.md`, "For the player's side") |
| nothing else of an unseen entity | not its existence, hit points or order |
| `gone` | a remembered structure with a visible cell that is not among the visible entities. Decided from the picture alone; it never asks whether the entity is alive, which would tell the AI about structures it cannot see |

**Entity ids are opaque handles.** An id (in `Seen`, `Known` or the AI's own entities) is a slot index plus generation, which hints at how
many entities the world holds; no rule may reason from its value, only compare and look it up.

A world without vision rules shows everything to everyone, as it does for a human. **Not in the observation yet** (a later part asks when
a rule needs it): the public tables (unit, building and production data, terrain), the ore on seen or explored cells, the explored bit.

**Memory.** `AiState::known_structures: BTreeMap<EntityId, Known { kind, owner, anchor, width, height, seen_tick }>` is written at every
think for each visible structure of another player and an entry is dropped when the observation says it is gone (the ghost rule of
`fog_look.md`, on the AI's side). Units are not remembered. A structure that was visible only between two thinks is missed (half a
second). The AI is also given **map knowledge**: `start_sites`, the cell of every player's construction centre from the match file
(`sf_ai::start_sites`), as a human who knows the map would know them. It is the only thing it learns about others without seeing.

## The schedule

An AI thinks when `(tick + stagger * player) % think_interval == 0` (`schedule::due`; `tick` is the number of ticks the world has run, the
tick the next step runs). Shipped: interval 10 (half a second), stagger 3. So two players never think on the same tick: the profile
check requires `think_interval / gcd(think_interval, stagger) >= 8`, the most players of a match (player 0 thinks at tick % 10 == 0,
player 1 at 7, player 2 at 4, ...). Each think runs the observation, the memory update and **one** manager in rotation, so each of the
four managers (`Economy`, `Construction`, `Production`, `Army`, numbered 0 to 3, which are hashed) runs every 40 ticks. **The managers are
stubs in WP9.1a**: a manager's turn only counts (`manager_runs`); economy and construction are WP9.1b, production and army WP9.1c.

**The driver.** `Ai::due(tick)` and `Ai::think(&World, &mut Vec<Command>)`, called by the host immediately before `World::step`; the host
schedules the commands for `world.tick()` under the AI's player id, AIs in ascending player id. In `sf_headless` that is `run::run_full`
(`run` is a wrapper). The game's sim thread does not call it yet (WP9.1d).

## The profile (data)

`assets/ai/normal.ron`, read by `sf_data::ai_profile` (`AiProfile::from_ron`, then `check` against the unit definitions) and turned into
kinds for one faction by `sf_ai::Profile::resolve`. One file holds the numbers of the schedule, the economy and the army, and a section per
faction:

* `build_order`: a **goal list** `[(role, count)]` meaning "have at least `count` of `role`". The AI will walk it from the top and build the
  first entry it does not satisfy (WP9.1b), which gives the opening, the tech choice and rebuilding with one rule. Checked: every role
  exists and is open to the faction, is a buildable structure (not the anchor), every `requires` of an entry is earlier in the list
  (the anchor counts as always there), a role that comes again asks for more than before, counts 1 to 32.
* `unit_mix`: `[(role, weight)]`, the share of the army each role aims at. Checked: infantry or vehicle (no vessel, no harvester: the
  economy makes those), open to the faction, has a cost, listed once, weight 1 to 1000, the factory of its category and everything its
  `requires` names are in the build list.
* Numbers: `think_interval`, `stagger`; `economy` (`power_margin`, `harvesters_per_refinery`, `reserve`, `max_parallel_builds`,
  `silo_percent`, `repair_below_percent`, `blocked_ticks`, `placement_cap`); `army` (`attack_value`, `max_wait`, `min_units`,
  `first_attack_tick`, `defend_ticks`, `gather_distance`). Every one is range-checked when the file is read; the values are placeholders of the memo.
  **Only `think_interval` and `stagger` act in WP9.1a**; the others are checked and carried, and used from WP9.1b and WP9.1c.

The shipped Meridian list: power_plant 1, refinery 1, barracks 1, power_plant 2, vehicle_factory 1, defence_tower 1, refinery 2, radar 1,
defence_tower 2, advanced_defences 1; Veyran adds fortified_bunker 1 after the first tower. The ore silo is a rule of the economy
(`silo_percent`), not a list entry. The shipyard and vessels are out of 9.1. All pinned by `the_shipped_profile_is_pinned` (literals) and,
as kinds, by `the_roles_of_the_shipped_profile_become_the_kinds_of_the_shipped_definitions`.

## In a headless scenario

`ai: [(player: 0, profile: "normal.ron", seed: 0xA1_0001)]` (the field table in `tools/sf_headless/src/lib.rs`). The profile is a file in the
`unit_defs` directory (a **frozen copy**, so the shipped profile or definitions can change without moving a pin), the faction and start sites
come from `match_file`, so both are needed; the problems are reported when the scenario loads. A scenario with `game` takes no `ai`.

**`ai_fog`** (`assets/scenarios/ai_fog.ron`, `ai_fog_defs/`): player 0 (Meridian, the AI) has a construction centre and a scout; player 1
(Veyran) is static with a construction centre, a power plant, a barracks and a refinery, and an army in the far corner. The scout walks to
the power plant and barracks, the barracks is removed at tick 500, the scout walks home at 520. Pinned: final checksum `0xd369218db6025eff`
(`ai_fog.expected`), AI digest `0x993afc7cd2c42cfd` (a literal in `tests/ai_fog.rs`), 800 ticks. The tests assert: the AI knows exactly the
two structures the scout saw (kind, anchor, footprint, tick) and not the centre, refinery or army; it forgets the removed barracks and keeps
the power plant after the scout leaves, also when it is destroyed out of sight at tick 700 (same memory and digest as without); without the scout it knows nothing; the observation at six ticks never holds a shrouded entity;
**an invisible army changes nothing the AI sees or does** (same observation, memory and digest as a world without the army, while the world
checksums differ) and the positive control (a unit inside the scout's sight is seen); the AI thinks 80 times in 800 ticks, manager runs
`[20, 20, 20, 20]`; two AIs think at tick % 10 == 0 and == 7; the AI sends no command (every executed or ignored command was scheduled by the scenario).

## Cost

Measured by `tools/sf_headless/tests/ai_perf.rs` (release, the container of this PR, load average about 2, three runs): in `ai_fog` the mean is
0.05 to 0.07 microseconds per AI per tick (worst think 1.4 to 14 us); with 5,000 wanderers of player 1 over the map (every one a cell test) the mean is
6.2 to 6.6 us per AI per tick (62 to 66 us per think, worst think 101 to 113 us). The memo's budget: mean at most 20 us per AI per tick, worst at most
1 ms. A whole run of that crowd, alternating, medians of three: 633 ticks/s with the AI against 623 without (noise). The assertions are loose (ten times the
budget, release only); the numbers are the evidence. The observation walks every entity once per think; a map with far more entities
than 5,000 costs proportionally more, and the managers of WP9.1b and WP9.1c add to this.

## Not done

* **No rule.** No building, no production, no attack, no defence, no scouting, no command of any kind. The managers are stubs.
* **No test that the AI emits only the allowed commands** (`BuildStructure`, `QueueUnit`, `SetRally`, `Repair`, `Order Move/AttackMove`):
  there is nothing to emit yet; WP9.1b adds it with the first command.
* The public tables, terrain and the ore of seen or explored cells are not in the observation yet.
* The game does not run an AI (WP9.1d). `sf_game --match` is unchanged and its opponent is static.
* Difficulty levels (WP9.4), the "unfair" level and its two cheats, AI-vs-AI (WP9.5).
* The AI state is plain data but is not saved anywhere (WP13.1).
* A shuffled spawn order test (the iteration-order check of the memo) is not built: the AI iterates `BTreeMap`s and ascending vectors only, and the
  determinism script forbids hash collections, but no test shuffles.
