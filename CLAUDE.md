# Staalfront

RTS in Rust (eigen dunne engine op wgpu + winit). Het volledige plan staat in
`PLAN.md`; lees §1 (architectuur) en §5 (kwaliteitsregels) vóór elk werk.

- Werkpakketten worden uitgevoerd door de agent `bouwer-sonnet` (`.claude/agents/`).
- Eén werkpakket per branch `wp/<nummer>-<naam>`, altijd via PR.
- `sf_sim` is deterministisch: geen floats, geen HashMap-iteratie, geen klok, geen I/O.
- Dit project draait nooit op de thuisserver (OptiPlex).
