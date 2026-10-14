# Missies — inspiratie uit Red Alert (clean-room)

Opgesteld 14-10-2026 als voorwerk voor fase 10 (missies en editor) en fase 15
(content). Bron: de openbare EA-repository `electronicarts/CnC_Red_Alert`
(headers `TEVENT.H`, `TACTION.H`, `TRIGTYPE.H`, `TEAMTYPE.H`, `SCENARIO.H`) en
algemene kennis van de campagnes.

**Clean-room (B5):** dit document beschrijft *wat* het systeem kan, in eigen
woorden. Er is geen code overgenomen, en dat mag ook niet gebeuren bij het
bouwen. Namen, personages, verhalen, eenheden en teksten uit Red Alert zijn
handelsmerk/auteursrecht van EA en komen niet in Staalfront.

**Belangrijk:** de repository bevat **geen missiebestanden**. De scenario's
(kaarten, triggers, briefings) zaten in de datapakketten van de gekochte game,
niet in de broncode. Wat de code wél laat zien is het *gereedschap* waarmee de
missies gebouwd zijn: het triggersysteem. De missietypen hieronder komen uit
algemene kennis van de campagnes, op het niveau van spelontwerp.

---

## 1. Hoe een missie in elkaar zit

Een scenario bestaat uit:

- **startopstelling**: kaart, spelers (huizen), beginobjecten, geld, zicht;
- **teams**: groepen eenheden met een vaste opdrachtenreeks (zie §3);
- **triggers**: *gebeurtenis(sen) → actie(s)*, vastgemaakt aan een object, een
  cel, een zone of aan een speler;
- **globale vlaggen en een missietimer** om triggers aan elkaar te knopen;
- **opties per scenario**: o.a. eenheden meenemen naar de volgende missie,
  timer erven van de vorige missie, laatste missie van de campagne, geen
  scorescherm, slechts één keer speelbaar, geen kaartkeuze, geld uit
  kratten/grondstof.

### Trigger-opbouw

- Een trigger heeft **twee gebeurtenissen** en **twee acties**, gecombineerd als
  *alleen de eerste*, *beide (EN)*, *een van beide (OF)*, of *gekoppeld*
  (gebeurtenis 1 → actie 1, gebeurtenis 2 → actie 2).
- **Levensduur**: *eenmalig* (verdwijnt na afgaan), *half-blijvend* (gaat af
  als alle objecten waar hij aan hangt het melden) of *blijvend* (gaat elke
  keer af).
- Een trigger hoort bij een **speler**; gebeurtenissen als "gebouw vernietigd"
  gelden dan voor die speler.

Voor Staalfront: dit model is klein, krachtig en makkelijk deterministisch te
maken. Verbetering t.o.v. het origineel: willekeurig veel voorwaarden en
acties per trigger in plaats van precies twee, en benoemde variabelen in
plaats van genummerde globale vlaggen.

## 2. Bouwstenen (functioneel)

**Gebeurtenissen**

| Groep | Wat |
|---|---|
| Plaats | speler betreedt object/cel; eenheid betreedt een zone; lijn horizontaal of verticaal overgestoken; eenheid verlaat de kaart |
| Ontdekking | object ontdekt; een speler ontdekt een andere speler |
| Gevecht | aangevallen; vernietigd; N eenheden / N gebouwen vernietigd; alle eenheden / gebouwen / alles van een speler vernietigd; alle bruggen vernietigd; nepgebouwen vernietigd; geen fabrieken meer |
| Infiltratie | gebouw bespioneerd; gebouw beroofd; burger geëvacueerd |
| Economie en bouw | geld boven een grens; gebouw/eenheid/infanterie/vliegtuig van een type gebouwd; gebouwtype bestaat; stroomtekort |
| Tijd | tijd verstreken; missietimer afgelopen |
| Logica | globale vlag gezet / gewist; "altijd" |

**Acties**

| Groep | Wat |
|---|---|
| Afloop | winst; verlies; winst toestaan (pas winnen als iets anders ook klopt) |
| Leger | team aanmaken / opheffen; versterkingen; alles op jacht; AI begint te produceren; AI mag automatisch teams vormen; voorkeursdoel voor de AI |
| Basis | AI bouwt basis af; AI verkoopt alles ("uitverkoop") |
| Kaart en zicht | hele kaart onthullen; gebied / zone onthullen; zicht weer laten dichtgroeien; landingszone markeren |
| Presentatie | filmpje, geluid, muziek of spraak afspelen; tekstbericht |
| Logica | andere trigger forceren of verwijderen; timer starten / stoppen / ophogen / verlagen / zetten; globale vlag zetten / wissen |
| Speciaal | specifiek object vernietigen; superwapen vol laden of afvuren |

## 3. Teams: gescripte groepen

Een team is een samenstelling (bv. 4 tanks + 6 infanteristen) met een
**opdrachtenlijst** die stap voor stap wordt afgewerkt:

aanvallen (doelsoort) · aanvallen naar routepunt · formatie aannemen · naar
routepunt · naar cel · bewaken (tijdsduur) · lus terug naar stap N · aanvallen
op aanwijzing · uitladen · ontplooien · achtervolgen tot de dood · een
eenheidsopdracht uitvoeren · globale vlag zetten · tijdelijk onkwetsbaar ·
inladen · spioneren · patrouilleren.

Teams hebben daarnaast eigenschappen als: opnieuw aanvullen als leden sneuvelen,
alleen bij een gebeurtenis vertrekken, wachten tot het team compleet is, en
of de AI ze automatisch mag aanmaken.

## 4. Missietypen (spelontwerp, geen verhaal)

| Type | Kern | Bouwstenen |
|---|---|---|
| **Basis vernietigen** | klassiek: bouwen, oogsten, tegenstander uitschakelen | "alles vernietigd" → winst |
| **Redding/escorte** | een kwetsbare eenheid naar een evacuatiepunt brengen | zone betreden / kaart verlaten → winst; die eenheid vernietigd → verlies |
| **Commando** | weinig eenheden, geen basis, sterke held; sluipen en saboteren | geen fabrieken; specifieke gebouwen vernietigd → winst |
| **Konvooi** | vrachtwagens moeten de overkant halen; jij beveiligt de route | N van M aangekomen → winst; teams in hinderlagen |
| **Verdedigen tot de tijd om is** | golven aanvallen overleven | missietimer afgelopen → winst; sleutelgebouw vernietigd → verlies |
| **Infiltratie** | een spion of dief moet een gebouw binnen; daarna onthult/opent iets | bespioneerd → kaart onthullen / versterkingen |
| **Brug/doorgang** | bruggen vernietigen of juist heel houden | alle bruggen vernietigd → winst of verlies |
| **Tijdsdruk** | een superwapen laadt op; je moet het uitschakelen voor de timer afloopt | timer + "specifiek object vernietigd" |
| **Ontsnapping** | basis kwijt, eenheden moeten de kaart af | kaart verlaten door N eenheden → winst |
| **Verovering** | gebouwen innemen in plaats van slopen | gebouw van eigenaar gewisseld (eigen uitbreiding) |
| **Nep en misleiding** | nepgebouwen of lokaas om de vijand te misleiden | nepgebouwen vernietigd; voorkeursdoel AI |

Afwisseling in een campagne komt vooral uit: wel/geen basis, tijdsdruk,
kwetsbare sleutel-eenheid, en het kantelmoment halverwege (versterkingen, nieuw
doel, onthulde kaart) via triggers.

## 5. Wat dit betekent voor het plan

- **WP10.1/10.2** (scenarioformaat en triggers): het model van §1 met
  onbeperkt aantal voorwaarden/acties, benoemde variabelen en de levensduur
  eenmalig/half-blijvend/blijvend. Triggers evalueren in de sim-tik, in vaste
  volgorde (determinisme).
- **WP10.3** (acties): de tabel van §2 is een goede eerste lijst; "winst
  toestaan" en "AI-uitverkoop" zijn klein en geven veel speelgevoel.
- **Teams (§3)** horen bij de AI (fase 9) én bij missies; één opdrachtentaal
  voor beide.
- **WP15** (content): de missietypen van §4 als checklist, met een eigen wereld
  en eigen verhaal. Die wereld is nog niet bepaald.
