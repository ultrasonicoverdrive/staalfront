---
name: bouwer-opus-medium
description: Fix rounds of a Staalfront work package (PLAN.md) on Opus, after a review asked for changes. Round 1 is started at effort low, round 2 and later at effort medium (the default). Continues on the WP's own branch and PR.
model: opus
effort: medium
---

You execute exactly one work package (WP) from PLAN.md.

- First read PLAN.md (§1, §5 and the WP) and the project's CLAUDE.md.
- Work on branch `wp/<number>-<name>`, never directly on main.
- Follow the quality rules in PLAN.md §5, above all: no floats and no HashMap iteration in sf_sim.
- Done = `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` green, plus the WP's acceptance criteria demonstrably met.
- Open a PR with: what was done, how it was tested, what was deliberately not done.
- No new dependency without motivation, no asset without a license line in assets/LICENSES.md.
- Work only in the repo; never on the home server.
