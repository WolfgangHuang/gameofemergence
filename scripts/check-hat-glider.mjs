// Flies hat glider A on this project's own tiling and compares it with what James
// Edmond's paper reports for it: a heading of 45.523 degrees modulo 60, and the
// corridor's 89.867 units per 42 generations.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const { instance } = await WebAssembly.instantiate(readFileSync(resolve(root, 'src/sim/engine.wasm')));
const e = instance.exports;
const HR3 = Math.sqrt(3) / 2;
const SEEDS = [0, 3, 17, 41, 77, 120, 190, 260, 333, 420, 500];
const SETTLE = 1000;
const FLIGHT = 5000;
const SPEED = 89.867 / 42;
const HEADING = 45.523;
const TABLE = [0, 1, 2, 2, 0, 255, 2, 1, 3, 1, 1, 3, 2, 2, 1, 1, 0, 1, 1, 1, 2, 2, 1, 2, 1, 3, 2, 0, 0, 0, 1, 1, 1, 2, 2, 1, 3, 2, 0, 2, 0, 0];
/** More than one glider's worth of cells: their shared centre says nothing about either. */
const ONE_GLIDER = 8;

function centre(w) {
  const n = e.hat_cell_count(w);
  const cells = new Int32Array(e.memory.buffer, e.hat_cells(w, 0), n * 5).slice();
  const states = new Uint8Array(e.memory.buffer, e.hat_states(w), n);
  let [x, y, count] = [0, 0, 0];
  for (let i = 0; i < n; i++) {
    if (!states[i]) continue;
    const q = cells[i * 5 + 1] + cells[i * 5 + 3];
    const r = cells[i * 5 + 2] + cells[i * 5 + 4];
    x += q + 0.5 * r;
    y += HR3 * r;
    count++;
  }
  return [x / count, y / count];
}

let failures = 0;
let single = 0;
for (const cell of SEEDS) {
  const w = e.hat_new();
  const at = e.hat_scratch(w, TABLE.length);
  new Uint32Array(e.memory.buffer, at, TABLE.length).set(TABLE);
  e.hat_set_table(w, TABLE.length);
  e.hat_load(w, 0, 0, 60);
  e.hat_seed_pair(w, cell, 1, 2);
  e.hat_step(w, SETTLE);
  const start = centre(w);
  let most = 0;
  for (let g = 0; g < FLIGHT; g++) {
    e.hat_step(w, 1);
    most = Math.max(most, e.hat_population(w));
  }
  const end = centre(w);
  const [dx, dy] = [end[0] - start[0], end[1] - start[1]];
  const speed = Math.hypot(dx, dy) / FLIGHT;
  const heading = ((Math.atan2(dy, dx) * 180) / Math.PI + 360) % 360;
  let problem = most > 16 ? `population reached ${most}` : e.hat_population(w) === 0 ? 'died' : null;
  if (!problem && most <= ONE_GLIDER) {
    single++;
    if (Math.abs((heading % 60) - HEADING) > 0.1) problem = `heading ${heading.toFixed(2)}`;
    else if (Math.abs(speed - SPEED) > 0.01) problem = `speed ${speed.toFixed(3)}`;
  }
  if (problem) failures++;
  console.log(`${problem ? 'FAIL' : 'ok  '} seed on cell ${cell}: population up to ${most}${most <= ONE_GLIDER ? `, heading ${heading.toFixed(2)}, speed ${speed.toFixed(3)}` : ', more than one glider'}${problem ? ` - ${problem}` : ''}`);
  e.hat_free(w);
}
if (single === 0) failures++;

// As plain WASM, what the page runs: a large field, cells by their place in it, and a
// rule on two rings. Engine tests run natively, so they say nothing about this.
{
  const w = e.hat_new();
  e.hat_load(w, 0, 0, 1500);
  const cells = e.hat_cell_count(w);
  const all = new Int32Array(e.memory.buffer, e.hat_cells(w, 0), cells * 5).slice();
  const keys = [...all.subarray(5000, 5005), ...all.subarray(400000, 400005)];
  const at = e.hat_scratch(w, keys.length);
  new Int32Array(e.memory.buffer, at, keys.length).set(keys);
  const missing = e.hat_paint_keys(w, keys.length, 1);
  const live = new Int32Array(e.memory.buffer, e.hat_live(w), 10).slice();
  const sorted = (list) => [list.slice(0, 5).join(), list.slice(5, 10).join()].sort().join();
  const ok = cells > 500000 && missing === 0 && e.hat_population(w) === 2 && sorted([...live]) === sorted(keys);
  const bogus = new Int32Array(e.memory.buffer, e.hat_scratch(w, 5), 5);
  bogus.set([0, 1, 1, 1, 1]);
  const refused = e.hat_paint_keys(w, 5, 1) === 1;
  e.hat_clear(w);
  const ruled = e.hat_set_range_rule(w, 2, 3, 5, 2, 7) === 1 && e.hat_set_range_rule(w, 2, 0, 5, 2, 7) === 0 && e.hat_set_range_rule(w, 9, 3, 5, 2, 7) === 0;
  e.hat_soup(w, 0, 0, 25, 0.4, 11);
  e.hat_step(w, 25);
  const ran = e.hat_population(w) > 0;
  const good = ok && refused && ruled && ran;
  if (!good) failures++;
  console.log(`${good ? 'ok  ' : 'FAIL'} large field of ${cells} cells as plain WASM: places ${ok}, refusing ${refused}, rule on rings ${ruled}, running ${ran}, ${(e.memory.buffer.byteLength / 1e6).toFixed(0)} MB`);
  e.hat_free(w);
}
console.log(failures ? `${failures} glider checks failed` : `all glider checks passed, ${single} single flights measured`);
process.exitCode = failures ? 1 : 0;
