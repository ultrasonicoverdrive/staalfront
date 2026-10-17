# Content: world, cast, campaign, missions, briefings

This folder holds Staalfront's **story content**, written by hand before the engine can
play it. The files are plain text, so they work in any editor, diff well in git, and
can later be converted into the engine formats:

- missions → scenario format (WP10.1–10.3);
- briefings → cutscene scripts (WP12.1). Later they also feed the voice tool and the
  auto-direction proposed in `docs/cutscene-inspiration.md` §4 (not yet in PLAN.md).

| File | What goes in it |
|---|---|
| `world.md` | The world bible: setting, history, factions, tone. Fill this in first. |
| `cast.md` | Every speaking character: role, look, voice. |
| `campaign.md` | Both campaigns, one line per mission. |
| `missions/<code>.md` | One file per mission, copied from `missions/_template.md`. |
| `briefings/<code>.fountain` | One briefing script per mission, in Fountain format, copied from `briefings/_template.fountain`. |
| `units.md` | The unit and structure roles missions may use, their stats, **speeds in cells per second**, the tech tree per campaign. |
| `CONVENTIONS.md` | The contract every mission file must satisfy (sections A-K, messages, timers, triggers). |
| `production.md` | Production plan: phases, how they map onto PLAN.md (phases 10, 12, 15, M2), the trigger primitives the engine must offer (§3a), voice and art scope, risks. |
| `scripts/validate.py` | The checker, see "Checking" below. |
| `scripts/export_messages.py` | Writes every in-mission line to `voiceover.csv`. |
| `STATUS.md` | Where the package stands. |

## Rules

- **Own content only (B5).** No names, characters, stories, dialogue or scenes from Red
  Alert or other games, not even redrawn or paraphrased. Taking over the *structure* is
  fine: see `docs/mission-inspiration.md` and `docs/cutscene-inspiration.md`.
- **Game text is English (B8).** Dialogue, objectives and messages go in English. Notes
  for yourself may be in any language. Put them in Fountain notes `[[ ... ]]` or in the
  "Notes" sections.
- **Units by role, not by final name**: `light_tank`, `artillery`, `engineer`, and so on.
  Use a plain English role name (`battle_tank`, `construction_center`,
  `resource_truck`). The final unit names come later.
- **Map sizes in cells**: one cell is 4 m (PLAN.md B12). Maps are 64×64 to 256×256
  cells (256 m to 1 km): each side 64 to 256 cells. A tank is about one cell, a building 2×2 to 4×3 cells.
- **Speeds in cells per second** (`units.md`): infantry 0.7, light tank 2.0, heavy tank 1.1. Work out every travel time as distance in cells ÷ speed, and write the arithmetic at the end of the mission's Notes (CONVENTIONS.md rule 15).
- **Codes**: missions are `F1-M01` … `F2-M10` (faction, mission). A mission file and
  its briefing share the code.

## Writing briefings in Fountain

Fountain is an open plain-text screenplay format: <https://fountain.io/syntax>.

Free tools that preview Fountain as a formatted script:

- **VS Code** with the extension *Better Fountain*. It works on Windows, has a live
  preview and shows the number of lines per character.
- **Afterwriting** (<https://afterwriting.com>). It runs in the browser, turns Fountain
  into PDF, and shows statistics such as the estimated length of each scene.
- Any text editor also works: Fountain is just text.

The Staalfront conventions on top of standard Fountain all use Fountain *notes*, so
the files stay valid Fountain:

| Write | Means |
|---|---|
| `INT. COMMAND TENT - NIGHT` | Location/background. Each new location is a new drawing, so keep briefings to 1–2 locations. |
| `COMMANDER` (line in capitals, dialogue below it) | A spoken line. Later it becomes a close-up, a voice line and lip-sync. |
| `(grim)` under the name | Pose or emotion for the figure and the voice. |
| `[[SFX: radio static]]` | Sound effect at this point. |
| `[[MUSIC: tense, low]]` / `[[MUSIC: stop]]` | Music cue. |
| `[[AMBIENCE: wind, distant guns]]` | Background sound under the scene. |
| `[[SHOT: map]]` / `[[SHOT: wide]]` / `[[SHOT: reaction ADJUTANT]]` | Camera hint for the auto-director. Optional: without one it picks a shot itself. |
| `[[MAP: arrow from P_START to BRIDGE_W]]` | Something drawn on the briefing map. |
| `CUT TO:` / `> FADE IN:` / `> FADE OUT.` | Transition. Fountain recognises lines ending in `TO:` by itself; every other transition, `FADE IN:` and `FADE OUT.` included, needs a leading `>`. |
| `ENGINEER VENN (V.O.)` / `(O.S.)` | A speaker who is not in the room (on the radio or off screen). Everyone else must be named in the action lines before they speak. |

Length guideline: 30–90 seconds per briefing, which is about 60–200 spoken words.
The objective must be spoken clearly in the last 2–3 lines.

## Checking

```
python3 -I content/scripts/validate.py
python3 content/scripts/export_messages.py
```

`validate.py` checks the 20 mission files (sections, references, roles, objectives, win/lose, MSG speakers) and the 20 briefings (every cue is in `cast.md`; every speaker is in the room or marked `(V.O.)`/`(O.S.)`; transitions other than `TO:` start with `>`). It skips `_template.*` and `EXAMPLE-*`; name a file to check it anyway. The exit code is 1 on any error. Run it before each commit, and run `export_messages.py` after changing a message.

## Workflow

1. Fill in `world.md` and `cast.md`.
2. Fill in `campaign.md`: 10 missions per faction, one line each, using the mission
   types from `docs/mission-inspiration.md`. Do not make every mission "destroy base".
3. Work out missions 1–3 per faction first. M2 needs those 6 with briefings.
4. For each mission, copy the two templates, fill them in, and commit.
5. Check that every trigger in the mission file refers to zones, teams, messages and
   objects that are defined in that same file.
