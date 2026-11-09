# Game of Emergence

Conways „Game of Life“ im Browser, auf einem unbegrenzten Feld und auf vielen Kachelformen: von verformbaren Vierecken über Dreiecke, Hexagone und alle 15 bekannten Fünfeck-Typen bis zu aperiodischen Kachelungen: dem „Hat“, einer der Einstein-Kacheln, und den Rauten von Penrose. Die Oberfläche ist weiß und minimalistisch, die Berechnung läuft in Rust/WebAssembly.

## Features

- **Unbegrenztes Feld:** Es wird nur gespeichert und berechnet, wo etwas passiert. Rund 3 Milliarden Zellen pro Sekunde auf Quadraten.
- **Torus:** Wahlweise ein Feld fester Größe, dessen Ränder sich berühren. Was hinausläuft, kommt von der anderen Seite zurück. Das gibt es für alle Kachelformen außer Einstein-Kacheln und Penrose-Rauten, die sich nicht wiederholen.
- **3D-Donut:** Der Torus lässt sich als drehbarer Ring betrachten, auch während die Simulation läuft. Gemalt und eingestellt wird in der flachen Ansicht.
- **Kachelformen:** Vierecke (Quadrat, Rechteck, Parallelogramm, Raute, Trapez, Drachen, Dart), Dreiecke, Hexagone, die 15 bekannten Typen konvexer Fünfecke, Einstein-Kacheln und Penrose-Rauten (P3).
- **Live verformbar:** Die Ecken einer Kachel lassen sich ziehen, auch während die Simulation läuft. Das gilt nicht für die Fünfecke, sie sind feste Beispiele ihres Typs. Die Einstein-Kacheln morphen stufenlos zwischen Comet, Turtle, Spectre-Form, Hat und Chevron.
- **Regeln frei einstellbar:** Geburt und Überleben pro Nachbarzahl, dazu passende Vorlagen je Kachelform. Die Nachbarschaft ist wählbar, also nur über Kanten oder auch über Ecken. Auf dem Torus ist auch Geburt bei 0 Nachbarn möglich.
- **Zeichnen:** Pinsel in 12 Größen, Radierer, Zufallsfeld mit einstellbarer Dichte.
- **Musterbibliothek:** 24 bekannte Muster, darunter Glider, Spaceships, Gosper Gun und Acorn, mit Vorschau am Mauszeiger. Sie gilt für Vierecke mit 8 Nachbarn. Dazu kommt ein Glider für die Hat-Kachelung, der eine erweiterte 4-Zustands-Regel mitbringt. Solange sie aktiv ist, erklärt das Rule-Panel sie Zeile für Zeile und blendet die B/S-Regler aus; ein Knopf führt zurück zur Grundregel.
- **Statuserkennung:** Zeigt an, ob ein Feld ausgestorben oder statisch ist, oszilliert, nur noch Spaceships ausschickt oder stetig wächst. Auf dem Torus nennt sie die volle Periode, etwa den Umlauf eines Gliders. Auf Wunsch pausiert die Simulation dann automatisch.
- **Suchmodus:** Kommandozeilen-Suche nach Glidern auf dem Hat, siehe unten.
- **Teilen per Link:** Ein Link enthält Kachelform, Regel, Ansicht und Zellen. Es gibt keinen Server, alles steckt im Link selbst. Bei sehr vielen Zellen sagt die Seite, was der Link stattdessen enthält.
- **RLE-Import und -Export:** Muster aus dem LifeWiki oder aus Golly einfügen und das Feld als RLE-Text kopieren, für Vierecke mit 8 Nachbarn.
- **Nachglühen:** Eine einstellbare Spur, wo eben noch Zellen lebten.

## Bedienung

| Taste | Funktion |
|---|---|
| `Space` | Start / Pause |
| `N` | Einzelschritt |
| `B` / `E` / `H` | Pinsel / Radierer / Verschieben |
| `P` | Musterbibliothek öffnen |
| `S` | Zufallsfeld |
| `C` | Feld löschen |
| `D` | Torus als 3D-Donut / flach |
| `R` / `F` / `Esc` | Muster drehen / spiegeln / Platzieren beenden |

Das Mausrad zoomt. Verschieben geht auch mit rechter oder mittlerer Maustaste oder mit gedrückter Shift-Taste.

Links und RLE findest du im Abschnitt **Share & export**. Geöffnete Links starten pausiert bei Generation 0.

Den Torus und seine Größe stellst du im Abschnitt **Field** ein. Die Seiten sind gerade Zahlen von 8 bis 4096. Bei Fünfecken rasten sie zusätzlich auf ein Vielfaches der Periode ein, aus 128×128 wird bei Typ 15 also 128×126. Ebene und Torus behalten beim Wechsel jeweils ihre Zellen.

## Suche nach Glidern (Kommandozeile)

`search` durchsucht B/S-Regeln auf dem Hat nach Mustern, die wandern, auf derselben Engine wie die Seite. Funde lassen sich dort öffnen. Die bisherigen Läufe über alle 32.768 Regeln mit Kanten und Ecken fanden Oszillatoren und Stillleben, aber keine Glider. Details stehen in der [Readme Entwicklung](Readme%20Entwicklung.md).

```
npm run search:build
cp engine/target/x86_64-unknown-linux-musl/release/search .   # statisch, läuft auch auf anderen Linux-Rechnern

./search control                  # Prüft die Messung am bekannten Glider, dauert 15 Sekunden
./search --smoke --out results/smoke
./search --estimate 500           # Wie lange dauert alles? Schreibt nichts
./search --out results/moore      # Alle 32.768 Regeln
```

**Dauer**, hochgerechnet aus Stichproben von 500 bis 1.500 Regeln auf einem Rechner mit 16 logischen Kernen, unter Last durch andere Jobs. Andere Rechner: `--estimate 500`.

| Lauf | Regeln | 16 Kerne | 64 Kerne |
|---|---|---|---|
| Nur Regelkatalog, `--screen-only` | 32.768 | 7 Sekunden | |
| Kanten+Ecken, `polyhat:3-6` | 32.768 | 15 Minuten | 4 Minuten |
| Kanten+Ecken, voll (`polyhat:3-8`) | 32.768 | 3,3–5,8 Stunden | 50–90 Minuten |
| Nur Kanten, voll | 8.192 | 1,2–1,8 Stunden | 18–27 Minuten |
| 2 Ringe, `polyhat:3-6` | 69.828 | 30 Minuten | 7 Minuten |
| 2 Ringe, voll | 69.828 | 11–15 Stunden | 2,7–3,7 Stunden |
| Späte Glider, `late:3-5,latesoup:3-4` | 32.768 | 20 Sekunden bis 7 Minuten | bis 2 Minuten |
| Dasselbe mit explosiven Regeln (`--keep explosive,stable,chaotic`) | 32.768 | 4,3–5,7 Stunden | 1–1,4 Stunden |

Die normalen Seeds müssen klein bleiben, ein Muster mit zu vielen Zellen bricht sofort ab. Glider entstehen aber oft erst aus einer langen, großen chaotischen Phase. Dafür gibt es die Seeds `late` und `latesoup`, die standardmäßig nicht mitlaufen: Sie dürfen 2.000 Generationen lang bis zu 300 Zellen haben (`--late-generations`, `--late-max-pop`), und nur was sich dabei löst, muss die strengen Kriterien bestehen. Sie gehören in einen eigenen `--out`-Ordner.

**In Teilen arbeiten:** Ein Lauf lässt sich jederzeit abbrechen, auch hart. Derselbe Befehl im selben `--out` macht weiter, fertige Regeln kommen nicht noch einmal dran. Die Regeln laufen in fester, gemischter Reihenfolge, also kostet jeder Teil ungefähr gleich viel.

| Option | Wirkung |
|---|---|
| `--batch 3/50` | nur der 3. von 50 gleich großen Teilen |
| `--max-rules 1000` | nur die nächsten 1.000 offenen Regeln |
| `--time-limit 2h` | nach dieser Zeit keine neue Regel mehr beginnen (`90s`, `45m`, `2h`) |
| `--screen-only` | nur der Regelkatalog, in Minuten fertig |
| `--seeds polyhat:3-6,corridor,soup` | kleinere Seeds für einen schnellen ersten Durchgang |
| `--neighborhood edge\|r2\|r3` | andere Nachbarschaft |

Teile von mehreren Rechnern führt `./search merge --out results/alle a b c` zusammen. Die Fortschrittsanzeige nennt Restzeit, Regelklassen und bisherige Funde. Alle Optionen zeigt `./search --help`.

**Oszillatoren ansehen:** `./search unique --out results/moore-klein` lässt die Funde erneut laufen, zählt die wirklich verschiedenen Formen (unabhängig von Ort, Drehung und Spiegelung) und schreibt `unique.html`, eine sortier- und filterbare Tabelle mit Links in die Seite, die dafür mit `npm run dev` laufen muss.

**Ergebnisse** in `--out`: `SUMMARY.md` mit den besten 20, `results.jsonl`, `catalog.jsonl` mit der Einstufung jeder Regel, und `finds/` mit einer Datei je Fund. Eine Fund-Datei öffnest du in der Seite unter **Share & export** mit **Open a find file**, oder du hängst den Wert von `link` an die Adresse an. Sie startet pausiert, `Space` lässt sie laufen.

## Installation

Voraussetzungen:

- [Node.js](https://nodejs.org/) 22
- [Rust](https://rustup.rs/) mit dem WebAssembly-Target:

  ```
  rustup target add wasm32-unknown-unknown
  ```

Starten:

```
npm install
npm run dev
```

Die Seite läuft dann unter `http://localhost:5173`.

## Veröffentlichen

```
npm run build
```

Das Ergebnis in `dist/` ist eine rein statische Seite, ein Server-Backend ist nicht nötig. Der Webserver muss `.wasm`-Dateien mit dem Typ `application/wasm` ausliefern.

Der Browser muss WebGL2 unterstützen.

## Lizenz und Danksagung

Die Substitutionsregeln der Hat-Kachelung stammen aus [hatviz](https://github.com/isohedral/hatviz) von Craig S. Kaplan (BSD 3-Clause), siehe [public/THIRD_PARTY_NOTICES.txt](public/THIRD_PARTY_NOTICES.txt). Die Kachelung selbst beschreiben Smith, Myers, Kaplan und Goodman-Strauss in „An aperiodic monotile“ (2023). Der Hat-Glider stammt von James Edmond ([monotile-ca](https://github.com/jamesedmond/monotile-ca), MIT-Lizenz). Die Rauten gehen auf Roger Penrose zurück und entstehen hier durch die Teilung von Robinson-Dreiecken. Die Bedingungen der 15 Fünfeck-Typen stammen aus der Wikipedia, die Kachelungen selbst berechnet ein Skript im Repository.

Für das Projekt selbst ist noch keine Lizenz festgelegt.
