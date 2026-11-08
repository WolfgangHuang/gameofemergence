// Verifies the tilings geometrically, independently of the engine's tables:
// polygons must fill the plane without gaps, and cells that really touch must be
// what the engine counts as neighbours. Then the engine is run against a naive
// simulation driven by those geometric neighbours.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { area, buildTiling, cellPolygon, FAMILIES, fullOutline, gridToWorld, isValidOutline, locate, TOPOLOGY } from '../src/tiles/tiles.ts';

const root = resolve(import.meta.dirname, '..');
const { instance } = await WebAssembly.instantiate(readFileSync(resolve(root, 'src/sim/engine.wasm')));
const e = instance.exports;

const key = ([x, y]) => `${Math.round(x * 1e6)},${Math.round(y * 1e6)}`;
const edgeKey = (a, b) => [key(a), key(b)].sort().join('|');

/** Neighbour offsets of a cell, read off the polygons around it. */
function geometricNeighbours(tiling, x, y, withCorners) {
  const own = cellPolygon(tiling, x, y);
  const ownEdges = new Set(own.map((v, i) => edgeKey(v, own[(i + 1) % own.length])));
  const ownVertices = new Set(own.map(key));
  const found = [];
  for (let dy = -6; dy <= 6; dy++) {
    for (let dx = -6; dx <= 6; dx++) {
      if (dx === 0 && dy === 0) continue;
      const other = cellPolygon(tiling, x + dx, y + dy);
      const sharesEdge = other.some((v, i) => ownEdges.has(edgeKey(v, other[(i + 1) % other.length])));
      const sharesVertex = other.some((v) => ownVertices.has(key(v)));
      if (withCorners ? sharesVertex : sharesEdge) found.push([dx, dy]);
    }
  }
  return found;
}

function naiveStep(cells, neighboursOf, birth, survive) {
  const counts = new Map();
  for (const id of cells) {
    const [x, y] = id.split(',').map(Number);
    if (!counts.has(id)) counts.set(id, 0);
    for (const [dx, dy] of neighboursOf(x, y)) {
      const other = `${x + dx},${y + dy}`;
      counts.set(other, (counts.get(other) ?? 0) + 1);
    }
  }
  const next = new Set();
  for (const [id, n] of counts) {
    if (((cells.has(id) ? survive : birth) >> n) & 1) next.add(id);
  }
  return next;
}

function engineCells(w, x0, y0, size) {
  const ptr = e.world_frame(w, x0, y0, size, size, 0);
  const data = new Uint8Array(e.memory.buffer, ptr, size * size);
  const cells = new Set();
  data.forEach((v, i) => v && cells.add(`${x0 + i % size},${y0 + Math.floor(i / size)}`));
  return cells;
}

function parseRule(rule) {
  const [, b, s] = /^B(\w*)\/S(\w*)$/.exec(rule);
  const bits = (text) => [...text].reduce((m, c) => m | (1 << parseInt(c, 16)), 0);
  return [bits(b), bits(s)];
}

let failures = 0;
const report = (ok, label) => {
  if (!ok) failures++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}`);
};

for (const [family, info] of Object.entries(FAMILIES)) {
  for (const preset of info.presets) {
    const label = `${info.name} / ${preset.name}`;
    const outline = fullOutline(family, preset.outline);
    const tiling = buildTiling(family, preset.outline);
    report(isValidOutline(outline), `${label}: valid outline`);

    // Unit area, with the polygons of one period exactly filling the lattice cell.
    const sum = tiling.polygons.reduce((total, p) => total + Math.abs(area(p.vertices)), 0);
    const lattice = Math.abs(tiling.t1[0] * tiling.t2[1] - tiling.t1[1] * tiling.t2[0]);
    report(Math.abs(sum - lattice) < 1e-9 && Math.abs(Math.abs(area(tiling.polygons[0].vertices)) - 1) < 1e-9, `${label}: no gaps, unit area`);

    // Each sample point has to land in exactly the cell whose polygon contains it.
    let wrong = 0;
    for (let i = 0; i < 400; i++) {
      const x = Math.sin(i * 12.9898) * 9;
      const y = Math.sin(i * 78.233) * 9;
      const [cx, cy] = locate(tiling, x, y);
      const polygon = cellPolygon(tiling, cx, cy);
      let inside = false;
      for (let a = 0, b = polygon.length - 1; a < polygon.length; b = a++) {
        const [xa, ya] = polygon[a];
        const [xb, yb] = polygon[b];
        if (ya > y !== yb > y && x < ((xb - xa) * (y - ya)) / (yb - ya) + xa) inside = !inside;
      }
      if (!inside) wrong++;
    }
    report(wrong === 0, `${label}: position lookup (${wrong} wrong of 400)`);

    // On a torus, a cell one period away has to be the same polygon, shifted.
    let misfits = 0;
    for (const [width, height] of [[40, 38], [66, 128]]) {
      for (const [dx, dy] of [[width, 0], [0, height]]) {
        const [sx, sy] = gridToWorld(tiling, dx, dy);
        for (const [x, y] of [[0, 0], [1, 0], [0, 1], [1, 1], [7, 4]]) {
          const shifted = cellPolygon(tiling, x, y).map(([px, py]) => key([px + sx, py + sy]));
          const there = cellPolygon(tiling, x + dx, y + dy).map(key);
          if (shifted.some((v, i) => v !== there[i])) misfits++;
        }
      }
    }
    report(misfits === 0, `${label}: tiles fit across the seam of a torus (${misfits} misfits)`);

    for (const n of info.neighbourhoods) {
      const withCorners = n.topology !== 'vonNeumann' && n.topology !== 'triangleEdges';
      const table = new Map();
      const counts = new Set();
      for (const [x, y] of [[0, 0], [1, 0], [0, 1], [1, 1]]) {
        const found = geometricNeighbours(tiling, x, y, withCorners);
        table.set(`${x & 1},${y & 1}`, found);
        counts.add(found.length);
      }
      report(counts.size === 1 && counts.has(n.count), `${label}: ${n.name} has ${[...counts]} neighbours, expected ${n.count}`);

      const neighboursOf = (x, y) => table.get(`${x & 1},${y & 1}`);
      const [birth, survive] = parseRule(n.rules[0]);
      const w = e.world_new();
      e.world_set_topology(w, TOPOLOGY[n.topology]);
      e.world_set_rule(w, birth, survive);
      e.world_soup(w, -20, -20, 40, 40, 0.35, 12345);
      const size = 160;
      let cells = engineCells(w, -80, -80, size);
      let same = true;
      for (let g = 0; g < 25 && same; g++) {
        cells = naiveStep(cells, neighboursOf, birth, survive);
        e.world_step(w, 1);
        const actual = engineCells(w, -80, -80, size);
        same = actual.size === cells.size && [...actual].every((c) => cells.has(c));
      }
      report(same, `${label}: engine matches geometry under ${n.rules[0]}`);
    }
  }
}

console.log(failures ? `${failures} checks failed` : 'all tiling checks passed');
process.exitCode = failures ? 1 : 0;
