# Unit and structure roster — Staalfront

Canonical role names. Mission documents, team tables and triggers must only use names from this file. Stats are design targets for first implementation (tune in playtest).

**Units of measure**: distances and ranges are in cells (1 cell = 4 m, per B12). **Speed is in cells per 10 s of game time**, so speed 4 = 1.6 m/s (infantry walking pace), speed 6 = 2.4 m/s, speed 7 = 2.8 m/s. These are relative gameplay values on a compressed map scale, not realistic vehicle speeds. HP is abstract. Cost is in credits.

## Shared structures

| Role | Cost | Power | HP | Notes |
|---|---:|---:|---:|---|
| construction_center | — | 0 | 1500 | Base anchor. Build radius 8 cells (32 m). |
| power_plant | 300 | +100 | 600 | |
| refinery | 1200 | −30 | 900 | Comes with 1 harvester. |
| barracks | 400 | −20 | 700 | Infantry. |
| vehicle_factory | 1500 | −40 | 1000 | Vehicles. |
| radar | 900 | −50 | 700 | Minimap + reveals 12-cell radius. Unlocked F1-M08 / F2-M08. |
| defence_tower | 500 | −20 | 600 | Anti-infantry/light vehicle, range 6. Unlocked F1-M04 / F2-M04. |
| advanced_defences | 900 | −40 | 800 | Anti-armour gun emplacement, range 7. Unlocked F1-M09 / F2-M09. |
| naval_yard | 1000 | −40 | 900 | Only on coastal maps (F1-M06, F2-M06). |
| decoy_emplacement | 150 | 0 | 150 | Meridian only. Looks like defence_tower to enemy AI; draws fire. Unlocked F1-M05. |
| fortified_bunker | 600 | 0 | 1200 | Veyran only. Garrison 5 infantry. Unlocked F2-M04. |

## Shared units

| Role | Cost | HP | Speed (cells/10 s) | Weapon | Notes |
|---|---:|---:|---:|---|---|
| harvester | 1100 | 800 | 4 | — | Collects ore. |
| rifleman | 100 | 50 | 4 | rifle, range 4 | Baseline infantry. |
| rocket_trooper | 300 | 45 | 4 | rocket, range 5, anti-armour | Unlocked from M03 both factions. |
| engineer | 500 | 25 | 4 | — | Captures/repairs structures; places demolition_charge on marked objects. Unlocked M02. |
| scout | 400 | 25 | 6 | pistol, range 3 | Sight 8, invisible to enemies while stationary in forest/ruin cells. Unlocked M02. |
| sniper | 600 | 40 | 4 | long rifle, range 8, infantry only | Meridian. Unlocked F1-M07. |
| medic | 400 | 40 | 4 | — | Heals infantry in radius 2. Veyran. Unlocked F2-M07. |
| demolition_team | 700 | 60 | 4 | satchel charge | Veyran. Destroys any structure on contact, consumed. Unlocked F2-M05. |
| light_tank | 700 | 400 | 6 | cannon, range 5 | |
| heavy_tank | 1400 | 900 | 4 | twin cannon, range 5 | Unlocked F1-M08 / F2-M09 (Veyran gets heavy_gun_team at F2-M04 instead). |
| siege_artillery | 1600 | 300 | 3 | howitzer, range 10, min range 3 | Unlocked F1-M09 / F2-M09. |
| repair_truck | 800 | 350 | 5 | — | Repairs vehicles in radius 2. Unlocked F1-M03 / F2-M08. |
| transport_truck | 600 | 300 | 6 | — | Carries 5 infantry. Veyran F2-M03; Meridian has light_transport. |
| light_transport | 600 | 250 | 7 | — | Meridian. Carries 4 infantry. Available from F1-M03. |
| amphibious_transport | 900 | 400 | 5 (land) / 4 (water) | — | Carries 5 infantry or 1 vehicle. Unlocked F1-M06. |
| combat_engineer | 600 | 60 | 4 | carbine, range 3 | Veyran. Engineer that can fight and build fortified_bunker in the field. Unlocked F2-M04. |
| heavy_gun_team | 500 | 80 | 3 | anti-tank gun, range 6 | Veyran. Deploys (immobile while firing). |
| gunboat | 1200 | 600 | 6 | cannon, range 6 | naval_support. |
| supply_truck | — | 300 | 5 | — | Mission object only (convoys). Not buildable. |
| civilian | — | 25 | 3 | — | Neutral. |
| technician | — | 25 | 3 | — | Neutral specialist (mission object). |

## Enemy-only / mission objects

| Role | HP | Notes |
|---|---:|---|
| relay_transmitter | 400 | Structure. |
| fuel_depot | 500 | Explodes, damages radius 3. |
| archive_building | 800 | Must survive in evidence missions. |
| evidence_crate | 100 | Picked up by infantry walking onto it. |
| ferry | 600 | Neutral vessel, mission object. |
| bridge_segment | 500 | Destroyable/repairable by engineers. |
| rail_spur | 600 | Demolition target. |
| automated_gun | 700 | Hub prototype defence, range 8, fires at all factions. F2-M10 only; foreshadowed in F2-M09 as "unmanned batteries". |
| generator | 400 | Backup power structure. |
| unmanned_artillery | 300 | Enemy structure, fires when uplink alive (F2-M09). |
| ore_field | — | Terrain resource. |

## Tech tree per campaign (cumulative)

| After | Meridian (F1) | Veyran (F2) |
|---|---|---|
| M01 | power_plant, barracks, refinery, rifleman, harvester | same |
| M02 | engineer, scout | engineer, scout |
| M03 | vehicle_factory, light_tank, rocket_trooper, repair_truck, light_transport | vehicle_factory, light_tank, rocket_trooper, transport_truck |
| M04 | defence_tower | defence_tower, combat_engineer, fortified_bunker, heavy_gun_team |
| M05 | decoy_emplacement | demolition_team |
| M06 | amphibious_transport, naval_yard | naval_yard, gunboat (naval_support) |
| M07 | sniper | medic |
| M08 | heavy_tank, radar | repair_truck, radar |
| M09 | siege_artillery, advanced_defences | siege_artillery, heavy_tank, advanced_defences |
| M10 | full roster | full roster |

## Difficulty and timing conventions

- Money values are credits; starting credits are in each mission's Players table.
- Timers are in game seconds (mm:ss in text).
- AI "defends" = rebuilds, does not attack outside its zones; "attacks" = sends T_ teams on timers; "harasses" = small teams every 90–150 s.
- Enemy attack wave sizes by difficulty: D1 4–6 units, D2 6–8, D3 8–12, D4 12–16, D5 16–20.
