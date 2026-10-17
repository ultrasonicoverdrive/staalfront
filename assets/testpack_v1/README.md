# Staalfront assetpakket 1.0

Deelbare prototype-set voor een eigen 2.5D strategiespel. Geïnspireerd door klassieke RTS-spellen, waaronder Red Alert 1, maar met eigen namen, geometrie en pictogrammen. Geen bestanden, logo's, textures of geluiden uit dat spel gebruikt.

## Begin hier
1. Pak de ZIP volledig uit.
2. Open START_HIER.html in een browser. De viewer werkt offline en bevat alle 14 modellen en iconen.
3. Kies een eenheid, LOD en spelerskleur. Sleep om het model te draaien.
4. Bekijk preview/iconen_14px_1x.png op 100% voor de echte iconengrootte. De 4x-versie is een pixelvergroting.

## Inhoud
- 14 GLB-bestanden, elk met drie LOD-roots.
- 14 SVG-iconen, plus 28 transparante PNG's op 64 en 128 px.
- manifest.json met namen, rollen, bestandspaden en driehoekaantallen.
- Offline modelviewer, conceptreferentie, iconencontroleplaat en technisch controleverslag.
- Reproduceerbare model- en icoongenerator in docs. Vereist Python, numpy, Pillow en cairosvg. De generator bouwt alleen modellen, iconen, manifest en controleplaten; niet de viewer of dit complete distributiepakket. Bij uitvoeren vanuit docs ontstaat de uitvoer onder docs/staalfront_assets.

## Status: blockout, geen productie-assets
De GLB's zijn eenvoudige uit blokken opgebouwde modellen voor schaal-, materiaal-, LOD- en importtests. Ze zijn bewust veel eenvoudiger dan de AI-conceptplaat. Vooral de drie tankklassen verschillen nu voornamelijk in proporties. Wielen/rupsbanden zijn blokvormig; geen wielanimatie, effecten, botsingsvormen of uitgewerkte gameplay.

## Modelafspraken
- Meters, Y omhoog, voorkant +X, grond op Y=0.
- Per LOD: <eenheid>_LOD0/1/2 met hull_LOD*, turret_LOD* en barrel_LOD*. Hull bevat turret, turret bevat barrel. Niet-relevante onderdelen hebben een lege transform-node.
- Het materiaal team markeert inkleurbare vlakken. De blauwe basiskleur is een voorbeeld.
- Turret draait om lokale Y; barrel kan om lokale Z kantelen. Geen animatieclips of skinning.
- Alle drie de roots staan in de standaard glTF-scene. De engine moet slechts één root activeren. Dit pakket gebruikt geen automatische glTF-LOD-extensie.
- Beoogde selectie: LOD0 vanaf 60 schermpixels, LOD1 vanaf 30, LOD2 daaronder. Rond 28 px neemt het strategische icoon over. Dither-overgang moet de engine verzorgen.
- Detailniveaus delen de rompmaat en componenthiërarchie. Verwijderde details kunnen de totale bounding box veranderen. Sommige eenvoudige modellen hebben identieke LOD-geometrie.
- rompmaat_m in manifest.json is een ontwerpmaat in volgorde X/Y/Z, niet de gemeten totale bounding box. Lopen en antennes kunnen uitsteken. Gemeten grenzen staan in docs/validatie.json.

## Iconen
Witte basis met zwarte binnentekening; transparante buitenruimte. Alleen witte pixels moeten worden ingekleurd, niet het hele icoon vermenigvuldigen met de spelerskleur. Geen eigen buitenrand. De engine voegt de rand toe. De controleplaat toont de broniconen zonder engine-rand, in vier spelerskleuren op groen en zand.
De 14px-versies zijn bedoeld voor visuele beoordeling; herkenbaarheid is niet met gebruikers getest. Niet alle kleine uitsparingen halen de beoogde minimumdetailmaat. Beoordeel vooral lichte tank/gevechtstank en zware tank/raketwagen. Grijs en bruin terrein moeten nog in de engine worden beoordeeld.

## Wat gecontroleerd is
GLB-header en chunks, bufferbereiken, indexbereiken, eindige vertices, normale-lengtes en winding, aanwezigheid van componentnodes, grondcontact, driehoeken per LOD, PNG-maten en alpha. Zie docs/validatie.json.
Geen officiële Khronos-validator, Blender-importtest, browsertest of test in jouw engine uitgevoerd. De viewer is meegeleverd maar niet interactief getest. Geen FPS-benchmark: 5.000 eenheden / 60 fps is niet aangetoond. De hoeveelheid zichtbaar team-oppervlak is niet kwantitatief gecontroleerd.

## Overdracht aan ontwikkelaar
Begin met lichte_tank.glb en icon_lichte_tank.svg. Controleer asrichting, schaal, node-rotaties en vervanging van materiaal team. Verberg ongebruikte LOD-roots, controleer daarna alle modellen en de icoonovergang. Dit zijn integratievoorstellen, geen bevestiging dat de engine dit al ondersteunt.
