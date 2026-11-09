// Looks for rules that stay lively on Penrose's rhombs, the same way as
// pentagon-rules.mjs. Prints them for HAT_NEIGHBOURHOODS' counterpart in tiles.ts.
// Needs the engine built first: npm run wasm
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const { instance } = await WebAssembly.instantiate(readFileSync(resolve(import.meta.dirname, '../src/sim/engine.wasm')));
const e = instance.exports;
const RADIUS = 28;
const STEPS = 800;
/** Above this many times the start, a rule is taking over the unbounded field. */
const MOST_GROWTH = 5;

const subsets = (values, most) => {
  const list = [[]];
  for (const v of values) for (const s of [...list]) if (s.length < most) list.push([...s, v]);
  return list;
};
const mask = (list) => list.reduce((m, n) => m | (1 << n), 0);
const hex = (n) => n.toString(16);

function run(edgesOnly, birth, survive, seed) {
  const w = e.penrose_new();
  e.hat_set_edges_only(w, edgesOnly ? 1 : 0);
  e.hat_set_rule(w, birth, survive);
  e.hat_soup(w, 0, 0, RADIUS, 0.3, seed);
  const start = e.hat_population(w);
  const seen = new Set();
  let middle = 0;
  let last = 0;
  for (let g = 0; g < STEPS; g++) {
    e.hat_step(w, 1);
    const n = e.hat_population(w);
    if (n === 0 || n > start * MOST_GROWTH) {
      e.hat_free(w);
      return null;
    }
    if (g === STEPS / 2 - 1) middle = n;
    if (g >= STEPS - 60) seen.add(n);
    last = n;
  }
  e.hat_free(w);
  // Still growing at the end means it only took longer to take over.
  return last > middle * 1.5 ? null : { moves: seen.size };
}

for (const [name, edgesOnly, most] of [['edges and corners', false, 11], ['edges only', true, 4]]) {
  const values = Array.from({ length: most }, (_, i) => i + 1);
  const found = [];
  for (const birth of subsets(values.filter((v) => v >= 2 && v <= 6), 2).filter((s) => s.length)) {
    for (const survive of subsets(values.slice(0, 7), 3)) {
      if (!survive.length) continue;
      let worst = Infinity;
      for (const seed of [12345, 777, 40961]) {
        const r = run(edgesOnly, mask(birth), mask(survive), seed);
        if (!r || r.moves < 12) {
          worst = -1;
          break;
        }
        worst = Math.min(worst, r.moves);
      }
      if (worst > 0) found.push({ rule: `B${birth.map(hex).join('')}/S${survive.map(hex).join('')}`, worst });
    }
  }
  found.sort((a, b) => b.worst - a.worst);
  console.log(`${name}: ${found.length} lively, ${found.slice(0, 8).map((f) => f.rule).join(' ')}`);
}
