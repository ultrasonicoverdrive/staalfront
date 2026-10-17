# Docs index

`PLAN.md` in the repo root is the source of truth; these docs support it.

| File | What it is |
|---|---|
| [`architecture.md`](architecture.md) | Crate map, dependency rules, sim/presentation split, entity storage, coordinates, engine versus game. |
| [`conventions.md`](conventions.md) | Rules for agents and humans: branch/PR flow, review gate, agents, quality rules, determinism, dependencies, assets, testing. |
| [`red-alert-analysis.md`](red-alert-analysis.md) | Functional analysis of the Red Alert source code. Source material only; clean-room (B5): ideas, never code. |
| [`mission-inspiration.md`](mission-inspiration.md) | Groundwork for phase 10 (missions and editor) and phase 15 (content). Clean-room. |
| [`cutscene-format.md`](cutscene-format.md) | The scene format for cartoon cutscenes (WP12.1), its player (WP12.2) and the live preview (WP12.6): layers, rigs, poses, timeline events, validation, the Fountain importer, the `play_scene` viewer and its keys, and the live preview while writing (`--watch`, WP12.6). For writers, artists and the player implementer. |
| [`rules/`](rules/README.md) | Game rules (PLAN.md §5 rule 7). |
| [`progress.md`](progress.md) | Report after each phase. |

Read first: `PLAN.md` §1 (architecture) and §5 (quality rules), then
`conventions.md`.
