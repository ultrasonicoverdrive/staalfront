# Damage display: a damaged object looks and smokes damaged

Work package WP6.8 (PLAN.md phase 6). Damage is **presentation only**. The simulation already has hit points; the
picture works out a stage from them every time it draws. Nothing is kept between frames, nothing flows back into
`sf_sim` (`git diff origin/main -- crates/sf_sim crates/sf_bridge assets/scenarios tools/sf_headless` is empty; the
snapshot code only reads `EntityView::hp` and the unit table's `hp` that the game already had). Every number is in
`assets/config/damage.ron` (`sf_data::damage`), checked when it is parsed and against `effects.ron`, and pinned
(`the_shipped_file_is_these_numbers`, `the_shipped_file_matches_effects_ron`). All numbers are invented placeholders.

## The stage

`SimSnapshot::damage_stage(damage, view)` is the one place that turns hit points into a stage: the share of hit
points left, `hp * 1000 / max_hp` (integer, `hp` capped at the maximum), where the maximum is the unit table's `hp`
for the kind. At or below `heavy_at_permille` (330) the object is **heavily damaged**, at or below
`damaged_at_permille` (660) **damaged**, otherwise **intact**. An object that cannot be damaged (maximum 0, or made
without the unit table) is always intact. There is no memory: repair above a threshold ends the stage at once, and an
object at exactly a threshold can flip when its hit points move by one (`the_stage_follows_the_share_of_hit_points_left`).
Load refuses a heavy threshold that is not below the damaged one, or outside 1..=999.

## The model

`Looks::shape_in_state` is `Looks::shape_at` plus the stage, called by `runner::motion_of`. A **complete** structure
(not under construction, which has its own stage models, `construction.md`) that is damaged or heavily damaged is
drawn with the model whose file name is the finished one with `_damaged` / `_heavily_damaged` before `.glb`, where
the role (or the faction's look of the role) has that file on disk; `Roster` registers those shapes next to the
finished one, the same way WP6.3 registers its stage models. Without a file the object keeps its finished model and
shows the smoke and fire alone; a missing variant is never an error
(`a_role_without_a_damaged_model_keeps_its_look`). Today the art set has both variants for the structures of both
factions (21 heavily damaged files) and none for units, so units show smoke and fire only. The list of damaged looks
is pinned against the files on disk (`the_damaged_looks_are_the_art_sets_damaged_files`); the game path draws the
stage model (`the_game_draws_the_stage_model_of_a_damaged_structure`).

## Smoke and fire

`FxPlayer::frame_with` calls `damage_smoke` every frame from the snapshot. Damaged: smoke (4 puffs/s per source).
Heavily damaged: smoke (8/s) and fire (7/s). The puffs are the entries `damage_smoke` and `damage_fire` of
`effects.ron`, in the same particle pool (`max_particles`, the 75 % smoke refusal, the zoom bands) as the fight:

* **Where.** At the drawn model's `fx_smoke*` / `fx_fire*` anchor (set v4 has `fx_smoke_0` and `fx_fire_0` on every
  structure), 1 m above it, in a disc of 0.4 m. Without an anchor (a model of another kind, or none): above the
  middle of the body at 0.6 of its length (at least 1 m), in a disc of 0.15 of its length (at least 0.3 m)
  (`puffs_start_at_the_anchor_a_little_above_it`, `a_model_without_anchors_gets_the_rule_of_damage_ron`).
* **Who.** Structures, and mobile objects of at least `min_body_cells` (1.0) cells; infantry shows nothing
  (`a_damaged_vehicle_smokes_but_infantry_does_not`).
* **Zoom.** Nothing at strategic zoom, the middle factor (`effects.ron`) at middle zoom, all at close range
  (`damage_effects_follow_the_zoom_like_the_rest`).
* **Budget.** An object outside the screen by more than `cull_margin` (0.3 of the width or height) is skipped without
  a lookup; at most `max_sources` (256) objects give off puffs in a frame, the rest are counted
  (`off_screen_objects_are_skipped_and_the_sources_are_capped`); the pool stays inside its limit with 200 burning
  buildings (`many_burning_objects_stay_inside_the_pool`).
* **Stop.** Because the effect is worked out from the snapshot each frame, it stops the frame the hit points are
  above the threshold, and an object that dies is gone from the snapshot: the wreck's smoke and the death effects
  of WP5.5/5.6 take over (`repair_above_the_threshold_or_death_stops_the_effect`; the screenshot test's destroyed
  frames).

## Screenshots and measurement

`tests/damage_screenshot.rs` (real renderer on lavapipe or WARP, the production path: the sandbox world with combat,
`runner::motions_of`, `fx_events::record`, `FxPlayer::frame_with` with the instancer's anchors) shoots a power plant
of each faction with three tanks until it is damaged, heavily damaged and destroyed, and asserts the stage model, the
smoke at the anchor, the fire glow and that no damage puff starts at the dead plant's anchors after its death. It
does so twice: at a close camera (zoom 0.16, chosen to frame the plant and the smoke column above it) and at the game's
start zoom (`CameraConfig::default().start_zoom`, the middle band, where the player normally looks). The images are in
`docs/screenshots/wp6.8/` (`SF_SAVE_SHOTS=<dir>` writes them; no golden image); the tick is in the file name only, not
drawn in the picture.

Cost (release build, container, load average 1.8 to 3.3, relative numbers only; the fps requirement is the laptop's,
`docs/bench/README.md`): `cargo test --release -p sf_game --lib what_the_damage_effects_cost -- --ignored --nocapture`,
`FxPlayer::frame_with` for 5,000 objects, medians of 5 alternating rounds, feature switched off by RON thresholds
against on: none damaged 43.9 us / 43.6 us; plus 200 burning buildings 45.3 / 122.7 us (1,465 instances, pool peak
3,204 of 4,096, nothing dropped); plus 480 hurt tanks 42.8 / 146.1 us (256 sources, the cap held). Software
rasterizer, 960 x 540, 2,891 particle instances: 1.2 to 1.3 ms a frame without effects, 4.8 to 5.9 ms with (a bound
on the draw, not a GPU number). `sf_demo --bench --fight` under Xvfb and lavapipe, damage off (RON) and on, two runs
each in alternation: frame p99 69.9 / 69.6 ms off, 76.6 / 70.1 / 67.4 ms on (machine noise is larger than the
difference), effect instances per frame 39 off, 44 to 45 on; the first four checksums are equal in all runs.

## Not done

No sound (audio is another WP), no repair or sell (WP6.6), no damage models for units (the art has none), no
smoke from a structure under construction, no damage stage in the minimap or the icons, no fire on a damaged (not
heavily damaged) object by design. Hit-point bars are WP3.3's.
