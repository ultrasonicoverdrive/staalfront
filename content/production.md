# Production plan — Staalfront campaigns

Scope: two campaigns × 10 missions, 20 briefings, 7 voiced characters + system voice, 20 new maps. Target engine: Staalfront's own engine (B1) with the trigger model from WP10.1. The mission documents use the primitives of that model (zones, points, teams, timers, variables, messages); `CONVENTIONS.md` defines the document contract that engine data is generated from. Where a mission needs a primitive WP10.1 does not provide, §7 lists the fallback.

## 0. Current state

| Item | Status |
|---|---|
| World bible, cast, campaign overview | Done (`world.md`, `cast.md`, `campaign.md`) |
| Unit roster and tech tree | Done, untuned (`units.md`) |
| 20 mission specs, implementation-ready | Done, unvalidated in engine (`missions/`) |
| 20 briefing scripts | Done (first draft, 3–5 lines each; see §4) |
| Map sketches | Not started |
| Engine data (map files, trigger scripts) | Not started |
| Audio, art, UI | Not started |

## 1. Pipeline per mission

```
mission doc (md) ──► map sketch (png) ──► map file + trigger script ──► smoke test ──► playtest ──► tuning
       ▲                                                                              │
       └─────────────────── changes always go back into the mission doc ◄─────────────┘
```

Rules:
1. The mission doc is the source of truth. Nothing exists in the map script that is not in the doc, and vice versa (`scripts/validate.py` checks the doc side).
2. Zones/points/objects keep their doc names in the engine (`Z_`, `P_`, `OBJ_`, `T_`, `TR_`, `MSG_`).
3. Each mission gets a `missions/<code>-map.png` sketch before map work starts. Minimum content: zones outlined, points marked, ore fields, start positions, entry routes, map size in cells.
4. Test scenario list (section K) is executed as a checklist on every build of that mission.

## 2. Phases and order of work

| Phase | Missions | Why this order |
|---|---|---|
| P1 Vertical slice | F1-M01, F1-M03, F2-M02 | Deliberately not "the first three campaign missions" (F1-M01–M03): those are two conquest-style maps and one infiltration, which leaves escort AI and the inspection/penalty logic untested until P2. This set covers one of each core type: base-building tutorial, escort with route switch, infiltration with inspection/penalty — and proves the trigger model, escort AI, alarm mechanic and civilian-penalty logic in the first slice. |
| P2 Core systems | F1-M04, F2-M04, F1-M06, F2-M03 | Timed defence, bridge demolition/repair, flood stages, ferry repair. Everything timer/terrain-state related. |
| P3 Mid-campaign | F1-M02, F1-M05, F1-M07, F2-M05, F2-M06, F2-M07 | Commando and deception missions; reuse P1/P2 systems. |
| P4 Late campaign | F1-M08, F1-M09, F2-M08, F2-M09 | Large maps, waves, reserves, artillery scripting. |
| P5 Finals | F1-M10, F2-M10 | Need the full roster, automated_gun prototype and the three-player (Hub) setup. |
| P6 Polish | all | Difficulty curve pass, briefings recorded, interlude screens, localisation. |

Estimated effort per mission (one designer + one scripter, excluding art): P1 missions 4–5 days each (systems are new), later missions 2–3 days each, finals 5 days. Total ≈ 60–70 designer/scripter days before polish. [Gissing] — depends entirely on the engine's trigger tooling.

## 3. Engine data to build

- **Roles**: every row in `units.md` as a unit/structure definition. Mission-only objects (ferry, rail_spur, automated_gun, …) are structures with no build option.
- **Faction restrictions**: build lists per campaign step from the tech-tree table in `units.md`; a campaign-progress flag per mission gates the list.
- **Trigger primitives needed** (collected from all 20 docs): unit enters zone; structure destroyed/captured; unit count in zone; timer expired / timer at value; variable compare/set; team release; reveal zone; terrain state change (bridge_segment destroy/repair, flood cells impassable, tunnel open); play message; set objective state; win/lose. If the engine lacks "terrain state change", flood and scuttle twists are implemented by spawning/removing blocking neutral structures.
- **AI profiles**: `defends`, `attacks`, `harasses` as defined in `units.md`; team orders are explicit so the AI profile only governs base rebuilding and harvesting.
- **Campaign flags**: `completed_<code>`, `evidence_<code>`, `civilian_harm_F2-M02`, `reveal_F2`. Flags change one briefing line at most; never block progression.

## 4. Briefings and audio

- Current scripts are 3–5 lines; target 6–10 lines each (45–75 s) so each briefing states: situation, primary objective, the constraint that makes it hard, and one line that foreshadows the twist without revealing it. Expansion is a writing task, 1 day for all 20.
- In-mission messages: all MSG_ lines in the 20 docs are the voice-over list. Export them to a single sheet (`scripts/export_messages.py`) for the recording session.
- Voice cast: Elian, Mira, Rusk, Sava, Lenka, Petrov (recurring); Venn, Kovac (one-off); Operations (system). Nine actors or doubled roles; Venn/Kovac/Operations can be doubled.
- Music: three states (briefing, tension, combat) plus two stingers (twist, win/loss). Twist stinger plays on every `twist_revealed` trigger.

## 5. Art

- Map tilesets: North Ridge (snow/farmland), The Vale (forest/river), Coastworks (docks/industry), Glass Orchard (greenhouses/channels), Black Basin (foundry/concrete). Five tilesets, reused across both campaigns.
- Unit silhouettes per `world.md` §3. Decoy_emplacement must be pixel-identical to defence_tower from the enemy's side and subtly different for the owner.
- Briefing portraits: poses listed in `cast.md`.
- Interlude screens: 6 (three per campaign) still images with text from `campaign.md`.

## 6. Testing

1. **Doc validation** — `python scripts/validate.py` on every commit: references resolve, roles exist in `units.md`, tech tree respected, required objectives have a completing trigger, loss conditions present.
2. **Smoke test** per mission — the section K test scenario, scripted where the engine allows (force-trigger TR_ codes).
3. **Playtest** — one tester per difficulty band; record completion time vs. target length (±25 % acceptable), number of attempts, where players got stuck. Missions missing the window twice get a tuning pass.
4. **Difficulty curve** — plot attempts per mission; a lighter mission must follow each 4+ mission (F1-M07 after M06, F2-M07 after M06 are the intended breathers).

## 7. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| WP10.1 has no terrain-state triggers (flood, scuttle, bridge) | F1-M06, F2-M04, F2-M06, F2-M08 twists cannot be built as written | Decide in P1; fallback is blocking structures. |
| Escort AI makes convoys unplayable | F1-M03, F2-M03, F2-M07, F2-M08 | Convoy units move only on player order ("follow" mode) rather than scripted paths. Decide in P1. |
| Three-player final (F2-M10) with a hostile-to-all Hub | WP10.1 may not support a non-aligned third AI | Alternative: automated_guns as player-neutral structures with "fire at everyone" flag, no third player. |
| Message count (≈120 lines) outgrows voice budget | polish | Operations lines can be text-only. |
| Decoy mechanic depends on enemy AI targeting rules | F1-M05 | Make decoys explicitly high-priority targets for AI instead of relying on visual deception. |

## 8. Open decisions

- Whether surviving units carry over between missions (docs assume no).
- Localisation: English is the source; Dutch is the first translation if needed.
