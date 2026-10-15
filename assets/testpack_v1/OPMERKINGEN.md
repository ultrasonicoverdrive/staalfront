# Testpakket v1: status en bekende punten

Aangeleverd door Tijmen op 15-10-2026, gemaakt met Microsoft Copilot naar de
specificatie voor eenheid-assets. Het is bedoeld als **testmateriaal** voor WP1.10
(glTF laden) en WP1.14 (demo), niet als definitieve art. De offline viewer, de
generatorscript en de conceptplaat uit het originele pakket zijn niet opgenomen.

De orchestrator heeft de modellen gecontroleerd met een eigen parser, los van het
meegeleverde script:

- 14 bestanden in glTF 2.0 binair, elk met drie roots `<naam>_LOD0/1/2`.
- Per LOD de nodes `hull_LOD*`, `turret_LOD*` en `barrel_LOD*`.
- Materialen `steel`, `dark`, `team` en `lamp`.
- Geen textures en geen extensies.
- 48 tot 156 driehoeken per LOD.

Bekende punten:

1. **Elk onderdeel is per materiaal gesplitst** in kindnodes (`hull_steel_LOD0`,
   `hull_team_LOD0`, ...). Dat zijn ongeveer 7 meshes per eenheid per LOD, en met
   instancing per mesh dus veel draw calls. Oplossing: WP1.10 voegt bij het laden
   per onderdeel samen (één vertexbuffer, materiaal per vertex of per bereik), of
   het pakket wordt opnieuw geëxporteerd met één mesh per onderdeel en meerdere
   primitives.
2. **Onderdeelnamen hebben een LOD-achtervoegsel** (`hull_LOD0` in plaats van
   `hull`). De loader moet het achtervoegsel strippen.
3. **Iconen zijn op 14 px niet allemaal te onderscheiden.**
   - Zware tank en raketwagen zijn identiek: allebei een driehoek met `+`.
   - Verkenner, lichte tank en gevechtstank zijn op 14 px stipjes in een driehoek.
   - De rand van 1,5 px die de engine tekent, maakt de binnentekening nog kleiner.
   - Eigen iconen kunnen pas in het spel vanaf WP3.6.
4. **Assen** (+X voor, Y omhoog, meters, grond op Y=0) zijn een voorstel uit de
   specificatie. WP1.10 legt ze definitief vast.
