import { useEffect, useRef } from 'react';
import { ClearIcon } from './icons';

/** Mounted while open. The browser's modal dialog brings the backdrop, focus handling and Esc. */
export function InfoDialog({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    // On the close button, Space would close the dialog. This way it scrolls the text.
    dialog?.focus();
  }, []);

  return (
    <dialog
      ref={ref}
      className="info-dialog"
      tabIndex={-1}
      aria-labelledby="info-title"
      onClose={onClose}
      // The content fills the dialog, so a click on the element itself is on the backdrop.
      onClick={(e) => {
        if (e.target === ref.current) ref.current.close();
      }}
    >
      <div className="info-content">
        <header className="info-head">
          <h1 id="info-title">Game of Emergence</h1>
          <button className="icon" aria-label="Close" title="Close (Esc)" onClick={() => ref.current?.close()}>
            <ClearIcon />
          </button>
        </header>

        <h2>The Game of Life</h2>
        <p>
          The mathematician John Conway devised the Game of Life in 1970. It is a game without players: you set up a starting pattern and then watch what happens. Cells on a grid of squares are either alive or dead, and all of them change at once, generation after generation.
        </p>

        <h2>How it works</h2>
        <p>Each cell looks only at its eight neighbours:</p>
        <ul>
          <li><strong>Birth:</strong> a dead cell with exactly 3 live neighbours comes alive.</li>
          <li><strong>Survival:</strong> a live cell with 2 or 3 live neighbours stays alive.</li>
          <li><strong>Otherwise:</strong> the cell dies, or stays dead.</li>
        </ul>
        <p>
          This rule is written B3/S23. Nothing else is built in, and yet stable shapes, oscillators and gliders that travel across the field emerge from it. Arranged the right way, such patterns can even compute.
        </p>

        <h2>What Game of Emergence adds</h2>
        <ul>
          <li><strong>Other tiles:</strong> triangles, quadrilaterals, the 15 known types of convex pentagon that tile the plane, hexagons, and the never-repeating Penrose and Einstein tilings. Most shapes can be deformed while the simulation runs.</li>
          <li><strong>Your own rules:</strong> choose the neighbour counts for birth and survival, and whether cells touching only at a corner count as neighbours.</li>
          <li><strong>Field:</strong> unbounded, or a torus whose opposite edges meet, which can also be viewed as a 3D donut.</li>
          <li><strong>Patterns:</strong> a library of known patterns, and import and export as RLE text, for squares and other quadrilaterals with eight neighbours.</li>
          <li><strong>Status:</strong> tells you when a field has died out, stopped changing, started repeating or is growing steadily.</li>
          <li><strong>Sharing:</strong> a link holds the tile, rule, view and cells, with no server involved.</li>
        </ul>
      </div>
    </dialog>
  );
}
