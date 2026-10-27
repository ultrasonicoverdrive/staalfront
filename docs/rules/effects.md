# Visual effects (WP5.6a, WP5.6b)

WP5.6a made the effects; WP5.6b (sections 5 and 10, and the end of 8 and 9) put the flash at the model's muzzle node and
added the combat benchmark of M0c (`docs/bench/README.md`).

Muzzle flashes, hits, explosions, smoke and the tracers of projectiles in flight. **Presentation only**: nothing
here reaches the simulation (`git diff origin/main -- crates/sf_sim` is empty, no checksum or pinned value moved),
and nothing here is a rule of play. The numbers are ours (`assets/config/effects.ron`, invented, no third-party
material); the effects never deal damage. Not done, and why, is at the end.

```text
 sim thread                                      main thread (per frame)
 ----------                                      -----------------------
 World::step ──► fx_events::record ─► FxQueue ──► SimHandle::drain_fx ─► FxPlayer::take
                 (after EVERY step)   (mutex, cap)                         │
                                                  snapshot (wrecks,        ▼
                                                  projectiles) ─────► FxPlayer::frame
                                                                           │  release events due, spawn bursts,
                                                                           │  wreck smoke, tracers, move, emit
                                                                           ▼
                                                      Vec<EffectInstance> ─► Renderer::set_effects ─► 1 draw call
```

## 1. The events: lossless, bounded

The world's report of shots, impacts and deaths covers **one tick** and is replaced by the next step
(`World::shots`, `impacts`, `deaths`). The sim thread publishes one snapshot per pass of up to `1 + max_catch_up`
steps, to a slot the main thread reads when it likes. So a snapshot cannot carry them: the deaths of the earlier
steps of a catch-up pass reached no snapshot (follow-up 31 of `progress.md`, now closed). Therefore:

* `sf_game::fx_events::record(&World, &mut Vec<FxEvent>)` runs on the sim thread **right after every
  `World::step`** and appends that tick's events: for each shot a `Fire` at the shooter, along the facing of the
  weapon's mount (hull or turret); for an **instant** weapon also an `Impact` at the target (the world makes no
  impact of its own for it); for each impact of the world an `Impact` where it went off (`hit` = a direct victim,
  `splash` = its splash hurt something); for each death a `Death` (`wreck`, `structure`). Order: shots, impacts,
  deaths, each in the world's own order. An attacker or target that is gone is looked for in the deaths of the same
  tick; one that is in neither has no position and is counted, not drawn (`fx_unresolved`).
* An `FxEvent` is at most 32 bytes: tick, kind, weapon kind number, player, position in metres, facing, size in
  metres (the shooter's body length for a `Fire`, the splash radius for an `Impact`, the body for a `Death`).
  Positions are converted once, here (`sf_math::render_bridge`).
* They go into an `FxQueue` behind a mutex in the shared state of the sim thread. `SimHandle::drain_fx(&mut Vec)`
  takes the lot, once per frame; each event comes out once, in tick order. Without a sim thread the same `record`
  is called by hand (the screenshot test does).
* **Cap: `FX_QUEUE_CAP` = 4096 events** (128 KB), made once. When it is full the **oldest** is dropped and counted
  (`SimCounters::fx_dropped`). The F1 panel shows it: "effect events: N recorded, queue dropped D, no position U,
  expired E, waiting W".
* The picture shows an event of tick `n` once the snapshot's tick is past `n`, so the flash is in step with the
  interpolation of the units; events not due wait in the main thread's own list (`waiting`). An event more than
  `max_event_age_ticks` (40) older than the snapshot when its turn comes is dropped and counted (`expired`): a window
  that was minimised does not set off a backlog of explosions.

**Measured volume.** The `destruction` scenario (12 units, four structures, rocket troopers) makes 112 events in its
first 900 ticks (and no more up to tick 1500). The design's extrapolation to
5,000 units all fighting (100 to 500 events per tick) is an estimate, not measured; the cap is a guard, and what
happens when it is met is the counter above (tested on the queue, `a_full_queue_drops_the_oldest_and_counts_it`;
the plumbing of the counter from the sim thread to the panel is by construction and was not flooded).

## 2. `assets/config/effects.ron`

One file, read by `sf_data::effects`, compiled in, checked when it is parsed. Sizes are metres across, times are
seconds, speeds metres per second.

| Part | What it says |
|---|---|
| `limits` | `max_particles` 4096 (the pool), `max_tracer_dots` 2048, `min_px` 2 and `max_px` 64 (the size on the screen), `middle_below_px` 60, `middle_factor` 0.5, `smoke_refuse_percent` 75, `strategic_flash_alpha` 0.6, `max_event_age_ticks` 40; the curve shapes `rise_jitter` 0.4 (rise times 0.8 to 1.2), `smoke_fade_in` 8 (smoke comes in over the first eighth of its life), `flash_hold` 0.5 (a death flash holds over the first half), `tracer_taper` 0.6 (the last dot is 0.4 of the head). Pool and dots together must fit the GPU buffer (8192 instances; a test keeps `EFFECT_CAPACITY` and `MAX_INSTANCES` equal). |
| `muzzle` | where a flash starts **when the model has no muzzle anchor** (section 5; a model with one uses it): `reach_fraction` 0.5 of the body length out, at least `min_reach_m` 0.8, `height_fraction` 0.22 of the body length up. |
| `placement` | `hit_height_m` 0.6 (a hit, a death and wreck smoke start this high), `death_lift_m` 0.5 (WP5.6b, a review observation of WP5.6a: a death bursts this much higher than that, so the blast of a body starts above the wreck it leaves; test `a_death_bursts_a_little_higher_than_a_hit`, which also shows a hit is not lifted), `projectile_lift_m` 1.0 (a projectile on the ground is drawn this high), burst radius `impact_radius_fraction` 0.5 / `death_radius_fraction` 0.4 of the size, at least `min_burst_radius_m` 0.3. |
| `effects` | 23 named bursts: `count`, `life_s`, `size_m`, `growth_mps`, `colour` (sRGB), `alpha`, `glow`, `rise_mps`, `spread_mps`, `sound`. `glow: true` is additive light; `false` is the **smoke class**. |
| `by_damage_type` | per damage type of `unit_classes.ron`: the `fire` and `impact` lists. Every damage type has one. |
| `by_weapon` | per weapon **id** of `weapon_defs.ron`: a `fire` and/or `impact` list that replaces the damage type's (an empty list keeps it). Shipped: `howitzer` fires `flash_howitzer`. |
| `miss` | what a projectile that went off on nothing makes (dust). |
| `death` | `infantry_below_cells` 1.0 and `small_below_cells` 1.3 split the mobile bodies by size; four lists: infantry, small vehicle, vehicle, structure. |
| `wreck_smoke` | the puff, `per_second` 3 and `fade_fraction` 0.2 (a wreck stops over the last fifth of its life), `radius_fraction` 0.25 (puffs start in a disc of a quarter of the wreck's length). |
| `tracers` | per way of flying (`straight`, `arc`, `guided`, `falling`): `dots`, `spacing_m`, `size_m`, `colour`, `alpha`, a `trail` of puffs and `trail_per_s`. |

**Linking.** The event carries the weapon's kind number; the definitions give its id and damage type; `by_weapon`
(by id) wins over `by_damage_type`. An `Impact` with no direct victim, no splash victim and a weapon without a splash
radius is a miss (`miss` list); a shell with a splash that lands on nothing is still a blast.

**Checked at load** (`Effects::from_ron`, `check_against`): every effect named exists; every damage type has an entry
and every entry is a damage type; every `by_weapon` key is a weapon id; unknown fields are refused; every number is in
its range (count 1 to 64, life 0.01 to 10 s, size 0.05 to 50 m, alpha above 0, `min_px` below `max_px`, `middle_factor`
0.05 to 1, `strategic_flash_alpha` at least 0.5, trails only with a rate); a `sound` is a dotted lower-case name or empty
and no two effects share one.

**Every number is pinned** by a test: `the_shipped_limits_are_these_numbers` (which also pins `placement`, `death_lift_m` included), `every_shipped_effect_is_these_numbers`
(all 23 effects, field by field, sorted by name, so an effect added or removed fails it),
`the_links_and_the_rest_are_these_numbers` (the damage-type links, the override, the miss, the deaths, the wreck smoke,
the four tracers) and `every_shipped_weapon_resolves_to_a_fire_and_an_impact_effect` (all 15 shipped weapons).

## 3. The audio hook (not built)

Each effect has a `sound`: the **event name** WP11.7's mapping (`assets/audio/ra_mapping.ron`) hooks onto; nothing plays
it yet. `FxPlayer::sound_name(&FxEvent)` is the one function from event to name: the first effect of the event's list with
a sound.

| Event | Name |
|---|---|
| a shot | `weapon.fire.<damage type>` (`bullet`, `precision`, `cannon`, `rocket`, `shell`, `demolition`), or `weapon.fire.<weapon id>` where `by_weapon` has an effect (`weapon.fire.howitzer`) |
| a hit | `impact.<damage type>`; a miss `impact.miss` |
| a death | `unit.destroyed.infantry`, `explosion.small` (small vehicles), `explosion.medium` (vehicles), `explosion.large` (structures) |

Test: `the_event_names_follow_the_scheme` pins every name and checks that each shipped weapon names a fire and a hit;
`the_sound_names_are_well_formed_and_unique` checks the form and that no two effects share a name.

## 4. The particles and how they are drawn

**The pool** (`sf_render::effects::ParticlePool`): one ring of at most 4096 particles, 48 bytes each, made once. A burst
into a full pool pushes the **oldest** out (`dropped`); a smoke-class burst is **refused** while the pool is more than 75 %
full (`smoke_refused`), so flashes and explosions keep their room. Particles move on their own (a rise and a sideways
speed), grow, and fade: light linearly, smoke in fast and out slowly. The generator is the pool's own (presentation only).

**The draw** (`assets/shaders/effect.wgsl`, `Renderer::set_effects`): every particle and every tracer dot is one
`EffectInstance` (32 bytes: position, side in pixels, premultiplied colour and blend alpha). **One pipeline, one instanced
draw call**, premultiplied alpha (`One, OneMinusSrcAlpha`): a glowing particle arrives with blend alpha 0, which makes it
additive; smoke arrives with its alpha. The quad is screen-facing and laid out in pixels like the icons; the shader draws a
soft disc (no texture, no asset). Depth is tested against the scene, not written.

* The buffer is made **once at start-up** for 8192 instances (256 KB); a frame copies at most 6144 (196,608 bytes) into it.
  The frame's `Vec<EffectInstance>` is also made once. **No per-frame allocation**: `AllocStats` is unchanged by effect
  frames (asserted in the screenshot test), and `emitting_does_not_grow_the_vector` checks the vector.
* `DrawStats` gained `effect_instances` and `effect_draw_calls` (1 when there are any), counted at the one
  `draw_indexed` (`CountedPass::draw`, `DrawGroup::Effects`) and included in `total_draw_calls`; the F1 panel shows them.
* **Drawn after the icons, not before** (the design said before). Found in the screenshot test: the wreck that lies where
  a unit died has an icon at strategic zoom, and the icon covered the death flash. Effects are the last thing in the scene
  pass (before the egui overlay). Glow over an icon is no problem.
* Not sorted back to front: with premultiplied alpha the order of glow and smoke is hardly visible; sort only if a
  screenshot shows a problem (none did).

## 5. Where a flash starts

**At the model's muzzle node** when the shooter's model has one (WP5.6b), else by a **bounding-box rule** (WP5.6a).

### The anchors in the glb loader (`sf_render::glb`)

A node of the file named `muzzle*` or `fx_*` (after the `_LOD<n>` suffix is stripped) with no mesh of its own is an
**anchor**: a named point that is not drawn. The loader used to make such a node an empty part and prune it; now it keeps
it, before pruning, on `LoadedLevel::anchors` as `Anchor { name, parent, local }`: the name, the part whose frame it is in
and its matrix in that frame (converted to engine axes with the same `S M S^-1` as the parts, scaled with
`LoadedModel::scaled`). In the order of the file (`muzzle_l` before `muzzle_r`).

* **Re-parenting.** A part with no geometry and no kept part below it is pruned (the test pack's empty `turret` and `barrel`
  nodes). An anchor under such a part moves to its nearest kept ancestor, with the pruned parts' frames folded into its
  matrix (`an_anchor_under_a_pruned_part_moves_to_its_nearest_kept_ancestor`: a muzzle 2 m ahead of a pruned barrel that
  is 1 m ahead of the turret is on the turret, 3 m ahead). Its joint is then the kept ancestor's, not the pruned part's.
* A node called `muzzle` that **has geometry** is geometry, not an anchor (`a_muzzle_named_mesh_is_geometry_not_an_anchor`).
* In set v4: of 194 files, **44 carry `muzzle*` anchors and 119 carry `fx_*`**, 283 anchors in all
  (`set_v4_anchor_counts_are_these_numbers`). The heavy tanks have `muzzle_l` and `muzzle_r` on the barrel, the light tanks, the
  riflemen and rocket troopers and the siege artillery one `muzzle`. The test pack has **no** anchor; set v3 has `fx_*` on four
  neutral wrecks and no muzzle (`the_test_pack_and_set_v3_have_no_muzzle_anchors_and_fall_back`).
* `model_set` carries them per level to `ModelLods::anchors(level)` (`RigAnchor`); `anchor_count(prefix)` counts them.

### Where an anchor is in the world (`Instancer::anchor_world`)

`Instancer::anchor_world(slot, prefix, n, alpha, ground)` poses the unit **exactly as `build_frame` draws it at `alpha`**: the
same interpolated position and hull facing, the same turret yaw (`Motion::turret_yaw`, or the demo aim) and barrel pitch
(`Instancer::joint_angles`, the one function both use), the same `pose_parts`, on the terrain height `ground` gives; then
`posed[parent] * local`. The `n`-th matching anchor (the file's order) is taken **modulo the number of matching anchors**.
The finest level's anchors are used (every level of a file has the same ones). `slot` is the index half of a `UnitId`; the
instancer finds it by halving when the slots of the tick rise (the game's do) and by scanning when they do not. `None` when
there is no such unit or the model has no such anchor. Tests: `an_anchor_is_where_the_drawn_barrel_carries_it` (the barrel
instance the frame uploaded times the muzzle's own matrix, interpolated, on a slope, turret off the hull, barrel pitched),
`a_turret_turned_64_steps_rotates_its_anchor_90_degrees_about_the_pivot` and
`a_turret_64_steps_off_the_hull_swings_the_muzzle_a_quarter_turn_about_the_pivot` (the turret turned a quarter turn swings the
anchor 90 degrees about the pivot, `(x, z)` to `(-z, x)`, on a hull that itself faces 20 steps off the axis),
`an_anchor_is_found_by_slot_and_a_wreck_slot_is_not_a_unit`.

### Which muzzle, and which unit

* An `FxEvent` of a shot carries `shooter`: the **slot** of the attacker (4 bytes in what was padding; the event is still 32
  bytes), `NO_SHOOTER` (`u32::MAX`) for every other kind. `FxPlayer::frame_with(snap, ground, anchors, frame)` asks the
  `Anchors` trait (the runner answers it from the instancer, `DrawnAnchors`) for `("muzzle", n, alpha)` of that slot.
* **`n = weapon slot + the number of shots of that (shooter, weapon slot) already shown`**. So the shots of a burst
  alternate between the muzzles: the heavy tank's volley of two (`twin_cannon`, `burst: 2`) flashes at `muzzle_l`, then
  `muzzle_r`, and the next volley again at `muzzle_l`; a unit with one `muzzle` flashes there every time; a second weapon slot
  starts on the next muzzle. (A hull-mounted weapon on a model whose muzzle is on its barrel flashes at the barrel too: the
  event does not say which muzzle belongs to which weapon. Not built; no shipped unit has both.)
* **Wrecks and units are told apart by slot.** The picture numbers wrecks from the first slot no live entity has
  (`looks::first_wreck_slot`). `DrawnAnchors` answers only for slots **below** it, so a shooter that died after it fired is never
  looked for under the wreck that took its slot number (`drawn_anchors_do_not_answer_for_a_wreck_slot`).
* **The flash follows the drawn pose at the frame it is shown**, interpolated at the frame's alpha, not the pose of the tick of
  the shot: a unit that drives on between the shot and the frame has its flash at its barrel as drawn.
* Counters `flashes_at_anchor` and `flashes_by_rule` (F1 and the bench JSON): in the combat benchmark of 45 s on lavapipe,
  3,559 flashes at an anchor and 16 by the rule.

### The fallback: the bounding-box rule

For a model without a `muzzle` anchor (the test pack, set v3, the placeholder shapes, a shooter whose model is not drawn): from
the unit's middle, out along the facing of the weapon's mount (the turret's for a turret weapon, the hull's otherwise),
`reach_fraction` (0.5) of the body length, at least 0.8 m, at `height_fraction` (0.22) of the body length up. The **body
length is the unit's `size` in the definitions** (a structure: the longer side of its footprint), not the model's measured box:
an approximation (a 6 m tank gets 3 m of reach and 1.32 m of height). A burst of two flashes twice at the same point there.
Tests: `the_muzzle_is_out_along_the_mount_and_up`, `a_model_without_a_muzzle_gets_the_bounding_box_rule`.

### Seen

`docs/screenshots/wp5.6b/`: the flash at the barrel tip of a Veyran heavy tank (set v4) whose turret is 40 steps off its hull,
the first shot at `muzzle_l` (tick 5) and the second at `muzzle_r` (tick 10), 0.9 m apart; and of a light tank (one `muzzle`)
with its turret turned, with the tracer going away from it (tick 9). From the screenshot test, which asserts a bright change
there (1,514 and 1,708 pixels brighter, 1,694 for the light tank) and that the particles of each shot are within 0.3 m of the
instancer's answer for that muzzle (0.10 m and 0.09 m in the run printed).

## 6. Zoom

All of it is decided by two numbers a frame already has: the **model weight** of the model/icon cross-fade
(`Blend::model()`, `1` models only, `0` icons only) and the apparent size of the 2 m reference unit
(`ViewBand::reference_px`).

| Band | When | Particles | Alpha |
|---|---|---|---|
| close | models only and the reference unit at least 60 px | all of a burst | times the model weight (1) |
| middle | the cross-fade, or the reference unit under 60 px | `middle_factor` (0.5) of a burst, rounded up, at least one | times the model weight |
| strategic | icons only | none; **a death makes one flash** | zero, except the flash |

* **Pixel clamp.** A particle is `size` metres across; on the screen it is `size * focal / depth` pixels clamped to
  `min_px` (2) and `max_px` (64) (`size_px`; the same maths as `IconConfig::reference_px`). It is computed on the CPU, so
  it is unit-tested; the shader only lays out the quad.
* **Fade.** Everything is multiplied by the model weight, so effects are gone where the units have become icons
  (`effects_fade_with_the_model_weight_and_are_gone_at_icons_only`). Wreck smoke and tracers follow the same band
  (smoke at middle zoom at half the rate, none at strategic).
* **The death flash.** At strategic zoom a death makes one particle, from the first glowing effect of its list, flagged as
  a flash: it keeps `strategic_flash_alpha` (0.6, at least 0.5) for the first half of its life, and is clamped to the
  **icon's size** (`icon_px`). Tests: `a_death_flash_survives_at_strategic_zoom_clamped_to_the_icon`,
  `at_strategic_zoom_only_the_death_flash_is_drawn`; in the screenshot test the flash is 14 pixels in the picture.
* A burst's count is decided when it is made; particles already alive when the camera moves keep fading by the weight.

## 7. Wrecks and projectiles

* **Wreck smoke** comes from the wreck's state, not from events: every wreck or pile of rubble in the snapshot gives
  off `per_second` puffs while `ticks_left > 0`, and stops over the last `fade_fraction` of its `lifetime`.
* **Tracers.** `SimSnapshot::projectiles()` (from `Snapshot::collect_projectiles_into`) are drawn every frame as a row of
  glowing dots behind the head, in the same instance list and the same draw call: the head is the interpolated position
  (previous to current tick, by the same alpha as the units); a projectile that flies on the ground is lifted 1 m (the
  muzzle's height, `placement.projectile_lift_m`), an arc or a drop uses its own height. A guided one leaves a trail of puffs.
  Dots are budgeted by `max_tracer_dots`; a projectile that does not fit is counted (`tracer_dots_dropped`) and skipped.
  Tests: `a_projectile_in_flight_is_drawn_as_a_tracer`, `a_guided_projectile_leaves_a_trail`, `a_tracer_is_a_row_of_tapering_dots_behind_the_head`.

## 8. Tests and what they catch

| Test | Proves |
|---|---|
| `tests/fx_events.rs` `nothing_is_lost_when_the_sim_falls_behind_and_catches_up` | with `max_catch_up` 3 the sim stepped passes of 4 ticks (`max_pass_ticks` 4); the 112 events the sim thread delivered are exactly those of the single-threaded run, in order, and sum with the world's own counts (shots + instant hits + impacts + deaths). In the run printed by the test the snapshots, read every 7 ms meanwhile, showed **0 of the 4 deaths** (a race by nature; the test prints it and does not assert it) |
| `a_hitch_and_a_main_thread_that_reads_nothing_lose_nothing` | a hitch (80 ms at the first tick with an event) forces a catch-up pass; nothing is read during the run; the queue hands everything over whole |
| `fx_events::tests::a_full_queue_drops_the_oldest_and_counts_it` | 4097 events into 4096: the oldest goes, `dropped` is 1, the rest keep their order |
| `sf_render::effects` tests | pool budget and oldest-out, the 75 % smoke rule, move and die, the pixel clamp, bands and density, fade, additive vs normal alpha, the death flash, culling, no vector growth, tracers |
| `sf_game::fx` tests | events wait for their tick, too-old events expire, density per zoom for fire, hit, death and miss, only the flash from afar, the muzzle, tracers and trails from a real world |
| `tests/effects_screenshot.rs` | through the real renderer (lavapipe): at the tick of the first shot, hit and death the picture differs from the same instant without effects, with bright pixels, at the muzzle (1,760 pixels changed, 1,454 brighter), at the impact (1,618) and at the death (4,478, 3,459 brighter); one effect draw call counted in `DrawStats`; no buffer made; middle zoom has fewer particles than close; from afar a shot changes **no** pixel of the icons-only picture and a death leaves one flash of 14 pixels, no bigger than an icon |

**Mutations** (each made in a scratch copy, the test that failed named, then reverted): the queue recorded only per
published snapshot instead of per step: both `fx_events` tests; smoke limit off: `smoke_is_refused_above_three_quarters_and_light_is_not`;
no pixel clamp: `the_screen_size_is_clamped_between_min_and_max`; fade ignoring the model weight: `effects_fade_with_the_model_weight_...`
and `a_death_flash_survives_...`; no strategic death flash: `at_strategic_zoom_only_the_death_flash_is_drawn` and
`density_follows_the_zoom_...`; no thinning at middle zoom: `the_zoom_bands_and_the_density`; effects never drawn: the screenshot test;
a sound name changed: `every_shipped_effect_is_these_numbers`; events shown a tick early: `an_event_is_shown_once_...`; no minimum muzzle reach:
`the_muzzle_is_out_along_the_mount_and_up`; the queue dropping the newest: `a_full_queue_drops_the_oldest_...`; projectiles not copied into the
snapshot: both tracer tests.

**WP5.6b tests** (`sf_render::glb::anchor_tests`, `sf_render::instancing::tests`, `sf_game::fx::tests`, `tests/effects_screenshot.rs`,
`tests/bench_scene.rs`, `bench::tests`):

| Test | Proves |
|---|---|
| `the_veyran_heavy_tank_has_two_muzzle_anchors` | the real file: `muzzle_l` and `muzzle_r` on the barrel at all three levels, at (2.808, 0, -/+0.456) from the barrel pivot |
| `an_anchor_under_a_pruned_part_moves_to_its_nearest_kept_ancestor`, `anchors_follow_the_axis_and_unit_conversion_and_scaling`, `a_muzzle_named_mesh_is_geometry_not_an_anchor` | the loader's rules of the section above |
| `set_v4_anchor_counts_are_these_numbers`, `the_test_pack_and_set_v3_have_no_muzzle_anchors_and_fall_back` | 194 files, 44 with `muzzle`, 119 with `fx_`, 283 anchors; test pack 14 files and set v3 30 files with no `muzzle`, four v3 files with `fx_` |
| the four instancer tests of section 5 | an anchor is where the drawn barrel carries it; a quarter turn of the turret is a quarter turn of the anchor about the pivot; slots, wrecks, rising or not |
| `a_flash_starts_at_the_muzzle_anchor_when_the_model_has_one`, `a_model_without_a_muzzle_gets_the_bounding_box_rule`, `a_burst_alternates_between_the_muzzles_by_shooter_and_slot`, `drawn_anchors_do_not_answer_for_a_wreck_slot`, `a_death_bursts_a_little_higher_than_a_hit` | what `FxPlayer` asks the picture and does with the answer (a fake `Anchors`, and the real heavy tank in an instancer) |
| `fx_events.rs` `a_shot_points_along_its_mount_turret_or_hull` | gained: every `Fire` carries the attacker's slot, nothing else has a shooter |
| `a_flash_starts_at_the_barrel_tip_and_a_twin_cannon_alternates_its_muzzles`, `a_single_muzzle_tank_with_its_turret_turned_flashes_at_the_barrel` | through `record`, `frame_with`, `DrawnAnchors` and the real renderer: the particles of each shot start within 0.3 m of the instancer's answer for that muzzle (the second shot of a burst at the other one, 0.912 m apart), the turret is 40 steps off its hull, the picture changes brightly there; the light tank's barrel tip is 2.6 m from the rule's point (the test asserts more than 1 m) |
| `bench_scene.rs` (five tests) | the checksums of the benchmark worlds, section 10 |
| `bench::tests::the_fight_benchmark_reports_its_effects` | `scene.combat`, `scene.skirmish_units`, the `effects` block and `effect_instances` overall and per phase arrive in the JSON |

**Mutations of WP5.6b** (each made in place, the tests that failed named, then reverted; every one failed what it should):
anchors not moved to the nearest kept ancestor with the pruned frame folded in: `an_anchor_under_a_pruned_part_...` and
`anchors_follow_the_axis_...`; anchors not converted to engine axes: `anchors_follow_the_axis_and_unit_conversion_and_scaling`;
`anchor_world` posing the unit with joints at rest: `an_anchor_is_where_the_drawn_barrel_carries_it` and
`a_turret_64_steps_off_the_hull_...`; slots always taken as rising: `an_anchor_is_found_by_slot_...` (the first version of that
test did **not** catch it, two units cannot tell; it now has four unsorted ones); `n` without the shot counter:
`a_burst_alternates_between_the_muzzles_...` and the heavy-tank screenshot test; no cut at the first wreck slot:
`drawn_anchors_do_not_answer_for_a_wreck_slot`; no death lift: `a_death_bursts_a_little_higher_than_a_hit`; `death_lift_m` 0.5
changed to 0.6 in the RON: `the_shipped_limits_are_these_numbers`; `record` without the shooter: `a_shot_points_along_its_mount_...`
and both anchor screenshot tests; skirmish gap 3.6 cells changed to 4.0: `the_fight_benchmark_ends_at_these_checksums_...`;
`DEMO_SPEED` 1.5 changed to 2: all three checksum tests; the skirmish scheduled before the crowd:
`the_fight_benchmark_has_the_crowd_of_the_plain_one_...`; the effect instances counted from the triangle column:
`the_fight_benchmark_reports_its_effects`.

## 9. Cost (relative; the real number is M0c on the laptop after WP5.6b)

Release build, same machine, alternating runs, a shared container whose load average was 2.7 to 3.6 (so only the size of
the numbers means anything). The container has no GPU: the "GPU" figures are the **software rasterizer's** (Mesa
lavapipe) and say how the cost grows, not what a GTX 1650 does.

| What | Number |
|---|---|
| draw calls added | at most **+1** (`effect_draw_calls`) |
| triangles added | 2 per instance: at most 12,288 (6144 instances) |
| upload | at most 196,608 bytes per frame into a buffer made at start |
| sim thread: `record` per tick | **0.05 us** (median of 5 alternating rounds, 1500 ticks of `destruction`) against **25.6 us** for the step; stepping alone 25.6 us, stepping in the rounds that also record 23.4 us (noise) |
| main thread: `FxPlayer::frame`, empty pool, no events | **0.24 us** per frame |
| main thread: pool update (4096 alive) + emit (1224 of them in view) + 300 tracers | **109 to 116 us** per frame (2042 instances) |
| software rasterizer, whole frame 960 x 540, close zoom with the fight | no effects **21.9 to 22.2 ms**; 500 instances of 12 to 31 px **+0.3 to +1.0 ms**; 6144 instances all at 64 px **+15.3 to +15.6 ms** |

**WP5.6b** (release, `cargo test --release -p sf_render --lib what_the_anchor_lookup_costs -- --ignored --nocapture`, 5 alternating
rounds, load average 1.2 before the run and 4.3 after it because the release build ran in between, so the first round is the
slowest and the numbers are only the size): one `anchor_world` among 5,000 units takes **121 to 126 ns** when the slots rise (the
game's do; the first round 179 ns) and **1.3 to 1.4 us** when they do not (the scan; the first round 2.9 us). A frame asks once
per shot shown: a fight of 30 shots in a frame costs under 4 us. The only per-tick cost of the feature is the check that the
slots rise, in `Instancer::update_tick`: **2.0 to 2.1 us** of an `update_tick` of 5,000 units that takes **99 to 112 us** (about
2 %, measured as the check alone inside the same loop; there is no "before" binary, the rest of `update_tick` is unchanged).
Nothing else of the flash changed: one lookup in a `BTreeMap` of shot counts per shot (bounded by shooters times weapon
slots).

The 6144 x 64 px case is the worst the buffer allows (about 38 screens of overdraw at this size) and not something a
fight makes: the guards are `max_px`, the 75 % smoke rule and the pool size. The design's estimate for the GTX 1650
(at most 0.5 ms) is an estimate; nothing here measured it. Measure: `cargo test --release -p sf_game --test
effects_screenshot -- --ignored --nocapture` and `--test fx_events -- --ignored --nocapture`.

## 10. The combat benchmark and its checksums (WP5.6b)

`sf_demo --bench --fight` flies the M0 camera path over the M0 crowd with an armed skirmish added; how Tijmen runs it and
what to read is `docs/bench/README.md`. The skirmish (`sf_game::skirmish`) is fixed: three sites (the map centre, the place of
`mid_hold`, the middle of `low_pass`), a wave of 8 units per side every 100 ticks from tick 0 to 1400 (720 units in all), face
to face at 3.6 cells, no orders (the units pick their own targets, WP5.4), made by scheduled `SpawnUnit` commands on a world
given the unit table and `playfield::with_combat`. The crowd is the demo's own (same seed, kinds, owners; checked equal entity by
entity at tick 0), drawn with the test pack; the roles are drawn with their set v4 models (so the muzzle anchors are in the
benchmark), the crowd's pack models after them in the instancer (`Population::with_pack_shapes_from`).

`bench_scene` builds the world without a window (the **headless twin**), the runner builds it with the same functions, and
`tests/bench_scene.rs` pins three lists of 15 checksums (every 100 ticks to tick 1500). Compared with a real run under Xvfb and
lavapipe: the plain benchmark's checkpoints 100 to 400 and the fight's 100 to 300 are equal to the twin's.

* **The plain benchmark no longer ends at M0's `0x9e196a6ced2ef024`.** On `main` after WP7.1 (ore on the map, in the checksum) it
  ends at `0x8e57352fc4bdec7b`. Measured: the same world built without the ore gives all 15 of M0's checkpoints
  (`the_plain_benchmark_without_ore_is_the_world_m0_recorded`). Nothing of WP5.6b moved it (`git diff origin/main -- crates/sf_sim`
  is empty; the plain scene is built by the same calls in the same order as before). The brief of this WP said the old value
  still held; it had moved before this WP, and the test now says why.
* The fight's list ends at `0x568d257d092daa7b`. In the twin the fight makes 143 to 536 shots, 66 to 267 hits and 6 to 44 deaths
  per hundred ticks and at most 32 events in one tick: a light load next to the design's 100 to 500 events a tick for 5,000 units
  all fighting, which this does not try to be (the cap and the pool have headroom: 45 s flight on lavapipe, 7,075 events, pool
  peak 1,513 of 4,096, nothing dropped).

## 11. Not done

* **Smoke and fire from the `fx_*` anchors** (the loader keeps them, nothing reads them): WP6.8 for damaged buildings.
* **Which muzzle belongs to which weapon** when a unit has weapons on the hull and on the turret and a model with several
  muzzles: the flash alternates over all of them (no shipped unit has both).
* The sounds themselves: WP11.7 (the names are in the file; an audio position can use `Anchors` the way the flash does).
* **Building damage display, smoke and fire on damaged buildings**: WP6.8. Wrecks give off smoke now; a live building
  does not.
* An `Impact` of an instant weapon whose target died in the same tick is placed from the tick's deaths; one whose target
  is in neither list is not drawn (`fx_unresolved`).
* The effects do not follow a unit that moves after the shot (a flash stays where it was made, at the muzzle of the pose
  of the frame that showed it).
* No hot reload of `effects.ron`; no sorting of the blended particles; no distinct tracer colour per player; a wreck does
  not fade.
* **The strategic death flash is small by design** (an icon's size at most, drawn over the wreck's icon): a screenshot at
  strategic zoom shows it as a small orange dot on the dark icon (`docs/screenshots/wp5.6a/test_t0194_strategic_death_flash.jpg`).
* The queue's cap was not flooded in a running game; the counter's plumbing from the sim thread to the F1 panel is by
  construction (`SimCounters::fx_dropped`) and the panel line is tested (`the_panel_shows_the_effects_counters`).
