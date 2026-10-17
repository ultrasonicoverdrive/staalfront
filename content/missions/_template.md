# <CODE>: <Title>

Copy this file to `missions/<CODE>.md` (e.g. `F1-M03.md`). Each section maps to a part
of the scenario format (WP10.1–10.3).

## A. Core

| Field | Value |
|---|---|
| Code | |
| Title | |
| Player faction | |
| Mission type(s) | destroy base / rescue-escort / commando / convoy / defend-until-time / infiltration / bridge / time pressure / escape / conquest / deception |
| Difficulty (1–5) | |
| Target length (minutes) | |
| What the player learns | |
| New to build | |
| Still locked | |

## B. Story

- **Situation** (2–4 sentences):
- **Briefing**: `briefings/<CODE>.fountain`
- **Messages during the mission** (English; referenced by triggers):

| Code | Speaker | Text |
|---|---|---|
| MSG_01 | | |

- **Ending (win)**:
- **Ending (loss)**:

## C. Map

- **Map**: an existing map name, or "new".
- **Size and terrain**: in proportions, e.g. "medium; river north to south, two bridges".
- **Sketch**: attach `missions/<CODE>-map.png`. A photo of a paper drawing is fine.
- **Zones**:

| Zone | Where | Used for |
|---|---|---|
| Z_START | | |

- **Points**:

| Point | Where |
|---|---|
| P_ | |

## D. Players

| Player | Faction | Human/AI | Money | Colour | Allied with | AI at start |
|---|---|---|---|---|---|---|
| Player | | human | | | | — |
| Enemy 1 | | AI | | | | passive / defends / expands / attacks |
| Neutral | — | — | — | grey | | civilians, buildings |

**Vision at start**: all hidden, or which zones are revealed.

## E. Starting setup

| Owner | Object (role) | Count | Where | Note |
|---|---|---|---|---|
| | | | | |

**Key objects** (anything a trigger refers to, with a name such as `OBJ_...`):

## F. Variables and timers

| Name | Kind | Start | Meaning |
|---|---|---|---|
| | yes/no / number / timer | | |

## G. Teams

| Team | Owner | Composition (roles × count) | Leaves when | Orders, in order | Refill? |
|---|---|---|---|---|---|
| T_ | | | | | |

**Orders**:
- move to point/zone
- attack (type or target)
- guard (time)
- patrol between points
- load / unload
- deploy
- form up
- set variable
- invulnerable for a while
- go to step N
- hunt to the death

## H. Triggers

| Code | When (events) | And also (conditions) | Then (actions) | How often |
|---|---|---|---|---|
| TR_01 | mission start | — | | once |

**Events**
- **Place**: enters zone/object, crosses line, leaves map.
- **Discovery**: discovers object/player.
- **Combat**: object attacked/destroyed, N units/buildings of X destroyed, all of X destroyed, all bridges destroyed, no factories left.
- **Infiltration**: building spied on/robbed, civilian evacuated.
- **Economy**: money above threshold, type built, building exists, power shortage.
- **Time**: time elapsed, timer expired.
- **Logic**: variable becomes true/false, always.

**Actions**
- **Outcome**: win, lose, allow win.
- **Army**: create/disband team, reinforcements at point, all hunt, AI starts producing, AI may form teams, AI preferred target.
- **Base**: AI expands, AI sells everything.
- **Vision**: reveal map/zone, mark landing zone.
- **Presentation**: message MSG_, sound, music, cutscene, camera to point.
- **Logic**: force/disable trigger, timer start/stop/set, set variable.
- **Special**: destroy object, superweapon charge/fire, show/complete objective.

**How often**: once / every time / when all attached objects have reported.

## I. Objectives (as the player sees them)

| ID | Text (English) | Required? | Completed by trigger |
|---|---|---|---|
| A | | yes | |

**Win**:
**Lose**:

## J. Flow in words

_5–10 lines describing how the mission is meant to play out, including the twist. A
reviewer must be able to check that the triggers do this._

## K. After the mission

- **Carry over**: surviving units yes/no, timer yes/no.
- **Choice**: next mission, or a branch (e.g. M04A / M04B).
- **Test scenario**: what a headless test must prove, e.g. "with no input, T_ATTACK_1
  leaves at 3:00".

## Notes
