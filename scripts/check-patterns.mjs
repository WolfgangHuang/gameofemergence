// Simulates every library pattern and compares it with what the library claims.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { parseRle } from '../src/patterns/rle.ts';

const root = resolve(import.meta.dirname, '..');
const patterns = JSON.parse(readFileSync(resolve(root, 'src/patterns/patterns.json'), 'utf8'));
const { instance } = await WebAssembly.instantiate(readFileSync(resolve(root, 'src/sim/engine.wasm')));
const e = instance.exports;
const KINDS = ['active', 'empty', 'still', 'oscillating', 'drifting', 'growing'];
const STEP_LIMIT = 40000;

function load(rle) {
  const w = e.world_new();
  const { cells } = parseRle(rle);
  const ptr = e.world_scratch(w, cells.length);
  new Int32Array(e.memory.buffer, ptr, cells.length).set(cells);
  e.world_paint(w, cells.length / 2, 1);
  return w;
}

const bounds = (w) => {
  const ptr = e.world_bounds(w);
  return ptr ? [...new Int32Array(e.memory.buffer, ptr, 4)] : null;
};

function runUntil(w, wanted) {
  for (let i = 0; i < STEP_LIMIT; i++) {
    e.world_step(w, 1);
    if (KINDS[e.world_status_kind(w)] === wanted) return true;
  }
  return false;
}

function check({ expect, rle }) {
  const w = load(rle);
  const fail = (message) => {
    const status = KINDS[e.world_status_kind(w)];
    return `${message}; status ${status}, period ${e.world_status_period(w)}, detail ${e.world_status_detail(w)}, population ${e.world_population(w)}, generation ${e.world_generation(w)}`;
  };
  switch (expect.kind) {
    case 'still':
      return runUntil(w, 'still') && e.world_generation(w) === 1 ? null : fail('not still');
    case 'oscillator':
      if (!runUntil(w, 'oscillating')) return fail('no oscillation');
      return e.world_status_period(w) === expect.period ? null : fail('wrong period');
    case 'spaceship': {
      if (!runUntil(w, 'drifting')) return fail('no drift');
      if (e.world_status_period(w) !== expect.period) return fail('wrong period');
      const before = bounds(w);
      e.world_step(w, expect.period);
      const after = bounds(w);
      const dx = after[0] - before[0];
      const dy = after[1] - before[1];
      return dx === expect.dx && dy === expect.dy ? null : fail(`moved by ${dx},${dy}`);
    }
    case 'growth':
      if (!runUntil(w, 'growing')) return fail('no steady growth');
      return e.world_status_period(w) === expect.period && e.world_status_detail(w) === expect.delta
        ? null
        : fail('wrong growth');
    case 'methuselah':
      if (!runUntil(w, expect.settles)) return fail('did not settle as expected');
      return e.world_population(w) === expect.population ? null : fail('wrong final population');
  }
  return `unknown kind ${expect.kind}`;
}

let failures = 0;
for (const pattern of patterns) {
  const problem = check(pattern);
  if (problem) failures++;
  console.log(`${problem ? 'FAIL' : 'ok  '} ${pattern.name}${problem ? ` - ${problem}` : ''}`);
}
console.log(`${patterns.length - failures}/${patterns.length} patterns behave as listed`);
process.exitCode = failures ? 1 : 0;
