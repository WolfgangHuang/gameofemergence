# Game of Emergence: Entwicklung

Stand: 8. November 2026, nach Schritt 4 von ursprünglich 5, ergänzt um den Hat-Glider, den Torus und das Teilen per Link.

Dieses Dokument hält fest, was gebaut wurde, warum es so gebaut ist und was als Nächstes ansteht. Installation und Bedienung stehen in der [Readme.md](Readme.md).

## Architektur im Überblick

Die Grundidee: **Geometrie und Simulation sind getrennt.** Die Engine kennt nur Zellen und Nachbarn, das Rendering nur Polygone. Darum lässt sich eine Kachel verformen, ohne die Simulation anzufassen.

```
React-Oberfläche (src/App.tsx, src/ui/)
        │
Stage (src/stage/Stage.ts): Kamera, Eingabe, Bildschleife
        │                         │
   Web Worker                  WebGL2
   (src/sim/worker.ts)         (src/render/renderer.ts, HatScene.ts)
        │
Rust/WASM-Engine (engine/src/)
```

| Datei | Aufgabe |
|---|---|
| `engine/src/life.rs` | Chunk-Engine für alle periodischen Gitter |
| `engine/src/status.rs` | Statuserkennung |
| `engine/src/hat.rs` | Hierarchie der Hat-Kachelung, exakte Koordinaten |
| `engine/src/hat_world.rs` | Simulation auf dem Hat-Graphen, stückweise Erzeugung |
| `engine/src/lib.rs` | C-ABI-Exporte für den Worker |
| `src/sim/worker.ts`, `protocol.ts` | Simulationsschleife, Nachrichten zwischen Stage und Worker |
| `src/tiles/tiles.ts` | Periodische Kachelungen, Vorlagen, Regelvorschläge |
| `src/render/renderer.ts` | Periodisches Feld und Nachglühen |
| `src/render/HatScene.ts` | Hat-Kacheln als Instanzen, Zellsuche |
| `src/patterns/` | Musterbibliothek als JSON, RLE lesen und schreiben, Regeln als Text |
| `src/share/link.ts` | Feldzustand als URL-Fragment, Prüfung eingehender Links |
| `scripts/` | WASM-Build, Benchmark, Prüfskripte |

## Bisherige Schritte

### Schritt 1: Grundgerüst

Vite, React und TypeScript, Rust/WASM im Worker, WebGL2-Rendering, Pinsel, Zoom und Nachglühen. Die ursprünglich für Schritt 5 geplanten Sparse Chunks wurden vorgezogen, weil das unbegrenzte Feld früh feststand.

### Schritt 2: Regeln, Muster, Status

Regeleditor, Musterbibliothek mit Vorschau am Mauszeiger, Statuserkennung und Pause bei stabilem Zustand. Das Zufallsfeld wanderte dabei von `R` auf `S`, weil `R` das Muster dreht.

### Schritt 3: Periodische Kachelformen

Neighbourhood als Parameter der Engine, Dreiecke und Hexagone, Nachschlagetextur im Renderer, Kacheleditor mit ziehbaren Ecken.

### Schritt 4: Einstein-Kacheln

Hat-Hierarchie in exakten Koordinaten, Graph-Simulation, stückweise Erzeugung und Morphing über `Tile(a, b)`.

### Nachtrag: Hat-Glider

Priority-Table-Regeln mit 4 Zuständen in der Hat-Engine und Glider A von James Edmond als erstes Muster für die Hat-Kachelung, siehe „Glider und Kompassrichtungen“.

### Nachtrag: Torus

Felder fester Größe für alle periodischen Gitter, siehe „Torus“.

### Nachtrag: Teilen per Link und RLE

Links mit Einstellungen und Zellen, dazu RLE-Import und -Export, siehe „Teilen per Link“.

## Designentscheidungen

### Rust/WASM statt JavaScript, numpy oder WebGPU

numpy schied aus, weil es einen Server oder Pyodide bräuchte. WebGPU ist auf regulären Gittern schnell, aber noch nicht überall verlässlich, unter Linux besonders, und bei dünn besetzten Feldern unhandlich. Gemessen wurde Rust/WASM gegen eine einfache JS-Schleife mit Typed Arrays (`npm run bench`, in Node, ein CPU-Kern):

| Variante | Zellen pro Sekunde |
|---|---|
| WASM, Viereck mit 8 Nachbarn | 3,1–3,5 Mrd. |
| WASM, Viereck mit 4 Nachbarn | 2,0 Mrd. |
| WASM, Hexagon | 1,7 Mrd. |
| WASM, Dreieck mit 3 Nachbarn | 1,7 Mrd. |
| WASM, Dreieck mit 12 Nachbarn | 0,66 Mrd. |
| WASM, Viereck mit 8 Nachbarn auf dem Torus | 2,6–3,4 Mrd. |
| JavaScript, Viereck mit 8 Nachbarn | 77–93 Mio. |

Bewusst ohne wasm-bindgen und wasm-pack: Die Engine exportiert einfache C-Funktionen, der Build braucht nur Cargo und das WASM-Target.

### Speicherform: Sparse Chunks

Eine Punktliste kostet 64 Bit pro lebender Zelle und lohnt sich damit erst unter rund 1,6 % Dichte. Zufallsfelder pendeln sich aber bei etwa 3 % ein, und Nachbarn nachzuschlagen wäre langsam. Stattdessen:

- Eine Hash-Map hält Chunks von 64×64 Zellen, eine Zeile ist ein `u64`.
- Bit-Slicing zählt die Nachbarn für 64 Zellen gleichzeitig. Die 8-Nachbar-Variante hat einen eigenen schnellen Kern (`advance`), alle anderen teilen sich `advance_general`.
- Chunks werden übersprungen, wenn sich weder sie noch ihre Nachbarn in der letzten Generation geändert haben.
- Leere Chunks ohne belegte Nachbarn werden alle 64 Generationen entfernt.

Auf der Ebene wird die Regel B0 abgelehnt, weil sie die unendliche Fläche in einem Schritt füllen würde. Der Torus erlaubt sie.

### Kachelungen teilen sich ein Gitter

Alle periodischen Formen nutzen dieselben Chunks und unterscheiden sich nur in den Nachbar-Offsets (`Topology` in `life.rs`):

| Topologie | Nachbarn | Besonderheit |
|---|---|---|
| Moore | 8 | |
| Von Neumann | 4 | |
| Hexagonal | 6 | axiale Koordinaten, also ein schräges Quadratgitter |
| Dreieck, Kanten | 3 | Offsets hängen von der Parität von x + y ab |
| Dreieck, Ecken | 12 | dto., Reichweite 2 Zellen |

Mathematische Grundlage der Verformung:

- Jedes einfache Viereck kachelt die Ebene, wenn jede zweite Kopie um 180° gedreht wird. Die Nachbarschaft bleibt die des Quadrats, darum laufen alle Muster auf verformten Vierecken weiter.
- Jedes Dreieck ergibt mit seiner Kopie ein Parallelogramm.
- Punktsymmetrische Hexagone kacheln allein durch Verschiebung. Andere kachelnde Hexagon-Typen fehlen deshalb bewusst.

Kacheln werden auf Fläche 1 normiert, damit das Bild beim Verformen gleich groß bleibt.

### Torus

Ein Torus ist ein eigenes `World`-Objekt neben der Ebene. Beide behalten beim Wechsel ihre Zellen und ihre Kamera. Regel und Nachbarschaft schickt die Oberfläche beim Wechsel neu an das aktive Feld.

**Geisterzellen statt neuer Kerne:** Vor jedem Schritt kopiert `fill_ghosts` die Randzellen auf die Stellen knapp hinter dem gegenüberliegenden Rand, zwei Spalten und eine Zeile, also so viel, wie die größte Nachbarschaft braucht. Die schnellen Kerne lesen sie wie gewöhnliche Nachbarn und bleiben unverändert. Danach wischt eine Maske die Geister aus Chunks, die über den Rand hinausragen. Darum gehen auch Größen, die kein Vielfaches von 64 sind. Alle Chunks existieren von Anfang an, nichts wird erzeugt oder eingesammelt. Das Überspringen ruhiger Chunks bleibt erhalten und läuft über die Position statt über die Links.

**Nur gerade Seiten:** Vierecke und Dreiecke brauchen sie, damit die Kacheln an der Naht passen (`check-tiles.mjs` prüft das für alle Vorlagen). Hexagone kämen auch mit ungeraden Seiten aus, die Engine verlangt aber überall gerade Zahlen. Das hält die Regel einfach und spart Sonderfälle: Zwei Geisterspalten liegen so immer im selben Wort, und die letzte Chunk-Spalte enthält mindestens zwei Zellen, sodass Einfluss nie einen Chunk überspringt.

**B0:** Unter B0 ist ein leeres Feld nicht in Ruhe. Darum markiert `clear` auf dem Torus alle Chunks als geändert, und „Empty“ ist dort kein Endzustand.

**Rendering:** Der Worker liefert höchstens eine Periode, den Rest wiederholt der Shader durch Umwickeln der Zellkoordinaten. Die Tapete kostet also nichts zusätzlich, unabhängig vom Zoom. Die Grenzlinie ist eine Gerade in Gitterkoordinaten. Bei Dreiecken und schrägen Formen läuft sie deshalb durch Zellen, statt ihren Kanten zu folgen. Das ist nur Anzeige, die Simulation betrifft es nicht.

### Teilen per Link

Die Seite hat keinen Server, also steckt alles im URL-Fragment (`#v=1&p=Square&r=B3/S23&c=0,0,14&a=100,50,bo-2bo-3o`). Fragmente gehen nie an einen Webserver. Das Feld `v` trägt die Formatversion.

**Was ein Link enthält,** entscheidet `buildLink` in dieser Reihenfolge:

| Lage | Inhalt des Links | Ergebnis beim Öffnen |
|---|---|---|
| Zufallsfeld, Generation 0, sonst unberührt | Startwert und Dichte | exakt dieses Feld |
| Zellen passen in 32.000 Zeichen | Zellen als RLE | exakt dieses Feld |
| zu viele Zellen, Feld stammt aus einem Zufallsfeld | Startwert und Dichte | der Start dieses Laufs, die Seite weist darauf hin |
| zu viele Zellen, sonst | nur Einstellungen | leeres Feld, die Seite weist darauf hin |

Geöffnete Links stehen immer pausiert bei Generation 0, der Zählerstand wird nicht übertragen.

**Zellen:** RLE, als `a=` im Klartext oder als `z=` mit Deflate und Base64url, je nachdem, was kürzer ist. `$` wird im Klartext zu `-`, das schließende `!` entfällt, weil Link-Erkennungen dort gern aufhören. Ein Glider braucht so unter 100 Zeichen, ein zufälliges 128×128 rund 3.900. Ab 2.000 Zeichen warnt die Seite, dass manche Programme den Link kürzen.

**Zufallsfelder:** `Stage` merkt sich je Feld, ob es seit dem letzten Leeren nur eine einzige Zufallsfüllung erhalten hat. Jede Malaktion löscht diese Angabe. Auf dem Hat hing die Verteilung zunächst von den Kachel-Ids ab, also davon, wo die Ansicht vorher war. `HatWorld::soup` geht deshalb jetzt nach Position.

**Links sind fremde Eingaben.** `parseLink` prüft jeden Wert und meldet Verworfenes in der Oberfläche, der Rest des Links gilt weiter. Begrenzt sind Größen, Koordinaten, Zellzahl (2 Millionen) und die entpackte Länge (16 MB), damit ein kurzer Link nicht den Tab lahmlegt. Unbekannte Versionen werden ganz ignoriert.

**RLE-Import:** Kommentare und Kopfzeile sind optional, die alte Form `23/3` und der Zusatz `:T100,100` werden verstanden. Bringt das Muster eine Regel mit, wird sie aktiv. Mehrzustandsmuster und nicht-totalistische Regeln werden mit einer Meldung abgelehnt. Der Export schreibt die Torus-Größe so in die Regel, wie Golly es tut.

### Rendering über eine Nachschlagetextur

Für periodische Kacheln rendert `Renderer.setTiling` eine Gitterperiode in eine Textur von 512×512 Texeln: Jeder Texel enthält die Zelle und den Abstand zu ihrem Umriss. Das geschieht nur, wenn sich die Kachel ändert. Der Display-Shader braucht danach keine Geometrie mehr, jede Form kostet gleich viel, und die Abstandswerte liefern die Gitterlinien.

Die Zellzustände kommen als 8-Bit-Textur vom Worker, mit 1 Texel pro `2^level` Zellen je Achse. Herausgezoomt zählt der Wert die Abdeckung.

Weitere Punkte:

- **Präzision:** Ganzzahlige und gebrochene Positionsanteile gehen getrennt an den Shader, damit sie auch weit vom Ursprung genau bleiben.
- **Datenrate:** Der Worker baut Frames nur auf Anfrage und nur, wenn sich etwas geändert hat. Puffer werden wiederverwendet.
- **Nachglühen:** Eine Textur pro Zustandstextur, die in jedem Frame verblasst und beim Verschieben mitwandert. Es liegt komplett auf der GPU und kostet die Engine nichts.

### Einstein-Kacheln

Die Substitutionsregeln stammen aus [hatviz](https://github.com/isohedral/hatviz) (BSD-3-Clause, Hinweis in `public/THIRD_PARTY_NOTICES.txt`). *Tactile* wurde geprüft (ebenfalls BSD-3-Clause), wird aber nicht verwendet.

**Exakte Koordinaten:** Die Umrisse der Metatiles verlassen nach wenigen Ebenen das Raster, die Hats nicht. Die Umrisse werden deshalb mit Gleitkomma berechnet, und jede Kindposition wird dann relativ zum ersten Hat des Metatiles auf ganze Zahlen gerundet. Die größte Abweichung liegt bei 0,0002 Rastereinheiten auf Ebene 16.

**Morphing:** Im `Tile(a, b)`-Morph hängt die Position eines Punkts davon ab, wie viele Kanten jeder Sorte zu ihm führen, was sich aus den Rasterkoordinaten allein nicht bestimmen lässt. Jeder Punkt speichert darum zwei Anteile: `p` für a-Kanten, `q` für b-Kanten. Die Position ist `a·p + b/√3·q`, beim Hat also `p + q`. Jedes Metatile leitet die Anteile seiner Kinder an einer gemeinsamen Ecke zweier Kinder ab (`derive_splits`). Dadurch liegen auch weit entfernte Stücke korrekt, ohne dass die Fläche dazwischen erzeugt werden muss.

**Unbegrenztes Feld:** Die Hierarchie umfasst 16 Ebenen und entsteht in rund 41 ms. Die Wurzel enthält rund 12 Billionen Hats bei einem Radius von gut 11 Millionen Rastereinheiten. Erzeugt werden Stücke der Ebene 2 mit bis zu 169 Hats, sobald die Ansicht oder lebende Zellen sie erreichen.

**Simulation:** Es gibt kein Raster auszunutzen, also ist das Feld ein expliziter Graph. Pro Generation werden nur Zellen rund um Änderungen betrachtet, Nachbarzahlen werden laufend mitgeführt. Offene Kanten werden aus der Tabelle gelöscht, sobald ihr Gegenstück da ist.

**Speicher:** Rund 700 Byte pro Zelle. Das Limit von 1,5 Millionen Zellen ergibt also rund 1 GB. Der erste Entwurf sprengte den WASM-Speicher bei explosiven Regeln, deshalb gibt es kompakte Schlüssel und das Limit.

**Rendering:** Hats sind Instanzen mit ganzzahligen Attributen, ein Dreiecksnetz wird pro Morph-Wert per Ear Clipping neu berechnet. Stapel von 8.192 Zellen werden außerhalb der Ansicht übersprungen. Die Zellsuche beim Malen läuft im Hauptthread über ein Raster aus Eimern.

**Spectre:** `Tile(1,1)` hat die Form des Spectre, die Kachelung bleibt aber die des Hats und enthält gespiegelte Kacheln. Das Panel weist darauf hin.

### Statuserkennung

Eine periodische Population ist für jeden stabilen Zustand notwendig und billig zu prüfen, aber kein Beweis. Sie dient als Vorfilter, erst danach laufen die teuren Prüfungen.

| Status | Nachweis | Art |
|---|---|---|
| Empty, Still | keine Zelle hat sich geändert | exakt |
| Oscillating | Hash des gesamten Felds kehrt wieder, liefert die echte Periode | exakt bis auf Hash-Kollisionen |
| Drifting | Objekte wiederholen sich bis auf Verschiebung, und Objekte verschiedener Geschwindigkeit können sich nie mehr treffen | Beweis, über zwei Perioden bestätigt |
| Growing | Population wächst periodisch um einen festen Wert | Heuristik |

Zum Beweis bei „Drifting“: Einfluss breitet sich pro Generation höchstens um die Reichweite der Nachbarschaft aus. Liegen zwei Objekte auf einer Achse weiter als `2 · Reichweite · (Periode + 1)` auseinander und nähern sich dort nicht an, stören sie sich nie. Kleinere Abstände führen nicht zum falschen Ergebnis, die Erkennung kommt dann nur später.

Der Kostenaufschlag lag im Benchmark bei rund 3–5 %.

Auf dem Torus kommt alles zurück, was sich bewegt, also gibt es dort kein „Drifting“. Glider ergeben stattdessen eine lange Oszillation des ganzen Felds, ihre Periode beträgt 4·kgV(Breite, Höhe). Für so lange Perioden vergleicht die Erkennung den Hash nach Brents Verfahren weiter: Findet sie innerhalb der Grenze nichts, setzt sie einen neuen Referenzpunkt und verdoppelt die Grenze. Gemessen wurden 4.096 Generationen auf 1024×1024 und 9.240 auf 70×66, Letzteres ist fast 13-mal so lang wie die Grenze der Ebene. Die Population muss dafür weiterhin mit einer Periode bis 720 wiederkehren, weil sie der Vorfilter bleibt.

Auf dem Hat meldet die Erkennung nie „Drifting“. Der Beweis oben braucht eine Verschiebung, die das Feld auf sich selbst abbildet, und die gibt es auf einer aperiodischen Kachelung nicht. Glider gibt es dort trotzdem, sie laufen in der Anzeige als „Evolving“.

### Glider und Kompassrichtungen

**Korrektur:** Eine frühere Fassung dieses Projekts behauptete in Oberfläche und Dokumentation, auf dem Hat könne sich nichts fortbewegen. Das war falsch. Unmöglich ist nur die exakte Verschiebung. Ein Muster kann sich aber Schritt für Schritt auf Kacheln neu bilden, die es noch nie besucht hat.

**Quelle:** James Edmond, „Gliders on Aperiodic Monotilings: Cellular Automata on the Hat and Spectre“, [arXiv:2609.22579](https://arxiv.org/abs/2609.22579) (Preprint), mit dem Repository [monotile-ca](https://github.com/jamesedmond/monotile-ca) (MIT/Apache-2.0, Hinweis in `public/THIRD_PARTY_NOTICES.txt`). Laut Paper reichen life-artige Regeln mit Kanten-Nachbarn dafür nicht: Es braucht die Ecken-Nachbarschaft und Regeln, bei denen Nachbarn auch die übrigen Zustände sehen.

**Engine:** `HatWorld` kennt deshalb zusätzlich Priority Tables. Die erste Zeile, deren Eigenzustand und Nachbarbedingungen passen, bestimmt den nächsten Zustand, ohne Treffer folgt Zustand 0. Zellbytes sind zugleich Texturwerte (0, 255, 150, 90), so schattiert der Renderer die Zustände ohne eigene Logik. Die Tabelle von Glider A ist unverändert übernommen, inklusive wirkungsloser Zeilen. Das Rule-Panel zeigt nur die wirksamen Zeilen: `effectiveRows` in `src/patterns/tableRows.ts` lässt verdeckte Zeilen weg, für Glider A bleiben 5 von 8. Einmalig erschöpfend geprüft (alle Eigenzustände, Nachbarzahlen bis 12), aber nicht Teil von `npm test`.

**Platzieren:** Die Kachel-Adressen des Papers gelten für dessen eigenen Generator und lassen sich nicht übertragen. Das ist auch nicht nötig: Laut Paper zünden 89 % von 188 Startpaaren. Ein Klick setzt Zustand 1 auf die Kachel und Zustand 2 auf eine Kanten-Nachbarin.

**Unabhängige Bestätigung** auf der eigenen Kachelung (`scripts/check-hat-glider.mjs`):

| | Paper | Hier gemessen |
|---|---|---|
| Richtung modulo 60° | 45,523° | 45,52° |
| Tempo pro Generation | 2,140 (89,867 u in 42 Generationen, Wert für den Korridor, gleiche Einheit angenommen) | 2,141 |
| Population | höchstens 16 | höchstens 15 |
| Zündung | 89 % von 188 Paaren | 11 von 11 (kleine Stichprobe) |

Gemessen über 5.000 Generationen. Tempo und Richtung sind nur für 4 Läufe mit genau einem Glider auswertbar. Bei den übrigen Läufen fliegen zwei, deren gemeinsamer Schwerpunkt nichts aussagt.

**Sechsecke:** Ein Fund auf B36/S234 passt dazu. Zufallsfelder bilden dort einen scharfen, fast gleichwinkligen sechseckigen Umriss, der ab etwa Generation 300 nicht mehr wächst, während es innen weiterarbeitet. Gemessen wurden die Kantenrichtungen 45–46° modulo 60°, bei verschiedenen Startwerten, Größen und Orten. Das Paper erklärt die Richtung: Die Graphmetrik des Hats hat sechs schnelle Achsen bei 45,523°, entlang derer auch die Glider fliegen. Sie sind eine Eigenschaft der Kachelung und nicht der Regel. Das Paper nennt chaotische Wachstumsfronten dagegen nahezu kreisförmig, der Umriss hier wächst aber gar nicht. Warum die Regel gerade an diesen Kanten anhält, ist ungeklärt. Ein Beitrag aus dem HatLife-Umfeld nennt ähnliche sechseckig begrenzte Regionen mit der Regel `B3456S012345N2`. Den Beitrag kennen wir nur als Zitat, und `N2` steht vermutlich für eine größere Nachbarschaftsreichweite, die die Engine nicht unterstützt. Geprüft ist die Regel also nicht.

### Verwandte Projekte

- **[Aperiodic Life](https://bellissimogiorno.github.io/aperiodic-life/)** von Murdoch J. Gabbay: Webversion für Life auf Einstein-Hat, Penrose P3, Quadrat- und Hexagon-Gitter, mit kuratierten Konfigurationen und laufenden Mustern. Eine gute Referenz zum Vergleichen, Code wurde hier nicht übernommen.
- **HatLife** von Andrew Trevorrow (2023, Lua-Skript für Glu): enthält die Regel *Worms*, deren „Worms“ zwischen Reihen gespiegelter Hats laufen. Der [Forenthread](https://conwaylife.com/forums/viewtopic.php?p=161425) ließ sich automatisiert nicht lesen (Bot-Schutz), die Angaben stammen aus LifeWiki und aus Edmonds `GLIDERS.md`.
- **monotile-ca** von James Edmond: siehe oben, enthält auch Glider auf dem Spectre und auf Penrose.

### Regelvorschläge

Für die neuen Gitter stammen die Regeln aus Suchläufen über viele B/S-Kombinationen mit Zufallsfeldern, nicht aus der Literatur. Auswahlkriterium: Sie bleiben aktiv, ohne auszusterben oder das Feld zu fluten. Die Skripte dazu liegen nicht im Repository, und die Suchläufe nutzten nur je einen Startwert. Die Vorschläge sind also lebendig, aber nicht als „beste“ Regeln belegt.

Beim Hat ist B25/S3 Standard: rund 5 % Dichte und über 600 Generationen aktiv. Eine erste Wahl (B36/S234) sah zu dunkel aus, weil der Suchlauf die Dichte zunächst falsch gemessen hatte. B3/S23 endete im Test auf dem Hat in kleinen Oszillatoren.

### Oberfläche

Weiß, luftig, Haarlinien. Die Panels blenden beim Lauf aus und kommen bei Mausbewegung zurück. Bewusst keine Hintergrundunschärfe, weil sie über einer animierten Fläche viel Leistung kostet. Stattdessen fast deckendes Weiß mit feiner Kontur. Die Schrift ist Inter, lokal eingebunden. Die Oberflächensprache ist Englisch.

## Tests

```
npm test        # Engine-Tests, Musterbibliothek, Kachel-Geometrie
npm run bench   # Geschwindigkeit
```

Native Rust-Tests bräuchten einen C-Linker. Die Engine-Tests laufen deshalb unter WASI in Node (`scripts/wasi-run.mjs`), `npm test` installiert das Target `wasm32-wasip1` bei Bedarf selbst.

Letzter Stand: alle grün, mit einer Einschränkung: `npm test` brach am 8. November einmal mit einem Speicherzugriffsfehler (SIGSEGV) in Node ab, während die Engine-Tests liefen. Das geschah im ersten von gut 25 Läufen und kam danach nicht wieder, auch nicht bei unverändertem Code. Ein erneuter Lauf genügt dann. Die Ursache ist nicht geklärt, siehe auch `check-hat-glider.mjs` unten. Ein Fehler in der Engine selbst scheidet als direkte Ursache aus, weil WASM-Code aus seiner Sandbox heraus Node nicht zum Absturz bringen kann.

- **51 Engine-Tests:** unter anderem Vergleich mit naiven Referenzen auf allen Gittern, auch auf dem Torus mit Seiten, die kein Vielfaches von 64 sind, und mit B0, Überlappungsfreiheit und Passgenauigkeit der Hats, Flächenfaktor φ⁴ pro Ebene, langfristige Gültigkeit gemeldeter Zustände.
- **`check-patterns.mjs`:** Simuliert alle 24 Muster und prüft Periode, Verschiebung und Wachstum. R-Pentomino und Acorn erreichen ihre aus der Literatur bekannten Endpopulationen 116 und 633.
- **`check-tiles.mjs`:** Liest die Nachbarn geometrisch aus den Polygonen ab und vergleicht das Ergebnis einer davon gesteuerten naiven Simulation mit der Engine. Prüft außerdem für alle Vorlagen, dass die Kacheln an der Naht eines Torus passen.
- **`check-hat-glider.mjs`:** Fliegt Glider A von 11 Startkacheln und vergleicht Population, Richtung und Tempo mit dem Paper. Die langen Flüge laufen hier statt in den Engine-Tests, weil Node im WASI-Testaufbau bei solchen Läufen mit einem Speicherzugriffsfehler abbricht. Direkt als WASM läuft derselbe Code sauber, die Ursache ist nicht geklärt.

- **`check-share.mjs`:** Rundläufe für RLE und Links über alle Vorlagen, dazu rund 30 fehlerhafte oder böswillige Eingaben, unter anderem eine Deflate-Bombe.

Die Browsertests liefen mit Playwright und liegen nicht im Repository. Das gilt auch für den Torus. Er wurde in Headless-Chrome über das DevTools-Protokoll geprüft: alle Kachelfamilien, Malen über die Naht, Glider-Umlauf, B0, Wechsel zwischen den Feldern, 4096×4096. Beim Verschieben über die Naht stimmten die Bilder bis auf den Versatz pixelgenau überein. Links wurden dort ebenfalls geprüft: erzeugen, in einem frischen Tab öffnen, Bilder vergleichen. Auf Quadraten und Dreiecken war das pixelgenau, auf schrägen Hexagonen und dem Hat wichen bei gleicher Population höchstens 0,3 % der dunklen Pixel ab, vermutlich wegen der gerundeten Kameraposition.

## Bekannte Lücken

- **Nur im Headless-Chrome mit Software-Rendering getestet.** Eine echte GPU, Firefox, Safari und Touch-Gesten sind ungeprüft. Im Headless-Browser lag die Anzeige bei rund 17 Bildern pro Sekunde und damit etwa 300 ms hinterher. Auf echter Hardware ist das noch nicht gemessen.
- **Zell-Limit im Browser ungetestet.** Geprüft ist die Logik mit niedrigem Limit. Ein Lauf am vollen Limit stürzte in Node ab, die Ursache ist nicht abschließend geklärt. Ein Glider erzeugt rund 30 neue Zellen pro Generation, erreicht das Limit im Dauerlauf also nach einiger Zeit und endet dort.
- **Die Kamera folgt einem Glider nicht.** Bei hohem Tempo fliegt er schnell aus dem Bild.
- **Speicher wird nicht freigegeben.** Erzeugte Hat-Zellen bleiben bis zum Neuladen, auch wenn dort nichts mehr lebt.
- **Weit herausgezoomt** wird beim Hat ab einem Sichtradius von 700 Rastereinheiten nichts Neues erzeugt.
- **Auto-Pause läuft nach.** Bei Maximaltempo hält die Simulation einige Tausend Generationen zu spät an, weil die Oberfläche den Status nur mit der Bildrate erfährt. Am Ergebnis ändert das nichts, aber Glider sind dann schon aus dem Bild.
- **Formwechsel deutet Zellen um.** Beim Wechsel von Familie oder Nachbarschaft bleiben die Zellen stehen und bedeuten auf dem neuen Gitter etwas anderes.
- **Hat-Zellen im Link:** Von Hand gemalte Zellen und platzierte Glider auf dem Hat gehen nicht in einen Link, nur Einstellungen, Pattern-Tabelle und Zufallsfelder. Kachel-Ids hängen von der Erzeugungsreihenfolge ab, es bräuchte also Positionen. Die Seite weist darauf hin.
- **Link kennt die Generation nicht.** Der Zählerstand beginnt beim Öffnen wieder bei 0, und bei der Rückfallstufe „Start des Laufs“ ist der Fortschritt verloren.
- **RLE** nur für Vierecke mit 8 Nachbarn. Andere Programme würden Muster auf anderen Gittern ohnehin anders deuten.
- **Musterbibliothek** nur für Vierecke mit 8 Nachbarn, dazu ein Glider auf dem Hat.
- **Perioden über 720 Generationen** erkennt die Statusanzeige auf der Ebene und dem Hat nicht. Auf dem Torus gilt die Grenze nur noch für die Periode der Population.
- **Torus, weit herausgezoomt:** Am Rand einer Periode kann ein Texel etwas zu hell ausfallen, wenn die Seite kein Vielfaches der Texelgröße ist.
- **Torus, große Felder:** 4096×4096 erreichte im Headless-Browser rund 130 Generationen pro Sekunde, die Engine allein in Node 156–183. Auf echter Hardware ist das noch nicht gemessen.
- **Kein automatischer WASM-Neubau.** Nach Änderungen an der Engine: `npm run wasm`, dann die Seite neu laden.

## Nächste Schritte

1. **Torus als 3D-Ansicht (optional).** Ein drehbarer Donut, zunächst nur zum Anschauen, gemalt wird in 2D. Die Nachschlagetextur lässt sich auch auf einer gekrümmten Fläche nutzen. Die Zellen werden dabei verzerrt, weil ein flacher Torus nicht verzerrungsfrei in den 3D-Raum passt. Die Entscheidung dafür oder dagegen steht noch aus.
2. **Pentagons.** Reguläre Fünfecke kacheln die Ebene nicht, es braucht also einen der kachelnden Typen, etwa die Kairo-Kachelung als Startpunkt.
   - Eine Periode enthält vier Fünfecke. Der Renderer erlaubt schon heute 4 Polygone pro Periode mit zusammen 24 Ecken, das passt.
   - Die Engine unterscheidet bisher nur zwei Zellklassen über die Parität. Die Kairo-Kachelung braucht vermutlich vier (x und y modulo 2), `advance_general` muss also verallgemeinert werden.
   - Vor dem Bau klären: Welche Typen sollen verformbar sein, und wie viele Nachbarn ergeben sich? Das Prüfskript `check-tiles.mjs` kann die Tabellen wie bei den anderen Formen gegen die Geometrie absichern.
   - Für die Regeln ist wieder ein Suchlauf nötig.
3. **Deployment** auf den eigenen Ubuntu-Server bei Hetzner (CX22). Die Seite ist statisch, der Server muss `.wasm` als `application/wasm` ausliefern. COOP/COEP-Header werden erst nötig, falls später WASM-Threads dazukommen.
4. **Mehr Hat-Muster und Regeln.** Glider C (ein Dreifach-Glider) aus monotile-ca lässt sich wie Glider A übernehmen. *Worms* von Trevorrow und die Regel von Gabbay nur nach Klärung der Rechte, monotile-ca nutzt sie mit deren Erlaubnis. Dazu eine Kamera, die einem Muster folgt, und die in HatLife übliche größere Nachbarschaftsreichweite (`N2`).
5. **Test auf echter Hardware.** GPU, Firefox, Safari und Touch, wie unter „Bekannte Lücken“ beschrieben.
6. **Mögliche Erweiterungen, noch nicht entschieden:**
   - echter chiraler Spectre mit eigener Substitution
   - Muster für Hexagone und Dreiecke suchen
   - Hashlife für extrem lange Läufe auf Quadraten
   - Speicher nicht mehr benötigter Hat-Stücke freigeben

## Organisatorisches

- **Git:** Lokales Repository ohne Remote. `src/sim/engine.wasm` und `engine/target` sind ausgeschlossen, der Build erzeugt sie.
- **Nextcloud:** `node_modules` und `dist` stehen in der globalen Ausschlussliste, `target` noch nicht. Ein Eintrag dort gilt für alle Ordner dieses Namens im Sync. `engine/target` sollte also vor dem nächsten Sync ausgeschlossen oder anders gelöst werden.
- **Lizenz:** Für das Projekt selbst noch nicht festgelegt.
