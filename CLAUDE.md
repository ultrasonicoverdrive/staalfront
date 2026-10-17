# Staalfront

RTS in Rust (own thin engine on wgpu + winit). The full plan is in
`PLAN.md`; read §1 (architecture) and §5 (quality rules) before any work.

- Work packages are executed by the agent `bouwer-opus` (Opus, effort low), also the WPs marked **[xhigh]** in PLAN.md; a WP that fails review twice is rerun on `bouwer-opus-zwaar` (Opus, effort high) (`.claude/agents/`, decision Tijmen 17-10-2026).
- Every PR passes the review gate of agent `reviewer` (Opus, medium) before merge; research questions go to `onderzoeker` (Opus, low).
- One work package per branch `wp/<number>-<name>`, always via PR.
- `sf_sim` is deterministic: no floats, no HashMap iteration, no clock, no I/O.
- This project never runs on the home server (OptiPlex).
