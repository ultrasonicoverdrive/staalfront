# Command & Conquer: Red Alert — verdiepende broncodeanalyse

**Taal:** Nederlands  
**Vorm:** functionele beschrijving zonder programmeercode  
**Doel:** begrijpen wat de originele game-software doet en welke ideeën bruikbaar zijn voor een nieuwe RTS-game in Rust.

## Leeswijzer en betrouwbaarheid

Dit document is gebaseerd op de openbare EA-repository voor de originele *Command & Conquer: Red Alert*, de repositorybeschrijving en inhoud die via openbare bronbestanden zichtbaar is. De relevante bronbestanden zijn onder meer `MAP.CPP`, `BUILDING.CPP`, `DRIVE.CPP`, `BULLET.CPP` en `TERRAIN.H`.

De analyse is inhoudelijk verder uitgewerkt dan een algemene beschrijving, maar is **geen volledige regel-voor-regel-audit van alle bestanden**. De repository kon in deze werkomgeving niet als volledige lokale kopie worden opgehaald en gebouwd. Daarom worden exacte waarden, complete missieflows en alle uitzonderingen niet als gecontroleerd gepresenteerd wanneer ze niet rechtstreeks uit de geraadpleegde broninformatie blijken.

### Primaire bronnen

- Officiële EA-repository: https://github.com/electronicarts/CnC_Red_Alert
- EA’s modding- en broncode-informatie: https://www.ea.com/games/command-and-conquer/news/modding-faq
- Kaartlogica: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/MAP.CPP
- Gebouwen: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/BUILDING.CPP
- Beweging van voertuigen: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/DRIVE.CPP
- Projectielen en inslagen: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/BULLET.CPP
- Terreinobjecten: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/TERRAIN.H

---

## 1. Samenvatting voor niet-programmeurs

Red Alert is een real-time strategiespel waarin de spelwereld voortdurend wordt bijgewerkt. De speler selecteert eenheden, geeft opdrachten, bouwt een basis, verzamelt grondstoffen, produceert troepen en probeert missiedoelen te halen of de tegenstander uit te schakelen.

De broncode laat zien dat de game niet simpelweg afbeeldingen over een kaart beweegt. Elk object heeft speltoestand en regels. Een voertuig heeft bijvoorbeeld een positie, richting, snelheid, opdracht en mogelijk een doel. Een gebouw heeft onder andere een bouw- of animatietoestand, productiefuncties, gezondheid, wapengedrag en een relatie met de stroomvoorziening. Een projectiel heeft eigen bewegings- en inslaglogica. De kaart registreert waar objecten zich bevinden en welke cellen bezet of toegankelijk zijn.

De game is opgebouwd uit afzonderlijke, samenwerkende subsystemen. Dat maakt het mogelijk om complexe gameplay te laten ontstaan uit eenvoudige herhaalde updates: een gebouw produceert een voertuig, dat voertuig krijgt een bestemming, beweegt over de kaart, zoekt een geldig doel, valt aan en kan uiteindelijk worden vernietigd.

**Hoofdconclusie:** de originele broncode is vooral waardevol als naslagwerk voor spelregels, systeemverantwoordelijkheden en de manier waarop een klassieke RTS is georganiseerd. Ze is geen direct bruikbare moderne game-engine en ook geen complete verzameling originele muziek, filmpjes en andere game-assets.

## 2. Wat zit er in de repository?

EA heeft de broncode openbaar gemaakt onder de GPL-licentie met aanvullende voorwaarden. De repository is inmiddels gearchiveerd en alleen-lezen. EA geeft aan dat de huidige code niet volledig compileert en dat bepaalde oude afhankelijkheden moeten worden vervangen, verwijderd of opnieuw geïmplementeerd.

De hoofdstructuur bevat:

- **CODE:** de centrale spelcode en ondersteunende onderdelen.
- **IPX:** historische netwerkfunctionaliteit.
- **LAUNCH en LAUNCHER:** onderdelen rond starten en opstartvoorzieningen.
- **TOOLS:** hulpmiddelen die bij de code of het ontwikkelproces horen.
- **VQ en WINVQ:** onderdelen die verband houden met videoverwerking.
- **WIN32LIB en WWFLAT32:** oude Windows- en grafische ondersteuning.

De aanwezigheid van een map of subsystemen bewijst niet dat alle oorspronkelijke bestanden van de game-media ook in deze repository zitten. De code die muziek of filmpjes afspeelt is iets anders dan de muziek- en videobestanden zelf.

### Oude afhankelijkheden

EA noemt onder meer oude versies van DirectX, DirectX Media, de Greenleaf Communications Library en HMI Sound Operating System. Voor de oorspronkelijke ontwikkelomgeving worden Watcom C/C++ en Borland Turbo Assembler genoemd. De huidige repository is dus vooral geschikt voor onderzoek en herstelwerk door ontwikkelaars die met historische software kunnen omgaan.

## 3. De spelwereld: objecten en toestand

De software beheert verschillende soorten objecten. De belangrijkste functionele categorieën zijn:

- mobiele eenheden, waaronder voertuigen en infanterie;
- gebouwen die produceren, verdedigen of andere functies uitvoeren;
- terreinobjecten die de kaart beïnvloeden;
- projectielen en tijdelijke gevechtsobjecten;
- animaties en effecten die bij acties horen.

Een object is niet alleen een plaatje. De code moet onthouden waar het staat, welke eigenaar het heeft, wat het op dat moment doet, hoeveel gezondheid het nog heeft en hoe het met andere objecten kan samenwerken.

De broncode gebruikt verschillende gespecialiseerde objecttypen. Dat is zichtbaar in bestanden als `BUILDING.CPP`, `DRIVE.CPP` en `BULLET.CPP`: gebouwen, bewegende voertuigen en projectielen hebben ieder eigen gedrag. Ze delen tegelijkertijd algemene mechanismen voor zaken zoals positie, kaartinteractie en actieve spelstatus.

### Waarom die scheiding belangrijk is

Een gebouw kan niet zomaar dezelfde bewegingsregels krijgen als een tank. Een projectiel volgt evenmin dezelfde regels als infanterie. De game heeft daarom gespecialiseerde logica nodig die op gedeelde basisfuncties voortbouwt.

Voor een nieuwe Rust-game is dit een nuttig ontwerpprincipe: maak onderscheid tussen soorten spelobjecten en hun gedrag, zonder elk object volledig los van de rest te ontwerpen.

## 4. De kaart en het terrein

`MAP.CPP` bevat een groot deel van de kaartlogica. `TERRAIN.H` laat daarnaast zien dat terreinobjecten functies hebben voor hun positie, weergave, doelcoördinaten en de vraag of ze een kaartcel kunnen betreden.

De kaart heeft dus twee rollen:

1. **Visuele rol:** tonen welk terrein en welke objecten de speler ziet.
2. **Speltechnische rol:** bepalen waar objecten zich bevinden, welke cellen bezet zijn en waar beweging of plaatsing is toegestaan.

### Kaartcellen

De spelwereld is onderverdeeld in cellen. Dat maakt het eenvoudiger om posities te registreren, te controleren of een cel toegankelijk is en te bepalen welke objecten elkaar in de weg zitten. Een gebouw neemt doorgaans meer ruimte in dan één cel; mobiele eenheden moeten rekening houden met hun bewegingsklasse en de bezetting van de kaart.

### Weergavevolgorde

De positie van een object op de kaart is niet hetzelfde als de positie waarop het beeld op het scherm wordt getekend. Bij een isometrische weergave is de tekenvolgorde belangrijk: een object kan visueel vóór of achter een ander object horen te staan. De terrein- en objectcode bevat verantwoordelijkheden voor coördinaten en sortering die daarbij passen.

### Wat hiermee nog niet volledig is vastgesteld

Deze analyse heeft niet elke kaartregel afzonderlijk gecontroleerd. Daarom worden de exacte regels voor ieder terreinsoort, alle bewegingskosten, zichtlijnen, bruggen, water, obstakels en speciale kaartovergangen niet als volledig geverifieerd beschreven.

## 5. Opdrachten van de speler

De speler geeft opdrachten via de interface. Een opdracht moet worden vertaald naar gedrag in de simulatie. Conceptueel ziet dat er zo uit:

1. De speler selecteert een eenheid of groep.
2. De speler kiest een doel of actie.
3. Het spel beoordeelt of de actie passend en uitvoerbaar is.
4. De eenheid slaat de opdracht of bestemming op.
5. De eenheid probeert de opdracht zelfstandig uit te voeren.
6. De simulatie controleert in volgende spelstappen of verdere actie nodig is.

Een belangrijk onderscheid is dat **een opdracht ontvangen niet betekent dat de actie direct voltooid is**. Een eenheid kan bijvoorbeeld onderweg zijn, moeten draaien, wachten tot een cel vrij is of een doel verliezen doordat dat doel vernietigd is.

De voertuigenlogica in `DRIVE.CPP` ondersteunt dit principe: een bestemming wordt vastgelegd, de route kan opnieuw worden berekend en de beweging verloopt vervolgens zelfstandig totdat de eenheid moet stoppen of een andere opdracht krijgt.

### Ontwerpimplicatie

In een nieuwe RTS is het verstandig om opdrachten als een duidelijke toestand te behandelen. Denk aan: niets doen, bewegen, een doel benaderen, aanvallen, laden/lossen of terugkeren. Dat voorkomt dat alle logica uitsluitend afhangt van de muisklik die de speler eerder gaf.

## 6. Beweging van voertuigen

`DRIVE.CPP` is een van de meest informatieve bestanden voor het begrijpen van voertuigbeweging. De documentatie in het bestand beschrijft onder meer functies voor het instellen van een bestemming, het bepalen van de volgende bewegingsstappen, draaien, vloeiend bewegen, het volgen van een route en het stoppen van beweging.

### Wat een voertuig tijdens beweging moet doen

- De gewenste bestemming onthouden.
- Bepalen in welke richting het moet draaien.
- Een route of reeks tussenstappen volgen.
- Controleren of de volgende kaartcel toegankelijk is.
- De positie stap voor stap bijwerken.
- Bij aankomst de volgende actie uitvoeren of in een wachttoestand terechtkomen.
- Reageren op een nieuwe opdracht of een blokkade.

De code beschrijft ook beweging als een proces dat zelfstandig doorgaat nadat een route of draaibevel is gegeven. Dat beperkt de hoeveelheid scriptlogica die nodig is: de missie- of opdrachtlogica hoeft niet elk klein stukje beweging zelf aan te sturen.

### Bestemming en route zijn niet hetzelfde

De bestemming is waar de eenheid heen wil. De route is hoe ze daar probeert te komen. Als de route ongeldig wordt of het doel verandert, moet de route opnieuw kunnen worden bepaald. Dit is een belangrijk patroon om ook in een nieuwe game te behouden.

### Bijzondere gevallen

De voertuigcode bevat ook speciale afhandeling, zoals het verplaatsen van objecten naar een vrije nabijgelegen cel wanneer een gewenste cel niet toegankelijk is, en speciale verplaatsingsmechanismen. Zulke uitzonderingen maken de oorspronkelijke code complexer dan een eenvoudige route van A naar B.

### Niet volledig gecontroleerd

Deze analyse bevestigt niet elk detail van het gebruikte routezoekalgoritme, alle prioriteiten bij blokkades of elk voertuigtype afzonderlijk.

## 7. Infanterie en andere mobiele eenheden

Infanterie en voertuigen delen bepaalde algemene spelregels, maar verschillen in hoe ze bewegen en met de kaart omgaan. Infanterie kan bijvoorbeeld anders over cellen worden verdeeld dan een groot voertuig. De openbare broncode bevat een apart infanterieonderdeel; daarin zijn onder meer acties zichtbaar rond opdrachten, aanvallen, schade, celbezetting en het reageren op mijnen.

Dat illustreert dat eenheden niet alleen varianten van één afbeelding zijn. Hun type bepaalt welke regels van toepassing zijn.

Voor een nieuwe RTS is het nuttig om per eenheidsklasse vast te leggen:
- toegestane terreintypen;
- bewegings- en draaigedrag;
- aanvalsmogelijkheden;
- interactie met gebouwen;
- speciale eigenschappen;
- hoe de eenheid reageert wanneer een cel bezet raakt.

## 8. Gevechten en projectielen

`BULLET.CPP` laat zien dat projectielen hun eigen logica hebben. Een projectiel wordt aangemaakt met gegevens over het type, het doel, de bron of verantwoordelijke aanvaller, de kracht, het type schade en de snelheid.

Tijdens de vlucht verwerkt het spel de beweging, eventuele richtingaanpassing naar het doel, een lont of eindvoorwaarde en het moment van ontploffen. De broncode maakt onderscheid tussen onder andere projectielen die een boog volgen, vallen of hun richting bijstellen richting een doel.

### Wat gebeurt er bij een aanval?

Op hoofdlijnen bestaat een aanval uit:
1. Een eenheid of gebouw kiest een doel.
2. Het spel controleert of de aanvaller het doel kan aanvallen.
3. De aanval wordt gestart en eventueel een projectiel aangemaakt.
4. Het projectiel beweegt of de aanval wordt als directe werking verwerkt.
5. Bij de inslag wordt schade afgehandeld.
6. Het doel kan beschadigd of vernietigd raken.
7. De kaart en weergave worden bijgewerkt en effecten kunnen worden getoond.

De broncode voor projectielen beschrijft expliciet beweging, doelgericht gedrag, lont- en detonatielogica. De precieze schadeformules, alle wapentabellen en alle interacties tussen pantser en schadetype zijn in deze analyse niet volledig uitgelezen.

### Waarom projectielen zelfstandige objecten zijn

Een raket of granaat kan na het afvuren nog onderweg zijn terwijl de schutter iets anders doet. Het projectiel heeft daarom een eigen levenscyclus. Dat maakt het mogelijk om verschillende soorten projectielen te simuleren zonder dat elke aanvallende eenheid alle details zelf hoeft te beheren.

## 9. Schade, vernietiging en gevolgen

Een gevecht verandert de toestand van objecten. Een gebouw of eenheid heeft een gezondheidstoestand; schade kan die verminderen en uiteindelijk tot vernietiging leiden. Vernietiging heeft gevolgen voor de rest van de spelwereld: het object moet uit de actieve simulatie verdwijnen, bezette kaartcellen moeten worden vrijgegeven en eventuele effecten moeten worden afgehandeld.

De gebouwcode beschrijft onder meer vernietigingsafhandeling, puin, schadeweergave en het verwijderen van een gebouw nadat de vernietigingsfase is afgehandeld. De code laat ook zien dat de visuele staat kan veranderen wanneer een gebouw zwaar beschadigd raakt.

Dit betekent dat schade niet alleen een getal is. De toestand van het object beïnvloedt ook hoe het wordt getekend en welke acties nog mogelijk zijn.

## 10. Gebouwen: veel meer dan statische objecten

`BUILDING.CPP` is een omvangrijk bestand en beschrijft een groot aantal verantwoordelijkheden. Gebouwen kunnen onder andere:

- worden geplaatst en uit de actieve wereld worden verwijderd;
- een bouw-, idle- of andere animatietoestand hebben;
- eenheden produceren;
- aanvallen of doelen kiezen als ze bewapend zijn;
- draaien met een geschutskoepel;
- gerepareerd of verkocht worden;
- eenheden ontvangen of laten vertrekken;
- samenwerken met andere objecten via berichten;
- invloed hebben op de stroomvoorziening;
- speciale functies uitvoeren, zoals laden, lossen, opladen of een raket lanceren.

Niet ieder gebouw heeft al deze functies. De code gebruikt type-afhankelijke regels om te bepalen wat een specifiek gebouw mag doen.

### Bouwanimatie en speltoestand

De gebouwlogica verwerkt animatiestaten en missiestaten in een bewuste volgorde. De documentatie beschrijft dat de animatiestatus vóór bepaalde missieacties moet worden bijgewerkt, zodat de logica op het juiste moment reageert op een toestandsovergang.

Dit is een belangrijk detail: visuele animatie en gameplay zijn gekoppeld, maar niet identiek. De game moet voorkomen dat een animatie pas later zichtbaar wordt terwijl de bijbehorende spelactie al had moeten plaatsvinden, of omgekeerd.

### Productie

De gebouwcode bevat expliciete productieafhandeling. Fabrieken hebben voortgang en kunnen geproduceerde objecten laten vertrekken. Daarvoor moet het spel rekening houden met de uitgang van het gebouw en een geschikte vrije plek op de kaart. Productie is dus een samenwerking tussen gebouw, productiesysteem en kaart.

### Stroomvoorziening

De code past de stroomuitvoer aan wanneer de gezondheid of toestand van een gebouw verandert. Daardoor is stroom geen uitsluitend visueel getal: schade of verlies van een gebouw kan de beschikbare energie van de speler beïnvloeden. De precieze gevolgen hangen af van de spelregels en het gebouwtype.

### Reparatie en verkoop

Gebouwen kunnen een reparatieproces hebben en er is logica voor het verkopen of slopen van gebouwen. Dit zijn actieve gameplayprocessen met voortgang en toestand, niet alleen knoppen in de interface.

### Verdedigingsgebouwen

Gebouwen met wapens kunnen doelen zoeken, draaien, aanvallen en hun eigen missiegedrag verwerken. De code behandelt de richting van geschutskoepels en speciale eigenschappen van verschillende typen verdediging.

## 11. Economie, grondstoffen en raffinaderijen

De gebouwcode bevat specifieke interacties voor raffinaderijen en oogstmachines. Zo controleert een raffinaderij of een inkomend voertuig een type is dat daar mag lossen, en de communicatie tussen objecten wordt gebruikt om te bepalen of een interactie toegestaan is.

Dit laat zien dat grondstoffenwinning uit meerdere stappen bestaat:
1. Een oogstmachine zoekt of bereikt een bron.
2. Ze verzamelt grondstoffen.
3. Ze keert terug naar een gebouw dat die grondstoffen kan verwerken.
4. De raffinaderij accepteert de machine wanneer de voorwaarden kloppen.
5. De verzamelde middelen worden verwerkt in de economie van de eigenaar.
6. De machine kan weer vertrekken.

De volledige economie is meer dan alleen een teller: er zijn objecten, capaciteit, interacties en voorwaarden nodig. De gebouwcode verwijst bijvoorbeeld naar opslagcapaciteit en toont visueel verschillende vulniveaus van opslaggebouwen.

**Beperking:** deze analyse bevat geen complete lijst van grondstofprijzen, opbrengsten, capaciteiten of balanswaarden. Die moeten rechtstreeks uit alle relevante gegevensbestanden en spelregels worden verzameld.

## 12. AI van gebouwen en productie

De gebouwcode onderscheidt grafische verwerking van niet-grafische AI. De AI-afhandeling van een gebouw verwerkt onder meer missiegedrag, productie, reparatie, rotatie van geschutskoepels, speciale gebouwfuncties en de afhandeling van vernietiging.

Voor computerbestuurde gebouwen is er logica om te bepalen wat er geproduceerd moet worden en om productie te starten. Dat betekent dat de computer niet alleen vooraf geplaatste eenheden gebruikt: gebouwen kunnen ook deel uitmaken van een productieproces.

Deze observatie is concreet voor de gebouw-AI. Ze bewijst op zichzelf niet hoe het volledige strategische brein van de tegenstander beslist welke basis of aanval op lange termijn verstandig is. De complete AI-strategie moet apart worden onderzocht.

## 13. Samenwerking tussen objecten

De gebouwcode gebruikt berichtuitwisseling om objecten op elkaar af te stemmen. Een gebouw kan bijvoorbeeld controleren of een voertuig mag laden of aanmeren. Er bestaan ook interacties rond raffinaderijen, reparatiefaciliteiten en landingsplaatsen.

Dit is een opvallend ontwerppatroon: objecten roepen niet altijd rechtstreeks elkaars interne functies aan, maar gebruiken een afgesproken vorm van communicatie om een interactie aan te vragen en te bevestigen of weigeren.

Voor een nieuwe RTS kan een vergelijkbaar systeem helpen bij:
- voertuigen die een reparatieplaats zoeken;
- helikopters die een landingsplaats nodig hebben;
- oogstmachines die grondstoffen lossen;
- infanterie die in transportvoertuigen stapt;
- gebouwen die alleen bepaalde objecttypen accepteren.

## 14. Grafische weergave en beschadigingen

De code voor gebouwen bevat aparte tekenlogica. Die bepaalt niet alleen welk gebouwbeeld moet worden getoond, maar houdt ook rekening met bijvoorbeeld de richting van een geschutskoepel, schade, animatiefase, opslagniveau en speciale effecten.

Een zwaar beschadigd gebouw kan dus anders worden weergegeven dan een intact gebouw. Een opslaggebouw kan het gevulde niveau tonen en een fabriek kan een eigen visuele laag hebben. De spelstatus wordt daarmee teruggekoppeld naar de speler.

Voor een RTS is dit essentieel: de speler moet snel kunnen aflezen wat er gebeurt zonder elk object afzonderlijk te openen.

## 15. Geluid, spraak en video

De voertuigcode bevat verwijzingen naar gesproken feedback bij selecteren, verplaatsen en aanvallen. Daardoor krijgt de speler niet alleen visuele bevestiging van opdrachten, maar ook geluidssignalen of stemreacties.

De repository bevat daarnaast oude ondersteuning voor audio en video. Maar er is een belangrijk onderscheid:

- **Audio-/videocode:** bepaalt hoe en wanneer geluid of video wordt verwerkt.
- **Media-assets:** zijn de daadwerkelijke muziek, spraakbestanden, geluidseffecten en filmpjes.

De repositorybeschrijving noemt oude audio- en mediabibliotheken als afhankelijkheden. Dat betekent niet dat alle originele muziek en filmpjes in de broncoderepository zijn opgenomen.

Voor een nieuwe Rust-game kunnen geluid en video als zelfstandige presentatieonderdelen worden ontworpen. Originele Red Alert-media moeten apart worden beoordeeld op beschikbaarheid en rechten; voor een eigen game zijn eigen of correct gelicentieerde assets de veilige basis.

## 16. Netwerk en multiplayer

De repository bevat een IPX-map en verwijst naar de Greenleaf Communications Library als een oude afhankelijkheid. Dat wijst op historische netwerkfunctionaliteit. Het is geen bewijs dat de originele netwerkcode direct bruikbaar is voor modern online spelen.

Een moderne RTS moet nadenken over:
- hoe spelers verbinding maken;
- wie de waarheid over de speltoestand bepaalt;
- hoe vertraging en verbindingsverlies worden verwerkt;
- hoe alle deelnemers dezelfde spelregels volgen;
- hoe valsspelen en inconsistenties worden beperkt.

Deze onderwerpen zijn ontwerpkeuzes voor een nieuwe game. Ze zijn niet één-op-één af te leiden uit het bestaan van de oude IPX-code.

## 17. Missies en scenario's

De spelsoftware moet scenario-informatie inladen en de voortgang van een missie beheren. In een RTS kan een missie specifieke doelen, gebeurtenissen en eindvoorwaarden bevatten. De exacte details van de volledige scenarioverwerking zijn in deze sessie niet volledig bestand voor bestand gevolgd.

Voor een volledige technische audit moet worden nagegaan:
- hoe scenario's worden ingelezen;
- hoe objecten bij de start worden geplaatst;
- hoe triggers of gebeurtenissen worden geactiveerd;
- hoe de game controleert of doelen zijn bereikt;
- hoe winst, verlies en vervolgacties worden afgehandeld.

Daarom worden in dit document geen exacte voorwaarden van afzonderlijke campagnes of missies ingevuld zonder die bronbestanden rechtstreeks te controleren.

## 18. De hoofdcyclus: hoe alles samenwerkt

Een real-time strategiespel moet zijn wereld voortdurend bijwerken. De precieze volledige hoofdcyclus is in deze analyse niet door alle opstart- en spelcyclusbestanden gevolgd, maar uit de gespecialiseerde subsystemen is het algemene patroon goed te beschrijven.

Tijdens opeenvolgende spelstappen moet het spel onder andere:
1. invoer en opdrachten verwerken;
2. de toestand van objecten bijwerken;
3. beweging en draaien uitvoeren;
4. projectielen laten bewegen en inslagen verwerken;
5. AI en gebouwmissies laten reageren;
6. productie, schade en vernietiging verwerken;
7. kaartbezetting en zichtbare toestand bijwerken;
8. de bijgewerkte spelwereld tekenen en presentatie afhandelen.

Deze onderdelen beïnvloeden elkaar. Een gebouw dat vernietigd wordt, kan bijvoorbeeld de stroomvoorziening wijzigen. Een voertuig dat de raffinaderij bereikt, kan grondstoffen lossen. Een projectiel kan een gebouw beschadigen, waarna de gebouwweergave en mogelijk de beschikbare energie veranderen.

## 19. Opslaan, laden en herstel van de speltoestand

Een complete RTS moet de speltoestand kunnen bewaren en herstellen wanneer de game dat ondersteunt. Daarbij gaat het niet alleen om posities, maar ook om gezondheid, opdrachten, productievoortgang, grondstoffen, missievoortgang en onderlinge verwijzingen tussen objecten.

Deze analyse heeft de volledige save/load-keten niet door de relevante bronbestanden gevolgd. Daarom kan hier niet worden bevestigd welke gegevens in elk type savebestand worden opgenomen of hoe iedere verwijzing wordt hersteld.

Voor een nieuwe Rust-game is het verstandig de speltoestand expliciet te modelleren en save/load te ontwerpen voordat het project groot wordt. Anders kan het later moeilijk zijn om tijdelijke animaties, lopende opdrachten en productiewachtrijen consistent te herstellen.

## 20. Wat weten we concreet uit de onderzochte bronbestanden?

| Bronbestand | Direct herkenbare verantwoordelijkheid | Betekenis in gewone taal |
|---|---|---|
| `MAP.CPP` | Kaartlogica | Houdt de spelkaart en kaartinteracties bij |
| `TERRAIN.H` | Terreinobjecten | Bevat terreinfuncties voor coördinaten, weergave en celtoegang |
| `DRIVE.CPP` | Voertuigbeweging | Verwerkt bestemmingen, draaien, routevolging en zelfstandig bewegen |
| `BULLET.CPP` | Projectielen | Verwerkt projectielbeweging, doelgericht gedrag en ontploffing |
| `BUILDING.CPP` | Gebouwgedrag | Verwerkt onder meer productie, animaties, schade, reparatie, stroom en interacties |
| `IPX/` | Historisch netwerk | Onderdeel van oude netwerkondersteuning |
| `VQ/`, `WINVQ/` | Videoverwerking | Ondersteuning voor video in de oude gameomgeving |
| `WIN32LIB/`, `WWFLAT32/` | Platform-/grafische ondersteuning | Koppeling met de oorspronkelijke Windows-omgeving |

## 21. Wat is bevestigd, wat is een gevolgtrekking en wat moet nog worden onderzocht?

### Rechtstreeks ondersteund door de geraadpleegde broninformatie
- EA heeft de originele Red Alert-broncode openbaar gemaakt.
- De repository is gearchiveerd en alleen-lezen.
- De code compileert niet volledig zonder herstelwerk.
- Er zijn oude externe afhankelijkheden nodig voor de oorspronkelijke build.
- Het terreinonderdeel bevat logica rond coördinaten, weergave en toegankelijkheid van kaartcellen.
- De voertuigcode behandelt bestemmingen, beweging en draaien.
- De projectielcode behandelt beweging en ontploffing.
- De gebouwcode behandelt onder meer AI, productie, schade, reparatie, stroom en interacties met andere objecten.

### Functionele gevolgtrekkingen
- De game werkt door objecten met eigen toestand en gedrag in een gedeelde kaart te laten samenwerken.
- Beweging, gevechten en productie moeten over meerdere spelstappen worden verwerkt.
- Gebouwschade kan niet alleen het uiterlijk maar ook de spelwerking beïnvloeden.
- Het scheiden van objecttypen en subsystemen is een nuttig patroon voor een nieuwe RTS.

### Nog niet volledig onderzocht
- De volledige hoofdcyclus en alle timingregels.
- Alle bewegingsalgoritmen en uitzonderingen.
- Alle wapen-, schade-, pantser- en bereikregels.
- Complete economie- en productietabellen.
- Het strategische besluitvormingsproces van de AI.
- Alle triggers, scenario-events en missie-eindvoorwaarden.
- De volledige save/load-implementatie.
- De precieze inhoud van alle originele audio- en videobestanden.

## 22. Vertaling naar een nieuwe RTS in Rust

De oude code is een goede referentie voor het soort problemen dat een RTS moet oplossen. Voor een nieuwe game zou ik de volgende subsystemen scheiden.

### A. Wereld en kaart
Verantwoordelijk voor kaartcellen, terrein, bezetting, toegankelijkheid en coördinaten.

### B. Spelobjecten
Eenheden, gebouwen, terreinobjecten, projectielen en effecten. Elk objecttype heeft zijn eigen regels en deelt alleen gemeenschappelijke eigenschappen waar dat logisch is.

### C. Opdrachten en missies
Verwerkt speleropdrachten, doelen, wachtrijen, scenario-events, overwinning en nederlaag.

### D. Beweging
Beheert bestemmingen, routeplanning, draaien, blokkades, beweging per stap en het opnieuw plannen bij veranderingen.

### E. Gevechten
Beheert doelkeuze, aanvalvoorwaarden, projectielen, inslagen, schade, vernietiging en visuele effecten.

### F. Gebouwen en economie
Beheert bouwen, productie, grondstoffen, stroom, reparatie, verkoop, laden/lossen en productie-uitgangen.

### G. AI
Scheidt strategische keuzes van de concrete uitvoering door eenheden en gebouwen.

### H. Presentatie
Verzorgt sprites of modellen, animaties, selectie, UI, geluid, muziek en eventuele tussenfilmpjes.

### I. Opslaan en multiplayer
Bewaar de speltoestand op een controleerbare manier. Voeg multiplayer pas toe met een bewust gekozen netwerkmodel.

### Wat je niet letterlijk moet kopiëren
- De oude Windows- en DirectX-koppelingen.
- De historische audio- en netwerkbibliotheken.
- De oude buildomgeving als basis voor een moderne engine.
- Onbegrepen code zonder eerst de bijbehorende spelregel vast te leggen.

De nieuwe game moet een eigen naam, wereld, eenheden, graphics, muziek en andere herkenbare content krijgen. De broncodevrijgave geeft niet automatisch toestemming om alle handelsmerken en originele media te gebruiken.

## 23. Aanbevolen vervolg voor een echte volledige audit

Voor een volledig betrouwbare, bestand-voor-bestand-beschrijving moeten de overige subsystemen gericht worden onderzocht. De volgorde met de meeste waarde is:

1. **Opstart en hoofdcyclus:** hoe de game start, scenario's initialiseert en de simulatie laat lopen.
2. **Algemene objectstructuur:** hoe objecten worden aangemaakt, bijgehouden en verwijderd.
3. **Spelerinvoer en opdrachten:** selectie, klikken, doelkeuze en orderverwerking.
4. **Kaart en zicht:** bezetting, verkenning, zichtbaarheid en kaartverversing.
5. **Beweging:** routeberekening, botsingen, verschillende bewegingsklassen en uitzonderingen.
6. **Gevechten:** wapentypes, doelvalidatie, schade, pantser en vernietiging.
7. **Economie en productie:** grondstofstromen, bouwvoorwaarden, kosten en productiewachtrijen.
8. **AI:** strategie, tactiek, doelkeuze en moeilijkheidsinstellingen.
9. **Scenario's en missies:** inladen, triggers, doelen en eindvoorwaarden.
10. **Presentatie:** rendering, animaties, geluid, muziek en video.
11. **Opslaan en laden:** welke toestand wordt bewaard en hoe verwijzingen worden hersteld.
12. **Netwerk:** wat daadwerkelijk werkt en wat aan oude bibliotheken vastzit.

Per subsysteem hoort de audit te noteren: welke verantwoordelijkheid het heeft, welke andere subsystemen het aanroept, welke toestand het verandert, welke uitzonderingen bestaan, en welke onderdelen niet te bouwen zijn zonder ontbrekende afhankelijkheden.

## Eindconclusie

De originele Red Alert-code beschrijft een uitgebreide simulatie, niet alleen de zichtbare actie op het scherm. De kaart bepaalt waar objecten kunnen staan; voertuigen volgen opdrachten en routes; projectielen hebben een eigen vlucht- en inslagproces; gebouwen beheren productie, schade, stroom en interacties; en al deze onderdelen beïnvloeden de toestand van de spelwereld.

Dat is precies de waarde voor een nieuwe RTS in Rust: niet de oude technologie overnemen, maar begrijpen hoe de spelregels samenhangen. Deze analyse is concreter dan een algemene introductie, maar blijft eerlijk over wat nog niet volledig is gecontroleerd. Een echte volledige audit vereist een volledige lokale kopie van de repository en systematische inspectie van de resterende bronbestanden.
