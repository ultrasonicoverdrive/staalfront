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
- WP1.3 `sf_headless` (scenario runner, checksum log, Linux/Windows comparison in CI): in review (PR #12)
- WP1.4 `sf_render` basics: done (PR #6, 0ae43bc)
- WP1.5 camera, log zoom toward the cursor: done (PR #8, 4e7b88a)
- WP1.6 instancing (placeholder meshes, interpolation, tile culling): done (PR #11, ab98531)
- WP1.7 terrain (map format, chunk meshes, material texture array): done (PR #10, e2f920c)
- WP1.13 `sf_audio`: done (PR #7, afad8d2)
- WP1.8 terrain LOD, seams, culling, ray-terrain picking: in progress
- WP1.11 egui debug panels: in progress
- WP1.9, WP1.10, WP1.12, WP1.14: _pending_

Done when: milestone M0 (engine demo), the list under "Done when" in PLAN.md §3, phase 1.

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
4. **Camera ray to ground uses the plane `y = 0`.** `sf_render::camera` finds the point
   under the cursor on a flat plane, which is wrong on relief (terrain is up to
   about 6 m high in the generated maps). Intersect the ray with the heightmap in
   WP1.8 (terrain LOD and culling) or the picking work package, whichever comes first.
5. **The command insertion sequence is machine-local.** `CommandQueue` orders a
   tick's commands by `(player, insertion sequence)`; the sequence is a counter of
   the local queue. A lockstep package (WP14.1) must carry a per-player sequence
   number in the command packet and schedule with it, so that two machines order a
   player's commands identically no matter how packets arrive.
6. **Release clippy runs on Linux only in CI.** The step
   `cargo clippy --workspace --release --all-targets -- -D warnings` is guarded with
   `if: runner.os == 'Linux'`, so a lint that appears only in release mode on
   Windows (for example behind `cfg(windows)`) would not be caught. Run it on both
   systems once the CI time allows.
