// Verifies the tilings geometrically, independently of the engine's tables:
// polygons must fill the plane without gaps, and cells that really touch must be
// what the engine counts as neighbours. Then the engine is run against a naive
// simulation driven by those geometric neighbours.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { area, buildTiling, cellPolygon, earClip, FAMILIES, fitTorus, fullOutline, gridToWorld, isValidOutline, locate, neighbourhoodsOf, PENTAGONS, TOPOLOGY, torusPeriod } from '../src/tiles/tiles.ts';

const root = resolve(import.meta.dirname, '..');
const { instance } = await WebAssembly.instantiate(readFileSync(resolve(root, 'src/sim/engine.wasm')));
const e = instance.exports;

const key = ([x, y]) => `${Math.round(x * 1e6)},${Math.round(y * 1e6)}`;
const TOUCH = 1e-5;

/**
 * Where two polygons meet: 2 for a stretch of edge, 1 for a point, 0 for not at all.
 * Goes by distances rather than equal corners, because many pentagons lie against
 * a part of their neighbour's edge.
 */
function contact(a, b) {
  let kind = 0;
  for (let i = 0; i < a.length; i++) {
    const p = a[i];
    const q = a[(i + 1) % a.length];
    const length = Math.hypot(q[0] - p[0], q[1] - p[1]);
    const ux = (q[0] - p[0]) / length;
    const uy = (q[1] - p[1]) / length;
    const along = ([x, y]) => (x - p[0]) * ux + (y - p[1]) * uy;
    const across = ([x, y]) => Math.abs((x - p[0]) * uy - (y - p[1]) * ux);
    for (let j = 0; j < b.length; j++) {
      const r = b[j];
      const s = b[(j + 1) % b.length];
      if (across(r) < TOUCH && across(s) < TOUCH) {
        const shared = Math.min(length, Math.max(along(r), along(s))) - Math.max(0, Math.min(along(r), along(s)));
        if (shared > TOUCH) return 2;
        if (shared > -TOUCH) kind = 1;
      } else if (across(r) < TOUCH && along(r) > -TOUCH && along(r) < length + TOUCH) {
        kind = 1;
      }
    }
  }
  return kind;
}

/** Neighbour offsets of a cell, read off the polygons around it. */
function geometricNeighbours(tiling, x, y, withCorners) {
  const own = cellPolygon(tiling, x, y);
  const found = [];
  for (let dy = -8; dy <= 8; dy++) {
    for (let dx = -8; dx <= 8; dx++) {
      if (dx === 0 && dy === 0) continue;
      const touch = Math.max(contact(own, cellPolygon(tiling, x + dx, y + dy)), contact(cellPolygon(tiling, x + dx, y + dy), own));
      if (touch === 2 || (withCorners && touch === 1)) found.push([dx, dy]);
    }
  }
  return found;
}

const hasOverlap = (a, b) => {
  for (const [p, q] of [[a, b], [b, a]]) {
    for (let i = 0; i < p.length; i++) {
      const c = p[(i + 1) % p.length];
      const nx = -(c[1] - p[i][1]);
      const ny = c[0] - p[i][0];
      const size = Math.hypot(nx, ny);
      if (Math.max(...q.map((v) => ((v[0] - p[i][0]) * nx + (v[1] - p[i][1]) * ny) / size)) < 1e-6) return false;
    }
  }
  return true;
};

/** Conditions of each type on sides and angles, from Wikipedia's "Pentagonal tiling". */
const PENTAGON_CONDITIONS = {
  1: (p) => [p.B + p.C - 180, p.A + p.D + p.E - 360],
  2: (p) => [p.c - p.e, p.B + p.D - 180],
  3: (p) => [p.a - p.b, p.d - p.c - p.e, p.A - 120, p.C - 120, p.D - 120],
  4: (p) => [p.b - p.c, p.d - p.e, p.B - 90, p.D - 90],
  5: (p) => [p.a - p.b, p.d - p.e, p.A - 60, p.D - 120],
  6: (p) => [p.a - p.d, p.d - p.e, p.b - p.c, p.B + p.D - 180, 2 * p.B - p.E],
  7: (p) => [p.b - p.c, p.c - p.d, p.d - p.e, p.B + 2 * p.E - 360, 2 * p.C + p.D - 360],
  8: (p) => [p.b - p.c, p.c - p.d, p.d - p.e, 2 * p.B + p.C - 360, p.D + 2 * p.E - 360],
  9: (p) => [p.b - p.c, p.c - p.d, p.d - p.e, 2 * p.A + p.C - 360, p.D + 2 * p.E - 360],
  10: (p) => [p.a - p.b, p.b - p.c - p.e, p.A - 90, p.B + p.E - 180, p.B + 2 * p.C - 360],
  11: (p) => [2 * p.a + p.c - p.d, p.d - p.e, p.A - 90, p.C + p.E - 180, 2 * p.B + p.C - 360],
  12: (p) => [2 * p.a - p.d, p.d - p.c - p.e, p.A - 90, p.C + p.E - 180, 2 * p.B + p.C - 360],
  13: (p) => [p.d - 2 * p.a, p.a - p.e, p.B - 90, p.E - 90, 2 * p.A + p.D - 360],
  14: (p) => [2 * p.a - 2 * p.c, 2 * p.c - p.d, p.d - p.e, p.A - 90, 2 * p.B + p.C - 360, p.C + p.E - 180],
  15: (p) => [p.a - p.c, p.c - p.e, p.b - 2 * p.a, p.A - 150, p.B - 60, p.C - 135, p.D - 105, p.E - 90],
};

/** Sides and angles of a pentagon with corners A to E. Side a runs from E to A, b from A to B, and so on. */
function measure(v) {
  const [A, B, C, D, E] = v;
  const side = (s, t) => Math.hypot(t[0] - s[0], t[1] - s[1]);
  const angle = (o, s, t) => {
    const dot = (s[0] - o[0]) * (t[0] - o[0]) + (s[1] - o[1]) * (t[1] - o[1]);
    return (Math.acos(dot / (side(o, s) * side(o, t))) * 180) / Math.PI;
  };
  return { a: side(E, A), b: side(A, B), c: side(B, C), d: side(C, D), e: side(D, E), A: angle(A, E, B), B: angle(B, A, C), C: angle(C, B, D), D: angle(D, C, E), E: angle(E, D, A) };
}

/** Sides and corners going round, either way, so that congruent tiles compare equal. */
function signature(vertices) {
  const n = vertices.length;
  const round = (list) => list.map((value) => value.toFixed(5)).join(' ');
  const parts = vertices.flatMap((v, i) => {
    const m = vertices[(i + 1) % n];
    const before = vertices[(i + n - 1) % n];
    const length = Math.hypot(m[0] - v[0], m[1] - v[1]);
    const dot = (m[0] - v[0]) * (before[0] - v[0]) + (m[1] - v[1]) * (before[1] - v[1]);
    return [{ length, corner: Math.acos(dot / (length * Math.hypot(before[0] - v[0], before[1] - v[1]))) }];
  });
  const forms = [];
  for (let start = 0; start < n; start++) {
    const forward = [];
    const backward = [];
    for (let i = 0; i < n; i++) {
      const f = parts[(start + i) % n];
      forward.push(f.corner, f.length);
      const b = parts[(start - i + 2 * n) % n];
      backward.push(b.corner, parts[(start - i - 1 + 2 * n) % n].length);
    }
    forms.push(round(forward), round(backward));
  }
  return forms.sort()[0];
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
    report(Math.abs(sum - lattice) < 1e-7 && Math.abs(Math.abs(area(tiling.polygons[0].vertices)) - 1) < 1e-9, `${label}: no gaps, unit area`);

    // Each sample point has to land in exactly the cell whose polygon contains it.
    let wrong = 0;
    // Not from 0, because the origin itself can lie exactly on an edge.
    for (let i = 1; i <= 400; i++) {
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
    const period = torusPeriod(family, preset.name);
    for (const size of [{ width: 40, height: 38 }, { width: 66, height: 128 }]) {
      const { width, height } = fitTorus(size, period);
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

    if (info.fixed) {
      // Lattice copies must leave no gaps, as checked above, and no overlaps.
      let overlaps = 0;
      for (const a of tiling.polygons) {
        for (const b of tiling.polygons) {
          for (let i = -3; i <= 3; i++) {
            for (let j = -3; j <= 3; j++) {
              if (a === b && i === 0 && j === 0) continue;
              const moved = b.vertices.map((v) => [v[0] + i * tiling.t1[0] + j * tiling.t2[0], v[1] + i * tiling.t1[1] + j * tiling.t2[1]]);
              if (hasOverlap(a.vertices, moved)) overlaps++;
            }
          }
        }
      }
      report(overlaps === 0, `${label}: tiles do not overlap (${overlaps})`);

      const source = PENTAGONS.find((p) => `Type ${p.type}` === preset.name);
      const measured = measure(source.pentagon);
      const off = Math.max(...PENTAGON_CONDITIONS[source.type](measured).map(Math.abs));
      report(off < 1e-5, `${label}: meets the conditions of its type (off by ${off.toExponential(1)})`);
      const allAngles = [measured.A, measured.B, measured.C, measured.D, measured.E];
      report(allAngles.every((a) => a < 180 - 1e-3), `${label}: convex`);
      const expected = signature(source.pentagon);
      report(tiling.polygons.every((polygon) => signature(polygon.vertices) === expected), `${label}: all ${tiling.polygons.length} tiles congruent to the pentagon`);
    }

    for (const n of neighbourhoodsOf(family, preset.name)) {
      const withCorners = n.topology !== 'vonNeumann' && !n.topology.endsWith('Edges');
      const table = new Map();
      const counts = new Set();
      const [periodX, periodY] = period;
      const classKey = (x, y) => `${((x % periodX) + periodX) % periodX},${((y % periodY) + periodY) % periodY}`;
      for (let y = 0; y < periodY; y++) {
        for (let x = 0; x < periodX; x++) {
          const found = geometricNeighbours(tiling, x, y, withCorners);
          table.set(classKey(x, y), found);
          counts.add(found.length);
        }
      }
      const fewest = n.fewest ?? n.count;
      report(Math.max(...counts) === n.count && Math.min(...counts) === fewest, `${label}: ${n.name} has ${[...counts]} neighbours, expected ${fewest} to ${n.count}`);

      const neighboursOf = (x, y) => table.get(classKey(x, y));
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

// The hat's triangles have to add up to its outline at every position of the slider.
// Some positions, such as the hat itself, have corners exactly in line with others.
{
  // Creating the hat can grow the memory, so the buffer is only fetched afterwards.
  const pointer = e.hat_outline(e.hat_new());
  const outline = new Int32Array(e.memory.buffer, pointer, 14 * 4).slice();
  const hex = (q, r) => [q + 0.5 * r, (Math.sqrt(3) / 2) * r];
  const wrong = [];
  const cracked = [];
  for (let degrees = 6; degrees <= 84; degrees++) {
    const angle = (degrees * Math.PI) / 180;
    const [a, b] = [Math.cos(angle), Math.sin(angle) / Math.sqrt(3)];
    const local = Array.from({ length: 14 }, (_, i) => {
      const p = hex(outline[i * 4], outline[i * 4 + 1]);
      const q = hex(outline[i * 4 + 2], outline[i * 4 + 3]);
      return [a * p[0] + b * q[0], a * p[1] + b * q[1]];
    });
    const triangles = earClip(local);
    let covered = 0;
    for (let i = 0; i < triangles.length; i += 3) covered += Math.abs(area([local[triangles[i]], local[triangles[i + 1]], local[triangles[i + 2]]]));
    if (Math.abs(covered - Math.abs(area(local))) > 1e-9) wrong.push(degrees);
    // A corner of one triangle on the edge of another leaves pinholes when rasterised.
    const onEdge = (w, u, v) => {
      const [dx, dy] = [local[v][0] - local[u][0], local[v][1] - local[u][1]];
      const [px, py] = [local[w][0] - local[u][0], local[w][1] - local[u][1]];
      const along = (px * dx + py * dy) / (dx * dx + dy * dy);
      return along > 1e-9 && along < 1 - 1e-9 && Math.abs(px * dy - py * dx) < 1e-9;
    };
    const used = new Set(triangles);
    for (let i = 0; i < triangles.length; i += 3) {
      for (const [u, v] of [[triangles[i], triangles[i + 1]], [triangles[i + 1], triangles[i + 2]], [triangles[i + 2], triangles[i]]]) {
        if ([...used].some((w) => w !== u && w !== v && onEdge(w, u, v)) && !cracked.includes(degrees)) cracked.push(degrees);
      }
    }
  }
  report(wrong.length === 0, `hat triangles match the outline at every angle${wrong.length ? `, wrong at ${wrong.join(', ')} degrees` : ''}`);
  report(cracked.length === 0, `hat triangles meet corner to corner at every angle${cracked.length ? `, not at ${cracked.join(', ')} degrees` : ''}`);
}

console.log(failures ? `${failures} checks failed` : 'all tiling checks passed');
process.exitCode = failures ? 1 : 0;
