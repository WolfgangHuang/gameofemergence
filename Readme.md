# Game of Emergence

Conways „Game of Life“ im Browser, auf einem unbegrenzten Feld und auf vielen Kachelformen: von verformbaren Vierecken über Dreiecke und Hexagone bis zum aperiodischen „Hat“, einer der Einstein-Kacheln. Die Oberfläche ist weiß und minimalistisch, die Berechnung läuft in Rust/WebAssembly.

## Features

- **Unbegrenztes Feld:** Es wird nur gespeichert und berechnet, wo etwas passiert. Rund 3 Milliarden Zellen pro Sekunde auf Quadraten.
- **Torus:** Wahlweise ein Feld fester Größe, dessen Ränder sich berühren. Was hinausläuft, kommt von der anderen Seite zurück. Das gibt es für alle Kachelformen außer den Einstein-Kacheln, die sich nicht wiederholen.
- **Kachelformen:** Vierecke (Quadrat, Rechteck, Parallelogramm, Raute, Trapez, Drachen, Dart), Dreiecke, Hexagone und Einstein-Kacheln.
- **Live verformbar:** Die Ecken einer Kachel lassen sich ziehen, auch während die Simulation läuft. Die Einstein-Kacheln morphen stufenlos zwischen Comet, Turtle, Spectre-Form, Hat und Chevron.
- **Regeln frei einstellbar:** Geburt und Überleben pro Nachbarzahl, dazu passende Vorlagen je Kachelform. Die Nachbarschaft ist wählbar, also nur über Kanten oder auch über Ecken. Auf dem Torus ist auch Geburt bei 0 Nachbarn möglich.
- **Zeichnen:** Pinsel in 12 Größen, Radierer, Zufallsfeld mit einstellbarer Dichte.
- **Musterbibliothek:** 24 bekannte Muster, darunter Glider, Spaceships, Gosper Gun und Acorn, mit Vorschau am Mauszeiger. Sie gilt für Vierecke mit 8 Nachbarn. Dazu kommt ein Glider für die Hat-Kachelung, der eine erweiterte 4-Zustands-Regel mitbringt. Solange sie aktiv ist, erklärt das Rule-Panel sie Zeile für Zeile und blendet die B/S-Regler aus; ein Knopf führt zurück zur Grundregel.
- **Statuserkennung:** Zeigt an, ob ein Feld ausgestorben oder statisch ist, oszilliert, nur noch Spaceships ausschickt oder stetig wächst. Auf dem Torus nennt sie die volle Periode, etwa den Umlauf eines Gliders. Auf Wunsch pausiert die Simulation dann automatisch.
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
| `R` / `F` / `Esc` | Muster drehen / spiegeln / Platzieren beenden |

Das Mausrad zoomt. Verschieben geht auch mit rechter oder mittlerer Maustaste oder mit gedrückter Shift-Taste.

Links und RLE findest du im Abschnitt **Share & export**. Geöffnete Links starten pausiert bei Generation 0.

Den Torus und seine Größe stellst du im Abschnitt **Field** ein. Die Seiten sind gerade Zahlen von 8 bis 4096. Ebene und Torus behalten beim Wechsel jeweils ihre Zellen.

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

Die Substitutionsregeln der Hat-Kachelung stammen aus [hatviz](https://github.com/isohedral/hatviz) von Craig S. Kaplan (BSD 3-Clause), siehe [public/THIRD_PARTY_NOTICES.txt](public/THIRD_PARTY_NOTICES.txt). Die Kachelung selbst beschreiben Smith, Myers, Kaplan und Goodman-Strauss in „An aperiodic monotile“ (2023). Der Hat-Glider stammt von James Edmond ([monotile-ca](https://github.com/jamesedmond/monotile-ca), MIT-Lizenz).

Für das Projekt selbst ist noch keine Lizenz festgelegt.
