# Conventions for agents and humans

Source of truth: `PLAN.md` (§0, §5, §6, §10, §12) and `CLAUDE.md`. If this page
and `PLAN.md` disagree, `PLAN.md` wins; fix this page.

## 1. Language and naming (B8, B9)

- Code, comments, docs, commit messages and PRs are in **English**.
- All game text (menus, UI, voices, subtitles) is **English** and lives in
  language files, not in code, so translation stays possible later.
- Crates use the `sf_` prefix and snake_case, as in the workspace map in
  `architecture.md`. Rust naming follows standard Rust conventions. Further
  naming rules: undecided (see PLAN.md §5).
- Branches: `wp/<number>-<name>`, for example `wp/0.3-docs`.

## 2. Work package flow (PLAN.md §12.2)

1. The orchestrator (main session) starts an agent with the WP number, its
   acceptance criteria from PLAN.md and the relevant `docs/`.
2. One work package per branch `wp/<number>-<name>`, always via PR, never directly
   on `main`. The agent works in its own git worktree, writes tests first where
   possible, and runs `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` and
   the headless scenarios.
3. The agent opens a PR with: **what was done, how it was tested, what was
   deliberately not done.**
4. **Review gate:** the `reviewer` agent checks out the PR, runs the checks itself,
   assesses it against the WP's acceptance criteria, the quality rules (section 4)
   and the don'ts (section 6), and posts `APPROVE` or `CHANGES REQUESTED` as a PR
   comment. The orchestrator merges **only after `APPROVE`**. On `CHANGES
   REQUESTED` the builder fixes the findings on the same branch and the reviewer
   reviews again.
5. After each phase: a short playable build and a report in `docs/progress.md`.

Parallelism (§12.3): at most 3 agents at a time, only on WPs without overlap in
crates. Conflicts in `Cargo.lock` or shared types are resolved by the
orchestrator.

## 3. Which agent for which job (PLAN.md §12.1)

Agent definitions live in `.claude/agents/`.

| Agent | Model / effort | Use |
|---|---|---|
| `bouwer-sonnet` | sonnet, medium | Default for every WP. |
| `bouwer-sonnet-xhigh` | sonnet, xhigh | WPs marked **[xhigh]** in PLAN.md §3 (determinism, simulation core, terrain LOD seams, picking/LOD, pathfinding, AI, full-state serialization, lockstep). |
| `reviewer` | opus, medium, read-only | Review gate per PR. |
| `onderzoeker` | opus, low, read-only | Research questions (crates, APIs, algorithms, asset licenses); does not review. |

If a medium WP fails review twice, the orchestrator reruns it on
`bouwer-sonnet-xhigh`. The two supporting agents have no edit, commit or push
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

Done for any WP: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
and `cargo test` are green, and the WP's acceptance criteria are demonstrably met.

## 5. Determinism rules for `sf_sim` (spelled out)

`sf_sim` must give bit-identical results on Linux and Windows for the same
start state and command sequence (B3, PLAN.md §6).

- **No floats:** no `f32`/`f64` anywhere in `sf_sim`. Use `sf_math` fixed-point
  types, vectors, angles and RNG. `glam` is presentation only.
- **No hash-order iteration:** no iteration over `HashMap`/`HashSet`. Use
  `BTreeMap`/`BTreeSet` or sorted vectors. CI lints the use of `HashMap`/`HashSet`
  in `sf_sim` (WP0.1).
- **No clock:** no `Instant`, `SystemTime` or any wall time. Time is the tick number.
- **No I/O:** no files, network or environment access. Loading happens outside
  (`sf_data`, `sf_engine`) and the data is passed in.
- **No threads that affect sim order:** nothing whose result depends on thread
  scheduling. Any parallelism must produce the same result as sequential
  execution, in a fixed order. Whether parallelism is used inside the sim at all:
  undecided (see PLAN.md §6).
- **Randomness** only from the deterministic RNG in `sf_math`, seeded as part of
  the sim state.
- **Mutation** only inside the sim tick, driven by `Command`s executed on tick N.
- **Entity iteration** in the fixed storage order.

### Changing something that affects the checksum

A change that alters sim behavior changes the per-tick checksums and thus the
regression baselines. Changes to the determinism rules themselves need approval
(PLAN.md §12.4). For an intended behavior change:

1. State in the PR that the checksum changes and why.
2. Update the affected headless scenario expectations in the same PR, and
   explain the new values.
3. Confirm Linux and Windows CI still agree on the new checksums.
4. Never update a baseline just to make a failing test pass without knowing why
   it changed: an unexplained checksum change is a determinism bug until proven
   otherwise.

The exact mechanism for storing baselines is defined with WP1.2/WP1.3 (undecided
until then, see PLAN.md §3).

## 6. What an agent does not do (PLAN.md §12.4)

- No change to the sim determinism rules without approval.
- No new dependency without motivation in the PR.
- No assets without a license line.
- Never original Red Alert files (`.mix`, `.aud`, `.vqa`, extracted WAVs) in the
  repo, a release or a CI artifact (B11).
- Never build or run the project on the home server (B6).
- Never commit directly to `main`; never merge before reviewer `APPROVE`.

## 7. Dependencies

Every new dependency needs a motivation in the PR description: what it is for and
why existing crates or the standard library do not suffice. The planned stack is
in PLAN.md §1.0; anything outside it counts as new. Dependencies used by `sf_sim`
and `sf_math` additionally must not break determinism (no floats in sim-visible
behavior, no hidden clocks, I/O or hash ordering). A formal policy beyond this
(license checks, `cargo deny`): undecided (see PLAN.md §12.4).

## 8. Assets and licensing (PLAN.md §10, B5, B11)

- Every asset gets a license line in `assets/LICENSES.md` **from the first file
  onward**: license, source URL, creator, per file.
- v0 assets: primitives, procedural textures, CC0 or Pixabay (Pixabay Content
  License) sounds. No rips from Red Alert or other games, even if they appear on
  Pixabay.
- **B11:** original Red Alert audio is optional, loaded at runtime from the
  player's own files (WP11.7). Original Red Alert files are never committed,
  released or put in a CI artifact. `.gitignore` already excludes `*.mix`,
  `*.aud`, `*.vqa` and `/ra-original/`. Tests use only fixtures generated in the
  test itself. Staalfront always works without original audio.
- **B5 clean-room:** use only the functional analysis, never read-and-retype EA
  code (EA code is GPL). Own name, units, graphics and music; Red Alert names,
  characters and texts are trademarks/copyright.

## 9. Commit messages

Commit messages are in English (B9). A finer style guide is undecided (see
PLAN.md §12). Working convention until then: a short imperative summary line,
optionally with the WP number (for example `WP0.3: add architecture docs`), and a
body explaining the why when it is not obvious. Agent commits end with the
attribution trailer supplied by the session.

## 10. Testing expectations

- **Gameplay WP:** a headless scenario in `sf_headless` that pins the behavior
  with a checksum regression (rule 4). Prefer tests written first.
- **Visual WP:** a screenshot test (offscreen render to PNG; Linux Mesa lavapipe,
  Windows DX12 WARP) or a recorded short clip (rule 5).
- **Math and sim primitives:** property tests (`proptest`), snapshots with `insta`
  where fitting.
- **Cross-OS:** CI runs on Linux and Windows; determinism checksums must be equal.
- **Performance:** fps cannot be measured in the cloud container or in CI
  (software rendering). The fps requirement is measured on the reference laptop.

## 11. Docs

- Game rules: `docs/rules/` (quality rule 7). One file per rule area; see
  `docs/rules/README.md`.
- Progress: add an entry to `docs/progress.md` after each phase (PLAN.md §12.2
  step 5).
- Architecture changes: update `docs/architecture.md` in the same PR.
- The analysis (`docs/red-alert-analysis.md`) and `docs/mission-inspiration.md`
  are source material, clean-room per B5; do not paste code from the original.
