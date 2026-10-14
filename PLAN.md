# Staalfront — bouwplan

*Werktitel. Een real-time strategiespel in Rust, 2.5D, met traploos zoomen van
close-up tot de hele kaart en tussenfilmpjes als tekenfilm. Geïnspireerd op de
spelregels van Command & Conquer: Red Alert, zonder diens naam, wereld, eenheden
of media.*

Opgesteld 14-10-2026. Bron: `docs/analyse-red-alert.md` (functionele analyse van
de EA-broncode). Uitvoering: Sonnet-agents op effort xhigh (zie §12).

---

## 0. Beslissingen vooraf (leg vast vóór fase 0)

| # | Vraag | Voorstel | Waarom |
|---|---|---|---|
| B1 | Engine | **Eigen dunne engine op wgpu + winit**, geen kant-en-klare engine (zie §1.0) | Een RTS heeft één camera, één soort wereld en duizenden gelijke objecten: precies wat je met instancing zelf strak regelt. Volledige controle over determinisme, tikvolgorde en geheugen. |
| B2 | Wat is "2.5D" hier | **Echte 3D-wereld, vaste camerahoek, low-poly modellen** | Sprites (zoals het origineel) worden lelijk bij traploos zoomen. 3D-modellen blijven scherp van dichtbij én versimpelen netjes van ver. Gameplay blijft plat (2D-raster). |
| B3 | Simulatie | **Deterministisch, vaste tik (20 Hz), fixed-point rekenen, los van rendering** | Maakt replays, opslaan, multiplayer (lockstep) en geautomatiseerde tests mogelijk. Achteraf toevoegen is vrijwel onmogelijk. |
| B4 | Tussenfilmpjes | **In-engine tekenfilmspeler (cutout/vectoranimatie, gescript) + video als terugval** | Klein, scherp op elke resolutie, lip-sync en vertaling achteraf aanpasbaar. Zie §9. |
| B5 | Licentie broncode-referentie | **Clean-room**: alleen de functionele analyse gebruiken, nooit EA-code lezen-en-overtypen | EA-code is GPL; letterlijk overnemen maakt Staalfront GPL. Eigen naam, eenheden, graphics, muziek verplicht (handelsmerk). |
| B6 | Waar wordt gebouwd | **GitHub + cloud-agents + laptop**, nooit op de thuisserver (zie §13) | De thuisserver draait live diensten en heeft geen ruimte (RAM, GPU) voor een Rust-gamebuild. |
| B7 | Doelplatform v1 | Windows + Linux desktop. Web en macOS later. | Muis + toetsenbord; RTS op telefoon is een ander spel. |
| B8 | Taal van het spel | **Engels** (menu's, UI-teksten, stemmen, ondertitels); alle spelteksten in taalbestanden, niet in code | Besluit Tijmen 14-10-2026. Taalbestanden houden vertalen later mogelijk. |

---

## 1. Architectuur

### 1.0 Technologiekeuze

Geen Bevy of Fyrox: die zijn algemene engines. Bevy breekt zijn API elk kwartaal,
plant systemen zelf in (lastig voor bit-exact determinisme) en compileert
traag; Fyrox brengt een complete editor en scènegraaf mee die een RTS niet
nodig heeft. Een RTS is juist smal: één camera, één terrein, veel identieke
objecten. Daarom losse, stabiele bouwstenen:

| Onderdeel | Keuze | Waarom |
|---|---|---|
| Venster/invoer | `winit` | De standaard, stabiel. |
| GPU | `wgpu` | Vulkan/DX12/Metal/WebGPU uit één codebase; instancing voor duizenden eenheden. |
| Rekenwerk render | `glam` | Snel, alleen in de weergave (sim gebruikt fixed-point). |
| Simulatie-opslag | **eigen struct-of-arrays met generatie-ID's** (geen ECS-framework) | Vaste, voorspelbare volgorde = determinisme; cache-vriendelijk; geen magie. |
| Modellen | `gltf` | Open formaat, Blender-export. |
| UI in het spel | eigen, getekend met wgpu (zijbalk, knoppen, minikaart) | Klein en strak in stijl. |
| Editor/debug-UI | `egui` (via `egui-wgpu`) | Snel bouwen, alleen voor tools en debugpanelen. |
| Audio | `kira` | Mengen, positioneel, overgangen voor dynamische muziek. |
| Data | `serde` + `ron` | Leesbare speldata. |
| Tekenfilm-video-terugval | `ffmpeg` als los proces dat frames levert | Geen zware bindingen in de build. |
| Tests | `proptest`, `insta` (snapshots) | Eigenschappen en regressies. |

### 1.1 Cargo-workspace

```
staalfront/
├─ crates/
│  ├─ sf_math      fixed-point getallen, vectoren, hoeken, deterministische RNG
│  ├─ sf_sim       de complete spelsimulatie — geen GPU, geen I/O, geen klok
│  ├─ sf_data      unit/gebouw/wapen-definities (RON), validatie, laden
│  ├─ sf_ai        computertegenstander, praat alleen via opdrachten met sf_sim
│  ├─ sf_engine    hoofdlus, venster, invoer, tijd, assets (geen spelregels)
│  ├─ sf_render    wgpu: terrein, modellen, camera, zoom, effecten, LOD
│  ├─ sf_ui        eigen UI: widgets, tekst, minikaart; later zijbalk, selectie, menu's
│  ├─ sf_audio     geluid, stemmen, muziek
│  ├─ sf_cutscene  tekenfilmspeler + scriptformaat
│  ├─ sf_net       lockstep-netwerk (later)
│  └─ sf_game      binaire app: plakt alles aan elkaar
├─ tools/
│  ├─ sf_editor    kaart- en missie-editor
│  ├─ sf_headless  simulatie zonder scherm: tests, balans, AI-tegen-AI
│  └─ sf_demo      engine-demo en benchmark (fase 1)
├─ assets/  data/  docs/
```

**Harde regel:** `sf_sim` hangt niet af van `sf_render`. De simulatie moet in
`sf_headless` duizenden tikken per seconde kunnen draaien zonder GPU. Dat is
wat agents in staat stelt gameplay te testen zonder scherm.

### 1.2 Simulatie en weergave gescheiden

- Simulatie draait op 20 tikken/s met vaste stap. Invoer wordt een **opdracht**
  (`Command`) die op tik N wordt uitgevoerd — nooit direct een wereldmutatie
  vanuit de UI.
- Rendering draait op schermfrequentie en **interpoleert** tussen de vorige en
  huidige tik.
- Elke tik levert een **checksum** van de wereldtoestand (voor desync-detectie
  en regressietests).
- Spelobjecten zijn entiteiten in de struct-of-arrays-opslag (§1.0) met kolommen
  (positie, eigenaar, gezondheid, missie, beweging, wapen…). Gedrag per type via data, niet via subklassen —
  dat is de moderne vertaling van de objecthiërarchie uit de analyse (§3).

### 1.3 Coördinaten

- Gameplay op een **celraster** (bv. 1 cel = 1 m, kaarten 64×64 t/m 256×256),
  posities binnen een cel in fixed-point ("leptons", zoals het origineel).
- Hoogte is **alleen visueel** plus een paar gameplay-effecten (zicht van een
  heuvel, langzamer bergop). Geen echte 3D-ballistiek in v1.

---

## 2. Camera en traploos zoomen (kerneis)

Doel: met het scrollwiel vloeiend van **close-up op één tank** naar **de hele
kaart in beeld**, zonder schokken en zonder dat de besturing breekt.

### 2.1 Camera
- Perspectiefcamera met vaste kijkrichting (noord boven). Draaien met Q/E
  optioneel en standaard uit.
- Zoom = cameraafstand op een logaritmische schaal (bv. 12 m → genoeg om de
  hele kaart te tonen). Scrollen zoomt **naar de cursor toe** (de wereldpositie
  onder de muis blijft onder de muis).
- **Hoek loopt mee met zoom:** van dichtbij ~45° (filmisch, je ziet zijkanten
  van gebouwen), van ver naar ~80° (bijna bovenaanzicht, overzichtelijk).
- Gedempte beweging (smooth damping), pannen met randscroll, WASD/pijltjes en
  middelste muisknop slepen. Pansnelheid schaalt mee met zoom.
- Dubbelklik op minikaart of spatie = naar laatste gebeurtenis.

### 2.2 Wat er per zoomniveau verandert
| Zoom | Eenheden | Terrein | UI |
|---|---|---|---|
| Close-up | volle modellen, animaties, deeltjes, schaduwen | volle detail, decoratie | gezondheidsbalken bij selectie |
| Midden (standaard speelhoogte) | volle modellen, minder deeltjes | middel-LOD | balken bij schade |
| Ver | vereenvoudigde modellen (LOD) | grove chunks | groepsbalken |
| Strategisch | **iconen in spelerskleur** (driehoek = voertuig, vierkant = gebouw, enz.), formaat constant op scherm | kaartachtige weergave | gebied-overlays, routepijlen |

Overgang tussen model en icoon met een korte crossfade, niet hard wisselen.

### 2.3 Besturing werkt op elke zoom
- Selecteren via straal → terrein + per entiteit een klikvolume; bij
  strategische zoom tellen de **icoon-vlakken** op het scherm.
- Sleepselectie werkt in schermruimte en dus op elke zoom.
- Opdrachten (bewegen, aanvallen) werken ongewijzigd ook volledig uitgezoomd.

### 2.4 Prestaties
- Terrein in chunks met LOD en frustum culling.
- Eenheden via GPU-instancing per modeltype.
- Doel: **60 fps met 1.000 eenheden** in beeld, uitgezoomd, op middenklasse
  hardware; eerste meting bij de engine-demo (fase 1, 5.000 eenheden), eindmeting in fase 16.

---

## 3. Fases

Elke fase eindigt met iets dat je **kunt spelen of zien**. Werkpakketten (WP)
zijn zo gesneden dat één agent er in één sessie mee klaar is. Acceptatie is
altijd toetsbaar: een test, een headless scenario of een screenshot.

Eerst de engine, dan het spel: fase 0 en 1 bevatten **geen gameplay**. Pas als
de engine-demo (mijlpaal M0) aantoonbaar draait, begint fase 2.

### Fase 0 — Fundering
- WP0.1 Workspace met alle crates (leeg), `cargo fmt`/`clippy`/`test` groen, GitHub Actions (Linux + Windows), lint die `f32`/`f64` en `HashMap`/`HashSet` in `sf_sim` weigert.
- WP0.2 `sf_math`: fixed-point type, vectoren, hoeken (256 richtingen), deterministische RNG; eigenschapstests (proptest) en een test die bewijst dat dezelfde reeks op Linux en Windows bit-identiek is.
- WP0.3 `docs/`: architectuur, conventies voor agents (zie §12), de analyse als bron.

**Klaar als:** CI groen op twee OS'en; `sf_math` bit-identiek op Linux en Windows.

### Fase 1 — Engine
Een dunne, eigen engine (§1.0) met alles wat het spel later nodig heeft, getest
met placeholders. Engine-crates kennen geen spelregels: geen "tank", geen
"erts", alleen meshes, instanties, entiteiten, opdrachten en checksums.

*Kern*
- WP1.1 `sf_engine` — hoofdlus en invoer: winit-venster (venster/volledig scherm, DPI-schaal), lus met vaste sim-tik (20 Hz) en rendering op schermfrequentie met interpolatiefactor, invoerlaag (toetsen/muis/wiel → acties, toewijzing in RON), frametijdmeting, asset-paden en hot-reload van RON in debug.
- WP1.2 `sf_sim`-kern: wereld als struct-of-arrays met generatie-ID's, vaste tik, `Command`-wachtrij op tik N, checksum per tik. Eén placeholder-gedrag "zwerven" (fixed-point, deterministische RNG) zodat duizenden entiteiten bewegen zonder gameplay.
- WP1.3 `sf_headless`: draait N tikken van een scenario-bestand, schrijft checksums en een JSON-log; CI vergelijkt de checksums van Linux en Windows (10.000 tikken, 5.000 zwervers).

*Weergave*
- WP1.4 `sf_render`-basis: wgpu-apparaat en surface, dieptebuffer, één richtingslicht + omgevingslicht, WGSL-shaders met hot-reload in debug, offscreen renderen naar PNG voor screenshot-tests (Linux: Mesa lavapipe; Windows: DX12 WARP).
- WP1.5 Camera volgens §2.1: logaritmische zoom van 12 m tot de hele kaart, zoomen naar de cursor, hoek loopt mee (~45° → ~80°), damping, pannen (randscroll, WASD/pijltjes, middelste muisknop), pansnelheid schaalt mee. Eenheidstest: het wereldpunt onder de cursor blijft binnen 1 cm bij elke zoomstap.
- WP1.6 Instancing: per mesh één instantiebuffer, posities per frame geïnterpoleerd uit twee sim-tikken, placeholder-primitieven (kubus, cilinder, kegel) in spelerskleur, CPU-frustum culling per kaartvak. Stresstest: 10.000 instanties.
- WP1.7 Terrein: engine-kaartformaat (RON + hoogtekaart, materiaalindex per cel), mesh in chunks van 32×32 cellen, procedurele materialen via een textuurarray.
- WP1.8 Terrein-LOD en culling: LOD-niveaus per chunk op basis van schermfout, naden dicht (rokjes of stitching), frustum culling; uitgezoomd op 256×256 ≤ 300 draw calls voor terrein.
- WP1.9 Model-LOD en strategische iconen: LOD-keten per model, en bij strategische zoom schermvaste iconen (vorm + spelerskleur) met crossfade (§2.2); klikvolume per zoom (straal op model, icoonvlak op scherm).
- WP1.10 glTF-modellen: laden van mesh, basiskleur-materiaal en node-hiërarchie (koepel los van romp), starre node-animatie; aansluiten op instancing en model-LOD. Skinned animatie bewust nog niet.

*Interface en geluid*
- WP1.11 Debugpanelen met `egui` (via `egui-wgpu`), aan/uit met F1: frametijdgrafiek, draw calls en instanties, camera-instellingen live bij te stellen, sim-tik en checksum, LOD-kleuring, celrooster-overlay.
- WP1.12 `sf_ui` — eigen UI op wgpu: rechthoeken, nine-slice, tekst (glyph-atlas), knoppen, panelen met ankers, UI-schaal, invoer die de UI eerst krijgt; minikaart (render naar textuur, klik = camera verplaatsen, kader van het zichtveld).
- WP1.13 `sf_audio` met `kira`: geluidsbank in RON, positioneel geluid ten opzichte van de camera (volume en panning, zachter bij uitzoomen), muziek met crossfade; zonder geluidsapparaat (CI) stil doorlopen in plaats van crashen. Eén CC0-testgeluid met licentieregel.

*Demo*
- WP1.14 `sf_demo` — de engine-demo: gegenereerde kaart van 256×256 met reliëf, 5.000 zwervende placeholder-eenheden (3 meshsoorten, 4 spelerskleuren, een paar glTF-testmodellen), minikaart, egui-panelen, geluid bij klikken. Benchmarkmodus `--bench`: vaste cameravlucht van close-up naar volledig overzicht en terug, schrijft frametijden (p50/p95/p99) en de sim-checksum naar JSON. In CI: draait op lavapipe/WARP als rooktest met screenshots op drie zoomniveaus (geen fps-eis, die hardware is software).

**Volgorde en parallellisme:** WP1.1 en WP1.2 eerst (parallel). Daarna drie
sporen naast elkaar: *sim* (1.3), *weergave* (1.4 → 1.5 → 1.6 → 1.7 → 1.8 →
1.9 → 1.10) en *interface* (1.11, 1.12, 1.13, zodra 1.4 er is). WP1.14 als
laatste.

**Klaar als (mijlpaal M0 — engine-demo):**
- `sf_demo --bench` haalt op de referentielaptop **p99-frametijd ≤ 16,7 ms (≥ 60 fps)** over de hele cameravlucht met 5.000 eenheden in beeld, van close-up tot de hele kaart;
- vloeiend zoomen naar de cursor zonder schokken, beoordeeld door Tijmen op de laptop;
- headless: 10.000 tikken met 5.000 zwervers, checksum gelijk op Linux en Windows;
- CI groen op twee OS'en, inclusief screenshot-tests;
- egui-panelen tonen frametijd, draw calls en checksum live.

### Fase 2 — Speelkaart
Het eerste gameplaywerk: de kaart krijgt betekenis voor het spel.
- WP2.1 Kaartformaat (RON) uitgebreid met terreintype per cel (gras, weg, water, rots, erts, brug) in `sf_data`; `sf_sim` kent per cel terreintype en bezetting.
- WP2.2 Materialen per terreintype in de weergave (op het engine-terrein van WP1.7).
- WP2.3 Minikaart toont terreintypen (op de minikaart van WP1.12).
- WP2.4 Celrooster-overlay kleurt naar terreintype en bezetting.

**Klaar als:** een 256×256-speelkaart laadt in sim en weergave; headless test controleert terreintypen en bezetting per cel.

### Fase 3 — Eenheden, selectie en opdrachten
- WP3.1 `sf_data`: definities van eenheden (snelheid, draaisnelheid, bewegingsklasse, gezondheid, pantser, wapens, kosten, model).
- WP3.2 Eenheden spawnen in de sim uit `sf_data`-definities; weergave via de engine (instancing, interpolatie, LOD uit fase 1).
- WP3.3 Selectie: klik, sleep, shift-toevoegen, dubbelklik = alle van dit type in beeld, groepen Ctrl+1–9.
- WP3.4 Opdrachten als toestand (analyse §5): stilstaan, bewegen, aanvallen, aanval-bewegen, bewaken, stoppen, verspreiden. Wachtrij met shift.
- WP3.5 Cursorcontext: de cursor laat zien welke opdracht een klik geeft.
- WP3.6 Iconen per eenheidstype en factie op het iconensysteem van WP1.9.

**Klaar als:** je eenheden selecteert en verplaatst op elke zoom; opdrachten zijn sim-commands en dus reproduceerbaar in headless.

### Fase 4 — Beweging en routes
- WP4.1 Bewegingsklassen (rups, wiel, voet, zweef, water, lucht) met terreinkosten.
- WP4.2 Hiërarchische A* (sectoren/portalen) voor enkele eenheden.
- WP4.3 Flowfields voor groepen naar één doel.
- WP4.4 Lokaal uitwijken en botsen (eenheden duwen, wachten, omrijden); vastzittende eenheid naar vrije buurcel (analyse §6).
- WP4.5 Draaien vóór rijden voor voertuigen, koepels draaien los van romp.
- WP4.6 Formaties (groep houdt vorm, langzaamste bepaalt tempo).
- WP4.7 Herplannen bij geblokkeerde route of verdwenen doel; bestemming ≠ route.

**Klaar als:** 200 eenheden steken een brug over zonder vast te lopen in een headless scenario (≤ 1% eindigt buiten 5 cellen van het doel).

### Fase 5 — Gevecht
- WP5.1 Wapens en kogeltypen als data: bereik, herlaadtijd, schade, schadetype, projectieltype.
- WP5.2 Projectielen als eigen entiteiten: recht, boog, geleid, val (analyse §8), met lont/detonatie.
- WP5.3 Pantsertabel: schade × pantsertype → percentage; spatschade met afname.
- WP5.4 Doelkeuze en automatisch terugvuren, prioriteiten, bereikcontrole.
- WP5.5 Vernietiging: cellen vrijgeven, wrakken/puin, effecten.
- WP5.6 Visuele effecten: mondingsvuur, inslag, explosies, rook (deeltjes), schaal mee met zoom.

**Klaar als:** headless "10 tanks tegen 20 infanteristen" geeft elke run dezelfde uitslag en checksum; het ziet er in het spel ook zo uit.

### Fase 6 — Gebouwen, bouwen en stroom
- WP6.1 Gebouwdefinities: voetafdruk, bouwtijd, stroom, vereisten (techboom), functie.
- WP6.2 Plaatsen: geldigheid (terrein, nabijheid eigen basis), spookvoorbeeld onder cursor.
- WP6.3 Bouwanimatie gekoppeld aan toestand — animatie vóór missielogica (analyse §10).
- WP6.4 Stroom: opwekking/verbruik, tekort vertraagt productie en schakelt verdediging uit; schade verlaagt opwekking.
- WP6.5 Productie: wachtrijen per fabriekstype, voortgang, uitgang + vrije plek, verzamelpunt.
- WP6.6 Reparatie en verkoop als processen met voortgang.
- WP6.7 Verdedigingsgebouwen met koepels.
- WP6.8 Schadeweergave: beschadigd en zwaar beschadigd model, rook, vuur.

### Fase 7 — Economie
- WP7.1 Erts op de kaart: velden, hoeveelheid, langzame aangroei.
- WP7.2 Oogstmachine: zoeken, oogsten, terugrijden, aanmelden bij raffinaderij via **berichten** (analyse §11, §13), lossen, opnieuw.
- WP7.3 Opslagcapaciteit en silo's, vulniveau zichtbaar.
- WP7.4 Geld in de UI, kosten afschrijven tijdens productie.

**Mijlpaal M1 — verticale snede (einde fase 7):** één skirmish-kaart, jij tegen
een statische tegenstander: basis bouwen, oogsten, eenheden maken, aanvallen,
winnen. Eerste moment om écht te spelen en het ontwerp bij te sturen.

### Fase 8 — Zicht
- WP8.1 Shroud (nooit gezien) en fog of war (gezien, niet actueel) per speler, celgebaseerd in de sim.
- WP8.2 Zichtbereik per eenheid, hoogtebonus.
- WP8.3 Weergave: zachte randen, laatst geziene gebouwen als "spook".
- WP8.4 Minikaart en strategische iconen respecteren zicht.

### Fase 9 — Computertegenstander
Gelaagd (analyse §12, §22G):
- WP9.1 Strategisch: bouwvolgorde, economie, techkeuze, aanvalsmoment.
- WP9.2 Operationeel: legers samenstellen, doel kiezen, verzamelen.
- WP9.3 Tactisch: doelprioriteit, terugtrekken, focussen.
- WP9.4 Moeilijkheidsgraden zonder valsspelen (reactietijd, efficiëntie), plus één "oneerlijke" graad.
- WP9.5 AI-tegen-AI in headless als balansgereedschap: 100 potjes, winstpercentage en duur per facties.

**Klaar als:** de normale AI wint van een passieve speler en verliest van een goede; headless toernooi draait zonder vastlopers.

### Fase 10 — Missies en editor
- WP10.1 Scenarioformaat: startobjecten, spelers, doelen, triggers (gebeurtenis → voorwaarde → actie), teams.
- WP10.2 Triggersysteem in de sim (deterministisch): tijd, gebied betreden, object vernietigd, geld, enz.
- WP10.3 Acties: berichten, versterkingen, filmpje starten, camera, overwinning/nederlaag.
- WP10.4 `sf_editor`: terrein schilderen, objecten plaatsen, triggers bewerken, testen vanuit de editor.
- WP10.5 Campagnestructuur: missiereeks, briefing (tekenfilm), keuzes.

### Fase 11 — Presentatie en UI
- WP11.1 Zijbalk à la klassieke RTS: bouwtabs, wachtrij, geld, stroombalk.
- WP11.2 Stemreacties bij selecteren, verplaatsen, aanvallen (analyse §15), met rem tegen spam.
- WP11.3 Geluid: positioneel, volume naar zoom (verder weg = zachter en minder).
- WP11.4 Muziek: dynamisch (rust/gevecht).
- WP11.5 Menu's, instellingen (toetsen, geluid, grafisch), toegankelijkheid (kleurenblind-kleuren, UI-schaal).
- WP11.6 Echte modellen en textures vervangen de placeholders (zie §10 assets).

### Fase 12 — Tekenfilm-tussenfilmpjes
Zie §9 voor het ontwerp.
- WP12.1 Scriptformaat (RON) voor scènes: lagen, personages, poses, beweging, camera, dialoog, geluid, tijdlijn.
- WP12.2 Speler in `sf_cutscene`: cutout-animatie (gelede 2D-figuren), easing, overgangen, ondertitels, overslaan.
- WP12.3 Lip-sync: mondvormen uit spraakbestanden (Rhubarb Lip Sync, offline), opgeslagen als tijdreeks.
- WP12.4 Videoterugval: WebM/AV1-afspelen voor scènes die als video worden aangeleverd.
- WP12.5 Eén volledige demoscène (briefing missie 1) met placeholder-figuren.
- WP12.6 Previewtool: scène live herladen bij opslaan (voor de tekenaar).

### Fase 13 — Opslaan, laden, replays
- WP13.1 Volledige wereldtoestand serialiseren (inclusief lopende opdrachten, productie, projectielen) — gelijke checksum na laden is de test.
- WP13.2 Replays = startstate + opdrachtenlog; afspelen met snelheden.
- WP13.3 Snel opslaan, autosave, versie-migratie van savebestanden.

### Fase 14 — Multiplayer (optioneel, na M2)
- WP14.1 Deterministische lockstep over UDP (of QUIC), opdrachtvertraging, ping-gecompenseerd.
- WP14.2 Desync-detectie via checksums + automatische dump.
- WP14.3 Lobby, directe verbinding, later een kleine relay-server.

### Fase 15 — Content en balans
- Twee facties met elk ~12 eenheden en ~14 gebouwen, eigen karakter (geen kopie van Geallieerden/Sovjets).
- 8–12 skirmishkaarten, campagne van 10 missies per factie.
- Balansrondes met headless AI-toernooien + eigen speelsessies.

**Mijlpaal M2 — speelbare alpha:** skirmish tegen AI op alle kaarten, eerste 3 campagnemissies met tekenfilmbriefings.

### Fase 16 — Afwerking en uitgave
- Prestatiebudget halen (§2.4), crash-rapportage, installer, itch.io/Steam, trailer.

---

## 4. Afhankelijkheden

```
F0 ─ F1 ═ M0 ─ F2 ─ F3 ─ F4 ─ F5 ─┬─ F6 ─ F7 ═ M1 ─┬─ F8 ─ F9 ─ F10 ─ F15 ═ M2 ─ F14 ─ F16
                                  │                └─ F13
F1 ─────── F12 (speler kan parallel vanaf F1; demoscène na F3)
F1 ─────── F11 (UI-onderdelen parallel zodra het betreffende systeem bestaat)
```

Parallel mogelijk (elk in een eigen git-worktree): F12 naast F4–F7; UI-WP's
naast het systeem waar ze bij horen; editor (WP10.4) naast F8–F9.

---

## 5. Kwaliteitsregels (gelden voor elk WP)

1. Geen wereldmutatie buiten de sim-tik. UI geeft opdrachten.
2. Geen `f32`/`f64` in `sf_sim`; alleen `sf_math`-types. CI controleert dit met een lint.
3. Geen `HashMap`-iteratievolgorde in de sim (niet deterministisch) — `BTreeMap` of gesorteerde vectors.
4. Elk gameplay-WP levert een headless scenario dat het gedrag vastlegt (checksum-regressie).
5. Elk visueel WP levert een screenshot-test of een opgenomen korte clip.
6. Speldata in RON, niet in code; nieuwe eenheid toevoegen = geen Rust-wijziging.
7. Documenteer elke niet-voor-de-hand-liggende spelregel in `docs/regels/`.

---

## 6. Gevaren en hoe ze te vangen

| Risico | Gevolg | Tegenmaatregel |
|---|---|---|
| Determinisme lekt (float, hashvolgorde, threads) | Replays/multiplayer kapot, pas laat ontdekt | Vanaf F0 checksum-tests op Linux én Windows in CI |
| Padvinden met veel eenheden | Vastlopers, traag | Flowfields + headless stresstests vanaf F4 |
| Zoom breekt prestaties | Haperen uitgezoomd | LOD/instancing/iconen in de engine (F1), fps-benchmark vanaf M0 bij elke fase |
| Art ontbreekt | Spel blijft er "programmeurs" uitzien | Vroeg beslissen wie modellen en tekenfilms maakt (§10) |
| Agents gaan uiteenlopen | Inconsistente code | Vaste conventies in `CLAUDE.md`, review-gate per fase (§12) |
| Juridisch | Handelsmerk/GPL | Clean-room, eigen naam en content (B5) |
| Scope | Nooit af | M1 is het eerste echte beslismoment; daarna pas uitbreiden |

---

## 9. Tussenfilmpjes als tekenfilm

**Keuze:** gescripte 2D-cutout-animatie in de engine (zoals South Park / Rive /
Spine-stijl), met video als terugval.

**Waarom niet alleen video:** video is groot (honderden MB), vast op één
resolutie, en elke tekstwijziging of vertaling betekent opnieuw renderen.
Gescripte scènes zijn kilobytes plus de tekeningen, haarscherp op 4K, en
dialoog/lipsync kan per taal anders.

**Pijplijn:**
1. Script en storyboard (tekst + schetsen per shot).
2. Tekenaar levert personages als losse onderdelen (hoofd, ogen, monden, armen…) als PNG of SVG, plus achtergronden.
3. Stemopname per regel (WAV).
4. Rhubarb haalt mondvormen uit elke regel.
5. Scène-RON beschrijft de tijdlijn: wie staat waar, welke pose, camerabeweging, welke regel.
6. Speler toont het in de engine; previewtool laat de maker live bijsturen.

**Stijl:** heldere tekenfilm met dikke lijnen en platte kleuren — sluit aan bij
de low-poly 3D-wereld en is haalbaar met cutout-animatie.

**Eerlijk:** agents bouwen de speler, het formaat en de tooling. De tekeningen,
stemmen en regie moeten van een mens of een gelicentieerde generatieve tool
komen. Placeholders (geometrische figuren) houden het werk intussen gaande.

---

## 10. Assets: wie maakt wat

| Asset | v0 (agents) | Echt |
|---|---|---|
| 3D-modellen | primitieven (kubus/cilinder) in factiekleur | Blender, artiest, of CC0-packs (Kenney, Quaternius) als tussenstap |
| Terreintextures | procedureel | CC0 (ambientCG, Poly Haven) |
| Effecten | eigen GPU-deeltjes | idem, getuned |
| Geluid | CC0 (freesound, licentie per bestand vastleggen) | eigen opnames/inkoop |
| Stemmen | tekst-naar-spraak placeholder | stemacteurs |
| Muziek | stilte/CC0 | componist |
| Tekenfilms | geometrische figuren | tekenaar |

Licentie per asset vastleggen in `assets/LICENTIES.md` — vanaf het eerste bestand.

---

## 12. Uitvoering met agents

### 12.1 Agent-type
`.claude/agents/bouwer-sonnet.md` (in de repo, reist mee naar elke omgeving) — model `sonnet`, effort `xhigh`. Voert één
werkpakket uit, werkt in een eigen git-worktree/branch, opent een PR.

### 12.2 Werkwijze per werkpakket
1. Orchestrator (hoofdsessie) start een agent met: WP-nummer, de acceptatiecriteria uit dit plan, relevante `docs/`.
2. Agent werkt op branch `wp/<nummer>-<naam>`, schrijft tests eerst waar mogelijk, draait `cargo fmt`, `clippy -D warnings`, `cargo test`, headless scenario's.
3. Agent opent een PR met: wat gedaan, hoe getest, wat bewust niet gedaan.
4. Review-gate: een `onderzoeker`-agent (Opus) beoordeelt de PR tegen §5 en het WP; daarna merge.
5. Na elke fase: korte speelbare build + verslag in `docs/voortgang.md`.

### 12.3 Parallellisme
Maximaal 3 agents tegelijk, alleen op WP's zonder overlap in crates (zie §4).
Conflicten in `Cargo.lock`/gedeelde types lost de orchestrator op.

### 12.4 Wat een agent niet doet
- Geen wijziging aan sim-determinismeregels zonder akkoord.
- Geen nieuwe dependency zonder motivatie in de PR.
- Geen assets zonder licentie-regel.

---

## 13. Bouwomgeving

Het project staat **alleen op GitHub**; er komt niets op de thuisserver
(live diensten, geen GPU, te weinig RAM).

- **Cloud-agents** (Claude Code remote, gekoppeld aan de GitHub-repo) bouwen,
  testen headless en openen PR's.
- **GitHub Actions** controleert elke PR op Linux en Windows (fmt, clippy,
  tests, determinisme-checksums, screenshot-test via Mesa lavapipe) en levert
  per PR een speelbare build als artefact.
- **Cloud-container van de sessie**: Rust (stable) en Mesa lavapipe (software-Vulkan, geen GPU) zijn aanwezig — genoeg voor bouwen, tests, headless en screenshot-tests, **niet** voor fps-metingen.
- **Laptop (Windows)** om te spelen en te beoordelen, en de referentiemachine voor de fps-eis van M0; eventueel ook als
  bouwmachine voor agents als visueel werk lokaal moet.

## 14. Eerste stappen

1. Cloud-agents aan de GitHub-repo koppelen (§13).
2. Beslissingen B1–B7 bevestigen.
3. Agent-type `bouwer-sonnet` actief, fase 0 starten (WP0.1 t/m WP0.3), daarna fase 1 (engine).
4. Bij M0 (engine-demo): benchmark op de laptop, zoom bekijken en de camera-gevoeligheid samen afstellen. Pas daarna fase 2.
