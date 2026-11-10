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
4. **Models grow toward the far end.** `zoom.model_scale_from` (0.45) to 1.0: a smoothstep from scale 1
   to `zoom.model_scale_max` (2.5, a cap: the function is clamped, never above it; the file may not set more than
   2.5, `MODEL_SCALE_CAP`), about the model's feet. Applied in
   `build_frame` to the instance matrices (`FrameView::model_scale`). The level selection and the icon anchor keep the
   true size. **Changed 10-11-2026 (fix round 1, orchestrator decision):** the click volume, the selection ring, the
   health bar and the cargo bar follow the drawn size (`DrawnUnit::size` and the box of `pick`/`pick_rect` are times
   the frame's model scale), so a click on the visible rim of an enlarged structure hits it and the ring goes around
   what is drawn. Checked on the M1 base at the cap
   (`the_scaled_structure_boxes_of_the_m1_base_overlap_within_the_measured_bound_at_the_farthest_zoom`): at the cap of
   1.6 (round 1) one pair per base overlapped by 0.40 m; **at 2.5 (round 2) 8 pairs of the 9 structures overlap, the
   worst by 13.0 m** (at 2.0: 5 pairs, 6.0 m). The test pins that measured bound, the scale was not lowered to hide it;
   a click there goes to the box the ray enters first. Round 1 decided by screenshots
   (`docs/screenshots/wp-zoom/`): the 384 m M1 map is 420 px wide at the farthest zoom of a 1280 x 720 window
   (about 1.1 px per metre), so a structure of 3 cells (12 m) is about 19 px true and 30 px with the cap, and a soldier
   is 1 to 2 px either way. The cap makes structures and vehicles readable as models; a soldier is only a dot as a model
   at that zoom, and the icon is what reads. Round 1 kept 1.6 for that reason; round 2 raised it to 2.5 because the farthest zoom is about twice as far (the 384 m map is about 225 px wide at 1280 x 720, 0.59 px per metre), so a structure of 12 m is about 7 px true and 18 px drawn, and accepts the overlap above.
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
`model_scale_max` 2.5 (1 to 2.5). Bad values are refused when
the file is read (`bad_camera_files_are_refused`).

## Tests

* `camera.rs`: `the_farthest_zoom_shows_the_map_plus_a_full_map_margin` (64, 256 and 1024 m maps, four window
  shapes: the margin corners are on screen, snug within 10 %, the distance is 2.6 to 3.4 times the bare fit),
  `a_zero_margin_fits_just_the_map`, `an_explicit_max_distance_wins_over_the_margin`,
  `near_far_cover_the_margin_view_at_the_farthest_zoom` (no corner clipped by the planes).
* `zoom.rs`: the pinned file, the alpha band (0 below, 1 above, monotone, no jump), models never hidden by the zoom,
  the capped scale, `the_icon_toggle_off_draws_models_only`, `the_model_scale_cap_is_two_and_a_half`.
* `instancing/frame_tests.rs`: `every_unit_is_a_model_at_every_zoom_and_the_icons_come_only_in_the_far_band` (the
  real camera and the shipped bands, 101 zooms), `the_model_scale_grows_the_drawn_model_only`,
  `the_overlay_decides_the_click_volume_by_its_opacity`.
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

## Not done

* A soldier is still 1 to 2 px as a model at the farthest zoom, cap or not; whether infantry needs its own scale is
  a question for Tijmen.
* The frustum culling of the tiles still uses the true size: a model scaled 1.6x at the very edge of the screen can
  be culled a little early (the tile margin covers most of it).
* The sound still attenuates with the zoom by `audio.ron` as before (untouched, as decided).
* No GTX 1650 measurement: see the pull request for the container numbers.
