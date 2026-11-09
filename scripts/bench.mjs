// Measures the wasm engine against a plain typed-array JS loop.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const bytes = readFileSync(resolve(import.meta.dirname, '../src/sim/engine.wasm'));
const { instance } = await WebAssembly.instantiate(bytes);
const e = instance.exports;

function wasmBench(size, generations) {
  const w = e.world_new();
  e.world_soup(w, 0, 0, size, size, 0.3, 42);
  // Skip the initial die-off so the rate reflects typical activity.
  e.world_step(w, 20);
  let cells = 0;
  const start = performance.now();
  for (let i = 0; i < generations; i++) {
    e.world_step(w, 1);
    cells += e.world_active_chunks(w) * 4096;
  }
  const seconds = (performance.now() - start) / 1000;
  console.log(
    `wasm ${size}x${size}: ${(generations / seconds).toFixed(1)} gen/s, ` +
      `${(cells / seconds / 1e6).toFixed(0)} M cells/s, population ${e.world_population(w)}`,
  );
}

function jsBench(size, generations) {
  let cur = new Uint8Array(size * size);
  let next = new Uint8Array(size * size);
  for (let i = 0; i < cur.length; i++) cur[i] = Math.random() < 0.3 ? 1 : 0;
  const start = performance.now();
  for (let g = 0; g < generations; g++) {
    for (let y = 0; y < size; y++) {
      const up = ((y + size - 1) % size) * size;
      const row = y * size;
      const down = ((y + 1) % size) * size;
      for (let x = 0; x < size; x++) {
        const l = (x + size - 1) % size;
        const r = (x + 1) % size;
        const n =
          cur[up + l] + cur[up + x] + cur[up + r] +
          cur[row + l] + cur[row + r] +
          cur[down + l] + cur[down + x] + cur[down + r];
        next[row + x] = n === 3 || (n === 2 && cur[row + x]) ? 1 : 0;
      }
    }
    [cur, next] = [next, cur];
  }
  const seconds = (performance.now() - start) / 1000;
  console.log(
    `js   ${size}x${size}: ${(generations / seconds).toFixed(1)} gen/s, ` +
      `${((size * size * generations) / seconds / 1e6).toFixed(0)} M cells/s`,
  );
}

function topologyBench(name, topology, birth, survive) {
  const w = e.world_new();
  e.world_set_topology(w, topology);
  e.world_set_rule(w, birth, survive);
  e.world_soup(w, 0, 0, 1024, 1024, 0.3, 42);
  e.world_step(w, 20);
  let cells = 0;
  const start = performance.now();
  for (let i = 0; i < 100; i++) {
    e.world_step(w, 1);
    cells += e.world_active_chunks(w) * 4096;
  }
  const seconds = (performance.now() - start) / 1000;
  console.log(`${name}: ${(cells / seconds / 1e6).toFixed(0)} M cells/s`);
}

function torusBench(size, generations) {
  const w = e.world_new();
  e.world_set_torus(w, size, size);
  e.world_soup(w, 0, 0, size, size, 0.3, 42);
  e.world_step(w, 20);
  let cells = 0;
  const start = performance.now();
  for (let i = 0; i < generations; i++) {
    e.world_step(w, 1);
    cells += e.world_active_chunks(w) * 4096;
  }
  const seconds = (performance.now() - start) / 1000;
  console.log(`torus ${size}x${size}: ${(generations / seconds).toFixed(1)} gen/s, ${(cells / seconds / 1e6).toFixed(0)} M cells/s, population ${e.world_population(w)}`);
}

function hatBench(birth, survive) {
  const w = e.hat_new();
  e.hat_set_rule(w, birth, survive);
  e.hat_soup(w, 0, 0, 500, 0.3, 42);
  e.hat_step(w, 20);
  const start = performance.now();
  e.hat_step(w, 100);
  const seconds = (performance.now() - start) / 1000;
  const cells = e.hat_cell_count(w);
  console.log(`hat: ${(100 / seconds).toFixed(1)} gen/s on ${cells} generated cells, population ${e.hat_population(w)}, ${((cells * 100) / seconds / 1e6).toFixed(0)} M cells/s equivalent`);
  e.hat_free(w);
}

wasmBench(1024, 500);
wasmBench(4096, 100);
torusBench(1024, 500);
torusBench(4096, 100);
topologyBench('von Neumann', 1, 0b10100, 0b1100);
topologyBench('hexagonal', 2, 0b10100, 0b1000);
topologyBench('triangle edges', 3, 0b100, 0b110);
topologyBench('triangle vertices', 4, 0b10000, 0b111000);
topologyBench('pentagon type 4, 7 neighbours', 5 + 2 * 3, 0b100, 0b100000);
topologyBench('pentagon type 15, up to 8 neighbours', 5 + 2 * 14, 0b1000100, 0b110000);
hatBench(0b100100, 0b1000);
jsBench(1024, 100);
jsBench(4096, 10);
