# Missions — inspiration from Red Alert (clean-room)

Written 14-10-2026 as groundwork for phase 10 (missions and editor) and phase 15
(content). Source: the public EA repository `electronicarts/CnC_Red_Alert`
(headers `TEVENT.H`, `TACTION.H`, `TRIGTYPE.H`, `TEAMTYPE.H`, `SCENARIO.H`) and
general knowledge of the campaigns.

**Clean-room (B5):** this document describes *what* the system can do, in our
own words. No code has been copied, and none may be when building. Names,
characters, stories, units, and texts from Red Alert are trademarks/copyright of
EA and do not appear in Staalfront.

**Important:** the repository contains **no mission files**. The scenarios
(maps, triggers, briefings) were in the data packages of the purchased game,
not in the source code. What the code does show is the *tooling* the missions
were built with: the trigger system. The mission types below come from general
knowledge of the campaigns, at the level of game design.

---

## 1. How a mission is put together

A scenario consists of:

- **starting setup**: map, players (houses), starting objects, money, vision;
- **teams**: groups of units with a fixed sequence of orders (see §3);
- **triggers**: *event(s) → action(s)*, attached to an object, a cell, a zone,
  or a player;
- **global flags and a mission timer** to tie triggers together;
- **per-scenario options**: among others carrying units over to the next
  mission, inheriting the timer from the previous mission, last mission of the
  campaign, no score screen, playable only once, no map selection, money from
  crates/resources.

### Trigger structure

- A trigger has **two events** and **two actions**, combined as
  *first only*, *both (AND)*, *either (OR)*, or *linked*
  (event 1 → action 1, event 2 → action 2).
- **Lifetime**: *one-shot* (disappears after firing), *semi-persistent* (fires
  when all objects it is attached to have reported in), or *persistent* (fires
  every time).
- A trigger belongs to a **player**; events such as "building destroyed"
  then apply to that player.

For Staalfront: this model is small, powerful, and easy to make deterministic.
Improvement over the original: an arbitrary number of conditions and
actions per trigger instead of exactly two, and named variables instead of
numbered global flags.

## 2. Building blocks (functional)

**Events**

| Group | What |
|---|---|
| Location | player enters object/cell; unit enters a zone; line crossed horizontally or vertically; unit leaves the map |
| Discovery | object discovered; one player discovers another player |
| Combat | attacked; destroyed; N units / N buildings destroyed; all units / buildings / everything of a player destroyed; all bridges destroyed; fake buildings destroyed; no factories left |
| Infiltration | building spied on; building robbed; civilian evacuated |
| Economy and construction | money above a threshold; building/unit/infantry/aircraft of a type built; building type exists; power shortage |
| Time | time elapsed; mission timer expired |
| Logic | global flag set / cleared; "always" |

**Actions**

| Group | What |
|---|---|
| Outcome | win; lose; allow win (only win once something else is also true) |
| Army | create / disband team; reinforcements; everything goes hunting; AI starts producing; AI may form teams automatically; preferred target for the AI |
| Base | AI builds out its base; AI sells everything ("fire sale") |
| Map and vision | reveal entire map; reveal area / zone; let vision fog over again; mark landing zone |
| Presentation | play movie, sound, music, or speech; text message |
| Logic | force or remove another trigger; start / stop / increase / decrease / set timer; set / clear global flag |
| Special | destroy specific object; fully charge or fire a superweapon |

## 3. Teams: scripted groups

A team is a composition (e.g. 4 tanks + 6 infantry) with an
**order list** that is worked through step by step:

attack (target type) · attack toward waypoint · adopt formation · move to
waypoint · move to cell · guard (duration) · loop back to step N · attack
on command · unload · deploy · chase to the death · carry out a
unit order · set global flag · temporarily invulnerable ·
load · spy · patrol.

Teams also have properties such as: refill when members die,
only depart on an event, wait until the team is complete, and
whether the AI may create them automatically.

## 4. Mission types (game design, not story)

| Type | Core | Building blocks |
|---|---|---|
| **Destroy base** | classic: build, harvest, eliminate the opponent | "everything destroyed" → win |
| **Rescue/escort** | bring a vulnerable unit to an evacuation point | enter zone / leave map → win; that unit destroyed → lose |
| **Commando** | few units, no base, strong hero; sneak and sabotage | no factories; specific buildings destroyed → win |
| **Convoy** | trucks must make it to the other side; you secure the route | N of M arrived → win; teams in ambushes |
| **Defend until time runs out** | survive waves of attacks | mission timer expired → win; key building destroyed → lose |
| **Infiltration** | a spy or thief must get into a building; afterwards something is revealed/opened | spied on → reveal map / reinforcements |
| **Bridge/crossing** | destroy bridges or, conversely, keep them intact | all bridges destroyed → win or lose |
| **Time pressure** | a superweapon is charging; you must disable it before the timer runs out | timer + "specific object destroyed" |
| **Escape** | base lost, units must leave the map | map left by N units → win |
| **Conquest** | capture buildings instead of demolishing them | building changed owner (own extension) |
| **Fakes and deception** | fake buildings or decoys to mislead the enemy | fake buildings destroyed; AI preferred target |

Variety within a campaign comes mainly from: base or no base, time pressure,
a vulnerable key unit, and the turning point halfway through (reinforcements, new
objective, revealed map) via triggers.

## 5. What this means for the plan

- **WP10.1/10.2** (scenario format and triggers): the model of §1 with an
  unlimited number of conditions/actions, named variables, and the
  one-shot/semi-persistent/persistent lifetimes. Triggers evaluate in the sim tick, in fixed
  order (determinism).
- **WP10.3** (actions): the table in §2 is a good first list; "allow win"
  and "AI fire sale" are small and add a lot of game feel.
- **Teams (§3)** belong to the AI (phase 9) and to missions; one order language
  for both.
- **WP15** (content): the mission types of §4 as a checklist, with a world
  and story of our own. That world has not been decided yet.
