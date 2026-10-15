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
- WP1.2 `sf_sim` core (world, commands, checksums, wander): in progress
- WP1.4 `sf_render` basics: done (PR #6, 0ae43bc)
- WP1.3, WP1.5 to WP1.14: _pending_

Done when: milestone M0 (engine demo), the list under "Done when" in PLAN.md §3, phase 1.

Report: _to be written at the end of the phase._
