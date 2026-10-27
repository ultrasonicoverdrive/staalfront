# Visual effects (WP5.6a)

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
| `limits` | `max_particles` 4096 (the pool), `max_tracer_dots` 2048, `min_px` 2 and `max_px` 64 (the size on the screen), `middle_below_px` 60, `middle_factor` 0.5, `smoke_refuse_percent` 75, `strategic_flash_alpha` 0.6, `max_event_age_ticks` 40. Pool and dots together must fit the GPU buffer (8192 instances; a test keeps `EFFECT_CAPACITY` and `MAX_INSTANCES` equal). |
| `muzzle` | where a flash starts when the model has no muzzle anchor (section 5): `reach_fraction` 0.5 of the body length out, at least `min_reach_m` 0.8, `height_fraction` 0.22 of the body length up. |
| `effects` | 23 named bursts: `count`, `life_s`, `size_m`, `growth_mps`, `colour` (sRGB), `alpha`, `glow`, `rise_mps`, `spread_mps`, `sound`. `glow: true` is additive light; `false` is the **smoke class**. |
| `by_damage_type` | per damage type of `unit_classes.ron`: the `fire` and `impact` lists. Every damage type has one. |
| `by_weapon` | per weapon **id** of `weapon_defs.ron`: a `fire` and/or `impact` list that replaces the damage type's (an empty list keeps it). Shipped: `howitzer` fires `flash_howitzer`. |
| `miss` | what a projectile that went off on nothing makes (dust). |
| `death` | `infantry_below_cells` 1.0 and `small_below_cells` 1.3 split the mobile bodies by size; four lists: infantry, small vehicle, vehicle, structure. |
| `wreck_smoke` | the puff, `per_second` 3 and `fade_fraction` 0.2 (a wreck stops over the last fifth of its life). |
| `tracers` | per way of flying (`straight`, `arc`, `guided`, `falling`): `dots`, `spacing_m`, `size_m`, `colour`, `alpha`, a `trail` of puffs and `trail_per_s`. |

**Linking.** The event carries the weapon's kind number; the definitions give its id and damage type; `by_weapon`
(by id) wins over `by_damage_type`. An `Impact` with no direct victim, no splash victim and a weapon without a splash
radius is a miss (`miss` list); a shell with a splash that lands on nothing is still a blast.

**Checked at load** (`Effects::from_ron`, `check_against`): every effect named exists; every damage type has an entry
and every entry is a damage type; every `by_weapon` key is a weapon id; unknown fields are refused; every number is in
its range (count 1 to 64, life 0.01 to 10 s, size 0.05 to 50 m, alpha above 0, `min_px` below `max_px`, `middle_factor`
0.05 to 1, `strategic_flash_alpha` at least 0.5, trails only with a rate); a `sound` is a dotted lower-case name or empty
and no two effects share one.

**Every number is pinned** by a test: `the_shipped_limits_are_these_numbers`, `every_shipped_effect_is_these_numbers`
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

## 5. Where a flash starts (fallback until WP5.6b)

The loader throws the `muzzle` nodes of the models away, so there are no anchors. WP5.6a starts the flash at the
**bounding-box rule**: from the unit's middle, out along the facing of the weapon's mount (the turret's for a turret
weapon, the hull's otherwise), `reach_fraction` (0.5) of the body length, at least 0.8 m, at `height_fraction` (0.22)
of the body length up. The **body length is the unit's `size` in the definitions** (a structure: the longer side of its
footprint), not the model's measured box: an approximation (a 6 m tank gets 3 m of reach and 1.32 m of height). A
burst of two (`twin_cannon`) flashes twice at the same point. WP5.6b replaces both with the anchors of the glb files.
Test: `the_muzzle_is_out_along_the_mount_and_up`.

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
  muzzle's height, `PROJECTILE_LIFT_M`), an arc or a drop uses its own height. A guided one leaves a trail of puffs.
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

The 6144 x 64 px case is the worst the buffer allows (about 38 screens of overdraw at this size) and not something a
fight makes: the guards are `max_px`, the 75 % smoke rule and the pool size. The design's estimate for the GTX 1650
(at most 0.5 ms) is an estimate; nothing here measured it. Measure: `cargo test --release -p sf_game --test
effects_screenshot -- --ignored --nocapture` and `--test fx_events -- --ignored --nocapture`.

## 10. Not done

* **Muzzle and `fx_*` anchors** in the glb loader, and the bench with a fight (`sf_demo --bench --fight`, M0c): WP5.6b.
* The sounds themselves: WP11.7 (the names are in the file).
* **Building damage display, smoke and fire on damaged buildings**: WP6.8. Wrecks give off smoke now; a live building
  does not.
* An `Impact` of an instant weapon whose target died in the same tick is placed from the tick's deaths; one whose target
  is in neither list is not drawn (`fx_unresolved`).
* The effects do not follow a unit that moves after the shot (a flash stays where the muzzle was at the tick).
* No hot reload of `effects.ron`; no sorting of the blended particles; no distinct tracer colour per player; a wreck does
  not fade.
* **The strategic death flash is small by design** (an icon's size at most, drawn over the wreck's icon): a screenshot at
  strategic zoom shows it as a small orange dot on the dark icon (`docs/screenshots/wp5.6a/test_t0194_strategic_death_flash.jpg`).
* The queue's cap was not flooded in a running game; the counter's plumbing from the sim thread to the F1 panel is by
  construction (`SimCounters::fx_dropped`) and the panel line is tested (`the_panel_shows_the_effects_counters`).
