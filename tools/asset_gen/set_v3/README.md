# Generator of `assets/set_v3`

Tijmen's procedural generator for the third model delivery (17-10-2026):
Python with `numpy` and `trimesh` (MIT; `trimesh` only supplies an icosphere
base mesh). It is **not part of the Rust build or CI**: nothing in the
workspace runs it, and the `.glb` files in `assets/set_v3` are committed as
delivered.

The scripts only build meshes in memory and write `.glb` files and a
`manifest.json`. They were read before they were committed: no network, no
subprocess, no `eval` or `exec`; the only file writes are in `glbkit.py`
(`GLBWriter.write`, the model file) and `models.py` (it creates the output
folder it is given and writes `manifest.json` there).

| File | What |
|---|---|
| `glbkit.py` | Low-poly geometry kit, material table, scene graph and the GLB writer |
| `figure_v4.py` | Parametric infantry figures and the unit table of the set |
| `figure_v3.py` | The older figure builder (still imported by `models.py`) |
| `models.py` | Ore fields, trees, rocks, wrecks, the registry and the entry point |

Run it (not needed to use the models):

```sh
python tools/asset_gen/set_v3/models.py <output folder> [model names...]
```

Without names it builds all 30 models and the manifest. The output of a re-run
is a new delivery: check it with `cargo run -p sf_asset_check -- <folder>`
before replacing anything in `assets/set_v3`.
