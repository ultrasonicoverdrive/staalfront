# Construction: a structure goes up before it counts

Work package WP6.3 (PLAN.md phase 6). A structure a player builds with `Command::BuildStructure` is not
finished when it is placed: it is **under construction** for its `build_ticks` (the data's `build_seconds` at 20
Hz, `docs/rules/buildings.md`), and counts for nothing until it is complete. This file is the rule, the order
inside a tick, what is hashed and how the picture follows. Nothing here costs credits (phase 7, WP7.4).

All numbers are invented placeholders (clean-room: nothing comes from another game). The idea that animation
and game state are coupled but not identical, and that the state must be updated in a deliberate order
(`docs/red-alert-analysis.md`, section 10) is the only thing taken from the analysis; the rule below is this
project's own.

## The state

One integer per entity, `Entities::build_left`: the ticks still to go. **Zero is complete**, and is what every
unit, every object and every structure placed with `PlaceStructure` has (a mission's pre-placed structure, the
sandbox, a scenario's start state: `a_pre_placed_structure_starts_complete`). `BuildStructure` sets it to the
structure's `build_ticks` in the tick it places it (`World::apply_from`).

The total is not stored: it is the building table's `build_ticks`, which is already hashed (section `B`).
`World::construction_of(id)` answers `Construction { left, total }` (`permille()` = progress in thousandths,
rounded down, never 1000 while it is going up) or `None` for a complete structure.

A structure under construction **takes its cells at once** (nothing else can be placed on them, the ground
routes round them) and has its full hit points from the start. It is a real entity in the world; if it is
destroyed it is removed like any structure and the state goes with it (`destroying_a_structure_under_construction_drops_its_state`).

## The order inside a tick: the state is the truth, the presentation follows

The analysis asks that animation and mission state are updated in a deliberate order so that the logic reacts to
a transition at the right moment, and that the game never shows a transition long after (or before) the action
that belongs to it. This project reads that as one rule: **the state is the truth, the presentation follows it.**
The simulation decides when a structure is complete; the picture is worked out from the state of a tick and
cannot run ahead of it, and nothing in the simulation waits for the picture.

In `World::step`, the first things that happen are:

1. **The structures under construction advance** by one tick, in ascending slot order
   (`World::advance_construction`). One that reaches zero is complete, and a `Completion { tick, id, kind,
   owner }` is recorded for this tick: `World::completions()` lists them until the next step and
   `StepReport::structures_completed` counts them.
2. **Then the commands of the tick run.** A structure that completed in step 1 already stands for a
   `BuildStructure` of the same tick: it is a prerequisite and a base anchor from that command on
   (`the_command_of_the_completing_tick_sees_the_structure_standing`).
3. The rest of the tick (paths, movement, crowd, targeting, weapons) as before.

A structure placed in the step of tick `t` with `n` build ticks has `n` ticks left after `t` and completes in
the step of tick `t + n` (`a_built_structure_is_complete_after_exactly_its_build_ticks`; in the headless scenario
`construction` the plant placed at tick 1 completes at tick 301, the refinery that needs it is refused at tick
300 and accepted at tick 301).

**Where a mission or rule hooks in.** "Structure completed" is `World::completions()` in the step it is
true, never anything drawn. WP10.x mission triggers listen there. The presentation reads the same state through
the snapshot and is never the source of an event.

## What counts only when complete

`World::stands(id)` (and the internal `is_standing`) is true for a structure that is **complete and has hit
points left** (or cannot be damaged, `hp` 0 in the table). It is what these ask:

| Asker | Rule |
|---|---|
| prerequisites (`requires`, WP6.1) | the structure must stand (`a_structure_under_construction_is_no_prerequisite_until_it_completes`, and on the shipped tree `meridian_builds_the_whole_shipped_tree_with_build_commands`) |
| the base anchor of the build radius (WP6.2) | the anchor must stand (`a_base_under_construction_gives_no_reach`) |
| power (WP6.4), production (WP6.5), repair and sell (WP6.6) | **to use `World::stands`**; nothing reads it yet |

A structure under construction must give no power and produce nothing; those work packages have to call
`stands` and not look at hit points alone.

## A structure at zero hit points does not finish

Until WP5.5 lands (it removes a structure at zero hit points and leaves rubble), a structure shot to zero hit
points stays where it fell. Construction does not advance on it and no completion fires: a wreck must not
"complete" (`a_structure_at_zero_hit_points_does_not_finish_building_and_fires_no_event`). If hit points come
back (repair, WP6.6) it carries on from where it stopped. When WP5.5 despawns it, the state is dropped with the
slot (`Entities::fill` and `clear` reset it).

Interaction with targeting (WP5.4): structures do not choose or take targets yet (WP6.7 for defences), so
nothing in the simulation damages a structure under construction today; the rule above is tested by setting
hit points directly. **Gap:** a structure has its full hit points while it goes up. Whether it should be fragile
at first (hit points rising with progress) is balance for phase 15; it would be one more column, not a
change of the rule.

## The checksum

A section `N` follows section `B`, **written only while some structure is under construction**: the number of
such structures, then for each in slot order the slot and the ticks still to go
(`construction_is_in_the_checksum_and_is_gone_when_it_completes`). A world that never builds with
`BuildStructure`, and a world whose builds are all complete, hash exactly as before; a world built the same
way but placed complete ends on the same checksum as one that was built
(that test compares the two at the end).

No earlier pin moved because of the section: the `golden.rs` constants, `wander_5000`, `command_mix`,
`terrain_256`, `units_spawn` and the others never use `BuildStructure`. The one pin that moved is
`placement.expected`, for a rule this work package changes on purpose (below).

### `placement.expected` re-pinned

Pinned `0x523c7fcc0105b96f` in WP6.2, now `0x9c83a43fc19af879`. Cause, measured: the old scenario with
`BuildStructure` temporarily set to place a structure complete (the WP6.2 rule) still ends on
`0x523c7fcc0105b96f`; the same old scenario file on the WP6.3 rules ends on `0x208d804b162a500b` with 17 of its 23
commands ignored (11 before), because each build that needed a prerequisite found it still going up. The
scenario was rewritten to wait for the builds (2110 ticks instead of 60; the 19 refusals of WP6.2 are the same,
the commands that needed a prerequisite wait for the tick it completes, and three new refusals show a
prerequisite that is only going up). The frozen definitions `placement_defs/` are untouched. A second scenario,
`construction` (`construction.expected`, `0x941d7392ff550040`), is the small one: a barracks placed complete
unlocks a decoy at tick 1, a plant goes up for 300 ticks and a refinery is refused until it is complete.

## The picture

Presentation only, from the snapshot (`SimSnapshot::construction_of`, filled from `World::constructions()` each
tick): nothing in `sf_game` counts a tick or decides a stage.

* **Stage models.** `assets/config/build_stages.ron` (checked when it is parsed, `sf_data::build_stages`):
  the stages in order, each with `from_permille` and a `suffix`; the stage's model is the finished model's file
  with the suffix before `.glb` (`set_v4/meridian_barracks.glb` + `_foundation`). Shipped: **foundation** from
  0, **scaffolding** from 400 thousandths, the finished model from the tick the simulation says the structure
  is complete. The thresholds and the names are data; nothing is in Rust. `Roster` builds one look per stage
  for every structure that can be built, per faction; `Looks::shape_at(view, Some(permille))` picks it.
* **Fallback.** A structure whose stage file does not exist (or that is drawn as a placeholder box) is a box of
  its footprint as high as the stage's `fallback_height_permille` of the finished box, never under 20 cm
  (`a_structure_without_stage_models_is_a_box_as_high_as_the_stage_says`). All 18 shipped looks (eight structures
  for two factions, the decoy and the bunker) have their art set models
  (`every_buildable_structure_has_stage_models_from_the_art_set`).
* **Progress bar.** `construction_view` draws a bar under the front edge of the footprint, filled to the
  progress, on the UI's world layer; none when it would be under 18 logical pixels wide (zoomed out), none when
  the structure is complete. Colours and height are in `build_stages.ron`.
* Every structure model is fitted to the footprint by the loader, so the foundation slab and the scaffolding
  stand where the structure will (seen in the screenshots).

Screenshots with the tick in the name: `docs/screenshots/wp6.3/` (`build_screenshot.rs` writes them with
`SF_SAVE_SHOTS`).

## Not done (gaps)

* **Nothing is charged** and nothing is refunded (phase 7, WP7.4). `BuildStructure` is free; a build cannot be
  cancelled or paused, and there is no builder unit or queue: the player places it and it goes up by itself.
  Rate of build is the table's `build_ticks` alone: no power shortage slows it (WP6.4).
* A structure under construction is not repaired, sold or cancelled (WP6.6).
* Hit points do not rise with progress (above).
* No sound or voice for "construction complete" (a hook: `World::completions()`).
* The decoy (`F1-M05`): `decoy_emplacement` builds in 8 s (160 ticks), the fastest of the tree; its prerequisite
  is the barracks only, so on that mission the player has a decoy 8 seconds after the click
  (`the_decoy_needs_only_the_barracks_that_was_placed_complete`, and `docs/rules/buildings.md`).

## Where it is tested

* `crates/sf_sim/tests/construction.rs` and the unit tests of `crates/sf_sim/src/construction.rs`: the rule,
  the order inside the tick, prerequisite and anchor, the checksum, the slot reuse, zero hit points.
* `crates/sf_bridge/tests/placement.rs`: both factions build the shipped tree, waiting for each structure.
* `tools/sf_headless/tests/construction.rs`, `placement.rs`, `pinned.rs`: the scenarios and their pins.
* `crates/sf_data/src/build_stages.rs` (the table and what it refuses), `crates/sf_game/src/roster.rs` (the
  stage looks), `construction_view.rs` (the bar), `tests/construction_snapshot.rs` (the sim thread's snapshot
  and the stage chosen from it), `tests/build_screenshot.rs` (the real renderer).
