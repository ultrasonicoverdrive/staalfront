# Conventions for agents and humans

Source of truth: `PLAN.md`, `CLAUDE.md` and `.claude/agents/*.md`. If this page
disagrees with them, they win; fix this page.

Rule of this page: the numbered rules and bullet lists state only what those
sources say (with the source cited). Anything else sits in a block labelled
**Proposal (not decided)** and is not binding until it is moved into PLAN.md,
CLAUDE.md or an agent file.

## 1. Language and naming (B8, B9)

- Code, comments, docs, commit messages and PRs are in **English** (B9).
- All game text (menus, UI, voices, subtitles) is **English** and lives in
  language files, not in code (B8).
- Crates use the `sf_` prefix (PLAN.md §1.1).
- Branches: `wp/<number>-<name>` (PLAN.md §12.2, CLAUDE.md), for example
  `wp/0.3-docs`.
- Further naming rules: not covered by PLAN.md.

## 2. Work package flow (PLAN.md §12.2)

1. The orchestrator (main session) starts an agent with the WP number, its
   acceptance criteria from PLAN.md and the relevant `docs/`.
2. One work package per branch `wp/<number>-<name>`, always via PR
   (CLAUDE.md). The agent works in its own git worktree/branch, writes tests
   first where possible, and runs `cargo fmt`, `clippy -D warnings`, `cargo test`
   and headless scenarios.
3. The agent opens a PR with: **what was done, how it was tested, what was
   deliberately not done.**
4. **Review gate:** the `reviewer` agent checks out the PR, runs the checks itself,
   assesses it against the WP's acceptance criteria, §5 and §12.4, and posts
   `APPROVE` or `CHANGES REQUESTED` as a PR comment. The orchestrator merges only
   after `APPROVE`; on `CHANGES REQUESTED` the builder fixes the findings on the
   same branch and the reviewer reviews again.
5. After each phase: a short playable build and a report in `docs/progress.md`.

Parallelism (§12.3): at most 3 agents at a time, only on WPs without overlap in
crates. Conflicts in `Cargo.lock` or shared types are resolved by the
orchestrator.

## 3. Which agent for which job (PLAN.md §12.1)

Agent definitions live in `.claude/agents/`.

| Agent | Model / effort | Use |
|---|---|---|
| `bouwer-opus` | opus, low | Default for every WP not marked **[xhigh]**. |
| `bouwer-opus-medium` | opus, medium | WPs marked **[xhigh]** in PLAN.md §3 (determinism, simulation core, terrain LOD seams, picking/LOD, pathfinding, AI, full-state serialization, lockstep), and a WP that failed review twice on `bouwer-opus`. |
| `reviewer` | opus, medium, read-only | Review gate per PR. |
| `onderzoeker` | opus, low, read-only | Research questions (crates, APIs, algorithms, asset licenses); does not review. |

If a WP fails review twice on `bouwer-opus`, the orchestrator reruns it on
`bouwer-opus-medium` (decision Tijmen 17-10-2026; before that the builders ran on Sonnet). The two supporting agents have no edit, commit or push
rights.

## 4. Quality rules (PLAN.md §5, apply to every WP)

1. No world mutation outside the sim tick. The UI issues commands.
2. No `f32`/`f64` in `sf_sim`; only `sf_math` types. CI checks this with a lint.
3. No `HashMap` iteration order in the sim; use `BTreeMap` or sorted vectors.
4. Every gameplay WP delivers a headless scenario that pins down the behavior
   (checksum regression).
5. Every visual WP delivers a screenshot test or a recorded short clip.
6. Game data in RON, not in code; adding a new unit means no Rust change.
7. Document every non-obvious game rule in `docs/rules/`.

The reviewer runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
and `cargo test` (`.claude/agents/reviewer.md`); they must be green, and the WP's
acceptance criteria demonstrably met.

## 5. Determinism in `sf_sim`

Sources: CLAUDE.md, PLAN.md §5 rules 1-3, §6, WP0.1, WP1.3.

- No floats (`f32`/`f64`); only `sf_math` types (§5 rule 2; CI lint, WP0.1).
- No `HashMap`/`HashSet` iteration order; `BTreeMap` or sorted vectors (§5 rule 3;
  CI lint, WP0.1).
- No clock and no I/O (CLAUDE.md).
- Threads are listed as a determinism leak (PLAN.md §6). Whether the sim may use
  threads at all: undecided (see PLAN.md §6).
- No world mutation outside the sim tick; the UI issues commands (§5 rule 1).
- Determinism is checked by checksum tests on Linux and Windows in CI (§6) and by
  comparing headless checksums across both OSes (WP1.3).
- Changes to the determinism rules need approval (§12.4).

### Changes that alter checksums

- A checksum baseline updated without explanation counts as a defect in review
  (`.claude/agents/reviewer.md`: "checksums updated without explanation").
- A change to the determinism rules needs approval (PLAN.md §12.4).
- How baselines are stored (as built in WP1.2): golden checksums are committed in
  `crates/sf_sim/tests/golden.rs`, produced by a fixed scenario (seed plus a
  command script in `crates/sf_sim/tests/common/mod.rs`) and replayed on Linux
  and Windows in CI (`cargo test --release -p sf_sim` for the full-size one).
  Regenerate only for an intended behavior change, with
  `cargo test --release -p sf_sim --test golden -- --ignored --nocapture print_golden_values`,
  and explain the change in the PR. The cross-OS comparison of headless runs is the
  `headless-compare` CI job (WP1.3): `sf_headless` runs the scenarios in
  `assets/scenarios/` on both OSes, each against the value committed in
  `<scenario>.expected`, and `scripts/compare-headless-logs.sh` compares the logs.
  An intended change to the simulation updates those `.expected` files too (steps in
  `tools/sf_headless/tests/pinned.rs`).

As built in WP1.2 (`crates/sf_sim`): the only randomness is `sf_math::Rng`,
explicitly seeded (the world seed and the number of entities spawned so far give
each entity its own stream), and entities are always visited in ascending slot
index.

> **Proposal (not decided)** — not in PLAN.md, CLAUDE.md or an agent file:
> - Concrete forbidden items: `Instant`, `SystemTime`, file/network/environment
>   access in `sf_sim`; loading happens outside `sf_sim` and data is passed in.
> - Parallelism, if ever used, must give the same result as sequential execution.
> - Steps for an intended checksum change: say in the PR that and why the
>   checksum changes; update the scenario expectations in the same PR and explain
>   them; confirm Linux and Windows CI agree; treat an unexplained change as a
>   determinism bug.

## 6. What an agent does not do

- No change to the sim determinism rules without approval (§12.4).
- No new dependency without motivation in the PR (§12.4).
- No assets without a license line (§12.4).
- Never original Red Alert files (`.mix`, `.aud`, `.vqa`, extracted WAVs) in the
  repo, a release or a CI artifact (§12.4, B11).
- Never build or run the project on the home server (B6, §13, CLAUDE.md).
- Never work directly on `main` (CLAUDE.md: always via PR on a
  `wp/<number>-<name>` branch); never merge before reviewer `APPROVE` (§12.2
  step 4).

## 7. Dependencies

No new dependency without motivation in the PR (PLAN.md §12.4). The planned stack
is in PLAN.md §1.0.

> **Proposal (not decided):** the motivation states what the dependency is for and
> why the standard library or existing crates do not suffice; dependencies used by
> `sf_sim`/`sf_math` must not break determinism; license checks (for example
> `cargo deny`).

## 8. Assets and licensing (PLAN.md §10, B5, B11)

- Record the license per asset in `assets/LICENSES.md`, from the first file onward
  (§10).
- Sound: CC0 (freesound) or Pixabay (Pixabay Content License); record license,
  source URL and creator per file. No rips from Red Alert or other games in the
  repo, even if they are on Pixabay (§10).
- **B11:** original Red Alert audio is optional and loaded at runtime from the
  player's own files (WP11.7); never in the repo, a release or a CI artifact.
  Staalfront always works without it. WP11.7 tests use only fixtures generated in
  the test itself. `.gitignore` excludes `*.mix`, `*.aud`, `*.vqa` and
  `/ra-original/`.
- **B5 clean-room:** use only the functional analysis, never read-and-retype EA
  code (EA code is GPL). Own name, units, graphics and music are mandatory.

## 9. Commit messages

Commit messages are in English (B9). Commit style is undecided (not covered by
PLAN.md).

## 10. Testing expectations

- **Gameplay WP:** a headless scenario that pins the behavior (checksum
  regression), §5 rule 4. Tests first where possible (§12.2).
- **Visual WP:** a screenshot test or a recorded short clip, §5 rule 5. Screenshot
  tests render offscreen to PNG (Linux: Mesa lavapipe; Windows: DX12 WARP), WP1.4.
  Goldens live in `crates/<crate>/tests/golden/`; regenerate after an intended
  change with `SF_UPDATE_GOLDEN=1 cargo test -p sf_render --test screenshot`
  and review the PNG in the diff. A missing GPU adapter fails the test.
- **Tools:** `proptest` for properties and `insta` for snapshots (PLAN.md §1.0);
  property tests for `sf_math` (WP0.2).
- **Cross-OS:** CI checks every PR on Linux and Windows (§13).
- **Performance:** the cloud container and CI cannot measure fps (§13); the fps
  requirement is measured on the reference laptop (B10, §2.4).

## 11. Docs

- Game rules go in `docs/rules/` (PLAN.md §5 rule 7).
- Add an entry to `docs/progress.md` after each phase (PLAN.md §12.2 step 5).
- `docs/red-alert-analysis.md` and `docs/mission-inspiration.md` are source
  material, clean-room per B5.

> **Proposal (not decided):** one file per rule area in `docs/rules/` (see
> `docs/rules/README.md`); update `docs/architecture.md` in the same PR as an
> architecture change.
