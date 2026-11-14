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
| `paint` | bright sand yellow `#dcb450` (220, 180, 80); highlight `#e8c868` | cool steel blue-grey `#64788c` (100, 120, 140); highlight `#7c90a4` |
| `paint2` | lively olive `#7a7a40` (122, 122, 64); shade `#50503c` | gunmetal `#3c3c50` (60, 60, 80); shade `#505064` |
| `metal` | dark olive `#282814` | `#282828` |
| `rubber` | `#141414` | `#141414` |
| `accent` | glass, weathered blue `#46606e` | fixed crimson `#a02030` (160, 32, 48) |
| `team` | the player colour (the file stores a neutral `#ebebeb`; the engine replaces it) | same |

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

- vivid **bands and panels**: the turret roof, hull side panels, the rear deck, building roof
  panels, bands round a drum or a tank; never a single triangle and never a diagonal;
- **20-40 % of the top-down view in every level** (`sf_asset_check`'s `team-share` rule; LOD1 and LOD2
  too), and at least 12 team triangles per level (`the_team_colour_is_bands_and_panels_never_one_triangle`);
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

- **ambient occlusion:** 17 rays per corner over the face's hemisphere, 1.4 m reach, up to 60 %
  darker where all are blocked (creases, under overhangs, between hull and tracks);
- **grime:** up to 22 % darker at the ground, fading out at 1.1 m;
- **downward faces** up to 30 % darker;
- **facet variation** of +-3.5 % per triangle from a fixed hash (no randomness), so large flat areas
  are not one sheet;
- the **tone** of a detail (panel line 56 %, recess 72 %).

Keep colours clean and bright: the AO darkens creases, not whole surfaces. Light edge wear is not
built yet (a gap: the bevels catch the light instead).

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

## 7. Node tree

As `docs/conventions.md` section 12 and `sf_asset_check` ask: roots `<stem>_LOD0..2`; vehicles
`hull -> turret -> barrel -> muzzle` (empty `turret`/`barrel` nodes for an unarmed vehicle);
buildings `body` (and `body -> turret -> barrel -> muzzle` for a defence); empty anchors
`fx_smoke_0` and `fx_fire_0` in every model. At most four parts per level (the engine's limit).

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
