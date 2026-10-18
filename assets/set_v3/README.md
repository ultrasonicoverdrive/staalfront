# Staalfront set v3: orchestrator note

Added to the repo on 17-10-2026. The delivery README of Tijmen follows below
this note **unchanged** (in Dutch).

- **Infantry budget: 1500 / 250 / 60 triangles (LOD0 / LOD1 / LOD2).** Tijmen
  raised it from 800 in this delivery and the orchestrator confirmed it. One
  line of the delivery README still says "infanterie 800/250/60" (under "Wat is
  nieuw"); that line is stale and the 1500 decision stands. Two-figure units
  (`veyran_demolition_team`) get twice the infantry budget (3000 / 500 / 120).
  Vehicles and decor stay at 1500 / 500 / 150, buildings at 5000 / 1500 / 300.
  `tools/sf_asset_check` uses these numbers.
- **What is in the repo.** The 30 `.glb` files in this folder; the two overview
  pictures in `docs/art/set_v3/`; the generator in `tools/asset_gen/set_v3/`
  (Python + trimesh, not part of the Rust build or CI). Not copied: the 30
  per-model preview renders (`renders/`, 3.4 MB together) and `manifest.json`
  (its triangle counts were checked against the engine loader: all 30 files
  agree).
- **Counts.** The delivery has 14 infantry models (12 roles over the two
  factions plus `neutral_civilian` and `neutral_technician`), 4 ore field
  stages, 2 rocks, 6 tree variants and 4 wrecks.
- **Viewing.** `models.ron` has the pack `set_v3`: `sf_game --models set_v3`.
  Infantry wander; decor and wrecks stand in the base area (a Square icon makes
  a model a fixed object in `sf_game`). Screenshots of the pack under Xvfb
  (software Vulkan) are in `docs/screenshots/set_v3/`, made with
  `xvfb-run -a -s "-screen 0 1920x1080x24" cargo run -p sf_game -- --models set_v3 --units 1100 --map-cells 64 --windowed --shots <dir>`;
  the demo's placeholder shapes are drawn next to the models there.
- **Checking.** `cargo run -p sf_asset_check -- assets/set_v3` prints, per
  file, whether it meets the art brief. The findings of this delivery are in the
  pull request that added it; the models were not changed.
- **Wired in (18-10-2026).** The 14 infantry models are referenced from
  `assets/config/unit_defs.ron`: roles with a model per faction (rifleman,
  rocket_trooper, engineer, scout) use the optional `models` map, the others
  their single `model` (see `docs/rules/units.md`, "Models per faction").
  Decor (ore stages, rocks, trees) and the wrecks are not unit roles and are not
  referenced yet: placing them on maps is a later WP (phase 2.2/2.3, wrecks 5.5).

---

# Staalfront — neutrale set + infanterie v3

Vervangt `staalfront_gedeelde_modellen_v1`. Opgebouwd volgens de art brief van 17-10-2026 (§4, §8, §11).

## Wat is nieuw t.o.v. v1
- v3: alle infanterie uit §5 (behalve `heavy_gun_team`) in dezelfde ronde stijl, per factie: rifleman, rocket_trooper, engineer, scout (M+V), sniper (M), medic, combat_engineer, demolition_team (V). Generator `gen/figure_v4.py` (parametrisch: hoofddeksel, wapen, rugzak, extra's).
- Infanteriebudget verhoogd naar **1500 / 250 / 60** (was 800). `demolition_team` is twee figuren in één bestand (nodes met `_2`), budget 2×.
- Factie-eenheden hebben materiaal `team` (placeholder grijs #9C9C9C): schouderstuk, helmtop/band, rugzakflap. Meridian: ronde helm, beenwindsels, rugzak met rolmat, olijf/zand. Veyran: helm met rechte rand, lange jas, hoge laarzen, leisteen/antraciet, kleine karmijnrode kraagspiegels of armband.
- Medic: armband + tasplaat in karmijn met een enkele lichte **balk** (geen kruis, §9).
- Wapens zijn generiek (kolf + loop + grendelblok), geen replica's.
- Drie detailniveaus per bestand: roots `<id>_LOD0`, `_LOD1`, `_LOD2` met dezelfde nodestructuur, oorsprong en afmeting.
- Alle modellen binnen het driehoekenbudget (infanterie 800/250/60, voertuig en decor 1500/500/150).
- Eén mesh per onderdeel, meerdere materialen als primitives; max. 6 materialen per model; alleen vlakke kleuren, geen textures.
- Nieuwe assets uit de brief: boomvarianten (omgevallen, verbrand), wrak middel en wrak vaartuig.
- Wrakken hebben lege nodes `fx_smoke_*` en `fx_fire_*` als ankerpunt, en nodes `hull` → `turret` → `barrel` waar van toepassing.
- Geen `team`-materiaal: alles in deze set is neutraal (conform §8).
- Controle-renders in `renders/` (LOD0 op 45° en 80°, LOD1/LOD2 op 60°) en `overzicht_neutrale_set_v2.png`.

## Assen en schaal
Meters, Y omhoog, voorkant +X, rechterkant +Z, grond op Y = 0. Oorsprong midden onder het model (ore_field: midden van de cel van 4 × 4 m). Bij `neutral_wreck_vessel` is Y = 0 de waterlijn.

## Bestanden
| Bestand | Klasse | Driehoeken LOD0 / 1 / 2 | Materialen | Nodes |
| --- | --- | --- | --- | --- |
| `neutral_civilian.glb` | infantry | 1040 / 238 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `neutral_technician.glb` | infantry | 1012 / 226 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `meridian_rifleman.glb` | infantry | 1176 / 238 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `veyran_rifleman.glb` | infantry | 1144 / 236 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `meridian_rocket_trooper.glb` | infantry | 1180 / 236 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `veyran_rocket_trooper.glb` | infantry | 1120 / 234 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `meridian_engineer.glb` | infantry | 1076 / 250 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `veyran_engineer.glb` | infantry | 1108 / 250 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `meridian_scout.glb` | infantry | 1116 / 218 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `veyran_scout.glb` | infantry | 1116 / 218 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `meridian_sniper.glb` | infantry | 1112 / 218 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `veyran_medic.glb` | infantry | 1048 / 236 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `veyran_combat_engineer.glb` | infantry | 1228 / 238 / 60 | 6 | leg_l, leg_r, torso, head, arm_l, arm_r |
| `veyran_demolition_team.glb` | infantry_pair | 2340 / 464 / 120 | 5 | leg_l, leg_r, torso, head, arm_l, arm_r, leg_l_2, leg_r_2, torso_2, head_2, arm_l_2, arm_r_2 |
| `neutral_ore_field_stage_01.glb` | decor | 368 / 92 / 72 | 3 | soil, ore |
| `neutral_ore_field_stage_02.glb` | decor | 608 / 112 / 72 | 3 | soil, ore |
| `neutral_ore_field_stage_03.glb` | decor | 848 / 172 / 112 | 3 | soil, ore |
| `neutral_ore_field_stage_04.glb` | decor | 1168 / 212 / 132 | 3 | soil, ore |
| `neutral_tree_pine.glb` | decor | 396 / 112 / 60 | 3 | trunk, foliage |
| `neutral_tree_pine_fallen.glb` | decor | 456 / 172 / 120 | 4 | trunk, foliage, root_plate |
| `neutral_tree_pine_burnt.glb` | decor | 145 / 127 / 45 | 1 | trunk, foliage |
| `neutral_tree_broadleaf.glb` | decor | 650 / 390 / 86 | 3 | trunk, foliage |
| `neutral_tree_broadleaf_fallen.glb` | decor | 710 / 450 / 146 | 4 | trunk, foliage, root_plate |
| `neutral_tree_broadleaf_burnt.glb` | decor | 218 / 198 / 66 | 1 | trunk, foliage |
| `neutral_rock_small.glb` | decor | 400 / 100 / 20 | 3 | rock |
| `neutral_rock_large.glb` | decor | 800 / 140 / 40 | 3 | rock |
| `neutral_wreck_light_vehicle.glb` | vehicle | 496 / 322 / 140 | 5 | scorch, hull, wheels, debris, fx_smoke_1, fx_fire_1 |
| `neutral_wreck_medium_vehicle.glb` | vehicle | 264 / 234 / 116 | 5 | scorch, hull, debris, fx_smoke_1, fx_fire_1 |
| `neutral_wreck_heavy_vehicle.glb` | vehicle | 388 / 310 / 144 | 6 | scorch, hull, turret, debris, fx_smoke_1, fx_smoke_2, fx_fire_1 |
| `neutral_wreck_vessel.glb` | vehicle | 332 / 300 / 120 | 4 | hull, debris, fx_smoke_1, fx_fire_1 |

Volgorde ertsvelden: `stage_01` = bijna op … `stage_04` = vol (zelfde volgorde als v1). Wil je het andersom, hernoem dan alleen de bestanden.

## Materialen (namen in de glb)
`steel`, `dark`, `charred`, `burnt`, `rust`, `hull_grey`, `soil`, `ore`, `rock`, `rock_dark`, `moss`, `bark`, `pine`, `pine_light`, `leaf`, `leaf_light`, `skin`, `hair`, `coat`, `trousers`, `cap`, `leather`, `overall`. Alleen baseColor + metallic/roughness-factor; `steel` heeft metallic 0,35.

## Nog niet in deze set
`heavy_gun_team` (kanon + bemanning; hoort bij de voertuigbatch), bouwmenu-portretten. Puinmodellen per footprintmaat, hekken en klifranden, de gebiedssets (§8), iconen (§7). De manifest (`manifest.json`) bevat per bestand de bounds en driehoekentellingen.
