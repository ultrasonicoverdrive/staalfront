# Art style: set v5 (WP11.6a)

The rules every generated model and cutscene part follows from WP11.6a on, so that WP11.6b-d and
the Phase 12 restyle look like one game. Style **G, "semi-realistic, bright"** (decision Tijmen
14-11-2026). The reference was a small concept sheet generated with Canva at the project owner's
request; it is a style reference only and is not in the repo. A later, more realistic target
(style F) needs textures and shadows in the engine: that is a separate engine WP, see "Later" below.

Everything here is made by our own generator, `tools/sf_model_gen` (`cargo run -p sf_model_gen`):
no external meshes, no textures, no downloaded art. The numbers that tune the look live in
`assets/config/model_palette.ron` (colours) and in the recipes and kit constants of the tool.

## 1. Palette (`assets/config/model_palette.ron`)

sRGB. One slot per surface type, the same six material slots in every model and in this order, so a
later texture step can key on them: `paint` (main hull paint), `paint2` (second paint), `team`,
`metal` (gun steel, fittings), `rubber` (tracks, tyres, running gear), `accent`.

| Slot | Meridian | Veyran |
|---|---|---|
| `paint` | bright sand yellow `#dcb450` (220, 180, 80); highlight `#e8c868` | cool steel blue-grey `#7894b2` (120, 148, 178); highlight `#96b0cc` |
| `paint2` | lively olive `#7a7a40` (122, 122, 64); shade `#50503c` | blue gunmetal `#4a4e68` (74, 78, 104); shade `#626680` |
| `metal` | dark olive `#282814` | `#282828` |
| `rubber` | `#141414` | `#141414` |
| `accent` | glass, weathered blue `#46606e` | fixed crimson `#a02030` (160, 32, 48) |
| `team` | the player colour (the file stores a neutral `#ebebeb`; the engine replaces it) | same |

Refinement round 2 (14-11-2026): Veyran's two steel tones are about 25 % lighter and a little
cooler (was `#64788c` / `#3c3c50`), so it reads on dark terrain next to Meridian; the crimson accent
stays darker than any player colour.

A test pins the shipped values (`tools/sf_model_gen/tests/models.rs`,
`the_shipped_palette_has_the_style_g_values`).

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
- **grime:** up to 40 % darker at the ground, fading out at 1.3 m (tuning round: about twice the
  first 22 %);
- **downward faces** up to 30 % darker;
- **facet variation** of +-3.5 % per triangle from a fixed hash (no randomness), so large flat areas
  are not one sheet;
- the **tone** of a detail (panel line 56 %, recess 72 %, track shoe 82 %);
- **top-edge highlight:** a face that slopes upward (a bevel or chamfer on top, normal `y` between
  0.2 and 0.97) is 16 % brighter;
- **edge wear:** a bevel strip (no axis-aligned normal, narrower than 320 mm) whose corner sees open
  sky (no AO ray blocked) is, on about two thirds of its triangles (fixed hash), moved 35 % towards a
  grey a quarter above its luminance: lighter and less saturated, as paint rubbed thin. Big slanted
  facets (a dome segment) are not edges and get none, and neither does any triangle of a turned
  surface (`kit::lathe`, marked `Tri::lathed`; refinement round 2: the narrow segments of the
  Meridian dome showed as light streaks; `a_lathed_dome_gets_no_edge_wear`).

The vertex colour is RGB: the wanted linear colour divided by the material factor, which is a grey at
the palette colour's brightest channel over 0.85 (`shade::factor`). A plain open face therefore shows
exactly the palette colour, and highlights and worn edges stay inside glTF's 0..1 range. The team
material keeps its own factor and ignores `COLOR_0`. Tests: `tools/sf_model_gen/tests/shade.rs`
(one per rule: plain colour, highlight, wear, no wear on lathes, crease and grime).

Keep colours clean and bright: the AO darkens creases, not whole surfaces; judge it in the engine
render (contact sheet), not only in numbers.

## 6. Levels of detail

One recipe builds all three levels from `kit::Lod`:

- round parts halve their segments per level (`Lod::segs`: 16, 8, 4);
- bevels halve at LOD1 and go at LOD2 (`Lod::bevel`);
- details drop out: wheels to three per side at LOD1 and none at LOD2, cleats, bolts, seams and
  hatches only at LOD0;
- the node tree, the anchors and the team panels stay in every level, and the bounds stay within
  10 % of LOD0 (parts that set the height, such as a crane or an antenna, stay in every level).

Budgets (the art brief): units 1500 / 500 / 150, buildings 5000 / 1500 / 300 triangles; LOD1 at
most 40 % and LOD2 at most 15 % of LOD0 (`every_level_is_inside_its_budget_and_really_reduced`).

**Detail budget** (tuning round 14-11-2026, `buildings_use_their_detail_budget`): a building uses
**2,500-3,500 triangles in LOD0** on real geometry: vents with louvres, pipe runs with flanges,
railings (`kit::railing`), roof units with fans, window rows with frames, door frames, bolted panels,
ladders and walkways; that detail is LOD0 only, so LOD1 and LOD2 stay far below. A vehicle stays
under 1,500 and spends it where it reads at game distance: capped road wheels with hubs, track
shoes (one raised face each), hatches, smoke dischargers, tools on the fenders, lamps and exhausts.

## 7. Node tree

As `docs/conventions.md` section 12 and `sf_asset_check` ask: roots `<stem>_LOD0..2`; vehicles
`hull -> turret -> barrel -> muzzle` (empty `turret`/`barrel` nodes for an unarmed vehicle);
buildings `body` (and `body -> turret -> barrel -> muzzle` for a defence) with empty anchors
`fx_smoke_0` and `fx_fire_0`. Vehicles get no smoke anchors yet: a hurt vehicle smokes by the
engine's rule like every other vehicle (anchors on the v5 tanks changed what
`damage_screenshot.rs` counts as anchor puffs; WP11.6b decides for all vehicles at once). At most four parts per level (the engine's limit).

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

## 9. Later (style F)

Style F ("realistic, bright") is the later target once the engine gets textures and shadows (an
engine WP, not WP11.6). The kit keeps a material slot per surface type for that: a texture step can
add UVs and generated maps per slot without rewriting the recipes.
