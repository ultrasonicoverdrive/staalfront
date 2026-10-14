# Staalfront

RTS in Rust (own thin engine on wgpu + winit). The full plan is in
`PLAN.md`; read §1 (architecture) and §5 (quality rules) before any work.

- Work packages are executed by the agent `bouwer-sonnet` (effort medium); WPs marked **[xhigh]** in PLAN.md by `bouwer-sonnet-xhigh` (`.claude/agents/`).
- One work package per branch `wp/<number>-<name>`, always via PR.
- `sf_sim` is deterministic: no floats, no HashMap iteration, no clock, no I/O.
- This project never runs on the home server (OptiPlex).
