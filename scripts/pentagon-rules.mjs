// Looks for rules that stay lively on the grids of the 15 pentagon tilings, and
// stores them in src/tiles/pentagons.json. A rule qualifies if from three random
// fields it neither dies out nor floods the field, and if its population keeps
// changing long after the start. That says it is lively, not that it is the best.
//
// Needs the engine built first: npm run wasm
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';

const root = resolve(import.meta.dirname, '..');
const dataPath = resolve(root, 'src/tiles/pentagons.json');
const SIDE = 72; // a multiple of every period
const STEPS = 360;

const subsets = (values, most) => {
  const list = [[]];
  for (const v of values) for (const s of [...list]) if (s.length < most) list.push([...s, v]);
  return list;
};
const mask = (list) => list.reduce((m, n) => m | (1 << n), 0);
const hex = (n) => n.toString(16);
const name = (birth, survive) => `B${birth.map(hex).join('')}/S${survive.map(hex).join('')}`;

if (isMainThread) {
  const data = JSON.parse(readFileSync(dataPath, 'utf8'));
  const jobs = [];
  data.forEach((tiling, t) => {
    const kinds = tiling.neighbours.map((list) => list);
    const count = (filter) => Math.max(...kinds.map((list) => list.filter(filter).length));
    jobs.push({ t, key: 'all', topology: 5 + 2 * t, most: count(() => true) });
    jobs.push({ t, key: 'edges', topology: 6 + 2 * t, most: count((n) => n[2] === 2) });
  });
  let next = 0;
  let running = 0;
  const finish = () => {
    if (running) return;
    writeFileSync(dataPath, `${JSON.stringify(data)}\n`);
  };
  const threads = Math.max(1, Math.min(Number(process.env.THREADS ?? 12), jobs.length));
  for (let i = 0; i < threads; i++) {
    const worker = new Worker(new URL(import.meta.url));
    running++;
    const give = () => {
      if (next < jobs.length) worker.postMessage(jobs[next++]);
      else {
        worker.terminate();
        running--;
        finish();
      }
    };
    worker.on('message', ({ job, rules }) => {
      (data[job.t].rules ??= {})[job.key] = rules;
      console.log(`type ${data[job.t].type} ${job.key}: ${rules.join(' ')}`);
      give();
    });
    give();
  }
} else {
  const { instance } = await WebAssembly.instantiate(readFileSync(resolve(root, 'src/sim/engine.wasm')));
  const e = instance.exports;
  const area = SIDE * SIDE;

  /** Density over the last part of a run, and how much the population still moves. */
  function run(topology, birth, survive, seed) {
    const w = e.world_new();
    e.world_set_torus(w, SIDE, SIDE);
    e.world_set_topology(w, topology);
    e.world_set_rule(w, birth, survive);
    e.world_soup(w, 0, 0, SIDE, SIDE, 0.3, seed);
    const seen = new Set();
    let sum = 0;
    let last = 0;
    for (let g = 0; g < STEPS; g++) {
      e.world_step(w, 1);
      last = Number(e.world_population(w));
      if (g === 60 && (last === 0 || last > 0.6 * area)) break;
      if (g >= STEPS - 80) {
        seen.add(last);
        sum += last;
      }
    }
    e.world_free?.(w);
    return { density: sum / 80 / area, moves: seen.size };
  }

  parentPort.on('message', (job) => {
    const values = Array.from({ length: job.most }, (_, i) => i + 1);
    const births = subsets(values.filter((v) => v >= 2 && v <= 6), 2).filter((s) => s.length);
    const survives = subsets(values.slice(0, 6), 3);
    const found = [];
    for (const birth of births) {
      for (const survive of survives) {
        const [b, s] = [mask(birth), mask(survive)];
        let worst = Infinity;
        let density = 0;
        for (const seed of [12345, 777, 40961]) {
          const r = run(job.topology, b, s, seed);
          if (r.density < 0.04 || r.density > 0.4 || r.moves < 15) {
            worst = -1;
            break;
          }
          worst = Math.min(worst, r.moves);
          density += r.density / 3;
        }
        if (worst > 0) found.push({ rule: name(birth, survive), worst, density });
      }
    }
    // Lively first, with mid densities that show structure rather than noise.
    // Without survival a field is only noise, so such rules are not the default.
    const barren = (f) => f.rule.endsWith('/S');
    found.sort((p, q) => barren(p) - barren(q) || q.worst * (1 - Math.abs(q.density - 0.15)) - p.worst * (1 - Math.abs(p.density - 0.15)));
    parentPort.postMessage({ job, rules: found.slice(0, 6).map((f) => f.rule) });
  });
}
