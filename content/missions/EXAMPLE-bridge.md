# EXAMPLE: Hold the Crossing

Worked example of `_template.md`. It is invented for the template and is not part of the
story; delete it or replace it. The briefing is `briefings/EXAMPLE-bridge.fountain`.

## A. Core
Bridge + defend-until-time · difficulty 2 · 15 minutes.
- **Learns**: build a defence tower.
- **New**: defence tower.

## B. Story
The enemy approaches over two bridges. The engineers need 12 minutes to blow the west
bridge.
- MSG_01 (LIEUTENANT ODA): "Armour on the north road, heading for the east bridge!"
- MSG_02 (MAJOR HALVERSEN): "West bridge is down. Hold the east!"

## C. Map
Medium size, with a river running north to south.
- **Bridges**: BRIDGE_W and BRIDGE_E.
- **Zones**: Z_BASE (player base, south-west) and Z_CROSSING_E (east bank by BRIDGE_E).
- **Points**: P_ENEMY_N and P_ENEMY_S.

## D. Players
- **Player**: starts with 3,000 money; only Z_BASE is revealed.
- **Enemy 1**: AI, attacks.

## E. Starting setup
- **Player**: construction_center, 2 light_tank and 4 rifleman in Z_BASE.
- **Neutral**: BRIDGE_W and BRIDGE_E.
- **Key object**: `OBJ_ENGINEER_POST`, a player building next to BRIDGE_W.

## F. Variables and timers
- `T_CHARGE`: timer, 12:00.
- `bridge_w_intact`: yes/no, starts as yes.

## G. Teams
- **T_WAVE_1**: 3 battle_tank. Route: P_ENEMY_N → BRIDGE_E → attack Z_BASE.
- **T_WAVE_2**: 4 battle_tank and 1 artillery. Route: P_ENEMY_S → BRIDGE_W → attack.

## H. Triggers

| Code | When | And also | Then | How often |
|---|---|---|---|---|
| TR_01 | mission start | — | briefing; show objective A; start T_CHARGE | once |
| TR_02 | 2:00 elapsed | — | create T_WAVE_1; MSG_01 | once |
| TR_03 | 6:00 elapsed | — | create T_WAVE_2 | once |
| TR_04 | T_CHARGE expired | `OBJ_ENGINEER_POST` alive | destroy BRIDGE_W; `bridge_w_intact` = no; MSG_02; complete A | once |
| TR_05 | `OBJ_ENGINEER_POST` destroyed | — | lose | once |
| TR_06 | objective A completed | no enemy units in Z_CROSSING_E | win | every time |

## I. Objectives
- **A**: "Protect the engineer post until the west bridge is down (12:00)."
- **Win**: TR_06.
- **Lose**: TR_05.

## J. Flow in words
The first wave comes over the east bridge at 2:00 and tests the player's starting
units. The player builds towers. At 6:00 a heavier wave aims at the west bridge and the
engineer post: that is the real threat. At 12:00 the west bridge drops (twist), the
southern wave is cut off, and the player clears the east bank to win.

## K. Test scenario
With no input, T_WAVE_1 leaves at 2:00. If the post survives, BRIDGE_W is gone at 12:00.
