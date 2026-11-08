/** One line of a priority table. Conditions are minimum neighbour counts, one per state. */
export interface TableRow {
  /** Null matches a cell in any state. */
  own: number | null;
  conditions: { state: number; count: number }[];
  next: number;
}

function parse(table: number[]): TableRow[] {
  const rows: TableRow[] = [];
  for (let i = 0; i < table.length; ) {
    const own = table[i] === 255 ? null : table[i];
    const pairs = table[i + 1];
    const minimum = new Map<number, number>();
    for (let p = 0; p < pairs; p++) {
      const state = table[i + 2 + 2 * p];
      const count = table[i + 3 + 2 * p];
      if (count > 0) minimum.set(state, Math.max(minimum.get(state) ?? 0, count));
    }
    const conditions = [...minimum].map(([state, count]) => ({ state, count }));
    rows.push({ own, conditions, next: table[i + 2 + 2 * pairs] });
    i += 3 + 2 * pairs;
  }
  return rows;
}

/** Whether `earlier` always fires when `row` would, so that `row` is never reached. */
function shadows(earlier: TableRow, row: TableRow) {
  if (earlier.own !== null && earlier.own !== row.own) return false;
  return earlier.conditions.every((c) => row.conditions.some((r) => r.state === c.state && r.count >= c.count));
}

/**
 * The lines of a table that have an effect, in order. Evolved tables carry lines
 * that never fire, and a cell matching no line becomes empty anyway.
 */
export function effectiveRows(table: number[]): TableRow[] {
  const reached: TableRow[] = [];
  for (const row of parse(table)) {
    if (!reached.some((earlier) => shadows(earlier, row))) reached.push(row);
  }
  // A line that empties a cell is redundant if no later line could catch that cell instead.
  const kept: TableRow[] = [];
  for (const row of reached.reverse()) {
    const caught = kept.some((later) => later.own === null || row.own === null || later.own === row.own);
    if (row.next !== 0 || caught) kept.unshift(row);
  }
  return kept;
}
