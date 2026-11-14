# Staalfront

RTS in Rust (own thin engine on wgpu + winit). The full plan is in
`PLAN.md`; read §1 (architecture) and §5 (quality rules) before any work.

- Work packages are built by the agent `bouwer-sonnet` (Sonnet 5, effort **medium**) with the done-checklist in its agent file. The hard WPs marked **[xhigh]** in PLAN.md use the same agent started with per-call `model: opus` and effort **low** (decision Tijmen 25-10-2026). Every fix round after `CHANGES REQUESTED` goes to `bouwer-opus-low` run at effort **low**, on the same branch (`.claude/agents/`, decision Tijmen 24-10-2026). A PR rejected twice goes first to `onderzoeker` on model fable, effort low (read-only), for a fix plan; then a fix round on that plan. Design and diagnosis work without a fixed outcome may use `onderzoeker` on fable, effort low (Tijmen, 10-11-2026).
- Before its first review every PR gets a read-only sweep by `onderzoeker` on model fable, effort low: every rule the PR changes, every place that uses it and whether a test pins it, claims without proof, likely review findings (Tijmen, 11-11-2026). Concrete defects go back to the builder before the review; the rest goes into the reviewer's brief.
- Every PR passes the review gate of agent `reviewer` (Opus, medium) before merge; research questions go to `onderzoeker` (Opus, low). Only `onderzoeker` and `reviewer` may check single facts against EA/OpenRA sources (B5, docs/conventions.md §8).
- One work package per branch `wp/<number>-<name>`, always via PR.
- `sf_sim` is deterministic: no floats, no HashMap iteration, no clock, no I/O.
- This project never runs on the home server (OptiPlex).
