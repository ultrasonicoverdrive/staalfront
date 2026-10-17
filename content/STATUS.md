# Staalfront campaign content

This package is the complete design plan for the Staalfront campaigns: world bible, cast, unit roster and tech tree, 20 implementation-ready mission documents, 20 Fountain briefing scripts, a production plan and validation tooling.

## Contents
- `world.md`: setting, tone, factions, regions, technology constraints.
- `cast.md`: recurring speakers and production notes.
- `campaign.md`: 10 missions per faction and story interludes.
- `missions/F1-M01.md` … `missions/F1-M10.md`: Meridian Compact scenarios.
- `missions/F2-M01.md` … `missions/F2-M10.md`: Veyran Directorate scenarios.
- `briefings/F1-M01.fountain` … `briefings/F2-M10.fountain`: original briefing dialogue.
- `units.md`: canonical unit/structure roles, target stats, per-campaign tech tree, difficulty conventions.
- `CONVENTIONS.md`: the contract every mission document must satisfy.
- `production.md`: phases, pipeline, engine data to build, audio/art scope, testing, risks, open decisions.
- `scripts/validate.py`: checks every mission document (references resolve, roles exist, objectives have completing triggers, win/lose present). Run before each commit.
- `scripts/export_messages.py`: exports all in-mission lines to `voiceover.csv` for the recording session.
- `voiceover.csv`: current export (193 lines).

## Implementation status
All 20 mission documents now carry mission-specific maps (zones, points, size in cells), players, starting setups, variables/timers in seconds, teams, triggers, objectives, flow and test scenarios, and pass `scripts/validate.py` with zero errors. Three engine assumptions are stated inside the documents where used: a per-mission HP override for escort characters (F2-M04), "inspect without capture" for engineers (F2-M02), and objects immune to ordinary fire until a condition is met (F2-M04, F2-M09, F1-M07). See `production.md` §7 for fallbacks. Map sketches (`missions/<code>-map.png`) and engine data remain production tasks.

All dialogue, characters, faction names, and story events are original. The campaign takes inspiration from the broad mission variety and briefing-led structure of classic RTS games, not from copied Red Alert content.
