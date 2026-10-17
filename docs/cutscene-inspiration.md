# Cutscenes — inspiration from Red Alert (clean-room)

Written 17-10-2026 as groundwork for phase 12 (cartoon cutscenes) and phase 15
(content, 10 missions per faction). Companion to `docs/mission-inspiration.md`.

Source: the public list of Red Alert (1996) cutscenes and their position in the
campaigns (C&C Central wiki, page "Red Alert cutscenes"), plus general knowledge
of how the games present them.

**Clean-room (B5):** this document describes *which kinds* of scenes exist,
*where* in a campaign they sit and *what job* they do. It does not describe
plots, characters, dialogue, shots or images. Nothing from those scenes may be
copied, traced or redrawn. This applies to characters, lines, story beats and
compositions, and also to real historical figures as Red Alert portrays them.
Staalfront's world and story are its own and have not been decided yet.

---

## 1. What Red Alert used

Red Alert has about 150 video clips across the base game and its expansions.
Grouped by function they fall into a handful of kinds:

| Kind | What it is | Where it sits | Rough count in Red Alert |
|---|---|---|---|
| **Game intro** | Long opening film that sets up the world and the conflict | once, before the main menu | 1 (+1 expansion) |
| **Mission briefing** | A commander talks to the player (to camera), often with a map or intelligence material; it states the goal and the stakes | before a mission | ~14 per side |
| **Story interlude** | Short scene with recurring characters that moves the plot on; often split around a mission ("before" and "after") | between missions, at turning points | a few per side |
| **Opening sting** | 5–20 s of action with units and no dialogue: a landing, a convoy at a bridge, a spotter with binoculars | right before gameplay starts | ~25 shared |
| **Victory sting** | Short action payoff: the target blows up, the units get away | right after a won mission | ~25 shared |
| **Defeat sting** | Short scene showing the cost of failing: a meltdown, a lost ship | after a lost mission | a handful |
| **Faction logo sting** | The faction emblem animates | after a win, as a "stamp" | 1 per side |
| **Campaign map** | The front line or territory moves across a map between missions | between missions | 1 recurring element |
| **Campaign ending** | A closing film per side; there can be more than one ending | after the last mission | 1 per side |
| **Expansion intros/outros** | Per mini-campaign | around expansion packs | — |

Patterns worth taking over (structure only):

- **The briefing is the backbone.** Every mission has one. It is short, one speaker
  plus sometimes a second, and it ends with a clear order.
- **Stings are reused across missions.** One "landing" or "artillery hit" clip is
  used before or after several missions. A small set of stings covers a whole
  campaign.
- **Story interludes are rare and placed at turning points:** around a third of the
  way in, at the midpoint, before the finale.
- **Defeat gets its own scene** for key missions only; otherwise the mission simply
  restarts.
- **The campaign map** gives a sense of progress with one reusable element.

## 2. What this means for Staalfront (2 factions × 10 missions)

Staalfront makes these in two ways (PLAN §9):

- **Cartoon scenes in `sf_cutscene`** (2D cutout figures, thick lines, flat
  colours): briefings, story interludes, the game intro and the endings. These need
  drawn figures, voices and direction.
- **In-engine 3D scenes** using the game's own renderer, models, terrain and
  effects, with a scripted camera: opening, victory and defeat stings, the logo
  sting and the campaign map. They need **no drawings and no voices**, only scene
  scripts. This is new compared with PLAN §9 and is proposed here as WP12.10 (see
  §4).

Proposed inventory [estimate; content still to be decided]:

| Kind | Made as | Number | Length each | Voices |
|---|---|---|---|---|
| Game intro | cartoon (or in-engine + cartoon) | 1 | 60–120 s | narrator |
| Mission briefing | cartoon | 20 | 30–90 s | 1–2 speakers |
| Story interlude | cartoon | 2–3 per faction | 30–60 s | 2–3 speakers |
| Opening sting | in-engine 3D | ~8 shared | 5–15 s | none |
| Victory sting | in-engine 3D | ~8 shared | 5–15 s | none |
| Defeat sting | in-engine 3D | 2–3 | 5–15 s | none |
| Faction logo sting | in-engine or 2D | 2 | 3–5 s | none |
| Campaign map | in-engine 2D/3D | 1 element, 20 states | 5–10 s | optional narrator line |
| Campaign ending | cartoon | 2 (more if branching) | 60–120 s | 2–3 speakers |

That totals about **25–27 cartoon scenes**: 1 intro, 20 briefings, 4–6
interludes and 2 endings, all with voices. On top come **about 20 in-engine
stings** without voices.

M2 (playable alpha) needs the first 3 briefings per faction plus a few stings.

## 3. Briefing pattern (structure, to fill with our own content)

1. Establishing shot: the location (command post, field tent, vehicle). 2–3 s, no
   dialogue.
2. The speaker addresses the player. The situation is told in 1–2 lines.
3. Cut to the map or intelligence material. The objective is shown with an arrow
   or marker.
4. Optional second speaker: a complication or a personal stake, 1–2 lines.
5. Back to the main speaker: a clear order and an encouragement or warning. End on
   a reaction or a hard cut to the mission.

This pattern maps directly to the mission template: the briefing table plus the
objectives.

## 4. Proposed additions to phase 12

To make the cartoon scenes possible without a full art and voice team (Tijmen,
17-10-2026), these could be added after M1. They are proposals and are not yet in
PLAN.md.

- **WP12.7 Voice tool**: offline text-to-speech for dialogue lines (English per
  B8), giving a WAV per line, with lip-sync from WP12.3. A candidate engine is
  Piper (MIT); the licence of each voice must be checked. It is good enough for
  placeholders. Whether it is good enough for final voices is to be judged by ear.
- **WP12.8 Cartoon figure generator**: parametric cutout characters built from
  shapes. Head, eyes, mouth shapes, uniform, arms and props are separate layers in
  the §9 style. The output is the part set the WP12.2 player expects.
- **WP12.9 Auto-direction**: from a dialogue script with stage directions, generate
  shots. Close-up on the speaker, occasional reaction shots, map inserts, timing
  from the audio length, transitions. The result can be edited in the WP12.6
  preview tool.
- **WP12.10 In-engine stings**: scripted camera paths and unit actions using the
  game renderer, played from triggers (WP10.3 "start cutscene"). Opening, victory
  and defeat stings, logo stings and the campaign map.
