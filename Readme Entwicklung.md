# Game of Emergence: Entwicklung

Stand: 9. November 2026, nach Schritt 4 von ursprünglich 5, ergänzt um den Hat-Glider, den Torus, das Teilen per Link, den 3D-Donut, den Suchmodus, die Fünfecke und Penrose.

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
| `engine/src/graph_world.rs` | Simulation auf einem Graphen, stückweise Erzeugung, für alle Kachelungen ohne Gitter |
| `engine/src/hat_world.rs` | Der Hat als solche Kachelung |
| `engine/src/penrose.rs` | Penrose-Rauten als solche Kachelung, exakte Koordinaten |
| `engine/src/lib.rs` | C-ABI-Exporte für den Worker |
| `engine/src/search.rs`, `engine/src/bin/search.rs` | Suchmodus für travelling patterns: Kern und Kommandozeile |
| `src/sim/worker.ts`, `protocol.ts` | Simulationsschleife, Nachrichten zwischen Stage und Worker |
| `src/tiles/tiles.ts` | Periodische Kachelungen, Vorlagen, Regelvorschläge |
| `src/tiles/pentagons.json`, `engine/src/pentagons.rs` | Die 15 Fünfeck-Kachelungen und ihre Nachbartabellen, beide erzeugt |
| `src/render/renderer.ts` | Periodisches Feld, 3D-Donut und Nachglühen |
| `src/render/GraphScene.ts`, `HatScene.ts`, `PenroseScene.ts` | Kacheln ohne Gitter als Instanzen, Zellsuche |
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

### Nachtrag: 3D-Donut

Der Torus als drehbarer Ring, nur zum Anschauen, siehe „3D-Donut“.

### Nachtrag: Suchmodus

Kommandozeilen-Suche nach Glidern mit reinen B/S-Regeln auf dem Hat, dazu Regeln auf mehreren Ringen und Hat-Zellen im Link, siehe „Suchmodus“.

### Nachtrag: Fünfecke

Alle 15 bekannten Typen konvexer Fünfecke als feste Kacheln, siehe „Fünfecke“.

### Nachtrag: Penrose

Die Rauten von Penrose (P3) als zweite aperiodische Kachelung, siehe „Penrose“.

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
| WASM, Fünfeck Typ 4 und Typ 15 | 1,0 Mrd. (zuletzt gemessen, die Zeilen darüber stammen aus älteren Läufen) |
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
| Fünfeck, 15 Typen | 5 bis 8 | Offsets hängen von x und y modulo der Periode ab, Reichweite 1 oder 2 Spalten, siehe „Fünfecke“ |

Mathematische Grundlage der Verformung:

- Jedes einfache Viereck kachelt die Ebene, wenn jede zweite Kopie um 180° gedreht wird. Die Nachbarschaft bleibt die des Quadrats, darum laufen alle Muster auf verformten Vierecken weiter.
- Jedes Dreieck ergibt mit seiner Kopie ein Parallelogramm.
- Punktsymmetrische Hexagone kacheln allein durch Verschiebung. Andere kachelnde Hexagon-Typen fehlen deshalb bewusst.

Kacheln werden auf Fläche 1 normiert, damit das Bild beim Verformen gleich groß bleibt.

### Fünfecke

**15 Typen, nicht verformbar.** So viele Typen konvexer Fünfecke sind bekannt. Rao hat 2017 einen Beweis vorgelegt, dass die Liste vollständig ist. Laut Wikipedia hat Thomas Hales die erste Hälfte unabhängig geprüft, und das Peer-Review war dort noch nicht abgeschlossen (Stand der Angabe: Dezember 2017). „Vollständig“ gilt hier also mit diesem Vorbehalt. Viele Typen haben freie Parameter, jede Vorlage zeigt nur ein Beispiel. Formeln wie beim Viereck gibt es nicht für alle, darum sind die Kacheln fest.

**Woher die Kachelungen kommen:** Die Literatur nennt Bedingungen an Seiten und Winkel, die verwendete Quelle aber nicht die Lage der Kacheln. Darum entstehen sie in drei Schritten, deren Ergebnis im Repository liegt:

```
node scripts/pentagon-tilings.mjs raw.json   # ein Fünfeck je Typ, dann Backtracking
node scripts/pentagon-tables.mjs raw.json    # Lage auf dem Gitter, Nachbartabellen
npm run wasm && node scripts/pentagon-rules.mjs   # Regelvorschläge
```

`pentagon-tilings.mjs` löst die Bedingungen numerisch und füllt dann eine Fläche, nächstgelegene Lücke zuerst. Es prüft jede Lücke darauf, ob Fünfeckwinkel sie überhaupt füllen können, und springt weit zurück, wenn eine gefüllte Fläche nicht periodisch ist, denn am Rand gibt es viele Varianten und keine davon repariert den Kern. Das Ergebnis wird danach ohne Hilfe der Suche geprüft: Gitterkopien einer Periode dürfen sich nicht überlappen und müssen die Gitterzelle genau füllen. Typ 3 sucht nichts, sondern baut sich aus Dreifach-Drehungen um A und D. Der Lauf ist deterministisch, die Zeitgrenze zählt aber Wanduhr. Bei Typ 13 fand sich erst bei A = 105° und größerem Ausschnitt eine Lösung, die Startwerte in `TYPES` sind entsprechend gesetzt.

**Bestätigung:** Für die Typen 1–13 stimmt die gefundene Zahl der Fünfecke pro Periode mit der von Wikipedia überein, soweit sie sich dort einem Typ zuordnen lässt. Die Zahlen 6 und 12 für die Typen 14 und 15 konnte die Quellenprüfung nicht im Original lesen, sie entsprechen aber dem, was der Löser unabhängig findet. Die Winkel von Typ 14 ergeben sich aus den Bedingungen zu 90°, 145,34°, 69,32°, 124,66° und 110,68° und stimmen mit dem Beispiel der Quelle überein. Bei welchen Typen weitere Kachelungen existieren, wurde nicht untersucht: Gezeigt wird die erste mit der kleinsten bekannten Periode.

**Nachbarn:** Viele Typen liegen nicht Kante an Kante, gleiche Ecken reichen zum Erkennen also nicht. Kante heißt hier: gemeinsames Stück positiver Länge. Ecke heißt: Berührung in einem Punkt, auch wenn dort eine Ecke auf einer fremden Kante liegt. Bei den Typen 1, 2, 3 und 12 kommt Letzteres nie allein vor, sie haben nur eine Nachbarschaft. Bei den Typen 10, 11, 13, 14 und 15 haben nicht alle Fünfecke gleich viele Nachbarn, das Panel zeigt dann einen Bereich und das Rule-Panel geht bis zum Höchstwert.

**Engine:** `advance_general` kannte nur zwei Zellarten über die Parität von x + y. Hier gibt es bis zu 12 davon, darum hat `advance_classes` eine Tabelle je Rest von x und y. Eine Chunk-Grenze fällt nicht immer auf eine ganze Periode, weil 64 kein Vielfaches von 3 ist, also richtet sich der Kern nach der Lage des Chunks. Die Perioden sind Rechtecke von 1 bis 4 Spalten und 1 bis 6 Zeilen. `pentagon-tables.mjs` legt die Zellen so, dass kein Nachbar weiter als 2 Spalten und 1 Zeile entfernt ist. Das genügt Chunks und Geisterzellen ohne Änderung. Eine Zellposition bedeutet für die Simulation sonst nichts. Der Typ 4 landet dabei auf einem 2×2-Gitter mit 7 der 8 Moore-Nachbarn.

**Torus:** Seiten müssen ein Vielfaches der Periode und gerade sein. `fitTorus` rastet sie auf den nächsten passenden Wert ein, aus 128×128 wird bei Typ 15 also 128×126. Die eingestellte Größe bleibt erhalten und gilt wieder, sobald die Kachel wechselt.

**Status:** Ein Muster gilt nur als dasselbe, wenn es sich um ganze Perioden verschiebt (`keeps_shape`). Alles andere bleibt „Evolving“, auch wenn es wandert.

**Regelvorschläge:** Suchlauf über Geburt mit 1 oder 2 Werten aus 2–6 und Überleben mit bis zu 3 Werten aus den ersten 6, auf einem Torus 72×72 mit 3 Startwerten. Eine Regel kommt in die Liste, wenn sie weder ausstirbt noch das Feld flutet und ihre Population in den letzten 80 von 360 Generationen in mindestens 15 verschiedenen Werten schwankt. Regeln ohne Überleben stehen hinten, weil ein Feld dort nur Rauschen zeigt. Die Vorschläge sind damit auf dem Torus lebendig, aber nicht als beste belegt. Auf der Ebene halten sie oft nicht, siehe „Bekannte Lücken“.

### Penrose

**Gemeinsamer Kern:** Die Simulation auf dem Graphen ist von der Kachelung getrennt. `CellWorld<T: Tiling>` in `graph_world.rs` kennt Zustände, Nachbarn, Regeln, Tabellen und stückweise Erzeugung. `HatWorld` ist `CellWorld<HatTiling>`, das Verhalten des Hats blieb dabei gleich, und die Tests zu Hat und Suchmodus liefen beim Umbau unverändert durch. Die C-Funktionen heißen weiter `hat_*` und gehen über ein Enum an beide Welten.

**Konstruktion:** Robinson-Dreiecke, von einem Rad aus zehn Dreiecken durch 34 Teilungen bis zu Rauten der Kantenlänge 1. Die Regeln sind die verbreitete Teilung nach Robinson, zur Absicherung siehe unten. Eine Raute sind zwei gespiegelte Dreiecke, ihre vierte Ecke ist einfach B + C − A. Die fertigen Stücke umfassen vier Teilungen.

**Exakte Koordinaten:** Alle Kanten zeigen in Richtungen der zehnten Einheitswurzeln, jede Ecke ist also eine ganzzahlige Kombination von 1, z, z² und z³ mit z = e^(2πi/5). Durch den goldenen Schnitt teilen heißt mit z + z⁴ malnehmen, es bleibt also ganzzahlig, ohne Rundung wie beim Hat. Für die Ecke genügen 16 Bit je Anteil: Ecken einer Kachelung liegen in der anderen Einbettung dieser Zahlen in einem beschränkten Fenster, also müssten zwei Ecken mit gleichen Anteilen modulo 2¹⁶ weiter auseinanderliegen, als das Feld breit ist. Ein Test prüft das auch weit draußen.

**Zellen im Link:** Wie beim Hat fünf Zahlen, `turn + 16 * thick` und die vier Anteile. Der Hat nimmt andere Codes an als Penrose, `parseLink` prüft das je Kachelung.

**Absicherung:** Die Tests prüfen Eigenschaften, die die Konstruktion nicht direkt verlangt. Flächen füllen eine Scheibe von Radius 100 bis auf 1 %, dicke und dünne Rauten stehen im goldenen Schnitt, jede Raute hat 4 Kantennachbarn, und Ecken haben 3 bis 7 Kacheln. Mit Ecken sind es 7 bis 11 Nachbarn (gemessen bis Radius 380). Die Zahl passt in die 16 Bit einer Regel. Das Erzeugen bei Bedarf ändert nichts gegenüber einem von Anfang an erzeugten Feld.

**Rendering:** `PenroseScene` nutzt wie der Hat ganzzahlige Instanzen. Für die Genauigkeit gehen Positionen relativ zu einer Zelle nahe der Kamera in den Shader. Es muss eine echte Zelle sein, weil erst deren Anteile nahe an denen auf dem Bild liegen: Ein anderer Punkt gleicher Position kann weit entfernte Anteile haben, die sich dann in Floats auslöschen müssten. In Headless-Chrome war das Bild bei einer Position rund 1,1 Mio. Einheiten vom Ursprung scharf, und Klicks trafen die richtige Raute.

**Regelvorschläge:** `scripts/penrose-rules.mjs` prüft Geburt mit 1 oder 2 Werten aus 2–6 und Überleben mit 1 bis 3 Werten aus den ersten 7, je 800 Generationen auf 3 Zufallsfeldern. Eine Regel darf die Startpopulation höchstens verfünffachen und in der zweiten Hälfte des Laufs nicht mehr als um die Hälfte zulegen. Eine erste Fassung erlaubte das 40-Fache und schlug `B3/S124` vor, die das unbegrenzte Feld im Browser in rund 550 Generationen von rund 1.300 auf 16.000 Zellen brachte. Mit Ecken bleiben 6 Regeln, nur über Kanten 10. Die Vorschläge sind damit lebendig, aber nicht als beste belegt.

**Build:** `over_phi` darf nicht inline werden. Sonst hält die feste Schleife in `PenroseTiling::new` den Optimierer von rustc viele Minuten auf, in einem Lauf über eine Viertelstunde.

### Torus

Ein Torus ist ein eigenes `World`-Objekt neben der Ebene. Beide behalten beim Wechsel ihre Zellen und ihre Kamera. Regel und Nachbarschaft schickt die Oberfläche beim Wechsel neu an das aktive Feld.

**Geisterzellen statt neuer Kerne:** Vor jedem Schritt kopiert `fill_ghosts` die Randzellen auf die Stellen knapp hinter dem gegenüberliegenden Rand, zwei Spalten und eine Zeile, also so viel, wie die größte Nachbarschaft braucht. Die schnellen Kerne lesen sie wie gewöhnliche Nachbarn und bleiben unverändert. Danach wischt eine Maske die Geister aus Chunks, die über den Rand hinausragen. Darum gehen auch Größen, die kein Vielfaches von 64 sind. Alle Chunks existieren von Anfang an, nichts wird erzeugt oder eingesammelt. Das Überspringen ruhiger Chunks bleibt erhalten und läuft über die Position statt über die Links.

**Nur gerade Seiten:** Vierecke und Dreiecke brauchen sie, damit die Kacheln an der Naht passen (`check-tiles.mjs` prüft das für alle Vorlagen). Hexagone kämen auch mit ungeraden Seiten aus, die Engine verlangt aber überall gerade Zahlen. Das hält die Regel einfach und spart Sonderfälle: Zwei Geisterspalten liegen so immer im selben Wort, und die letzte Chunk-Spalte enthält mindestens zwei Zellen, sodass Einfluss nie einen Chunk überspringt.

**B0:** Unter B0 ist ein leeres Feld nicht in Ruhe. Darum markiert `clear` auf dem Torus alle Chunks als geändert, und „Empty“ ist dort kein Endzustand.

**Rendering:** Der Worker liefert höchstens eine Periode, den Rest wiederholt der Shader durch Umwickeln der Zellkoordinaten. Die Tapete kostet also nichts zusätzlich, unabhängig vom Zoom. Die Grenzlinie ist eine Gerade in Gitterkoordinaten. Bei Dreiecken und schrägen Formen läuft sie deshalb durch Zellen, statt ihren Kanten zu folgen. Das ist nur Anzeige, die Simulation betrifft es nicht.

### 3D-Donut

Der Donut ist eine reine Ansicht. Solange er aktiv ist, sind Werkzeuge, Einstellungen und ihre Tasten aus. Play, Pause, Einzelschritt und Tempo bleiben. Der Abschnitt mit den Einstellungen wird nur ausgeblendet, damit erzeugte Links und eingefügter Text erhalten bleiben.

**Gemeinsamer Shader:** Die Zellfarbe (`fieldColor`) nutzen flache Ansicht und Donut gemeinsam, also auch Nachschlagetextur, Nachglühen und Naht. Nur die Frage, welche Feldposition ein Pixel zeigt, beantworten beide verschieden. Auf dem Donut kommt die Pixelgröße für die Gitterlinien aus den Ableitungen im Shader, weil sie sich über die Fläche ändert. Das Netz aus 320×160 Vierecken entsteht allein aus der Vertex-Nummer.

**Daten:** Der Donut zeigt immer das ganze Feld, also holt `Stage` eine volle Periode vom Worker. Die Stufe (`level`) folgt aus demselben Texel-Budget wie in der flachen Ansicht.

**Proportionen:** Die längere Feldseite läuft um den Ring. Das Rohr folgt dem Seitenverhältnis, begrenzt auf 0,18 bis 0,45 des Ringradius, weil es sonst das Loch schließt oder wie Draht aussieht. Ein quadratisches Feld ergibt also einen dicken Ring.

**Keine Spiegelung:** Von außen betrachtet laufen Ring und Rohr nach rechts und oben, das Feld aber nach rechts und unten. Die Abbildung in `DONUT` berücksichtigt das in beiden Fällen, also auch wenn Breite und Höhe die Rollen tauschen. Geprüft ist das im Browser mit einer L-Form.

**Verzerrung ist unvermeidlich:** Ein flacher Torus passt nicht verzerrungsfrei in den 3D-Raum. Außen werden Zellen gestreckt, innen gestaucht. Schräge Kacheln wie Hexagone erscheinen zusätzlich gestaucht und geschert, weil das schräge Feld auf die rechtwinkligen Richtungen von Ring und Rohr abgebildet wird. Die Topologie stimmt trotzdem: Nachbarn bleiben Nachbarn.

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

**Hat-Zellen:** Sie stehen als `h=` im Link, je 5 Zahlen: Drehung plus 8 falls gespiegelt, dann beide Anteile der Position. Das ist genau, was `hat_cells` schon exportiert. Kachel-Ids wären abhängig von der Erzeugungsreihenfolge. Regeln auf mehreren Ringen stehen wie andere in `r=`, zum Beispiel `r=R2B5-7S3-8`, gelten aber nur für den Hat mit Kanten und Ecken. Alte Links funktionieren unverändert. Zustände einer Pattern-Tabelle passen nicht in diese Form, solche Links enthalten weiterhin nur die Einstellungen.

**Links sind fremde Eingaben.** `parseLink` prüft jeden Wert und meldet Verworfenes in der Oberfläche, der Rest des Links gilt weiter. Begrenzt sind Größen, Koordinaten, Zellzahl (2 Millionen) und die entpackte Länge (16 MB), damit ein kurzer Link nicht den Tab lahmlegt. Unbekannte Versionen werden ganz ignoriert.

**RLE-Import:** Kommentare und Kopfzeile sind optional, die alte Form `23/3` und der Zusatz `:T100,100` werden verstanden. Bringt das Muster eine Regel mit, wird sie aktiv. Mehrzustandsmuster und nicht-totalistische Regeln werden mit einer Meldung abgelehnt. Der Export schreibt die Torus-Größe so in die Regel, wie Golly es tut.

**Oberfläche:** Link und RLE liegen gemeinsam im Abschnitt „Share & export“, auch der Import, weil er sich mit dem Export ein Textfeld teilt. Unter „Patterns“ hätte den Export niemand gesucht, dort steht nur noch ein Verweis auf den Import.

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

### Suchmodus

`engine/src/bin/search.rs` sucht auf derselben Engine wie die Seite nach Mustern, die über den Hat wandern. Bedienung steht in der Readme.

**Reihenfolge:** Jede Regel wird zuerst gescreent, das ergibt den Katalog `catalog.jsonl`. Nur stabile und chaotische Regeln werden danach durchsucht, mit Polyhats, Korridor-Ketten, Soups und Mutation. Das Budget `--max-seeds-per-rule` gilt je Familie, sonst bekämen die Korridore bei genau den interessanten Regeln nichts mehr ab.

**Ringe** sind die Graphdistanz über Kanten und Ecken, unabhängig von der Nachbarschaft der Regel. Gemessen hat ein Ring 4,1 bis 4,3 Einheiten, nahe am Start ab 3,6. Während des Laufs prüft die Suche billig die euklidische Distanz, Treffer werden dann per Breitensuche exakt gemessen. Die Stoppdistanz richtet sich nach dem breitesten Ring. Mit 4,1 hielten Flüge entlang der schnellen Achsen bei 290 Ringen an, und die Suche hätte nie einen Kandidaten gemeldet. Gefunden hat das die Positivkontrolle.

**Positivkontrolle:** `search control` fliegt Glider A durch dieselben Prüfungen. Einen 2-Zustands-Glider gibt es ja nicht, ohne Kontrolle hieße „nichts gefunden“ also gar nichts. Ein Test schickt den Flug auch durch die Bewertung bis zum Fund. Gemessen über 300 Ringe: Tempo 2,13 pro Generation, Paper 2,14. Vier von acht Starts bestehen alle Patches, die übrigen schicken zwei Glider los.

**Abweichungen von der ursprünglichen Aufgabe:**

| Vorgabe | Umgesetzt | Warum |
|---|---|---|
| Zell-IDs im Link | Positionsschlüssel, 5 Zahlen je Zelle | IDs hängen von der Erzeugungsreihenfolge ab |
| Population höchstens 3× Start | zusätzlich mindestens 24, höchstens 60 (`--pop-floor`) | Glider A startet mit 2 Zellen und erreicht 15–17, 3× Start hätte ihn abgelehnt |
| Fläche wächst linear = explosiv | belegte Fläche, in Feldern von 16 Einheiten | Zwei auseinanderfliegende Glider haben auch eine wachsende Bounding-Box |
| Eine Soup im Screening | drei, explosiv genügt bei einer (`--screen-soups`) | eine einzelne Soup ist Zufall |
| Patches 100/200/400 mit Rand | Feld erzeugt, Abbruch 3 bis 5 Ringe vor dem Rand | ohne Rand wäre der Test leer, das Feld wächst sonst bei Bedarf |
| eine Datei pro Fund | alles Bewegte, sonst der erste Fund je Regel und Periode (`--files all`) | Hunderttausende Stillleben helfen niemandem |
| Prozesse | Threads, je eine eigene Welt | gleiches Ergebnis, kein Serialisieren. Absturz einer Regel beendet den Lauf, `--resume` entfällt, er setzt immer fort |
| `--tiling spectre` | gleiche Kachelung in Spectre-Form | Es gibt nur die Kachelung des Hats, Ergebnisse sind identisch. Der chirale Spectre fehlt noch |

**Grenzen des Screenings:** Es wirft Regeln wie Seeds (B2/S) weg, die explodieren und trotzdem Glider haben können. `--keep explosive,stable,chaotic` nimmt sie mit. Die Populationsschranke beendet solche Läufe aber früh, das Ausschneiden hilft dort nur begrenzt.

**Herausgeschnittene Muster:** Ein Glider, der Trümmer hinterlässt, fällt auf dem Gesamtfeld durch jeden Test. Weit vom Start entfernte Teile werden deshalb einzeln noch einmal geprüft. In `results.jsonl` steht dann `derived_from`.

**Späte Entstehung:** Die Kriterien gelten für das fliegende Muster, die normale Suche wendet sie aber schon ab dem Start an: Nach 16 Generationen (Soups 200) sind höchstens 24 bis 60 Zellen erlaubt, und nach 300 Generationen ohne Ausdehnung ist Schluss. Das R-Pentomino wäre so in Life vermutlich früh gestoppt worden. Die Familien `late` und `latesoup` trennen deshalb Entstehung und Flug. Ein Seed darf lange brennen, und der Lauf endet, sobald eine Zelle 200 Einheiten entfernt ist. Dann werden entfernte Teile ausgeschnitten und einzeln streng geprüft. Ein großes Muster kann so nie selbst als Traveller gelten, nur ein kleines. In `derived_after_generations` steht, wann das Teil entstand. In einer Stichprobe von 150 Regeln entstand so bei `B3456/S2347` erst bei Generation 1.101 ein Oszillator der Periode 179, bei bis zu 60 Zellen. Glider fand sie dort nicht. Die Kontrolle mit zwei Glidern A prüft das Ausschneiden, aber nicht den Weiterflug, denn ausgeschnittene Teile tragen nur 2 Zustände.

**Verschiedene Formen:** Ein Eintrag in `results.jsonl` steht für alle Funde einer Regel mit gleicher Periode und Population, er ist also weder ein Einzelfund noch eine eigene Form. `search unique` lässt die Einträge erneut laufen und vergleicht die Phasen mit der kleinsten Population über die 12 Isometrien des Gitters. Die Zahl ist eine Untergrenze, weil die Suche Formen gleicher Periode und Population je Regel schon zusammengefasst hat. Formen, die sich nur in den Kacheln ringsum unterscheiden, zählen einmal.

**Anti-Hat-Linien:** Das Korridormodell ist aus der Kachelung gemessen, nicht aus der Literatur. Gleich orientierte gespiegelte Hats liegen auf Geraden entlang 46°, 106° und 166°, im Abstand 22 oder 34 Einheiten. Das sind die schnellen Achsen aus Edmonds Paper. Die Linien sind bis zu 167 Hats lang (begrenzt durch die untersuchte Fläche) und sehr gerade, im Median 0,42 Einheiten Abweichung. Parallele Nachbarn liegen meist rund 10 Einheiten auseinander. Der erste Versuch, Linien über den kürzesten Abstand von 10 Einheiten zu finden, ergab nur Paare. Trevorrows Vermutung selbst haben wir weiterhin nicht im Original gelesen.

**Reproduzierbar:** Ergebnisse hängen nur von Positionen und vom Startwert ab, nicht von Zell-IDs. `results.jsonl` ist byte-gleich bei 1 und 16 Workern, und drei gemischte Batches ergeben byte-gleich den Gesamtlauf. Der Fingerabdruck in `fingerprint.txt` verhindert, dass Läufe mit anderen Einstellungen gemischt werden. Gezählte Vorkommen (`occurrences`) sind Untergrenzen, weil Seeds abbrechen, sobald sie einen schon gesehenen Zustand erreichen.

**Build ohne C-Compiler:** musl und `rust-lld`, siehe `engine/.cargo/config.toml`. Das Ergebnis ist ein statisches Binary. Das `malloc` von musl hat eine globale Sperre: 16 Worker brauchten fast 9-mal so viel CPU pro Regel wie einer. Darum hat das Binary einen eigenen kleinen Allocator mit Freilisten pro Thread, danach waren es 1,2-mal so viel, und ein Kern wurde doppelt so schnell.

**Regeln auf mehreren Ringen** (`R2B5-7S3-8`) zählen alle Zellen innerhalb von r Ringen über Kanten und Ecken, ohne die Zelle selbst. Bei r = 2 sind das 18 bis 22 Zellen, bei r = 3 sind es 38 bis 45. Die Listen entstehen, wenn eine Zelle erstmals lebt, und die Umgebung wird entsprechend weiter erzeugt (8 Einheiten je Ring). Ein Test vergleicht das Erzeugen bei Bedarf mit einem von Anfang an erzeugten Feld.

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
npm test        # Engine-Tests, Musterbibliothek, Kachel-Geometrie, Hat-Glider, Links und RLE
npm run bench   # Geschwindigkeit
```

Die Engine-Tests laufen nativ, `npm test` installiert das Target `x86_64-unknown-linux-musl` bei Bedarf selbst. Es braucht keinen C-Compiler, siehe `engine/.cargo/config.toml`.

**Warum nicht mehr unter WASI:** Bis zum Suchmodus liefen sie in Node über WASI. Dort starb der Node-Prozess reproduzierbar mit einem Speicherzugriffsfehler (Exit-Code 139). Einzeln gestartet traf es genau die Tests, die große Felder erzeugen, in der ganzen Suite aber auch schon `neighbour_counts_stay_small`, der gar keine Welt anlegt. Große Felder sind also ein Auslöser, aber keine vollständige Erklärung. Die V8-Schalter `--no-wasm-tier-up`, `--no-wasm-dynamic-tiering`, `--no-liftoff`, `--liftoff-only`, `--stack-size` und `--max-semi-space-size` helfen nicht, die eigentliche Ursache ist nicht geklärt. Der alte Stand lief unter gleicher Last durch, er erzeugte in den Tests aber auch keine so großen Felder. Die Engine betrifft das nicht: Als reines WASM, wie auf der Seite, läuft derselbe Code sauber, zuletzt mit 538.000 Zellen und 389 MB. `check-hat-glider.mjs` prüft das jetzt mit. Tests auf 64 Bit sagen allerdings nichts über Fehler, die nur auf 32 Bit auftreten. Die Skripte unten decken das nur teilweise ab.

Letzter Stand: alle grün.

- **93 Engine-Tests:** unter anderem Vergleich mit naiven Referenzen auf allen Gittern, alle 30 Fünfeck-Tabellen eingeschlossen, auch auf dem Torus mit Seiten, die kein Vielfaches von 64 sind, und mit B0, Überlappungsfreiheit und Passgenauigkeit der Hats, Flächenfaktor φ⁴ pro Ebene, langfristige Gültigkeit gemeldeter Zustände.
- **`check-patterns.mjs`:** Simuliert alle 24 Muster und prüft Periode, Verschiebung und Wachstum. R-Pentomino und Acorn erreichen ihre aus der Literatur bekannten Endpopulationen 116 und 633.
- **`check-tiles.mjs`:** Liest die Nachbarn geometrisch aus den Polygonen ab und vergleicht das Ergebnis einer davon gesteuerten naiven Simulation mit der Engine. Prüft außerdem für alle Vorlagen, dass die Kacheln an der Naht eines Torus passen. Bei den Fünfecken prüft es dazu die Bedingungen des Typs, Konvexität, Lücken, Überlappungen und ob alle Kacheln deckungsgleich sind.
- **`check-hat-glider.mjs`:** Fliegt Glider A von 11 Startkacheln und vergleicht Population, Richtung und Tempo mit dem Paper. Prüft außerdem als reines WASM ein großes Feld, Zellen nach Position und eine Regel auf 2 Ringen.
- **`search control`:** Nicht Teil von `npm test`. Prüft die Messung des Suchmodus am bekannten Glider, siehe „Suchmodus“.
- **`check-share.mjs`:** Rundläufe für RLE und Links über alle Vorlagen, dazu rund 30 fehlerhafte oder böswillige Eingaben, unter anderem eine Deflate-Bombe.

Die Browsertests liefen mit Playwright und liegen nicht im Repository. Der Suchmodus wurde in Headless-Chrome über das DevTools-Protokoll geprüft: Funde mit Kanten, Kanten+Ecken und 2 Ringen per Link geöffnet, Fund-Datei importiert, einen geladenen Fund erneut geteilt. Rule-Panel und Statusanzeige nennen dabei dieselbe Regel und Periode wie das CLI. Automatische Tests hat das nicht. Das gilt auch für den Torus. Er wurde in Headless-Chrome über das DevTools-Protokoll geprüft: alle Kachelfamilien, Malen über die Naht, Glider-Umlauf, B0, Wechsel zwischen den Feldern, 4096×4096. Beim Verschieben über die Naht stimmten die Bilder bis auf den Versatz pixelgenau überein. Links wurden dort ebenfalls geprüft: erzeugen, in einem frischen Tab öffnen, Bilder vergleichen. Auf Quadraten und Dreiecken war das pixelgenau, auf schrägen Hexagonen und dem Hat wichen bei gleicher Population höchstens 0,3 % der dunklen Pixel ab, vermutlich wegen der gerundeten Kameraposition. Auch der Donut wurde dort geprüft: Seitenverhältnisse, Spiegelung, Drehen, Zoomen, gesperrte Tasten, Hexagone und Dreiecke, 4096×4096. Automatische Tests hat er keine.

## Bekannte Lücken

- **Fünfecke, nah herangezoomt:** Eine Periode von Typ 15 enthält 12 Fünfecke auf derselben Nachschlagetextur von 512×512 Texeln, die Kanten sehen bei starker Vergrößerung deshalb treppig aus. Eine größere Textur kostet bei jedem Ziehen einer Ecke mehr, das wurde noch nicht gemessen.
- **Fünfecke, Torus:** Die Naht verläuft bei langen, schrägen Perioden diagonal durch die Kacheln. Das betrifft wie bei den anderen schrägen Formen nur die Anzeige.
- **Fünfecke, Regelvorschläge auf der Ebene:** Sie stammen aus einer Suche auf einem Torus 72×72 und halten auf dem unbegrenzten Feld oft nicht. Gemessen über 2.000 Generationen aus einem Zufallsfeld: Bei den Standardregeln mit Ecken (Typ 4, 6, 7, 8, 13) und nur über Kanten (Typ 8, 10, 13) wächst die Population stark oder explodiert, bei vielen anderen stirbt sie fast aus. Die Suche sollte auf der Ebene wiederholt werden, mit den Kriterien von `penrose-rules.mjs`.
- **Fünfecke, Formen:** Pro Typ gibt es nur ein festes Beispiel, kein Muster und kein RLE. Beim Typwechsel gilt wieder die erste vorgeschlagene Regel.

- **Nur im Headless-Chrome mit Software-Rendering getestet.** Eine echte GPU, Firefox, Safari und Touch-Gesten sind ungeprüft. Im Headless-Browser lag die Anzeige bei rund 17 Bildern pro Sekunde und damit etwa 300 ms hinterher. Auf echter Hardware ist das noch nicht gemessen.
- **Zell-Limit im Browser ungetestet.** Geprüft ist die Logik mit niedrigem Limit. Ein Lauf am vollen Limit stürzte in Node ab, die Ursache ist nicht abschließend geklärt. Ein Glider erzeugt rund 30 neue Zellen pro Generation, erreicht das Limit im Dauerlauf also nach einiger Zeit und endet dort.
- **Die Kamera folgt einem Glider nicht.** Bei hohem Tempo fliegt er schnell aus dem Bild.
- **Speicher wird nicht freigegeben.** Erzeugte Zellen von Hat und Penrose bleiben bis zum Neuladen, auch wenn dort nichts mehr lebt.
- **Weit herausgezoomt** wird beim Hat ab einem Sichtradius von 700 Rastereinheiten nichts Neues erzeugt, bei Penrose ab 250, weil Rauten kleiner sind.
- **Penrose:** Keine Muster, keine Glider, keine Regeln auf mehreren Ringen in der Oberfläche und kein Suchmodus. Drachen und Pfeile (P2) fehlen.
- **Auto-Pause läuft nach.** Bei Maximaltempo hält die Simulation einige Tausend Generationen zu spät an, weil die Oberfläche den Status nur mit der Bildrate erfährt. Am Ergebnis ändert das nichts, aber Glider sind dann schon aus dem Bild.
- **Formwechsel deutet Zellen um.** Beim Wechsel von Familie oder Nachbarschaft bleiben die Zellen stehen und bedeuten auf dem neuen Gitter etwas anderes.
- **Hat-Zellen im Link:** Platzierte Glider mit Zuständen gehen nicht in einen Link, nur Einstellungen und Pattern-Tabelle. Die Seite weist darauf hin.
- **Regeln auf mehreren Ringen** lassen sich nur per Link oder Fund-Datei öffnen, nicht im Rule-Panel einstellen.
- **Der Suchmodus** kennt nur die Hat-Kachelung, keinen chiralen Spectre und kein Penrose.
- **Link kennt die Generation nicht.** Der Zählerstand beginnt beim Öffnen wieder bei 0, und bei der Rückfallstufe „Start des Laufs“ ist der Fortschritt verloren.
- **RLE** nur für Vierecke mit 8 Nachbarn. Andere Programme würden Muster auf anderen Gittern ohnehin anders deuten.
- **Musterbibliothek** nur für Vierecke mit 8 Nachbarn, dazu ein Glider auf dem Hat.
- **Perioden über 720 Generationen** erkennt die Statusanzeige auf der Ebene und dem Hat nicht. Auf dem Torus gilt die Grenze nur noch für die Periode der Population.
- **Torus, weit herausgezoomt:** Am Rand einer Periode kann ein Texel etwas zu hell ausfallen, wenn die Seite kein Vielfaches der Texelgröße ist.
- **Torus, große Felder:** 4096×4096 erreichte im Headless-Browser rund 130 Generationen pro Sekunde, die Engine allein in Node 156–183. Auf echter Hardware ist das noch nicht gemessen.
- **Donut, große Felder:** Bei 4096×4096 lief die Simulation im Headless-Browser mit rund 37 Generationen pro Sekunde, in der flachen Ansicht mit rund 130. Wie viel davon am Software-Rendering liegt, ist auf echter Hardware noch zu messen. Bei sehr vielen Zellen pro Pixel flimmert das Bild zudem, weil einzelne Texel statt eines Mittelwerts gezeichnet werden.
- **Donut im Link:** Ein Link öffnet immer die flache Ansicht, weder der Donut noch sein Blickwinkel werden übertragen.
- **Kein automatischer WASM-Neubau.** Nach Änderungen an der Engine: `npm run wasm`, dann die Seite neu laden.

## Nächste Schritte

1. **Mehr Hat-Muster und Regeln.** Glider C (ein Dreifach-Glider) aus monotile-ca lässt sich wie Glider A übernehmen. *Worms* von Trevorrow und die Regel von Gabbay nur nach Klärung der Rechte, monotile-ca nutzt sie mit deren Erlaubnis. Dazu eine Kamera, die einem Muster folgt, und die in HatLife übliche größere Nachbarschaftsreichweite (`N2`).
2. **Suchmodus auswerten.** Der volle Lauf steht noch aus, siehe „Suchmodus“.
3. **Test auf echter Hardware.** GPU, Firefox, Safari und Touch, wie unter „Bekannte Lücken“ beschrieben.
4. **Deployment** auf den eigenen Ubuntu-Server bei Hetzner (CX22). Die Seite ist statisch, der Server muss `.wasm` als `application/wasm` ausliefern. COOP/COEP-Header werden erst nötig, falls später WASM-Threads dazukommen. Erst danach funktionieren geteilte Links auch für andere. Der Server sollte HTTPS nutzen, sonst steht das automatische Kopieren in die Zwischenablage nicht zur Verfügung.
5. **Mögliche Erweiterungen, noch nicht entschieden:**
   - Penrose weiter ausbauen: Glider aus monotile-ca übernehmen, sofern die Rechte passen, den Suchmodus darauf anwenden, Drachen und Pfeile (P2)
   - Fünfecke verformbar machen, wo sich ein Typ dafür eignet (bei Typ 4 ist die Form allein durch die Lage eines Eckpunkts bestimmt)
   - echter chiraler Spectre mit eigener Substitution
   - Muster für Hexagone und Dreiecke suchen
   - Hashlife für extrem lange Läufe auf Quadraten
   - Speicher nicht mehr benötigter Hat-Stücke freigeben

## Organisatorisches

- **Git:** Repository mit Remote `origin`. `src/sim/engine.wasm` und `engine/target` sind ausgeschlossen, der Build erzeugt sie.
- **Nextcloud:** `node_modules` und `dist` stehen in der globalen Ausschlussliste, `target` noch nicht. Ein Eintrag dort gilt für alle Ordner dieses Namens im Sync. `engine/target` sollte also vor dem nächsten Sync ausgeschlossen oder anders gelöst werden.
- **Lizenz:** Für das Projekt selbst noch nicht festgelegt.
