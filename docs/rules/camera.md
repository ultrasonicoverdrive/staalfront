# Camera, far zoom and the icon overlay

Work package "zoom" (PLAN.md section 2, 08-11-2026; no number: it follows Tijmen's first play of M1). Code:
`crates/sf_render/src/zoom.rs` (the settings file, the bands), `camera.rs` (`far_margin`), `icons.rs`
(`Blend`), `instancing.rs` (`FrameView::model_scale`), `assets/shaders/icon.wgsl`,
`crates/sf_game/src/runner.rs` (`build_units`). Data: `assets/config/camera.ron`. Presentation only: nothing in
`sf_sim` changed and no pin moved.

What Tijmen said after playing M1 (laptop, GTX 1650, "far above 60 fps"): "I must be able to zoom out further",
and by that he meant that **the buildings and the little soldiers stay visible**. His screenshot showed the old
cross-fade half way: dithered, see-through buildings with red squares in them, a tank as a triangle in a circle.

## The rules

*Fix round 2 (10-11-2026, Tijmen after playing: "I want to zoom out much farther; they still turn into icons far
too soon"; orchestrator decision, his rule "your proposal holds if I do not answer"): `far_margin` 0.25 -> 1.0,
`icons_from` 0.85 -> 0.95, `icons_max_alpha` 0.6 -> 0.4, `model_scale_from`/`model_scale_max` 0.55/1.6 ->
0.45/2.5 (2.5 is the cap the file may set), and the key K (`ToggleIcons`) switches the icons off and on (on by
default; off = models only at every zoom, picking on the models).*

*Fix round 3 (10-11-2026, orchestrator decision under the same rule): at one scale of 2.5, 8 of the 9 pairs of M1
base structures overlapped, the worst by 13.0 m, so a click on one building could select its neighbour. Structures
now have their own cap `zoom.structure_scale_max` (1.6) on the same ramp (`model_scale_from`); units keep
`model_scale_max` (2.5). Validated: 1 <= structure cap <= unit cap <= 2.5. Measured again: 2 pairs (one per base),
0.40 m, as in round 1.*

*Fix round 4 (10-11-2026, second review and a Fable research round): every reader of a model's size was swept and
listed in rule 4 (drawn / true / does not care); the drag rectangle, the selection marks (one helper with
`visit_drawn`), the tile frustum culling and the muzzle and smoke anchors now follow the drawn size too, and ghosts
and rubble are tested at the structure scale.*

1. **The farthest zoom shows the whole map plus a whole map size on every side.** `camera.far_margin` (1.0) is a
   share of the map size. `Camera` fits the margin rectangle (map grown by `far_margin * size` on every side, at the
   far pitch of 80 degrees, centred) into the window and takes 3 % slack, as it always did for the bare map. That
   distance is the zoom 1.0 of the wheel, whatever the map size and window shape (a portrait window gets more
   distance). An explicit `max_distance` (`None` by default) still wins. `far_margin: 0` is the old behaviour. The
   zoom is still logarithmic in the distance, still toward the cursor, the pitch still follows the zoom. Focus is
   still limited to the map plus `margin` (16 m); at the new distances a long drag or pan reaches that limit sooner
   (the one case where the bounds win over the cursor, as before).
2. **Models are drawn at every zoom.** The zoom alone never removes a model: the blend of every frame has model
   weight 1 (`ZoomBands::blend`). The old rule (icons *instead of* models once the reference unit was smaller than
   14 px, a dithered cross-fade from 28 px) is gone, with `icons.model_above_px` and `icons.icon_below_px`.
3. **Strategic icons are an overlay in the farthest band.** Alpha 0 up to `zoom.icons_from` (0.95), 1 from
   `zoom.icons_full` (1.0), a smoothstep between: the last 5 % of the wheel travel (at 0.90 there is no icon at all). The zoom is the camera zoom
   (0 to 1, logarithmic), so the band is the same stretch of wheel on every map and resolution (the old trigger was
   a reference unit's apparent size). The icon shader multiplies its coverage by the alpha; the icons are still one
   draw call, not depth tested, over everything (`icons.md`). **Opacity cap (decision 10-11-2026, fix round 1):** the
   drawn opacity is the ramp times `zoom.icons_max_alpha` (0.4 since fix round 2), so even at the farthest zoom the model shows through
   the icon (`Blend::overlay_capped`). The cap is only the look: what switches with the zoom (click volume, ring and
   bars, effects) follows the uncapped ramp (`Blend::ramp`), so it switches at the same zoom whatever the cap is. The
   outline of a hovered or selected icon is drawn at full opacity (`icon.wgsl`), only the shape is dimmed.
4. **Models grow toward the far end; what follows the drawn size.** From `zoom.model_scale_from` (0.45) to zoom 1
   a smoothstep raises the scale from 1 to `zoom.model_scale_max` (2.5) for units and to `zoom.structure_scale_max`
   (1.6) for structures (`ZoomBands::model_scale`, `ZoomBands::structure_scale`; clamped, never above the cap; the
   file is refused unless 1 <= structure cap <= unit cap <= 2.5, `MODEL_SCALE_CAP`). The model grows about its
   feet. A motion is a structure when `Motion::with_structure` says so: `runner::view_motion` sets it from
   `SimSnapshot::is_structure_at` (the view has a footprint), `Looks::wreck_motion` for rubble
   (`WreckView::footprint`), and `GhostBook::motions` for every fog ghost (a ghost is a structure last seen). The
   frame carries both factors (`FrameView::with_model_scale`, `with_structure_scale`), and each record uses its own.
   **The rule: whatever the player sees or clicks follows the drawn size; what only chooses detail uses the true
   size.**

   | Consumer | Size | Pinned by |
   |---|---|---|
   | Instance matrix (`build_frame`) | drawn | `a_structure_is_drawn_and_picked_at_its_own_scale_and_a_unit_at_the_unit_scale`, `the_model_scale_grows_the_drawn_model_and_its_click_volume` |
   | Click volume `pick` | drawn | same two tests |
   | Pick tile pre-test (`grown`, the larger scale) | drawn | `a_structure_is_drawn_and_picked_...` (fails with `grown = 0`) |
   | Drag rectangle `pick_rect` | drawn | `the_rectangle_and_the_selection_marks_take_the_drawn_box_of_each_scale` |
   | `visit_drawn` / `mark_selected` (one helper, `drawn_unit`) -> ring, health bar, cargo bar | drawn | the same test, `the_ring_grows_with_the_drawn_scale_of_the_model` (sf_game) |
   | Tile frustum culling (tile box grown by the larger scale) | drawn | `a_unit_whose_drawn_box_reaches_into_the_view_is_not_culled_with_its_tile` |
   | Muzzle and smoke anchors `anchor_world` (muzzle flash, tracer start) | drawn, the last frame's scale | `an_anchor_is_where_the_drawn_barrel_carries_it` (also at 2x) |
   | Ghosts and rubble | drawn, structure scale | `a_structure_seen_and_then_left_behind_becomes_a_ghost_...`, `structure_rubble_is_drawn_at_the_structure_scale_and_a_hulk_at_the_unit_scale` |
   | M1 base structures (through `view_motion`) | drawn, structure scale | `the_scaled_structure_boxes_of_the_m1_base_overlap_at_most_in_a_sliver_at_the_farthest_zoom`: 2 pairs, 0.40 m |
   | Level of detail (`chain.select` by the true diameter) | true | by reading: a unit at 2.5 takes a coarser level than its drawn size would get |
   | Burst and smoke sizes of the effects (`fx.rs`, from the simulation's size) | true | by reading |
   | Construction, production, storage, repair and sale bars and signs (`construction_view::front_edge`) | true footprint edge | by reading |
   | Placement preview (`placement_view::draw_ghost`: footprint cells and outline) | true, by design | it marks the cells the structure will take |
   | Icons, icon anchor, minimap, rally flag, vision view | do not care | |

   Screenshots: at the farthest zoom of a 1280 x 720 window the 384 m M1 map is about 225 px wide (0.59 px per
   metre), so a structure of 12 m is about 7 px true and about 11 px drawn at 1.6; a vehicle is a few pixels; a
   soldier is a dot as a model and the icon is what reads. Overlap at the cap: on the M1 base, one pair per base (the
   construction centre and a neighbour) overlaps by 0.40 m; a click there goes to the box the ray enters first.
   Units overlap each other at 2.5 in a crowd; that is accepted (not measured in the 1,000 crowd). History of the
   numbers: the dated notes at the top.
5. **Picking at far zoom.** The click volume is the icon quad once the zoom is past the middle of the icon band
   (ramp above one half, `Blend::dominant`; not the capped opacity), the model's drawn box (at least `click_min_px`
   wide) before that. Never both, as before. Drag
   selection, the health bar and the cargo bar follow the same switch (`Blend::dominant`). The selection ring is drawn
   only while the model is the larger picture (ramp at most one half): in the far band the selected icon's own
   white outline shows the selection, as before (the models stay drawn, but a 2 px ring under an opaque icon is noise,
   and the test `selected_icons_are_highlighted...` had the rings of selected units land on neighbouring icons).
6. **Effects.** The effects fade with the icons: their weight is `1 - ramp` (`effects_weight`, into
   `FxFrame::model_weight`), the uncapped ramp, so the far end is still the strategic band with the cap. So
   particles and tracers are full up to `icons_from`, fade over the far band and are gone at the farthest zoom,
   where the death flash stays (`effects.md`, `ZoomBand::Strategic` exactly when the weight is 0). Decision: at the
   far end 1,000 units with particles cost more than they show, and the flash keeps a battle visible.
7. **Fog.** Unchanged: the units a player cannot see are removed on the CPU from the models and the icons alike
   (`fog_look.md`), at every zoom; the margin is empty clear colour.
8. **The icon toggle (fix round 2).** `ToggleIcons`, key K (I is `ProduceCycle`; K is free in `input.ron` and
   in the debug keys), rebindable in Settings > Keys ("Strategic icons on/off", one line in `menu_look.ron`). On by
   default: the band above. Off: `ZoomBands::blend_toggled` gives `Blend::MODELS` at every zoom, so no icon is drawn,
   the click volume, ring and bars stay on the model, and the effects are not faded by the band. Not saved between
   runs (an on/off line in the Settings tabs would be a new setting with storage and a widget, more than a one-line
   add, so it is the key only).
9. **Debug.** `--unit-display icons` still draws only icons (model weight 0, icon alpha 1), `models` the models
   without icons; F1 shows the icon opacity and the model scale. `--shots-zooms 0.1,0.45,1` makes `--shots` take its
   pictures at these zooms.

## Numbers (`assets/config/camera.ron`)

Every number of `CameraConfig` is in the file now (angles in degrees), pinned by `the_shipped_camera_file_is_pinned`;
`CameraConfig::default()` reads it (on the 256 m map; the game sets the map size). `far_margin` 1.0,
`icons_from` 0.95, `icons_full` 1.0, `icons_max_alpha` 0.4 (above 0, at most 1), `model_scale_from` 0.45,
`model_scale_max` 2.5 (1 to 2.5), `structure_scale_max` 1.6 (1 to `model_scale_max`). Bad values are refused when
the file is read (`bad_camera_files_are_refused`).

## Tests

* `camera.rs`: `the_farthest_zoom_shows_the_map_plus_a_full_map_margin` (64, 256 and 1024 m maps, four window
  shapes: the margin corners are on screen, snug within 10 %, the distance is 2.6 to 3.4 times the bare fit),
  `a_zero_margin_fits_just_the_map`, `an_explicit_max_distance_wins_over_the_margin`,
  `near_far_cover_the_margin_view_at_the_farthest_zoom` (no corner clipped by the planes).
* `zoom.rs`: the pinned file, the alpha band (0 below, 1 above, monotone, no jump), models never hidden by the zoom,
  the capped scale, `the_icon_toggle_off_draws_models_only`, `the_model_scale_cap_is_two_and_a_half`,
  `structures_grow_less_than_units`; `frame_tests`:
  `a_structure_is_drawn_and_picked_at_its_own_scale_and_a_unit_at_the_unit_scale` (instance matrix, drawn box and a
  click 6.5 m beside a 6 m cube: hit as a unit at 2.5, miss as a structure at 1.6).
* `instancing/frame_tests.rs`: `every_unit_is_a_model_at_every_zoom_and_the_icons_come_only_in_the_far_band` (the
  real camera and the shipped bands, 101 zooms), `the_model_scale_grows_the_drawn_model_and_its_click_volume`,
  `the_icon_band_position_decides_the_click_volume`.
* `tests/icons_screenshot.rs` (real renderer): `the_icon_overlay_blends_over_the_model_and_never_takes_a_model_pixel`
  (the pixel under the icon is the linear-light mix of model and icon by the alpha, the model's pixels are the same
  at every alpha), the 10,000 unit test (models at the farthest zoom, coarse levels, one icon draw call).
* `tests/terrain_lod_screenshot.rs`: `the_farthest_zoom_with_its_margin_has_no_holes_and_the_lod_holds` (all 64
  chunks in view, LOD against full detail within 5 % of the pixels, the silhouette does not move, the frame border is
  all clear colour, the map takes 20 to 60 % of the view).

## Re-pinned images

The goldens that use `CameraConfig::default()` at a zoom above 0 moved: `camera_mid`, `camera_far`,
`camera_cursor_zoom`, `glb_testpack_close`, `instancing_10k`, `terrain_close`, `terrain_far`, `terrain_relief`,
`terrain_types_far` (`crates/sf_render/tests/golden`). Cause, measured: with `far_margin: 0.0` in `camera.ron` all
of them match the old files again (and `terrain_lod_far` and `camera_close`, which did not move, match with both);
only the distance of a given zoom changed. `terrain_lod_far` is kept at the old framing by passing `far_margin: 0`
in its test, and a new golden `terrain_lod_far_margin_320.png` pins the new framing. `sun_facing_slopes...` uses
zoom 0.5 instead of 0.55 (the same eye distance as before). No `sf_sim` golden, `.expected` file or checksum moved.

Fix round 2 (`far_margin` 1.0): measured first, with `far_margin: 0.25` and `start_zoom: 0.35` back in `camera.ron` every
image test of `sf_render` matched the round-1 goldens. Re-pinned because they are about the framing: `camera_mid`,
`camera_far`, `camera_cursor_zoom` (the wheel test now takes five stages of six notches to reach the closest zoom),
`terrain_lod_far_margin` (the map share is 0.052 of the view, the bound is 0.03 to 0.15). Kept at the round-1 framing by
`far_margin: 0.25` in their test cameras, because they test shapes, colours, crowds and terrain materials:
`instancing_screenshot`, `terrain_screenshot`, `glb_screenshot`, and `selected_icons_are_highlighted...` in
`selection_screenshot`. `fog_screenshot`'s whole-map view uses zoom 0.77 (the distance 0.9 had). Logic tests that
placed units for the old pixel scale were spread (`frame_tests`: icon picking, rectangle). No `sf_sim` golden moved.

## Not done

* A soldier is still 1 to 2 px as a model at the farthest zoom, cap or not; whether infantry needs its own scale is
  a question for Tijmen.
* The sound still attenuates with the zoom by `audio.ron` as before (untouched, as decided).
* No GTX 1650 measurement: see the pull request for the container numbers.
* Fix round 2 performance (container, llvmpipe, release, 1,000 crowd units at the farthest zoom, six alternating runs against
  8207a4f): median frame 17.4 ms against 23.2 ms; terrain 1,024 of 1,024 chunks, all at the coarsest level, 9,984 triangles in one
  draw call, in both; with the icons off 13.4 to 14.9 ms. No cliff, so no distance LOD was added. Checked at the new distance:
  near/far planes (`near_far_cover_the_margin_view_at_the_farthest_zoom`), no terrain holes and the LOD holds
  (`the_farthest_zoom_with_its_margin_has_no_holes_and_the_lod_holds`, re-pinned), fog and the minimap (`fog_screenshot`,
  screenshots), pitch still follows the zoom.
* With the icons off the effects are not faded by the band (ramp 0): particles are drawn at the farthest zoom too.
* The icon toggle is not saved between runs.
* Follow-ups (zoom fix round 4, Fable plan): the bars and signs of construction, production, storage, repair and
  sale sit on the true footprint edge, not the drawn one; the level of detail is chosen by the true size, not the
  drawn one; the effects are not faded with the icons off; the toggle is not saved; infantry has no scale of its own;
  `runner::motion_of` is public and returns a motion without the structure flag (its callers only read `.shape`
  today); make it private or fold the flag into it.
* Not tested: that the runner passes the K toggle (`icons_on`) through to the picking. Checked by reading:
  `Runner::build_units` builds the frame with `blend_toggled(zoom, self.icons_on)`, and `pick` reads that frame's
  blend (`the_icon_toggle_off_draws_models_only` pins `blend_toggled`).
