# Strategic icons

Work package WP3.6 (PLAN.md phase 3, on the icon system of WP1.9). Code:
`crates/sf_render/src/icon_atlas.rs` (loading, the distance fields), `icons.rs`
(settings, instance, parameters), `renderer.rs` (`Renderer::set_icon_atlas`),
`assets/shaders/icon.wgsl`, `crates/sf_game/src/roster.rs` (a role's icon), and the
placeholder set in `assets/icons/` written by `tools/sf_icon_gen`. Tests:
`icon_atlas/tests.rs`, `crates/sf_render/tests/icon_atlas_screenshot.rs`,
`crates/sf_game/src/roster.rs`, `crates/sf_data/tests/units.rs`.

When the camera is far away every unit is an icon in its owner's colour (PLAN.md section
2.2); the cross-fade, the click quad and the hover and selection outlines are
unchanged from WP1.9 and WP3.3. This document is about what the icon *looks like* and
how an artist replaces it.

## The design (art brief)

**One icon per role, shared by both factions.** The owner's colour shows who owns the
unit; the sign inside shows what it is. The base shape is the kind of role:

| Base shape | Role kind | `strategic_icon` |
|---|---|---|
| triangle | vehicle | `Triangle` |
| square | structure (and object) | `Square` |
| circle | infantry | `Circle` |
| half-circle (a bowl, flat deck up) | vessel | `HalfCircle` |

The placeholder signs (`tools/sf_icon_gen`; the names are the role ids of
`assets/config/unit_defs.ron`):

* **circle:** rifleman dot, rocket_trooper diagonal stripe, engineer plus, scout eye,
  sniper crosshair, medic two horizontal bars (never a cross), demolition_team asterisk,
  combat_engineer plus with a dot, heavy_gun_team T, civilian empty, technician wrench;
* **triangle:** harvester diamond, light_tank one stripe, heavy_tank two stripes,
  siege_artillery arc, repair_truck wrench, light_transport short bar, transport_truck
  long bar, amphibious_transport wave, supply_truck small empty square;
* **half-circle:** gunboat vertical stripe, ferry long horizontal bar;
* **square:** construction_center hammer, power_plant lightning bolt, refinery diamond,
  barracks dot, vehicle_factory gear, radar two arcs and a dot, defence_tower sight
  ring, advanced_defences double sight (two rings), shipyard wave,
  **decoy_emplacement the same picture as the defence tower** (so it fools the enemy),
  fortified_bunker sight with a thick bottom edge, automated_gun sight with a dot,
  unmanned_artillery arch with a dot, relay_transmitter mast, fuel_depot drop,
  generator bolt in a ring, archive_building three lines.

Notes on the placeholders: the brief gave radar and unmanned_artillery the same sign
("arc with dot"); they differ here (radar: two quarter arcs spreading from a dot in the
corner; artillery: a symmetrical arch with a dot under it) because no two icons may look
alike. The tanks' stripes are vertical, the transports' bars horizontal, for the same
reason. The **bridge segment, rail spur, evidence crate and ore field** have no icon
of their own: they are drawn as the plain base shape (square). 39 roles have an icon
name, 37 pictures: `decoy_emplacement` is the tower's file and `civilian` is the plain
circle (`base_circle`), both by an entry of `icons.ron`.

## The format (for artists)

* **File:** `assets/icons/<name>.png`, **64 x 64**, 8-bit RGBA (grey, grey with alpha
  and RGB are read too). One file, one picture; no sizes, no mip levels (the engine makes
  what it needs).
* **A white symbol on transparent.** The **alpha is the silhouette**: opaque where the icon
  is, transparent around it. Inside it, **white** is where the owner's colour shows
  (the engine tints it: draw it white, not coloured). The **inner sign is black or cut
  out** (transparent holes inside the silhouette). Both are read the same way. A
  transparent area that touches the border of the image is background, not a sign, so a
  notch open to the outside changes the silhouette.
* **The engine draws its own dark rim** just inside the edge (1.5 px at UI scale 1,
  `icons.outline_px`), so do not draw one. At 14 px it takes about 7 texels of the 64:
  keep signs that must stay separate from the edge about 8 texels inside it.
* **The image is the art box**: it is drawn 1.2 times as large as the icon size
  (`icons.size_px`, 14 px; the box is 16.8 px). So the nominal 14 px is 53.3 texels:
  draw the square and the circle (diameter) **53.3 texels across, centred**, the
  triangle up to 62 texels wide (it is wider than the click quad on purpose, to weigh as
  much as the square), the half-circle 63 texels wide. **Leave at least one texel of
  transparency around the silhouette** (the generator test checks the placeholders).
* **Readable at 14 px.** A texel is 0.22 px at 14 px: a stroke needs to be **6 or more
  texels thick** (a pixel and a half) to survive, details smaller than 8 texels vanish.
  The picture has to work in all four player colours and on green, sand, grey and snow
  terrain (the contact sheet below).
* **Distinguishable.** At 14 px every pair of icons must differ in at least 10 of the 196
  pixels in every player colour (below).

Do not use anything from another game (PLAN.md section 12.4): every icon needs a licence
line in `assets/LICENSES.md`.

## Names, `icons.ron` and the fallback

`assets/icons/icons.ron` (optional) maps an icon **name** to a file:

```ron
(icons: { "decoy_emplacement": "defence_tower.png", "light_tank": "light_tank.png" })
```

Every other `*.png` in the directory is an icon named by its file stem (`rifleman.png`
is `rifleman`), so a new icon needs a file and nothing else. Names are `a-z 0-9 _`, at
most 48 characters. Names that point at the same file share one picture. The file also
decides `base_square`, `base_circle`, `base_triangle` and `base_halfcircle`: these four
are the plain shapes, always the first four layers, and the copies compiled into the game
are used when a file is missing.

`unit_defs.ron` says which picture a role has with **`icon: "<name>"`**. The fallback:

* a role without `icon`, or whose name the set does not have, is drawn as its base shape
  `strategic_icon`, which is the shape of its category (a test keeps the shipped
  definitions to that). A name the set does not have is also a warning in the log when
  the game starts (`sf_game: <role>: icon "x" is not in the icon set`);
* a file that cannot be read, is not 64 x 64, has no opaque pixel, or has a bad name is
  a warning in the log (`sf_game: icons: ...`) and the name is missing, so the same
  fallback; the game never stops for an icon;
* an icon id past the uploaded atlas (a bigger set than the one on the GPU) is the plain
  square, not an invisible icon.

`icon` used to be the build menu picture (`testpack_v1/icons/...png` paths, read by
nothing). It is the strategic icon's name now; the build menu (WP11.1) will get a field of
its own.

## How the engine draws it

Not the PNG itself but two **signed distance fields** made from it when the set is
loaded (an exact Euclidean transform, `icon_atlas::sdf_layer`): one for the silhouette
and one for the sign, 8 bits each, 16 texels either side of the edge. They go into one
texture array (`Renderer::set_icon_atlas`: **once**, not per frame, 2 x 64 x 64 bytes per
picture). The shader takes the distance to the edge in screen pixels from them, so the
edge, the rim, the sign and the outlines are crisp at every size and no mip chain or
texture minification is involved. A layer index is in the top half of the instance's flags
(`IconInstance::icon`); the instance is still 32 bytes and all icons are still **one draw
call**, which the benchmark and the tests check.

* the owner's colour fills the silhouette, the sign is the dark colour, the rim is the
  dark colour `icons.outline_px` wide inside the edge;
* hovered: a white outline `icons.hover_out_px` wide **outside** the edge; selected: a
  white outline `icons.selected_out_px` wide; both: the selected width in yellow. The
  owner's colour keeps its area in all four states;
* the cross-fade with the models is unchanged (the dither of WP1.9).

**UI scale (the WP3.3 review follow-up).** Every length is in physical pixels and scales with
the UI scale: the icon size, the rim, both outline widths and the room the quad leaves
around the art box (`IconConfig::params`). The margin is `ceil(widest outline) + 2` px
(the outline, plus one pixel for its antialiased edge and one for the art box not being
pixel aligned), so the quad always holds the outline: before WP3.6 the margin was 4 px
whatever the scale, and at UI scale 2 the corners of a selected triangle (which reach
past the click quad) were cut off. The click quad itself is still `size_px` square
(`pick.rs` is unchanged); the triangle's corners reach 1.2 times as far as the square's.
The test `the_outline_of_every_shape_fits_the_quad_at_every_ui_scale` draws every base shape in
every state at scales 1, 1.25, 1.5, 2 and 3 and fails if a pixel of the outline lies on the border
ring of the quad (it fails with the old fixed margin).

## Distinguishability at 14 px

`every_icon_is_distinguishable_at_14_px_and_the_contact_sheet`
(`crates/sf_render/tests/icon_atlas_screenshot.rs`) draws every picture of the shipped set
through the real renderer at the real 14 px, in the four player colours (red, blue,
yellow, magenta) on green, sand, grey and snow, and counts for every pair the pixels of
the 14 x 14 tiles that differ (a colour channel more than 40 of 255 apart). A pair's score
is its smallest count over the 16 colour and terrain combinations. **The score of every
pair must be at least `MIN_DIFFERING_PIXELS` = 10** (of 196, 5 %). Placeholders
reach 12 at the closest pair (a plain triangle and the light transport's short bar, a plain
triangle and the light tank's stripe, the plain half-circle and the gunboat, rifleman and the
plain circle, ...), so the margin is thin on purpose: signs of the triangle and of
the half-circle are what the 14 px have little room for. The test prints the twelve closest
pairs (`cargo test -p sf_render --test icon_atlas_screenshot -- --nocapture`). The plain
base shapes are pictures too, so a sign has to differ from the shape it sits in; that is
why `civilian` is the plain circle and nothing may be "almost empty".

The contact sheet is `docs/screenshots/wp3.6/icons_14px.png` (real size: one column per
picture, the legend in `icons_14px.txt`, rows are the four player colours, the four blocks
are the four terrains), and `icons_14px_x4.png` four times as big. Make them again with
`SF_SAVE_SHOTS=docs/screenshots/wp3.6 cargo test -p sf_render --test icon_atlas_screenshot`.
The terrain colours are flat (the grass and rock materials, sand, a snow white): the
sheet shows contrast, not the texture of the real terrain. Two more frames in the same folder:
`role_icons_strategic.png` (a few role icons through the instancer at strategic zoom, the
screenshot test) and `sandbox-strategic.jpg` (the `sf_game` sandbox, all roles of three sides,
two of the units selected).

## Replacing a placeholder

1. Draw the picture to the format above, 64 x 64, and save it as
   `assets/icons/<name>.png` over the placeholder (the name is the role id in
   `unit_defs.ron`). No code, no other file changes.
2. **Remove that name from the generator** (`icon_specs` and `sign` in
   `tools/sf_icon_gen/src/lib.rs`; also `ALIASES` if it is an alias): its test
   `the_committed_files_are_what_the_generator_writes` compares the committed files with
   what the generator writes, and running the generator would write the placeholder over
   your file. Then `cargo run -p sf_icon_gen -- --check` is quiet again.
3. Record the licence in `assets/LICENSES.md` (author and licence of the new files).
4. Run `cargo test -p sf_render --test icon_atlas_screenshot -- --nocapture`: the
   distinguishability test reports the closest pairs with your icon in; make the contact
   sheet and look at it at real size.
5. A role that should look like another (the decoy) gets an `icons.ron` entry pointing at the
   other's file, not a copy of it. A role that should get its own picture after all
   (the civilian, say) gets a file and loses its `icons.ron` alias.

`cargo run -p sf_icon_gen` writes the placeholders again (it is deterministic: the same bytes
every time, on every machine: f64 arithmetic with only `+ - * /` and `sqrt`, which IEEE 754 makes exact).

## Decisions that were open

* **`icon` in `unit_defs.ron` is the strategic icon's name** (the task of WP3.6), not the
  build menu path it was documented as; nothing read the old value. The two frozen
  definition copies under `assets/scenarios/*_defs` lost their `icon:` lines (the headless
  checksums do not depend on them).
* Signed distance fields instead of plain colour texels: a 64 x 64 texture minified to 14 px
  is soft or aliased with any filter, and the rim and the outer outlines need a distance
  to the edge anyway.
* The half-circle is a bowl (flat deck up), the triangle apex up.
* `civilian` and `decoy_emplacement` are aliases, not files, so that a pair of identical
  pictures never has to be excepted from the distinguishability rule.
