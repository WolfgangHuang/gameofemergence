/** Patterns for the hat tiling. They bring their own rule, as no life-like rule supports them. */
export interface HatPattern {
  id: string;
  name: string;
  note: string;
  /** Priority table, row by row: own state (255 for any), number of conditions, each as state and minimum count, next state. */
  table: number[];
  /** States placed on the clicked cell and one of its edge neighbours. */
  seed: [number, number];
}

export const HAT_PATTERNS: HatPattern[] = [
  {
    id: 'hat-glider-a',
    name: 'Hat glider A',
    note: 'James Edmond, 2026. Four states, about 2.14 units per generation',
    // Verbatim from results/hat-tableevolve-r48-s21.jsonl of monotile-ca, also in scripts/check-hat-glider.mjs.
    table: [0, 1, 2, 2, 0, 255, 2, 1, 3, 1, 1, 3, 2, 2, 1, 1, 0, 1, 1, 1, 2, 2, 1, 2, 1, 3, 2, 0, 0, 0, 1, 1, 1, 2, 2, 1, 3, 2, 0, 2, 0, 0],
    seed: [1, 2],
  },
];
