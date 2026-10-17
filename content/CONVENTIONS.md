# Mission document conventions (for writers and implementers)

Every `missions/*.md` must satisfy these rules before it counts as implementation-ready.

1. **Sections A–K stay in the same order and with the same headings** as the template. Section A stays as is (except correcting obvious errors). Section B keeps Situation/Briefing/Twist/Story beat, but the message table and endings are mission-specific.
2. **Messages**: MSG_01/02 may stay. Replace generic MSG_03 ("The situation has changed…") and MSG_04 ("Objective secured…") with mission-specific lines spoken by cast members (see `cast.md`; "OPERATIONS" is allowed only for neutral system text). Add MSG_05…MSG_08 as needed for the twist, warnings (e.g. timer at 50 %), and win/loss. Every message referenced in a trigger must exist in the table and vice versa.
3. **Endings**: win and loss text must be specific to this mission and must feed the story beat.
4. **Map (C)**: name the actual zones and points for this mission with descriptive names (`Z_FOREST_BYPASS`, `P_BRIDGE_EAST`), give an approximate map size in cells (e.g. 96×96), and describe each zone's location in map terms (north-west quarter, along the river, etc.). Drop generic rows that do not apply. Add a short "Sketch description" paragraph a map artist can draw from.
5. **Players (D)**: concrete credits, AI behaviour, alliances. Add a second enemy or a neutral "Hub" player only if the mission needs it.
6. **Starting setup (E)**: exact counts per role and zone, roles only from `units.md`. Commando/infiltration missions have no construction_center and no harvester. Every key object gets a unique `OBJ_` name and a location.
7. **Variables/timers (F)**: only variables actually used by triggers; timers with exact start values in seconds.
8. **Teams (G)**: composition with counts, trigger that releases them, ordered orders, refill yes/no. Waves must respect the difficulty wave sizes in `units.md`.
9. **Triggers (H)**: numbered TR_01…; every trigger references only zones, points, objects, teams, timers, variables and messages defined in this document. No generic "where applicable" rows. Cover: start, each objective completion, the twist, timer warnings, win, each loss condition.
10. **Objectives (I)**: player-facing text; each required objective completed by exactly one named trigger. Win and lose statements are exact.
11. **Flow (J)**: 8–12 numbered steps with approximate minute marks, matching the target length.
12. **After the mission (K)**: carry-over flags, optional evidence flag name (`evidence_<code>`), and a test scenario list of at least 6 concrete checks.
13. Keep the existing "## Notes" paragraph at the end.
14. Language: English, as in the source files.
