# Test pack v1: status and known issues

Supplied by Tijmen on 15-10-2026; generated with Microsoft Copilot from the unit
asset spec. **Test material** for WP1.10 (glTF loading) and WP1.14 (demo), not
release art. The offline viewer, the generator script, the concept image and the
128 px icon PNGs of the original pack are left out (see `LICENSES_bron.md`).

Checked by the orchestrator with an independent parser (not the pack's own
script): 14 files, glTF 2.0 binary, each with three roots `<name>_LOD0/1/2`; per LOD
the nodes `hull_LOD*`, `turret_LOD*`, `barrel_LOD*`; materials `steel`, `dark`,
`team`, `lamp`; no textures, no extensions; 48–156 triangles per LOD; ground at
Y=0, front at +X.

Known issues:

1. **Every part is split per material** into child nodes (`hull_steel_LOD0`,
   `hull_team_LOD0`, ...): about 7 meshes per unit per LOD, which with per-mesh
   instancing means many draw calls. Fix: WP1.10 merges them per part on load, or
   the pack is re-exported with one mesh per part and several primitives.
2. **Part names carry a LOD suffix** (`hull_LOD0` instead of `hull`); the loader
   strips it.
3. **Icons are not all distinguishable at 14 px**: heavy tank and rocket truck are
   identical (triangle with `+`); scout, light tank and battle tank are dots in a
   triangle at 14 px. The engine's 1.5 px outline makes the inner mark smaller
   still. Custom icons are only usable from WP3.6.
4. **Axes** (+X front, Y up, metres, ground Y=0) follow the proposal that WP1.10
   fixes in `docs/conventions.md`.
5. **Empty parts** (found in WP1.10): the scout, the armoured car, the
   construction vehicle, the resource truck and three buildings (power plant,
   refinery, vehicle factory) have `turret_LOD*`/`barrel_LOD*` nodes without
   any mesh below them, and the rocket truck's `LOD2` has an empty barrel. The
   loader drops parts without geometry, so those models are a hull only (no
   draw call for nothing).

Status after WP1.10: issues 1 and 2 are handled by the loader (`sf_render::glb`):
one mesh per part per level, suffixes stripped. Every orientation in the pack
matches the engine convention, so `assets/config/models.ron` loads it with
`front: PlusX` (no rotation).
