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

1. **The farthest zoom shows the whole map plus a quarter map on every side.** `camera.far_margin` (0.25) is a
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
3. **Strategic icons are an overlay in the farthest band.** Alpha 0 up to `zoom.icons_from` (0.85), 1 from
   `zoom.icons_full` (1.0), a smoothstep between: the last 15 % of the wheel travel. The zoom is the camera zoom
   (0 to 1, logarithmic), so the band is the same stretch of wheel on every map and resolution (the old trigger was
   a reference unit's apparent size). The icon shader multiplies its coverage by the alpha; the icons are still one
   draw call, not depth tested, over everything (`icons.md`). **Opacity cap (decision 10-11-2026, fix round 1):** the
   drawn opacity is the ramp times `zoom.icons_max_alpha` (0.6), so even at the farthest zoom the model shows through
   the icon (`Blend::overlay_capped`). The cap is only the look: what switches with the zoom (click volume, ring and
   bars, effects) follows the uncapped ramp (`Blend::ramp`), so it switches at the same zoom whatever the cap is. The
   outline of a hovered or selected icon is drawn at full opacity (`icon.wgsl`), only the shape is dimmed.
4. **Models grow a little toward the far end.** `zoom.model_scale_from` (0.55) to 1.0: a smoothstep from scale 1
   to `zoom.model_scale_max` (1.6, a cap: the function is clamped, never above it), about the model's feet. Applied in
   `build_frame` to the instance matrices (`FrameView::model_scale`). The level selection and the icon anchor keep the
   true size. **Changed 10-11-2026 (fix round 1, orchestrator decision):** the click volume, the selection ring, the
   health bar and the cargo bar follow the drawn size (`DrawnUnit::size` and the box of `pick`/`pick_rect` are times
   the frame's model scale), so a click on the visible rim of an enlarged structure hits it and the ring goes around
   what is drawn. Checked on the M1 base at the cap
   (`the_scaled_structure_boxes_of_the_m1_base_overlap_at_most_in_a_sliver_at_the_farthest_zoom`): in each of the
   two bases one pair (the construction yard and a neighbour, 8 m apart at true size) overlaps by 0.40 m, all others are clear;
   a click there goes to the box the ray enters first. Decided by screenshots
   (`docs/screenshots/wp-zoom/`): the 384 m M1 map is 420 px wide at the farthest zoom of a 1280 x 720 window
   (about 1.1 px per metre), so a structure of 3 cells (12 m) is about 19 px true and 30 px with the cap, and a soldier
   is 1 to 2 px either way. The cap makes structures and vehicles readable as models; a soldier is only a dot as a model
   at that zoom, and the icon is what reads. A bigger cap would make neighbouring structures overlap more (at 1.6 one pair per M1 base touches, by 0.40 m), so 1.6 stays.
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
8. **Debug.** `--unit-display icons` still draws only icons (model weight 0, icon alpha 1), `models` the models
   without icons; F1 shows the icon opacity and the model scale. `--shots-zooms 0.1,0.45,1` makes `--shots` take its
   pictures at these zooms.

## Numbers (`assets/config/camera.ron`)

Every number of `CameraConfig` is in the file now (angles in degrees), pinned by `the_shipped_camera_file_is_pinned`;
`CameraConfig::default()` reads it (on the 256 m map; the game sets the map size). `far_margin` 0.25,
`icons_from` 0.85, `icons_full` 1.0, `icons_max_alpha` 0.6 (above 0, at most 1), `model_scale_from` 0.55,
`model_scale_max` 1.6. Bad values are refused when
the file is read (`bad_camera_files_are_refused`).

## Tests

* `camera.rs`: `the_farthest_zoom_shows_the_map_plus_a_quarter_map_margin` (64, 256 and 1024 m maps, four window
  shapes: the margin corners are on screen, snug within 10 %, the distance is 1.35 to 1.65 times the bare fit),
  `a_zero_margin_fits_just_the_map`, `an_explicit_max_distance_wins_over_the_margin`,
  `near_far_cover_the_margin_view_at_the_farthest_zoom` (no corner clipped by the planes).
* `zoom.rs`: the pinned file, the alpha band (0 below, 1 above, monotone, no jump), models never hidden by the zoom,
  the capped scale.
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
