# Staalfront campaign content

This package is the complete design plan for the Staalfront campaigns: world bible, cast, unit roster and tech tree, 20 implementation-ready mission documents, 20 Fountain briefing scripts, a production plan and validation tooling. Story by Tijmen; alignment with PLAN.md (B5, B8, B12, phases 10, 12 and 15, M2) done in the alignment pass of 17-10-2026.

## Contents
- `README.md`: how the folder works, the rules (B5, B8, B12) and how to write briefings in Fountain.
- `world.md`: setting, tone, factions, regions, technology constraints.
- `cast.md`: recurring speakers and production notes. Its names are the only valid briefing cues and MSG speakers.
- `campaign.md`: 10 missions per faction and story interludes.
- `missions/F1-M01.md` … `missions/F1-M10.md`: Meridian Compact scenarios.
- `missions/F2-M01.md` … `missions/F2-M10.md`: Veyran Directorate scenarios.
- `briefings/F1-M01.fountain` … `briefings/F2-M10.fountain`: original briefing dialogue (6–10 lines each).
- `units.md`: canonical unit/structure roles, target stats, speeds in cells per second, per-campaign tech tree, difficulty conventions.
- `CONVENTIONS.md`: the contract every mission document must satisfy.
- `production.md`: phases, pipeline, mapping onto PLAN.md (§2a), engine data and trigger primitives against PLAN (§3, §3a), audio/art scope, testing, risks, open decisions.
- `scripts/validate.py`: `python3 -I content/scripts/validate.py` checks every mission document (references resolve, roles exist, objectives have completing triggers, win/lose present, MSG speakers are in `cast.md`) and every briefing (cues are in `cast.md`, speakers are in the room or marked (V.O.)/(O.S.), transitions other than `TO:` start with `>`), and scans all of `content/` for names that belong to other games (B5). Templates and examples are skipped unless named. Run before each commit.
- `scripts/export_messages.py`: exports all in-mission lines to `voiceover.csv` for the recording session or the voice tool.
- `voiceover.csv`: current export (193 lines, 16 of them OPERATIONS).

## Implementation status
All 20 mission documents carry mission-specific maps (zones, points, size in cells), players, starting setups, variables/timers in seconds, teams, triggers, objectives, flow and test scenarios. All 40 mission and briefing files pass `scripts/validate.py` with zero errors.

Since v3 (17-10-2026):
- **Speeds and timers** are in cells per second (B12, 1 cell = 4 m): infantry 0.7, light tank 2.0, heavy tank 1.1, siege artillery 1.0, light transport 2.5. Every mission has a "Timing check" at the end of its Notes with distance ÷ speed = time. Changed numbers (target lengths of F1-M05, F1-M07, F2-M05, F2-M07; timers of F1-M02, F1-M05, F2-M03, F2-M04; patrol loops, musters, orders) are listed in the PR description.
- **Briefings**: transitions use `>` (`> FADE IN:`, `> FADE OUT.`); radio speakers carry (V.O.) (ENGINEER VENN in F1-M06); `cast.md` lists OPERATIONS in the 11 missions where it speaks.
- **Validator** extended with the briefing checks above; `export_messages.py` skips the template.
- **Production plan** mapped onto PLAN.md; every trigger primitive is listed against WP10.1–10.3, and the ones PLAN does not cover are marked "needs to be added to WP10.x" (`production.md` §3a). This replaces the three engine assumptions that used to be listed here (HP override, inspect without capture, conditional immunity): the list is longer, with terrain state change as the largest item.

Map sketches (`missions/<code>-map.png`) and engine data remain production tasks.

## Open points for Tijmen
Creative or story-level questions that the alignment pass deliberately did not decide are collected in the "Points for Tijmen" list of PR #26 (map size of F1-M02, briefing and message lines whose distances or times no longer match the speeds, the F2-M01 slot in the production phases, the Petrov question, and more).

All content is original: no characters, dialogue, missions or scenes are taken from existing games. The campaign follows the broad mission variety and briefing-led structure that is common in the genre.
