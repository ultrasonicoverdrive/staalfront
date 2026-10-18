# Game rules

Quality rule 7 (PLAN.md §5): every non-obvious game rule is documented here.
Phases 0 and 1 contain no gameplay; game rules start in phase 2 (PLAN.md §3).

| File | Rules | Work package |
|---|---|---|
| [`terrain.md`](terrain.md) | Terrain types and what they allow, the map's terrain layer, occupancy and the `Place` command. Since WP4.1 only the buildable and harvestable flags are the type's own; who crosses a type is `movement.md`. | WP2.1 |
| [`units.md`](units.md) | Unit, structure and weapon definitions: files, every field and its unit of measure, flags, what is validated, the conversion for the sim. Stats are the design targets of `content/units.md`; the rest are placeholders. | WP3.1 |
| [`orders.md`](orders.md) | Orders as unit state: stand still (idle), move, attack, attack-move, guard, stop and scatter, the shift queue, arrival tolerance, destination versus route, what phases 4 and 5 replace, command sequence numbers. | WP3.4 |
| [`icons.md`](icons.md) | Strategic icons: the format for artists (64 x 64 PNG, white on transparent, black or cut-out sign), names and `icons.ron`, how a role gets its icon and what the fallback is, how the engine draws it (tint, rim, outline, UI scale), the 14 px distinguishability rule, how to replace a placeholder. | WP3.6 |
| [`cursor.md`](cursor.md) | The cursor context: which cursor the mouse shows (select, inspect, move, queue, attack, attack-move, guard, no-go, eight scroll arrows, placeholders for later phases) and so which order a right-click gives, the decision table, what each movement class can enter, the picture format with hotspots, how the software cursor is drawn and when the system cursor is hidden. | WP3.5 |
| [`movement.md`](movement.md) | Movement classes and terrain costs: the table (foot, wheeled, tracked, hover, water, air against grass, road, water, rock, ore, bridge), where it lives and how it is validated, the bridge rule (land crosses, vessels pass under), how a cost scales a step, a unit stopping at the edge with its order kept, the corner and snap rules, the spawn rule, what ignores terrain, why slope waits, the checksum, the `terrain_costs` scenario. | WP4.1 |
| [`pathfinding.md`](pathfinding.md) | Hierarchical A* for single units: which classes are routed, the costs, sectors of 16 by 16 cells and their portals, the abstract graph and how a sector is rebuilt when a structure changes, the stages of a request, the thick line of sight, the unreachable-goal rule, the route a unit keeps (ring, portal path, refill), the node budget and the queue in the order of the ids, the checksum section. | WP4.2 |
| [`selection.md`](selection.md) | Selection as presentation state: click, drag rectangle, shift, double-click, control groups, inspecting an enemy, the dead leaving the selection, the feedback (rings, icons, health bars, drag rectangle). | WP3.3 |

> **Proposal (not decided):** layout of this directory. One markdown file per rule
> area (for example movement, armor and damage, power, economy, vision). Per rule:
> what it does in numbers and order of evaluation, why, and which RON data fields
> and WP it belongs to. Rules in our own words (clean-room, B5), referencing the
> section of `docs/red-alert-analysis.md` where the idea comes from, never
> original code.
