# Progress

One entry per phase, written when the phase ends (PLAN.md §12.2 step 5): a short
report and a pointer to the playable build.

## Phase 0 — Foundation

Status: done.

- WP0.1 Workspace, CI, lint: done (PR #2, 8267a83)
- WP0.2 `sf_math`: done (PR #5, daccd6a)
- WP0.3 `docs/`: done (PR #3, 995da1c)

Done when (PLAN.md §3): CI green on two OSes; `sf_math` bit-identical on Linux and Windows.

Report: the workspace with all crates builds, and CI (fmt, clippy, tests, the
determinism lint) runs on Linux and Windows for every merged PR. `sf_math` ships
fixed-point `Fx` (Q47.16), `Vec2`, 256 facings and a PCG32 generator; its golden
hashes (`crates/sf_math/tests/golden.rs`) pass with identical values on Linux and
Windows, in debug and in release. No playable build yet (phase 0 has no gameplay).

## Phase 1 — Engine

Status: in progress.

- WP1.1 `sf_engine` main loop, input and frame timing: done (PR #4, aa5cf7b)
- WP1.2 `sf_sim` core (world, commands, checksums, wander): done (PR #9, 4df4759)
- WP1.3 `sf_headless` (scenario runner, checksum log, Linux/Windows comparison in CI): done (PR #12, 305a9e3)
- WP1.4 `sf_render` basics: done (PR #6, 0ae43bc)
- WP1.5 camera, log zoom toward the cursor: done (PR #8, 4e7b88a)
- WP1.6 instancing (placeholder meshes, interpolation, tile culling): done (PR #11, ab98531)
- WP1.7 terrain (map format, chunk meshes, material texture array): done (PR #10, e2f920c)
- WP1.13 `sf_audio`: done (PR #7, afad8d2)
- WP1.8 terrain LOD, seams, culling, ray-terrain picking: done (PR #15, 8f64071)
- WP1.9 model LOD and strategic icons: in progress
- WP1.11 egui debug panels: done (PR #13, 76ea42c)
- WP1.12 `sf_ui` (quads, nine-slice, text, buttons, anchors, minimap): done (PR #14, 58d3eb9)
- WP1.10, WP1.14: _pending_

Done when: milestone M0 (engine demo), the list under "Done when" in PLAN.md §3, phase 1.

M0 status:
- Headless 10,000 ticks with 5,000 wanderers, checksum equal on Linux and
  Windows: met (CI `headless-compare`, since PR #12).
- Smooth zooming to the cursor: met, judged by Tijmen on the laptop on
  2026-10-15 (main at 8f64071, terrain LOD and ray-terrain picking in).
  Judged before the 34-40 ms zoom-out hitches of follow-up 14 were measured;
  recheck once they are fixed.
- p99 frame time <= 16.7 ms with 5,000 units in view: open, needs `sf_demo
  --bench` (WP1.14). First run on the laptop (2026-10-15, `sf_game --units
  5000 --auto-zoom`, DX12): steady 60 fps at vsync with all 5,000 units in
  view; p99 17.4-18.2 ms is vsync timing jitter, not missed frames; two
  hitches of 34-40 ms when zooming out to the whole map. That laptop has a
  GTX 1650; Tijmen made it the reference card (B10, 2026-10-15).

Report: _to be written at the end of the phase._

## Open follow-ups

Findings from the phase 1 work packages that are not fixed yet. Each is meant for
the work package named in it (or the first one that touches the code); when one is
fixed, delete the entry in the same PR.

1. **Map generator is not bit-exact across operating systems.**
   `sf_data::map::generate` uses the platform's `sin`, `exp` and `sqrt` (floats), so
   its output may differ in the last bit between Linux and Windows. It is only a
   tool today. Before `sf_sim` reads map heights (phase 2, WP2.1) the generator must
   be made bit-exact (integer or `sf_math` fixed point) or the generated map must
   stay out of the simulation (a committed file instead of a generated one).
2. **Map file names are joined unchecked.** `sf_data::map` joins the names in
   `map.ron` (`heightmap`, `material_map`) onto the map directory without checking
   them, so an absolute path or `..` escapes the directory. Reject both before maps
   can come from other players (lobby, downloads, WP14.3 at the latest).
3. **Bit-depth error prints Debug names.** The map loader's error for a PNG of the
   wrong bit depth shows "Eight-bit" / "Sixteen" (the `Debug` form of `png::BitDepth`)
   instead of "8-bit" / "16-bit".
4. **The command insertion sequence is machine-local.** `CommandQueue` orders a
   tick's commands by `(player, insertion sequence)`; the sequence is a counter of
   the local queue. A lockstep package (WP14.1) must carry a per-player sequence
   number in the command packet and schedule with it, so that two machines order a
   player's commands identically no matter how packets arrive.
5. **Release clippy runs on Linux only in CI.** The step
   `cargo clippy --workspace --release --all-targets -- -D warnings` is guarded with
   `if: runner.os == 'Linux'`, so a lint that appears only in release mode on
   Windows (for example behind `cfg(windows)`) would not be caught. Run it on both
   systems once the CI time allows.
6. **The minimap's field-of-view frame is clamped per corner.** `sf_ui::fov_quad`
   clamps each of the four ground points to the map, which bends the frame when the
   view reaches past the map edge. Clip the quadrilateral against the map rectangle
   as a polygon instead (WP1.12 follow-up, or the first package that touches the
   minimap).
7. **The minimap is baked once.** It does not follow terrain changes. Re-bake it when
   the terrain changes (phase 2: terrain types in WP2.1 to WP2.3, later editing).
8. **The minimap bake shows relief weakly.** Its slope shading is faint next to the
   terrain in the main view. Use a stronger relief shading in the bake (WP1.12
   follow-up, together with WP2.3).
9. **Release packaging must ship the egui font licence texts.** `assets/LICENSES.md`
   lists the default egui fonts (OFL-1.1, Ubuntu-font-1.0); the texts themselves are
   in the `epaint_default_fonts` crate and have to be part of the third-party notice
   of any release build (installer, phase 16).
10. **The `draw_indexed` source test matches exact helper text.**
    `renderer.rs` pins the single `draw_indexed` call site by the text of
    `CountedPass::draw`. If that starts getting in the way of a harmless refactor,
    loosen it to match `.draw_indexed(` only.
11. **The determinism lint does not scan `tools/sf_headless`.**
    `scripts/check-sim-determinism.sh` covers `sf_sim` and `sf_math`; the headless
    runner is outside it. That is fine while it only calls `sf_sim`; extend the lint
    when it grows logic of its own.
12. **`kind % shapes` in `sf_game` is a stand-in.** The game maps the sim's entity kind
    to a placeholder shape and the owner to a player colour by modulo. Replace it
    with data-driven units (phase 2, WP3.1 and WP3.2).
13. **The p99 criterion cannot be met with vsync on.** At 60 Hz every frame
    takes 16.67 ms and timing jitter puts p99 just above 16.7 ms even when no
    frame is missed. `sf_demo --bench` (WP1.14) must run without vsync (or measure
    CPU and GPU frame time separately) and in real fullscreen at 1920x1080.
14. **Hitches of 34-40 ms when zooming out to the whole map** (laptop run,
    2026-10-15): possibly a one-time instance buffer growth or LOD change; find
    the cause in WP1.14 with the benchmark.
15. **Vulkan hangs on the test laptop.** Adapter creation hung with Vulkan
    (NVIDIA GTX 1650, Windows); the game window now uses DX12 on Windows.
    `WGPU_BACKEND` overrides it, `SF_GPU_TRACE=1` prints each GPU startup step.
16. **WARP crashes in parallel GPU tests on Windows.** `check (windows-latest)`
    crashed with STATUS_ACCESS_VIOLATION (0xc0000005) twice on the same commit
    (PR #18, run 39009873035), once in `debug_overlay` and once in
    `camera_screenshot`, while main and every other run stayed green. Likely
    cause: several tests create a DX12 WARP device at the same time. Mitigation:
    CI runs the workspace tests with `--test-threads=1` on Windows. Root fix still
    open: one shared device per test binary (or a lock in the screenshot helpers),
    then drop the flag.
