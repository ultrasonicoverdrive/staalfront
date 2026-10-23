# Staalfront set v4: orchestrator note

Added to the repo on 21-10-2026. The delivery README follows below this note
**unchanged** (in Dutch). The producer calls the delivery "GLB pakket v3"; in the
repo it is **set v4**, so the name does not clash with `assets/set_v3`.

- **Origin.** `Staalfront_GLB_Pakket_v3.zip`, sha256
  `cfe8f6890a30d610136df87e477f7c0f2f397249ae05beb36f3bf92e41780003`: 200 `.glb`
  files (all roles of both factions, building and mission-object states, rubble,
  wrecks, ore fields, trees, rocks, fences and cliff edges), 800 review renders, a
  register, scripts. Procedural geometry (Python and NumPy, no external meshes or
  textures), made by "Copilot" at the project owner's request, as the delivery's
  `LICENSES.md` says. The project owner, Tijmen, licensed them under the **project licence** on
  22-10-2026 (see `assets/LICENSES.md`).
- **What is in the repo.** The **194** `.glb` files in this folder, **byte for byte as
  delivered** (each file's sha256 equals the one in the delivery's `asset_register.csv`).
  Not copied: the 800 review renders (the contact sheet `controleblad.png` is in
  `docs/screenshots/set_v4/`), `review.html`, `asset_register.csv`,
  `validation_report.json`, `state_map.json` (the delivery says it is a proposal, "not a
  proven models.ron schema"), and the generator and validator scripts (never run here).
- **Left out (6 of 200)** because they fail `sf_asset_check` and cannot be fixed by tooling;
  the fallback is the set v3 model or the placeholder shape. They are findings for the
  producer in `docs/progress.md`:
  `neutral_wreck_light`, `neutral_wreck_medium`, `neutral_wreck_heavy`, `neutral_wreck_boat`
  (no empty `fx_smoke_*` / `fx_fire_*` nodes, although the delivery README says they have
  them), `veyran_heavy_gun_team` (five parts in one level, the engine takes four, and no
  figure nodes) and `veyran_demolition_team` (five parts as well, and the nodes of the second
  figure are missing; `unit_defs.ron` keeps the set v3 file for this role).
- **Layout of the levels.** Each file has three glTF scenes, `LOD0`, `LOD1` and `LOD2`, one root
  each, instead of three roots in one scene. The loader and the checker read that as the same
  model (`docs/conventions.md` section 12), so the files were not repacked.
- **Checking.** `cargo run -p sf_asset_check -- assets/set_v4`: 194 files, 3 pass, 191 with warnings
  only, 0 fail (exit code 0). The warnings are findings for the producer too (an emissive
  `lamp` material in 190 files, which the colour-only engine ignores, vessels that do not
  float on `Y = 0`, four fallen trees whose origin is not under the middle, eleven vehicles
  without empty turret and barrel nodes, a supply truck for two factions that
  `unit_defs.ron` gives to Neutral only).
- **Budgets.** The models are very low-poly (8 to 288 triangles in LOD0); most have the same
  triangle count in all three levels. The team colour is a diagonal triangle on the top
  surfaces, which meets the 20 to 40 % rule but reads as a placeholder.
- **Viewing.** `models.ron` has the pack `set_v4` (all 194 files: infantry, vehicles, vessels,
  then the structures, damage states, objects and decor): `sf_game --crowd --models set_v4`.
  The roles of `unit_defs.ron` are wired to it, so the sandbox (`sf_game`) shows it by default;
  pictures are in `docs/screenshots/set_v4/`.

---

# Staalfront GLB pakket v3 · technisch gecontroleerde blockouts

## Resultaat
200 echte GLB-bestanden met elk drie LOD-scenes. 200/200 slagen voor de meegeleverde eigen controles. Bij de 405 LOD-metingen waarvoor teamkleur geldt ligt de gemeten dekking tussen 27,479% en 30,889%. Neutrale assets en gedeelde wrakken/puin hebben geen team-materiaal.

## Hersteld en toegevoegd
- Alle 35 eerder gemelde teamkleurafwijkingen opgelost; de kleurverdeling is opnieuw berekend uit het zichtbare bovenaanzicht.
- Omgekeerde vlakken gecorrigeerd; meting houdt rekening met backface culling.
- Gelijke totale bounding boxes per LOD, dezelfde relatieve nodepaden, geen nul-oppervlakdriehoeken en geen geometrie onder de grond.
- De tien schadestanden met footprintoverschrijding zijn hersteld.
- Voor 20 factiegebouwen: fundering, steigers, af/intact, beschadigd en zwaar beschadigd. Af en intact delen het basisbestand.
- Zes gedeelde puinmodellen per unieke gebouwfootprint.
- Twee eenvoudige bemanningsfiguren bij heavy_gun_team, ook in LOD2.
- Zestien aanvullende missieobjecttoestanden, zestien ertsvarianten/stadia en zestien basisdecorassets.
- Vier controlebeelden per GLB: 45 en 80 graden, rood en blauw. Dit zijn software-renders van de daadwerkelijke LOD0-geometrie, geen gegenereerde conceptbeelden en geen screenshots uit de app.

## Bestanden bekijken
Open review.html na uitpakken. Zoek op assetnaam en bekijk de vier controlebeelden. Controleblad.png bevat een representatieve selectie. De afzonderlijke GLB-bestanden staan in models/.

## Importcontract
- glTF 2.0 binair, meters, Y omhoog, voorzijde +X; geen textures of externe buffers.
- Elke GLB heeft scenes LOD0, LOD1 en LOD2. De standaardscene is LOD0. De loader moet andere scenes bewaren en expliciet de juiste LOD kiezen; er is geen automatische LOD-extensie toegevoegd.
- Node-namen zijn lokaal binnen iedere LOD-root. Zoek niet globaal naar de eerste turret/barrel-node.
- Materiaal team vervangen door spelerskleur. Lampmateriaal kan vanuit de engine worden uitgeschakeld.
- Geen skinning en geen ingebakken animatieclips. Starre onderdelen vanuit de app aansturen.
- state_map.json koppelt basismodellen aan de losse toestandbestanden. Dit is een voorgestelde koppeling, geen bewezen models.ron-schema. Neem het niet blind over als engineconfig.
- asset_register.csv en validation_report.json bevatten aantallen, metingen, hashes en controle-uitkomsten.

## Reproduceerbaarheid
Python 3 en NumPy zijn nodig voor generatie en validatie. Pillow is alleen nodig voor controlebeelden. generate_staalfront_models_v3.py gebruikt de meegeleverde generate_staalfront_models_v2.py als basiswriter. Voer de scripts uit vanuit een uitgepakte kopie. De generator schrijft staalfront_build_v3/. De puinkoppelingen in state_map zijn administratieve aanvullingen in deze levering.

## Wat deze controle WEL bewijst
De opgeslagen GLBs zijn opnieuw ingelezen door een aparte controlescript. Gecontroleerd: headers, buffers, indices, eindige coördinaten en normals, aantallen driehoeken, maximaal zes materialen, geometrie boven Y=0, LOD-nodepaden, gelijke LOD-bounding boxes, footprints van de factiegebouwen en teamkleurdekking in orthografisch bovenaanzicht op 256x256 samples. Ook zijn alle 800 controlebeelden op aanwezigheid en niet-lege geometrie gecontroleerd.

## Wat NIET als gereed of bewezen geldt
Dit blijven eenvoudige procedurele blockouts, geen volledig artistiek afgewerkte assetset. De schadestanden gebruiken vervormde basisgeometrie; funderingen en steigers zijn gedeelde generieke bouwvormen. Ze zijn geen fijn uitgewerkte breuk- of bouwanimaties. De ertsvarianten verschillen door rotatie en hoogte; verbruik verandert hier niet de gehele gameplay-footprint.

De volledige gebiedsspecifieke decorsets, strategische iconen en bouwmenuportretten zijn niet geleverd. Er is geen .blend-bronbestand. Draaipunten, muzzle-ankers, lampgedrag, botsingen, docking, waterlijn, daadwerkelijke maatvoering per rol en prestaties zijn niet in de echte engine getest. De eigen checker controleert niet alle semantische eisen uit de artbrief en vervangt sf_asset_check niet. Het netwerk heeft geen embleem; geen nationale symbolen, medische kruisen of leesbare modeltekst toegevoegd.

Voor definitieve vrijgave ontbreken de echte app/loader, models.ron, sf_asset_check, visuele goedkeuring en een vastgelegde projectlicentie. Daarom staat full_art_brief_approved op false. Er wordt niet beweerd dat de hele artbrief of appintegratie gereed is.

## Herkomst
Alle meshes zijn procedureel opgebouwd met de meegeleverde scripts, zonder externe meshes of textures. LICENSES.md registreert de herkomst per GLB. Een definitieve licentie is niet namens de projecteigenaar vastgesteld; dit blijft een vrijgavepunt.
