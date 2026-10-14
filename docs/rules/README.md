# Game rules

Quality rule 7 (PLAN.md §5): every non-obvious game rule is documented here.

Nothing is here yet; game rules start in phase 2 (PLAN.md §3). Phases 0 and 1
contain no gameplay.

What goes in this directory:

- One markdown file per rule area (for example movement, armor and damage, power,
  economy, vision), named after the area.
- For each rule: what it does in numbers and order of evaluation (tick-exact where
  relevant), the reason for it, and which RON data fields and WP it belongs to.
- Rules in our own words (clean-room, B5). Reference the section of
  `docs/red-alert-analysis.md` if the idea comes from there; never paste original
  code.
- A rule change that alters sim behavior changes checksums; see
  `docs/conventions.md` section 5.
