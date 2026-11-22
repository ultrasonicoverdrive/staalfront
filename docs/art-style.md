# Art style: set v5 (WP11.6a)

The rules every generated model and cutscene part follows from WP11.6a on, so that WP11.6b-d and
the Phase 12 restyle look like one game. Style **G, "semi-realistic, muted"** (decision Tijmen
14-11-2026; made realistic in colour by WP11.6p, decision Tijmen 15-11-2026: "the images must be much
more realistic in colour"; the bright palette of WP11.6a is gone). The reference was a small concept sheet generated with Canva at the project owner's
request; it is a style reference only and is not in the repo. A later, more realistic target
(style F) needs textures and shadows in the engine: that is a separate engine WP, see "Later" below.

Everything here is made by our own generator, `tools/sf_model_gen` (`cargo run -p sf_model_gen`):
no external meshes, no textures, no downloaded art. The numbers that tune the look live in
`assets/config/model_palette.ron` (colours) and in the recipes and kit constants of the tool.

## 1. Palette (`assets/config/model_palette.ron`)

sRGB. One slot per surface type, the same six material slots in every model and in this order, so a
later texture step can key on them: `paint` (main hull paint), `paint2` (second paint), `team`,
`metal` (gun steel, fittings), `rubber` (tracks, tyres, running gear), `accent`.

| Slot | Meridian | Veyran | Neutral | Network |
|---|---|---|---|---|
| `paint` | warm sand `#b2a088` (178, 160, 136) | cool grey-blue `#6c7a8a` (108, 122, 138) | grey-green `#888e80` (136, 142, 128) | graphite `#4a4c50` (74, 76, 80) |
| paint light (highlight, `paint3` of neutral) | `#c8baa4` (200, 186, 164) | `#8896a4` (136, 150, 164) | `#9ea494` (158, 164, 148) | `#5c5f64` (92, 95, 100) |
| `paint2` | olive `#5e5838` (94, 88, 56) | dark slate `#3c424c` (60, 66, 76) | `#565a4c` (86, 90, 76) | `#303237` (48, 50, 55) |
| paint2 shade | `#45422e` (69, 66, 46) | `#4e535a` (78, 83, 90) | `#42463c` (66, 70, 60) | `#3c3f46` (60, 63, 70) |
| `metal` | `#3c3932` (60, 57, 50) | `#3a3d43` (58, 61, 67) | `#42413c` (66, 65, 60) | `#323237` (50, 50, 55) |
| `rubber` | `#2e2c28` (46, 44, 40) | same | same | same |
| `accent` | glass `#3e4a52` (62, 74, 82) | dull crimson `#7a1e28` (122, 30, 40) | `#7a4a30` (122, 74, 48) | amber `#8a6420` (138, 100, 32) |
| `team` | the player colour (the file stores a neutral `#ebebeb`; the engine replaces it, at full strength) | same | none (third slot is `paint3`) | same as Veyran |

WP11.6p (15-11-2026): the realistic palette. The first sample of the Canva restyle of the M1 shot
(reference only, not in the repo) gave the on-screen colours; they were lifted to albedo values (a top
face renders at about 0.95 of its albedo, a side at 0.5-0.8), and Veyran was set slightly bluer so the
factions stay apart at distance: Meridian reads warm tan and olive, Veyran cool grey-blue, at the
in-game zoom of 0.12 too (`docs/screenshots/wp11.6p/`). Previous values (style G bright): Meridian
paint `#dcb450`, Veyran paint `#7894b2`. No channel bound is pinned on the paints (Meridian paint
light reaches 200); the test pins muting instead: every `paint` and paint light has a saturation of at
most 0.30, and every `rubber` a brightest channel of at least 40. The Veyran accent stays darker than
any player colour (no channel above 160, asserted in the same test file).

A test pins the shipped values (`tools/sf_model_gen/tests/models.rs`,
`the_shipped_palette_has_the_realistic_values`).

## 2. Shape language

- **Meridian: rounded.** Large chamfers on hull and turret corners, domes, round tanks, a lying
  drum; sand hull, olive fenders, covers and fittings.
- **Veyran: angular.** Wedges with a pointed nose, sloped plates, octagonal stacks, sharp gun heads;
  steel blue-grey plates over a gunmetal base.
- **Proportions realistic, the silhouette slightly exaggerated:** turret and barrel about 15-20 %
  oversized against a real vehicle of that size, so they read at the 80 degree far view.
- **Detail as real geometry**, within the budgets: panel lines (thin raised strips, toned darker),
  hatches (a rim and a darker lid), louvres and vents, track cleats and road wheels, bolts on large
  flat areas, antennas. Dark running gear (`metal`, `rubber`).
- **Footprint:** a model stays inside the model it replaces (set v4) and inside the footprint of its
  role in `unit_defs.ron`; the barrel included.

## 3. Team colour

The team colour is the engine's dynamic region (the material named `team`):

- **narrow stripes, not frames** (refinement round 2, 14-11-2026): every band is about
  0.25-0.35 m wide, so the team colour reads as markings; a test holds every upward-facing team
  triangle of LOD0 and LOD1 to at most 350 mm across (`every_team_band_is_narrow`; LOD2 is the far
  view, where the harvester drum keeps two wider bands for its 150-triangle budget). Where narrow
  stripes do not reach 20 %, add stripes rather than widening them: two concentric roof bands
  (`team_rings`, built with `insets` so the band is equally wide across the chamfers), a platform
  edge stripe, two cross bands on a deck, stripes on a crane arm, a ring on a tank top, four narrow
  drum bands;
- **bands and trim, never whole surfaces** (tuning round 14-11-2026): a band round the turret roof
  edge, a stripe across the rear deck, a stripe along the fenders or the hull top, panels on the
  skirts, a band round a building's roof edge, a flange on a stack, the shoulder of a tank, a pad
  marking, bands round a drum; never a full roof, never a single triangle and never a diagonal. A
  roof band is built with `kit::frame` and the roof inside it is filled flush (`team_frame` in the
  recipes), because a band standing proud of a large roof face darkens all of it in the baked AO;
- **aim at 20-25 % of the top-down view in every level**; `sf_asset_check`'s `team-share` rule still
  asks 20-40 %, the generator test caps it at 30 %
  (`the_checker_finds_nothing_and_the_team_share_is_20_to_30_percent_in_every_level`); at least 12
  team triangles per level (`the_team_colour_is_bands_and_panels_never_one_triangle`);
- the team vertices carry no baked shade (the loader ignores `COLOR_0` on them), so every player
  colour shows at full strength.

## 4. The fixed accent

Veyran's crimson is a fixed faction mark, not a team colour: **small** (a trim at the hull rear, a
slit on the tower, a glow band on a stack, corner lamps) and **darker than any player colour** (no
channel above 160), so a red player's bands are never confused with it. Meridian has no fixed
accent; its `accent` slot is glass.

## 5. Shading: baked into the vertex colour

The renderer has one directional light, an ambient term and no shadows or textures, so depth is
baked per vertex into `COLOR_0` (`tools/sf_model_gen/src/shade.rs`), which the loader multiplies into
the material colour:

- **ambient occlusion:** 17 rays per corner over the face's hemisphere, 1.4 m reach; the blocked
  share times 2 (`AO_GAIN`, tuning round: twice as visible), up to 60 % darker (so half the rays
  blocked is already the full darkening: creases, under overhangs, between hull and tracks);
- **grime:** up to 20 % darker at the ground (`GRIME` 0.20), fading out at 1.3 m (`GRIME_HEIGHT_MM`
  1300; WP11.6p; 40 % in the bright style, which was about twice the first 22 %);
- **dust** (WP11.6p, `DUST`, `DUST_SRGB`, `GRIME_HEIGHT_MM`): at the ground the colour moves 0.28 of
  the way towards the fixed warm earth colour (118, 104, 82) (sRGB, taken to linear and lit like the
  face: times the occlusion and the downward term), fading linearly to none at 1.3 m, the same height
  as the grime (hulls, tracks and the lower walls of structures). It is applied after the edge wear.
  Because the dust lifts a dark slot (rubber, dark metal, glass) towards a brighter colour, the
  material factor has a floor (`FACTOR_FLOOR`, below). A triangle of the team material gets neither
  grime nor dust (`shade::bake_team`; the loader ignores its vertex colour, so this only makes the
  bytes say what the engine does). Fixed numbers, no randomness. Tests:
  `dust_darkens_and_pulls_the_foot_towards_the_dust_colour_and_a_team_triangle_has_none`,
  `dust_lifts_rubber_and_metal_towards_the_dust_hue_without_clamping`,
  `the_dust_never_clamps_a_vertex_of_the_dark_slots`;
- **downward faces** up to 30 % darker;
- **facet variation** of +-3.5 % per flat triangle from a fixed hash (no randomness), so large flat
  areas are not one sheet; a triangle of a turned surface gets none (WP11.6r: on a round body it
  only showed the facets again; `no_smooth_part_carries_facet_variation`);
- the **tone** of a detail (panel line 50 %, was 56 % before WP11.6p; recess 72 %, track shoe 82 %);
- **top-edge highlight:** a face that slopes upward (a bevel or chamfer on top, normal `y` between
  0.2 and 0.97) is 5 % brighter (a few per cent since WP11.6p, was 16 %);
- **edge wear:** a bevel strip (no axis-aligned normal, narrower than 320 mm) whose corner sees open
  sky (no AO ray blocked) is, on about two thirds of its triangles (fixed hash), moved 20 % (`WEAR`, was 35 %) towards a
  target that is half a grey of the luminance times 1.12 plus 0.01 (`WEAR_GREY_GAIN`,
  `WEAR_GREY_LIFT`) and half the colour itself times 1.12 (`WEAR_TINT` 0.5): lighter and less saturated, as paint rubbed thin, not
  bright metal. Big slanted
  facets (a dome segment) are not edges and get none, and neither does any triangle of a turned
  surface (`kit::lathe`, marked `Tri::lathed`; refinement round 2: the narrow segments of the
  Meridian dome showed as light streaks; `a_lathed_dome_gets_no_edge_wear`).

The vertex colour is RGB: the wanted linear colour divided by the material factor, which is a grey at
the palette colour's brightest channel over 0.85, at least `FACTOR_FLOOR` 0.15 (`shade::factor`; WP11.6p review: a dark slot needs head room for the dust, which lifts it towards a brighter earth colour: before the floor 17,471 vertices of the set's rubber and metal clamped to flat grey, after it none, `the_dust_never_clamps_a_vertex_of_the_dark_slots`). A plain open face therefore shows
the palette colour within the per-facet variation and 8-bit rounding (the test allows 6 %), and highlights and worn edges stay inside glTF's 0..1 range. The team
material keeps its own factor and ignores `COLOR_0`. Tests: `tools/sf_model_gen/tests/shade.rs`
(one per rule: plain colour, highlight, wear, no wear on lathes, crease and grime).

**Normals** (WP11.6r, `shade::corner_normals`, written as glTF `NORMAL`): a flat triangle keeps its
own normal at all three corners. A triangle of a turned surface (`Tri::lathed`: drums, domes,
barrels, wheels, tanks) gets at each corner the mean of the distinct facet normals of the lathed
triangles of the level that share that corner and meet it at no more than the **crease angle of
40°** (`CREASE_DEG`, `CREASE_COS` 0.766044 as a fixed decimal); an edge sharper than that (the cap of
a drum, a box marked as turned) stays hard. The top-edge highlight and the downward term read that
corner normal, so a dome brightens smoothly; the AO rays keep the face's own hemisphere, and AO and
wear are otherwise unchanged. Test: `smooth_normals_agree_on_a_lathe_and_stay_hard_past_the_crease_angle`.

**Bevel rule** (WP11.6r, `kit::auto_bevel`): in LOD0 every box of the kit (`bevel_box`, `cuboid`,
`plate`, `closed_box`) whose smallest side is at least 240 mm gets a bevel of 8 % of that side,
20..=120 mm, unless the recipe asked for more; a closed box is bevelled on its bottom edges too (seen
at 45°). A smaller box is a detail (a seam, a rail, a post, a bolt row) and stays plain; the far
levels keep only the recipes' own bevels (so their share of LOD0 does not grow). The recipes'
explicit bevels (`Lod::bevel`) are half as wide again in LOD0, at most 60 mm more; a value under 10
is a switch (`v * lod.bevel(1)`) and stays. Lofted hulls with their own outlines (the six WP11.6a
models, the transports) only get the widened recipe bevels: rebuilding those outlines would be a
per-model edit, which this round did not do. Test: `every_lod0_block_is_bevelled_by_the_kit_rule`
also scans every LOD0 for sharp block corners (three axis faces of at least 240 mm meeting); the
extruded profiles that still have them are a pinned list (the fortified bunker's buttresses, a few on
the Veyran vehicle factory and the amphibious transport).

Keep colours muted and clean: the AO darkens creases, not whole surfaces; judge it in the engine
render (contact sheet), not only in numbers.

## 6. Levels of detail

One recipe builds all three levels from `kit::Lod`:

- round parts have at least 16 segments in LOD0 (WP11.6r, `ROUND_MIN_SEGS`: an eight-sided wheel
  or barrel showed its facets at 45°) and the recipe's number halved per level after that
  (`Lod::segs`: 8 gives 16, 4, 4; 16 gives 16, 8, 4);
- bevels are half as wide again in LOD0 (at most 60 mm more), halve at LOD1 and go at LOD2
  (`Lod::bevel`), plus the bevel rule of section 5 in LOD0;
- details drop out: wheels to three per side at LOD1 and none at LOD2, cleats, bolts, seams and
  hatches only at LOD0;
- the node tree, the anchors and the team panels stay in every level, and the bounds stay within
  10 % of LOD0 (parts that set the height, such as a crane or an antenna, stay in every level).

Budgets (WP11.6r, Tijmen 15-11-2026, `sf_asset_check::class::budget`): vehicles and vessels
4000 / 1200 / 300, buildings 8000 / 2400 / 500 triangles (were 1500 / 500 / 150 and 5000 / 1500 /
300); infantry (1500 / 250 / 60), decor and wrecks (1500 / 500 / 150) are unchanged. LOD1 at most
40 % and LOD2 at most 15 % of LOD0 (`every_level_is_inside_its_budget_and_really_reduced`). The
budget is a ceiling, not a target: after the shape round a vehicle has 1,378-2,046 triangles in
LOD0 and a finished structure 2,650-3,646. The far levels keep the silhouette: along every axis
LOD1 and LOD2 span at least 90 % of LOD0 and reach at most 5 % past it (`far_levels_keep_the_silhouette`;
states are skipped, being derived from the finished far level; one named exception: the rubble
heaps' height, the tip of one random chunk). The ferry's funnel and the gunboat's mast stay in every
level for this. The far levels' triangle counts are pinned against main before the round
(`tests/far_levels.rs`); `kit::at_level` sets the level for `Model::new` and for the stages, so the
LOD0-only rules never reach a far level.

**Detail budget** (tuning round 14-11-2026, `buildings_use_their_detail_budget`): a building uses
**2,500-3,500 triangles in LOD0** on real geometry: vents with louvres, pipe runs with flanges,
railings (`kit::railing`), roof units with fans, window rows with frames, door frames, bolted panels,
ladders and walkways; that detail is LOD0 only, so LOD1 and LOD2 stay far below (WP11.6r: now
2,500-6,000, the bevels and rounder parts on top). A vehicle has 1,200-3,000 (WP11.6r; was under
1,500) and spends it where it reads at game distance: capped road wheels with hubs, track
shoes (one raised face each), hatches, smoke dischargers, tools on the fenders, lamps and exhausts.

## 7. Node tree

As `docs/conventions.md` section 12 and `sf_asset_check` ask: roots `<stem>_LOD0..2`; vehicles
`hull -> turret -> barrel -> muzzle` (empty `turret`/`barrel` nodes for an unarmed vehicle);
buildings `body` (and `body -> turret -> barrel -> muzzle` for a defence) with empty anchors
`fx_smoke_0` and `fx_fire_0`. Vehicles get **no** smoke anchors (decision of WP11.6b, for all vehicles at once): a hurt
vehicle smokes by the engine's rule like every other vehicle (anchors on the v5 tanks changed what
`damage_screenshot.rs` counts as anchor puffs). An armed vehicle (`light_tank`, `heavy_tank`,
`siege_artillery`, `gunboat`) has the muzzle under the barrel; an unarmed one keeps empty `turret`
and `barrel` nodes. A figure (the heavy gun team) has the six figure nodes `torso`, `head`, `arm_l`,
`arm_r`, `leg_l`, `leg_r` directly under the root, like the set v3 figures, and its gun as a
seventh part. At most four parts per level (the engine's limit).

## 8. Cutscene parts

The same palettes. Each part of a cutout rig becomes an 8-bit RGBA PNG of the size of the shape it
replaces, so names, pivots and offsets stay (`tools/sf_model_gen/src/cutout.rs`):

- **two tones** per part, light towards the top left (110 %) and the base, plus a **soft third
  shade** (86 %) before the shade (72 %) towards the bottom right;
- a **thin dark outline** of 2 px (30 % of the colour);
- colours close to a palette swatch (within 40 per channel distance) take the swatch; skin, mouths
  and teeth keep their own.

WP11.6a only made the parts of the M1 briefing's two rigs (`assets/scenes/restyle_v5/`); the shipped
scene does not use them yet (Phase 12).

## 9. Units (WP11.6b)

Every vehicle and vessel role has a generated model (`tools/sf_model_gen/src/units*.rs`); the
infantry is Tijmen's set v3 (WP11.6b put it back, it is far better than the set v4 figures), and
only the Veyran heavy gun team, which had no good figure, is generated.

- **Both factions, one recipe, two outlines.** A role of both factions is one builder with a
  `Style` (`Round` or `Angular`): the same parts, the hull and turret outlines of the faction
  (`units::ring`: chamfered rectangle or wedge). Sizes: a model stays inside the set v4 model it
  replaces, the barrel and the stripes included (`FOOTPRINTS` in `tools/sf_model_gen/tests/models.rs`).
- **Tracked** (`heavy_tank`, `siege_artillery`, the harvester): the shared `Track` (cleats, shoes,
  road wheels, a plain block at LOD2), skirts over the upper run, team stripes across the deck, along
  the fenders and a band round the roof of the turret or casemate, never a whole roof.
- **Wheeled** (repair truck, transport truck, supply truck, light and amphibious transport): wheels
  are tyres with a tread step, a rim face and a hub at LOD0, plain eight-sided wheels at LOD1 and one
  block a side at LOD2 (`units_wheeled::wheels`); mudguards over every wheel; the outer face of the
  hubs counts for the footprint. A boat-shaped body (amphibious transport) is an extruded side
  profile with a sloped bow.
- **Vessels** (gunboat, ferry): the waterline is `Y = 0` and **lies inside the hull**, so the hull
  has a keel about 0.35-0.45 m under it (`sf_asset_check`'s vessel rule; every other model has its
  lowest point at `Y = 0`). Test: `vessels_float_and_everything_else_stands_on_the_ground`. The
  team colour of a long deck is narrow cross stripes every 0.6-0.9 m (never a wide panel: the
  350 mm rule holds), plus a band round the bridge roof or the turret.
- **Neutral models** (supply truck, ferry) have **no player colour**: the third material slot is
  `paint3` (the palette's `neutral` entry, `paint_light`), the file has no `team` material, and the
  recipes mark lanes and stripes with `PAINT3`. Tests: `a_neutral_model_has_no_team_material`, the
  neutral branches of the team tests.
- **Far levels keep what sets the silhouette**: the height of an antenna or a roll bar, the length
  of the barrel, the keel and the waterline; they drop details, not extent (`lod-size` stays quiet;
  since WP11.6r also `far_levels_keep_the_silhouette`, see section 6).
- **Detail budget of a vehicle: 1,200-3,000 triangles in LOD0** (`buildings_use_their_detail_budget`;
  WP11.6r, was 1,200-1,500),
  a figure 400-1,500 (60 at most in LOD2, which needs at least 400 in LOD0 to stay under 15 %).

## 10. Later (style F)

Style F ("realistic, bright") is the later target once the engine gets textures and shadows (an
engine WP, not WP11.6). The kit keeps a material slot per surface type for that: a texture step can
add UVs and generated maps per slot without rewriting the recipes.

## 11. Structures and their states (WP11.6c)

Every structure role of both factions, the ore silo, the mission objects and the rubble heaps are
generated (`tools/sf_model_gen/src/buildings.rs`, `objects.rs`, `stages.rs`; `set.rs` lists them).

- **One recipe per role, both factions.** The parts carry the shape language (`slab`, `hall`,
  `tank`, `stack`, `door`, `dish`, `turret`, `pad`, `bags`): Meridian rounded (big chamfers, round
  tanks, domes, a rounded lintel), Veyran angular (a dark skirt, upright walls to 55 % and sloped
  walls above, octagonal tanks and stacks, wedge gun heads, a sloped door hood, the crimson accent
  on stacks and slits). The three finished structures of WP11.6a keep their own recipes.
- **Team colour by fitting** (`stages::fit_team`): a recipe gives the fixed team parts (two roof
  bands, flanges, a pad marking) plus an ordered list of candidate stripes (platform-edge rings,
  lane stripes, crane-arm stripes); candidates are added one by one until the top-down share
  reaches `TEAM_AIM` (21.6 % on a 400-pixel raster, so `sf_asset_check`'s 512-pixel raster reads
  20-30 %) and the level has at least 12 team triangles; the rest are left out. Every candidate is
  a stripe of at most 350 mm in LOD0 and LOD1.
- **Detail budget** (`stages::pad_model`): a finished structure has 2,500-6,000 triangles in LOD0
  (WP11.6r, was 2,500-3,500);
  a recipe that falls short, or whose reduced levels would exceed 38 % / 14 % of LOD0, gets rows
  of slab-coloured anchor bolts along its platform edges until it does. WP11.6r kept this floor
  (2,650): dropped, the finished small structures fall to 1,244-2,054 triangles and 21 of their
  stage files break the far-level share rule (the barracks foundation: LOD2 100 of 614, 16.3 %),
  because a stage's far levels come from the finished far level; lighter stage far levels come
  first (a gap, not done here).
- **Team fit** (`stages::fit_team`): when every candidate is used, a share up to `FIT_TOLERANCE`
  (0.010) under the aim is kept; anything lower stops the generator with an assertion that names
  the model and the share (WP11.6r: before, it panicked on 0.003 short and needed `SF_FIT_SOFT=1`,
  which hid the stage tests' own failures).
- **States from the finished model** (`stages::derive`), for every structure the same way:
  - *foundation*: a poured slab of the finished footprint, formwork boards, footing beams in a grid
    (team candidates: survey markings), rebar stubs and pallets (LOD0), survey stakes; at most 1.5 m;
  - *scaffolding*: the finished model cut at 60 % of its roof level (`stages::roof_level`: the
    height under which half the top-down area of the body lies, so a crane or an antenna does not
    set it; triangles across the cut are clipped, `stages::clip`), a light deck over the open
    walls, poles, ledgers, planks with team toe boards and a crane on one corner; the cut walls are
    bare (no panel lines, bolts, glass or steel fittings yet);
  - *damaged*: a hole blown in the roof at the one of four fixed places that takes the least team
    colour with it and then the most geometry, soot round it (team at the hole's rim burnt to the
    second paint), the small detail gone within 8 hole radii, debris on the free platform; the
    hole grows (1, 1.5, 2, 3 times, `HOLE_GROWTH`) until the stage is inside its ceiling;
  - *heavily damaged*: broken off along a slanted plane at half the roof level (slope at most
    250 mm/m), soot and char along the break, a heap of chunks inside the shell with fallen band
    pieces (team candidates) on top, debris, no small detail; a turret sinks onto the break and the
    smoke and fire anchors move down to it (a heap broken off under 30 % has lost its turret);
  - **no state is richer than its finished structure** (WP11.6c review): a state takes nothing of
    the finished model's anchor-bolt filler and has no detail floor; its LOD0 is at most a share of
    the finished LOD0 (`STAGE_CEIL_PERMILLE`: 30 / 70 / 90 / 70 %), its LOD1 and LOD2 are built on
    the finished far level, and anchor bolts are added only where a reduced level would otherwise
    exceed its share, never past the ceiling (tests `buildings_use_their_detail_budget`,
    `no_state_has_more_triangles_than_its_finished_model`); it stays inside the finished
    footprint; its team stripes are their top face only, up to 600 mm wide at LOD2 and never
    wider than a fifteenth of their length (`FAR_STRIPE_MM`).
- **Mission objects** are neutral (no player colour; the third slot is `paint3`, as the neutral
  units of WP11.6b) and keep the state names of set v4: `_damaged`, `_heavily_damaged`, `_rubble`,
  `_burnt_out`, `_destroyed` (heavy stages broken off lower), `_off`, `_disabled`, `_no_uplink`
  (lamps dark, a dish gone). The automated gun and the unmanned artillery are `network_`: they
  carry the player colour, palette entry `network` (graphite, amber lamp).
- **Rubble heaps** (`neutral_rubble_<w>x<h>`, `wrecks.ron`): a dusty spread of chunks, bent plates
  and rebar inside the set v4 heap's size, decor budget.
- **Far levels keep the height**: antennas, crane masts, stakes and posts stay in every level;
  an outer octagon stays eight-sided at LOD2 where it sets the bounds (the automated gun).

## 11. Terrain (WP11.6e, 21-11-2026)

The terrain has the same muted, realistic look as the models (decision Tijmen 15-11-2026, 11.6p). It is
data only (`assets/config/terrain_look.ron`), no shader or model change; `docs/rules/terrain.md` has the
rules and the tests. The two reference greens of the approved shot, `#4F511B` (79, 81, 27) and `#484E17`
(72, 78, 23), are used as numbers only; the stored grass colour is chosen so that what reaches the screen
lies between them.

| Type | Stored (old) | Stored (new) | Rendered mean (old) | Rendered mean (new) |
|---|---|---|---|---|
| grass | (82, 138, 54) | (81, 84, 27), contrast 0.14 | (77, 131, 51) | (76, 79, 25) |
| road | (206, 184, 132) | (180, 146, 92) | (195, 175, 125) | (171, 139, 87) |
| water | (40, 108, 164) | (48, 92, 90) | (36, 99, 151) | (44, 85, 83) |
| rock | (132, 128, 124) | (146, 142, 136), contrast 0.30 | (122, 118, 114) | (135, 132, 126) |
| ore ground (full cell) | (98, 66, 34) | (98, 59, 38), flecks (226, 138, 66) | (130, 95, 37) | (126, 76, 43) |
| bridge | (202, 142, 76) | (70, 34, 26) | (167, 117, 62) | (57, 28, 21) |

"Rendered mean" is the mean texel of the texture lit as level ground by the engine's default light (what
`terrain_readability.rs` measures; the old column is the same measurement on the file of main). The shore
band is a pale grey-green foam (linear (0.34, 0.42, 0.40) at 0.30; it was (0.80, 0.93, 0.96) at 0.40).

* **Why these colours.** Grass is the reference olive. Water is a dark grey-teal rather than blue because
  the Veyran hulls are cool grey-blue: on a first, greyer blue-green water a Veyran transport truck sat at dE 8
  from the water and 90 % of the gunboat's body lay within 12 of it; on the shipped teal the smallest margin is 14.2. The rock is lighter than the first muted
  attempt so that the Meridian sand and olive (mean body colours 65-106) stand apart from it, and the bridge
  is a dark timber because every mid-brown was either a Meridian colour or too close to the minimap
  colours of ore and grass (the 50-apart rule). The ore ground equals the decor nuggets of 11.6d and the
  fill levels rise from a dusty rose-brown (102, 60, 68) empty cell (fix round 0: the old dark red-brown
  (56, 36, 28) was 17 from the bridge on the minimap) through (110, 52, 52) and (112, 58, 46) to the full ore,
  each at least 50 from every other type on the minimap. A grey-brown bridge was tried and dropped: every
  candidate collided (see the PR body), so the bridge stays the dark timber (70, 34, 26).
* **Readability** (`crates/sf_render/tests/terrain_readability.rs`): every Meridian and Veyran unit model of
  `unit_defs.ron` (27), three levels of detail each, seen at the camera pitch of the views at zoom 0.30 and
  0.12 (`camera.ron`) over eight headings: the faces the camera sees, weighted by their projected area, lit
  with `Lighting::default`, against each terrain texture as rendered and each ore fill
  level, dE76. Thresholds: mean body colour at least 12 from the terrain mean (a chosen value, about five
  just-noticeable differences, not fitted to the minima), at most half of the body area
  within 12 of it, 5th-percentile texel at least 8, team stripes (eight player colours) at least 30,
  Meridian and Veyran paints at least 12 from every ground colour. Measured (fix round 0): smallest mean
  margin per terrain grass 15.0, road 32.3, water 15.2, rock 19.8, ore 24.5, bridge 16.7, ore levels
  19.4 / 24.6 / 24.3 / 24.5; largest close share 0.32 on rock, 0.10 on ore and bridge, 0.01 or less on the
  other terrains. The 5th-percentile and stripe margins below are from the first measurement (all faces
  looking up); the tests still hold them. Smallest 5th-percentile
  texel margin 10.2; smallest team stripe margin 47.4 (red on ore); closest paint to a ground colour 12.6
  (the Veyran light paint on rock). The olive of Meridian does not disappear on the olive grass: no
  face of any model is within 12 of the grass mean in any level (0 % close), the closest unit
  margin is the Meridian engineer's far-level blob at 13.7. Selection and health tones against every
  terrain: at least 42.7 (`the_selection_tones_and_the_health_bar_read_against_every_terrain`).
* **Known gaps.** The neutral supply truck's grey-green paint is about 6 from the new rock (it was 6 from
  the old rock too), so it is not part of the rule. Fogged ground and the fog ghost colour (`fog_look.ron`)
  were not re-measured.
