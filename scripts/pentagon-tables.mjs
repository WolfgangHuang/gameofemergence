// Puts the pentagons of each tiling from pentagon-tilings.mjs on the engine's
// integer grid, reads the neighbours off the polygons, and writes
//   src/tiles/pentagons.json   what the renderer and the panels need
//   engine/src/pentagons.rs    what the engine needs
//
// A grid position means nothing to the simulation beyond its neighbours, but the
// engine reaches at most two columns and one row, so cells have to be laid out
// to stay within that.
//
// Usage: node scripts/pentagon-tables.mjs raw.json [--dry]
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const MAX_DX = 2;
const MAX_DY = 1;
const TOUCH = 1e-6;

const sub = (a, b) => [a[0] - b[0], a[1] - b[1]];
const cross = (a, b) => a[0] * b[1] - a[1] * b[0];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1];
const norm = (a) => Math.hypot(a[0], a[1]);
const mod = (a, n) => ((a % n) + n) % n;

/** How two polygons touch: 2 along a stretch of edge, 1 in a point, 0 not at all. */
function contact(a, b) {
  let kind = 0;
  for (let i = 0; i < a.length; i++) {
    const p = a[i];
    const q = a[(i + 1) % a.length];
    const edge = sub(q, p);
    const length = norm(edge);
    const unit = [edge[0] / length, edge[1] / length];
    for (let j = 0; j < b.length; j++) {
      const r = b[j];
      const s = b[(j + 1) % b.length];
      const offR = cross(unit, sub(r, p));
      const offS = cross(unit, sub(s, p));
      const tr = dot(unit, sub(r, p));
      const ts = dot(unit, sub(s, p));
      if (Math.abs(offR) < TOUCH && Math.abs(offS) < TOUCH) {
        const shared = Math.min(length, Math.max(tr, ts)) - Math.max(0, Math.min(tr, ts));
        if (shared > TOUCH) return 2;
        if (shared > -TOUCH) kind = 1;
      } else if ((Math.abs(offR) < TOUCH && tr > -TOUCH && tr < length + TOUCH) || (Math.abs(offS) < TOUCH && ts > -TOUCH && ts < length + TOUCH)) {
        kind = 1;
      }
    }
  }
  return kind;
}

/** Everything touching each polygon of a period, as [other, u, v, kind]. */
function touching({ polygons, t1, t2 }) {
  return polygons.map((polygon) => {
    const list = [];
    for (let u = -4; u <= 4; u++) {
      for (let v = -4; v <= 4; v++) {
        polygons.forEach((other, index) => {
          if (other === polygon && u === 0 && v === 0) return;
          const moved = other.vertices.map((p) => [p[0] + u * t1[0] + v * t2[0], p[1] + u * t1[1] + v * t2[1]]);
          const kind = contact(polygon.vertices, moved);
          if (kind) list.push([index, u, v, kind]);
        });
      }
    }
    return list;
  });
}

const centre = (vertices) => {
  const sum = vertices.reduce((total, v) => [total[0] + v[0], total[1] + v[1]], [0, 0]);
  return [sum[0] / vertices.length, sum[1] / vertices.length];
};

/** Lattice bases that describe the same lattice, short ones. */
function latticeBases(t1, t2) {
  const vectors = [];
  for (let i = -2; i <= 2; i++) for (let j = -2; j <= 2; j++) if (i || j) vectors.push({ i, j, v: [i * t1[0] + j * t2[0], i * t1[1] + j * t2[1]] });
  const bases = [];
  for (const a of vectors) for (const b of vectors) {
    if (a.i * b.j - a.j * b.i === 1) bases.push([a, b]);
  }
  return bases;
}

/**
 * Looks for a layout within the engine's reach. The positions of cells matter only
 * through their neighbours' offsets, so this is a constraint problem: each polygon
 * takes a coset of the grid lattice, and every contact has to stay within reach.
 * Layouts that keep neighbours close make the picture of the grid less confusing,
 * but nothing depends on that.
 */
function embed(tiling, touches) {
  const n = tiling.polygons.length;
  let best = null;
  const seenLattices = new Set();
  for (const [a, b] of latticeBases(tiling.t1, tiling.t2)) {
    const world = cross(a.v, b.v);
    const convert = (u, v) => {
      const t = [u * tiling.t1[0] + v * tiling.t2[0], u * tiling.t1[1] + v * tiling.t2[1]];
      return [Math.round(cross(t, b.v) / world), Math.round(cross(a.v, t) / world)];
    };
    const contacts = touches.map((list) => list.map(([j, u, v, kind]) => ({ j, kind, step: convert(u, v) })));
    for (let P = 1; P <= n; P++) {
      if (n % P) continue;
      const Q = n / P;
      {
        // Rectangular periods let the engine find a cell's class from x and y alone.
        const s = 0;
        const g1 = [P, 0];
        const g2 = [s, Q];
        const signature = JSON.stringify([contacts.map((c) => c.map((o) => [o.j, o.step])), P, s]);
        if (seenLattices.has(signature)) continue;
        seenLattices.add(signature);
        // A polygon may sit a few lattice steps from the coset's first cell, because
        // the polygons of a period are not where the grid would put them.
        const cosets = [];
        for (let y = 0; y < Q; y++) {
          for (let x = 0; x < P; x++) {
            const shifted = [];
            for (let i = -3; i <= 3; i++) for (let j = -3; j <= 3; j++) shifted.push([x + i * g1[0] + j * g2[0], y + i * g1[1] + j * g2[1]]);
            cosets.push(shifted);
          }
        }
        const period = P * Q;
        const cells = new Array(n).fill(null);
        const taken = new Array(n).fill(false);
        const offset = (i, o) => [
          cells[o.j][0] + o.step[0] * g1[0] + o.step[1] * g2[0] - cells[i][0],
          cells[o.j][1] + o.step[0] * g1[1] + o.step[1] * g2[1] - cells[i][1],
        ];
        // Polygons with the most contacts to those already placed come first.
        const order = [0];
        while (order.length < n) {
          let pick = -1;
          let links = -1;
          for (let i = 0; i < n; i++) {
            if (order.includes(i)) continue;
            const count = contacts[i].filter((o) => order.includes(o.j)).length;
            if (count > links) [pick, links] = [i, count];
          }
          order.push(pick);
        }
        const fitsSoFar = (i) =>
          contacts[i].every((o) => {
            if (!cells[o.j]) return true;
            const [dx, dy] = offset(i, o);
            return Math.abs(dx) <= MAX_DX && Math.abs(dy) <= MAX_DY && (dx || dy);
          });
        let visits = 0;
        const place = (k) => {
          if (k === n) {
            // Distinct neighbours must stay distinct on the grid, and contacts
            // have to agree in both directions.
            let spread = 0;
            const offsets = contacts.map((list, i) =>
              list.map((o) => {
                const [dx, dy] = offset(i, o);
                spread += dx * dx + dy * dy;
                return { dx, dy, kind: o.kind };
              }),
            );
            if (!offsets.every((list) => new Set(list.map((o) => `${o.dx},${o.dy}`)).size === list.length)) return;
            // A small period keeps more torus sizes possible.
            const score = period * 1000 + spread;
            if (!best || score < best.score) best = { score, fits: true, maxX: Math.max(...offsets.flat().map((o) => Math.abs(o.dx))), maxY: Math.max(...offsets.flat().map((o) => Math.abs(o.dy))), g1, g2, a: a.v, b: b.v, cells: cells.map((c) => [...c]), offsets, P, Q, s };
            return;
          }
          const i = order[k];
          for (let c = 0; c < n && visits < 200000; c++) {
            if (taken[c]) continue;
            taken[c] = true;
            // The first polygon only fixes where the grid starts.
            for (const cell of k === 0 ? [cosets[c][24]] : cosets[c]) {
              visits++;
              cells[i] = cell;
              if (fitsSoFar(i) && contacts.every((list, x) => !cells[x] || x === i || list.every((o) => o.j !== i || (Math.abs(offset(x, o)[0]) <= MAX_DX && Math.abs(offset(x, o)[1]) <= MAX_DY)))) place(k + 1);
              cells[i] = null;
            }
            taken[c] = false;
            if (k === 0) break;
          }
        };
        place(0);
      }
    }
  }
  return best;
}

const args = process.argv.slice(2);
const raw = JSON.parse(readFileSync(args.find((a) => !a.startsWith('--')), 'utf8'));
const dry = args.includes('--dry');
const root = resolve(import.meta.dirname, '..');

const out = [];
for (const key of Object.keys(raw).sort((a, b) => a - b)) {
  const tiling = raw[key];
  const touches = touching(tiling);
  const layout = embed(tiling, touches);
  if (!layout) {
    console.log(`type ${key}: ${tiling.polygons.length} tiles, NO LAYOUT within reach`);
    out.push({ tiling, layout: { fits: false }, key: Number(key) });
    continue;
  }
  const counts = (kinds) => [...new Set(touches.map((list) => list.filter((t) => kinds.includes(t[3])).length))].sort((a, b) => a - b);
  console.log(`type ${key}: ${tiling.polygons.length} tiles, g1 ${layout.g1}, g2 ${layout.g2}, reach ${layout.maxX}x${layout.maxY}${layout.fits ? '' : ' TOO FAR'}, edges ${counts([2])}, all ${counts([1, 2])}`);
  out.push({ tiling, layout, key: Number(key) });
}

if (!dry) {
  if (out.some((o) => !o.layout.fits)) throw new Error('some layout does not fit the engine');
  const json = out.map(({ key, tiling, layout }) => {
    // Height of the rectangle after which the classes repeat.
    const { P, Q, s } = layout;
    const width = P;
    const height = Q;
    // Moves the picture so that polygons lie as close as they can to their grid
    // position, because the view only asks the engine for what it thinks is visible.
    const world = cross(layout.g1, layout.g2);
    const toWorld = ([x, y]) => {
      const u = cross([x, y], layout.g2) / world;
      const v = cross(layout.g1, [x, y]) / world;
      return [u * layout.a[0] + v * layout.b[0], u * layout.a[1] + v * layout.b[1]];
    };
    const off = tiling.polygons.map((p, i) => {
      const [wx, wy] = toWorld([layout.cells[i][0] + 0.5, layout.cells[i][1] + 0.5]);
      const c = centre(p.vertices);
      return [wx - c[0], wy - c[1]];
    });
    const shift = [off.reduce((t, o) => t + o[0], 0) / off.length, off.reduce((t, o) => t + o[1], 0) / off.length];
    const round = (v) => [Number(v[0].toFixed(12)), Number(v[1].toFixed(12))];
    const farthest = Math.max(...off.map((o) => norm(sub(o, shift))));
    console.log(`type ${key}: period ${width}x${height}, polygons up to ${farthest.toFixed(2)} from their grid position`);
    return {
      type: key,
      pentagon: tiling.pentagon.map(round),
      angles: tiling.angles,
      sides: tiling.sides,
      t1: round(layout.a),
      t2: round(layout.b),
      g1: layout.g1,
      g2: layout.g2,
      period: [width, height],
      polygons: tiling.polygons.map((p, i) => ({ cell: layout.cells[i], vertices: p.vertices.map((v) => round([v[0] + shift[0], v[1] + shift[1]])) })),
      neighbours: layout.offsets.map((list) => list.map((o) => [o.dx, o.dy, o.kind])),
    };
  });
  writeFileSync(resolve(root, 'src/tiles/pentagons.json'), `${JSON.stringify(json)}\n`);

  // Rust tables per residue class (x mod width, y mod height).
  const lines = ['//! Generated by scripts/pentagon-tables.mjs, do not edit.', '', 'use crate::life::Classes;', ''];
  const tables = [];
  for (const { key, layout } of out) {
    const { P, Q, s } = layout;
    const width = P;
    const height = Q;
    for (const [suffix, kinds] of [['ALL', [1, 2]], ['EDGES', [2]]]) {
      const name = `T${key}_${suffix}`;
      const rows = [];
      for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
          const v = Math.floor(y / Q);
          const cx = mod(x - v * s, P);
          const cy = y - v * Q;
          // Same coset: the difference is a whole number of lattice steps.
          const polygon = layout.cells.findIndex((c) => {
            const dy = c[1] - cy;
            if (mod(dy, Q)) return false;
            return mod(c[0] - cx - (dy / Q) * s, P) === 0;
          });
          if (polygon < 0) throw new Error(`type ${key}: nothing at ${x},${y}`);
          const list = layout.offsets[polygon].filter((o) => kinds.includes(o.kind)).map((o) => `(${o.dx}, ${o.dy})`);
          if (list.length > 15) throw new Error(`type ${key}: ${list.length} neighbours`);
          rows.push(`    &[${list.join(', ')}],`);
        }
      }
      lines.push(`const ${name}: &[&[(i32, i32)]] = &[`, ...rows, '];');
      tables.push(`    Classes { width: ${width}, height: ${height}, cells: ${name} },`);
    }
  }
  lines.push('', '/// Two per type, in order: edges and corners, then edges only.', `pub static TABLES: [Classes; ${tables.length}] = [`, ...tables, '];', '');
  writeFileSync(resolve(root, 'engine/src/pentagons.rs'), lines.join('\n'));
}
