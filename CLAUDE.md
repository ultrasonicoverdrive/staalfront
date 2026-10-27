# Staalfront

RTS in Rust (own thin engine on wgpu + winit). The full plan is in
`PLAN.md`; read §1 (architecture) and §5 (quality rules) before any work.

- Work packages are built by the agent `bouwer-sonnet` (Sonnet 5, effort **medium**) with the done-checklist in its agent file. The hard WPs marked **[xhigh]** in PLAN.md use the same agent started with per-call `model: opus` and effort **low** (decision Tijmen 25-10-2026). Every fix round after `CHANGES REQUESTED` goes to `bouwer-opus-medium` run at effort **low**, on the same branch (`.claude/agents/`, decision Tijmen 24-10-2026).
- Every PR passes the review gate of agent `reviewer` (Opus, medium) before merge; research questions go to `onderzoeker` (Opus, low). Only `onderzoeker` and `reviewer` may check single facts against EA/OpenRA sources (B5, docs/conventions.md §8).
- One work package per branch `wp/<number>-<name>`, always via PR.
- `sf_sim` is deterministic: no floats, no HashMap iteration, no clock, no I/O.
- This project never runs on the home server (OptiPlex).
