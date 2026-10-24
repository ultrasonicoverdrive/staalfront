//! `sf_headless`: runs the simulation without a screen.
//!
//! It loads a scenario file, runs the deterministic `sf_sim` world for N ticks
//! as fast as it can, and writes the checksums and a JSON log. It is how agents
//! test gameplay without a window or a GPU, how CI proves that Linux and
//! Windows compute bit-identical worlds (milestone M0: 10,000 ticks with 5,000
//! wanderers), and later the base for balance runs and AI-vs-AI games.
//!
//! The crate runs on `sf_sim` and `sf_math` (plus `ron` and `serde` for the
//! scenario file, and `sf_data` to read the map a scenario names, WP2.1;
//! `sf_ai` is listed for later work and unused today); it must never depend
//! on rendering, windowing or audio crates. The clock is read in exactly one
//! place, [`cli::run_cli`], around the run; [`run::run`] is a pure function
//! of the scenario and the options. Files are read only by
//! [`Scenario::load`](scenario::Scenario::load), never while a run is going.
//!
//! ```text
//! cargo run --release -p sf_headless -- assets/scenarios/wander_5000.ron \
//!     --every 100 --out target/wander_5000.json \
//!     --expect "$(cat assets/scenarios/wander_5000.expected)"
//! ```
//!
//! Options and exit codes are in [`cli`], the log layout in [`log`].
//!
//! # The scenario file
//!
//! A RON file with one `Scenario(...)`. Unknown fields are errors, as are
//! scenarios that cannot run (`ticks: 0`, a command after the last tick, an
//! empty or out-of-map area, ...); every problem is reported at once, with the
//! file name (see [`Scenario::problems`] for the rules).
//!
//! | Field | Meaning |
//! |---|---|
//! | `name` | Name in logs; the file name without extension when left out. |
//! | `seed` | World seed (decimal or `0x` hex, `_` allowed). Also seeds the placement stream that draws spawn positions. |
//! | `map` | `(width: .., height: ..)` in cells, 1 to 1024; 256 by 256 when left out. |
//! | `map_name` | `Some("playfield256")`: the world is made on the terrain layer of `assets/maps/<name>/` (next to the scenario's directory); its size in cells must equal `map`. Without it every cell is open ground. |
//! | `unit_defs` | `Some("units_spawn_defs")`: the world is made with the unit definitions in `assets/scenarios/<name>/` (`unit_classes.ron`, `weapon_defs.ron`, `unit_defs.ron`; a frozen copy, so balance changes do not move a pinned checksum) as its unit table, which `SpawnUnit` and `PlaceStructure` need (WP3.2). Read by `Scenario::load`, like `map_name`. |
//! | `path_budget` | `Some(2048)`: the nodes of path search work the world may do in one tick (WP4.2, `docs/rules/pathfinding.md`); the simulation's default when left out, at least `MIN_NODE_BUDGET`. It decides when a unit gets its route, never which. |
//! | `flowfield_threshold` | `Some(8)`: how many units of one movement class a single order must send to one place for the world to build a flowfield for them instead of routing each (WP4.3, `docs/rules/pathfinding.md`); the simulation's default (the number of `sim.ron`) when left out. `Some(0)` turns flowfields off; `Some(1)` is refused. |
//! | `collision` | `Some(false)`: bodies do not collide, units pass through each other as before WP4.4 (`docs/rules/collision.md`); on (the simulation's default) when left out. For a scenario that pins routes or terrain costs with exact positions and has units that start side by side or share a goal. |
//! | `formation` | `Some(false)`: a group move order does not form a formation, the group moves as a group order did before WP4.6 (`docs/rules/formations.md`); on (the simulation's default) when left out. For a scenario that pins routes, arrival or turning with exact positions and sends several units across the same ground to one place. |
//! | `formation_max_units` | The most units one move order may send and still make formations; the simulation's default (`sim.ron`) when left out. For probes above the shipped limit (`tests/formation_sweep.rs`). |
//! | `combat` | `true`: the world is given the weapon table of the `unit_defs` directory (WP5.1, `docs/rules/weapons.md`) and a unit with an attack order fires at its victim; off (nothing fires) when left out, which keeps every scenario of the earlier phases, some of them with attack orders, as it was pinned. The table is built only for a scenario with `combat` (WP5.2: a frozen copy of the definitions that predates a rule the table has to meet still loads for the scenarios that never fire), and a weapon that flies then flies as a projectile (`docs/rules/projectiles.md`). Since WP5.3 the table carries the armor table of the definitions, when they have one, and a hit goes through it and through the weapon's splash (`docs/rules/armour.md`); a frozen copy without an `armor_table` takes every hit at the weapon's damage. Needs `unit_defs`. |
//! | `targeting` | `true`: the units choose their own targets and fire back (WP5.4, `docs/rules/targeting.md`) with the simulation's own settings (the numbers of `sim.ron`); off when left out, which keeps the combat scenarios of WP5.1 to WP5.3, whose victims stand still, as they were pinned. Needs `combat`. |
//! | `ticks` | Length of the run, at least 1. |
//! | `spawns` | Groups of entities: `tick` (default 0), `player` (default 0; the issuer, which orders commands within a tick), `count`, `kinds`, `owners` (cycled: entity `i` gets `kinds[i % len]`), `area` (`(min: (x, y), max: (x, y))`, cells, max exclusive). |
//! | `commands` | Timed commands: `tick`, `player` (default 0), `command`. |
//!
//! The commands are the `sf_sim` commands apart from `Spawn`:
//! `SetWanderArea(ids: .., area: ..)`, `MoveTo(ids: .., target: (x, y))`,
//! `Despawn(ids: ..)` and `Place(kind: .., owner: .., cell: (x, y))`, a
//! standing structure that occupies a buildable, free cell (and is ignored
//! otherwise). With `unit_defs` there are two more:
//! `SpawnUnit(kind: .., owner: .., cell: (x, y))`, a unit or object of the
//! definitions in the middle of a cell, and `PlaceStructure(kind: .., owner:
//! .., cell: (x, y))`, a structure of the definitions with its footprint
//! anchored on the cell (all of its cells must be buildable and free).
//! Both take the next free id like a spawn and are ignored for an unknown
//! kind or the wrong sort of kind. The orders of WP3.4 (`docs/rules/orders.md`)
//! are `OrderMove`, `OrderAttackMove` and `OrderGuardPoint` (`ids: .., target: (x, y),
//! queued: ..`, the middle of the cell), `OrderAttack` and `OrderGuardEntity` (`target:
//! (index, generation)`), `OrderScatter(ids: .., queued: ..)`, `OrderHold(ids: .., queued: ..)` and `Stop(ids: ..)`;
//! `queued` (default `false`) appends to the unit's queue instead of replacing its
//! orders. `AimTurret(ids: .., target: Some((index, generation)))` (WP4.5,
//! `docs/rules/turning.md`) points the turrets of the units at an entity, and without a
//! target (`AimTurret(ids: ..)`) lets them go back to the hull. Entities are named by id: `Id(index, generation)` or
//! `Range(from, to)` (slots `from..to` at generation 0; in a world without
//! despawns the entities spawned `from`-th to `to`-th, in execution order).
//!
//! ```
//! use sf_headless::run::{RunOptions, run};
//! use sf_headless::scenario::Scenario;
//!
//! let scenario = Scenario::from_ron_str(r#"
//!     Scenario(
//!         name: "tiny",
//!         seed: 0x57A1_F407,
//!         map: (width: 64, height: 64),
//!         ticks: 200,
//!         spawns: [
//!             // 100 wanderers, three kinds and two owners in turn.
//!             (count: 100, kinds: [0, 1, 2], owners: [0, 1],
//!              area: (min: (0, 0), max: (64, 64))),
//!         ],
//!         commands: [
//!             // Ten of them go to the middle at tick 50, player 1 commanding.
//!             (tick: 50, player: 1,
//!              command: MoveTo(ids: [Range(0, 10)], target: (32, 32))),
//!         ],
//!     )
//! "#).unwrap();
//!
//! let outcome = run(&scenario, &RunOptions { ticks: 200, every: 50 }).unwrap();
//! assert_eq!(outcome.entities, 100);
//! assert_eq!(outcome.samples.len(), 4); // after 50, 100, 150 and 200 ticks
//! // Same scenario, same options, same checksums, on every machine.
//! assert_eq!(outcome, run(&scenario, &RunOptions { ticks: 200, every: 50 }).unwrap());
//! ```
//!
//! # Semantics worth knowing
//!
//! * Everything is scheduled **before tick 0**, spawns first (file order), then
//!   commands (file order), as a replay would be. Pending commands are part of
//!   the checksum, so the checksum of a scenario describes the whole script,
//!   not only what has run so far. A command in the past cannot be written
//!   (the world starts at tick 0); one at or after the last tick is rejected
//!   because it would never run.
//! * `--ticks N` replaces the scenario's length for one run. Commands after
//!   tick N simply stay pending.
//! * A checksum "after K ticks" is the one `World::checksum_history` has at
//!   index `K - 1`. The log also holds `initial_checksum` (before tick 0, with
//!   every command still pending) to tell a loading difference from a stepping
//!   difference.
//!
//! # Cross-platform proof (CI)
//!
//! The `headless` job in `.github/workflows/ci.yml` runs the scenarios in
//! `assets/scenarios/` in release mode on Linux and Windows, each with
//! `--expect` of the value committed next to the scenario (`*.expected`) and
//! uploads the logs. The `headless-compare` job then runs
//! `scripts/compare-headless-logs.sh` on both sets: everything except the
//! `host` block must be identical. A drift on one system fails its own
//! `--expect`; a drift between the systems fails the comparison; both name the
//! first checksum that differs.

pub mod cli;
pub mod log;
pub mod run;
pub mod scenario;

pub use run::{Outcome, RunOptions, Sample};
pub use scenario::{Scenario, ScenarioError};
