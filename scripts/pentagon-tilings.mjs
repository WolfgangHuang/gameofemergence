// Finds a periodic tiling for each of the 15 known types of convex pentagon.
//
// The literature gives conditions on sides and angles, not how the tiles lie. So
// this builds one pentagon per type from those conditions, then searches for a
// tiling by backtracking and keeps the one with the smallest period. Whatever the
// search finds is checked again without its help: lattice copies of one period
// must not overlap, and their area must fill the lattice cell.
//
// Usage: node scripts/pentagon-tilings.mjs [out.json] [--types=1,2,3] [--seconds=60]
import { writeFileSync } from 'node:fs';

const EPS = 1e-7;
const rad = (d) => (d * Math.PI) / 180;

// Sides as on Wikipedia's "Pentagonal tiling": going round, a lies between E and A,
// b between A and B, c between B and C, d between C and D, e between D and E.
// Each type lists its conditions as expressions that must vanish, then a start
// close to the pentagon wanted. Free parameters end up near that start, which is
// chosen away from special cases, so the pentagon is not also a simpler type.
const TYPES = {
  1: { when: (p) => [p.B + p.C - 180, p.A + p.D + p.E - 360], angles: [112, 103, 77, 128, 120], sides: [1.15, 0.8, 1.05, 1.3, 0.9] },
  2: { when: (p) => [p.c - p.e, p.B + p.D - 180], angles: [118, 84, 122, 96, 120], sides: [1.3, 0.9, 1, 1.25, 1] },
  3: { when: (p) => [p.a - p.b, p.d - p.c - p.e, p.A - 120, p.C - 120, p.D - 120], angles: [120, 95, 120, 120, 85], sides: [1, 1, 0.55, 1.45, 0.9] },
  4: { when: (p) => [p.b - p.c, p.d - p.e, p.B - 90, p.D - 90], angles: [125, 90, 110, 90, 125], sides: [0.7, 1, 1, 1.2, 1.2] },
  5: { when: (p) => [p.a - p.b, p.d - p.e, p.A - 60, p.D - 120], angles: [60, 130, 120, 120, 110], sides: [1.3, 1.3, 0.8, 0.7, 0.7] },
  6: { when: (p) => [p.a - p.d, p.d - p.e, p.b - p.c, p.B + p.D - 180, 2 * p.B - p.E], angles: [125, 68, 125, 112, 136], sides: [1, 1.2, 1.2, 1, 1] },
  7: { when: (p) => [p.b - p.c, p.c - p.d, p.d - p.e, p.B + 2 * p.E - 360, 2 * p.C + p.D - 360], angles: [95, 105, 115, 130, 127.5], sides: [1.3, 1, 1, 1, 1] },
  8: { when: (p) => [p.b - p.c, p.c - p.d, p.d - p.e, 2 * p.B + p.C - 360, p.D + 2 * p.E - 360], angles: [115, 135, 90, 80, 140], sides: [1.2, 1, 1, 1, 1] },
  9: { when: (p) => [p.b - p.c, p.c - p.d, p.d - p.e, 2 * p.A + p.C - 360, p.D + 2 * p.E - 360], angles: [120, 81.75, 120, 76.5, 141.75], sides: [0.43, 1, 1, 1, 1], patch: 1.5 },
  10: { when: (p) => [p.a - p.b, p.b - p.c - p.e, p.A - 90, p.B + p.E - 180, p.B + 2 * p.C - 360], angles: [90, 100, 130, 140, 80], sides: [1, 1, 0.45, 1.1, 0.55] },
  11: { when: (p) => [2 * p.a + p.c - p.d, p.d - p.e, p.A - 90, p.C + p.E - 180, 2 * p.B + p.C - 360], angles: [90, 145, 70, 125, 110], sides: [0.4, 1, 0.5, 1.3, 1.3] },
  12: { when: (p) => [2 * p.a - p.d, p.d - p.c - p.e, p.A - 90, p.C + p.E - 180, 2 * p.B + p.C - 360], angles: [90, 150, 60, 120, 120], sides: [0.75, 1.1, 0.7, 1.5, 0.8] },
  13: { when: (p) => [p.d - 2 * p.a, p.a - p.e, p.B - 90, p.E - 90, 2 * p.A + p.D - 360], angles: [105, 90, 105, 150, 90], sides: [0.7, 1.1, 0.7, 1.4, 0.7], patch: 1.4 },
  14: { when: (p) => [2 * p.a - 2 * p.c, 2 * p.c - p.d, p.d - p.e, p.A - 90, 2 * p.B + p.C - 360, p.C + p.E - 180], angles: [90, 145.34, 69.32, 124.66, 110.68], sides: [0.6, 1.3, 0.6, 1.2, 1.2] },
  15: { when: (p) => [p.a - p.c, p.c - p.e, p.b - 2 * p.a, p.A - 150, p.B - 60, p.C - 135, p.D - 105, p.E - 90], angles: [150, 60, 135, 105, 90], sides: [0.6, 1.2, 0.6, 1, 0.6], patch: 1.5 },
};

const named = (x) => ({ A: x[0], B: x[1], C: x[2], D: x[3], E: x[4], a: x[5], b: x[6], c: x[7], d: x[8], e: x[9] });

/** Vertices A, B, C, D, E, counterclockwise, from angles in degrees and sides. */
function walk(p) {
  const steps = [[p.b, p.B], [p.c, p.C], [p.d, p.D], [p.e, p.E], [p.a, p.A]];
  let at = [0, 0];
  let heading = 0;
  const vertices = [];
  for (const [length, angle] of steps) {
    vertices.push(at);
    at = [at[0] + length * Math.cos(heading), at[1] + length * Math.sin(heading)];
    heading += rad(180 - angle);
  }
  return { vertices, end: at };
}

function residuals(x, when) {
  const p = named(x);
  const { end } = walk(p);
  return [end[0], end[1], p.A + p.B + p.C + p.D + p.E - 540, p.a + p.b + p.c + p.d + p.e - 5, ...when(p)];
}

function solveLinear(m, b) {
  const n = b.length;
  const a = m.map((row, i) => [...row, b[i]]);
  for (let i = 0; i < n; i++) {
    let pivot = i;
    for (let r = i + 1; r < n; r++) if (Math.abs(a[r][i]) > Math.abs(a[pivot][i])) pivot = r;
    [a[i], a[pivot]] = [a[pivot], a[i]];
    if (Math.abs(a[i][i]) < 1e-12) return null;
    for (let r = 0; r < n; r++) {
      if (r === i) continue;
      const f = a[r][i] / a[i][i];
      for (let c = i; c <= n; c++) a[r][c] -= f * a[i][c];
    }
  }
  return a.map((row, i) => row[n] / row[i]);
}

/** Newton with the smallest step, so that free parameters stay near the start. */
function solvePentagon({ when, angles, sides }) {
  let x = [...angles, ...sides];
  for (let iteration = 0; iteration < 200; iteration++) {
    const r = residuals(x, when);
    if (Math.max(...r.map(Math.abs)) < 1e-12) break;
    const jacobian = r.map(() => new Array(x.length));
    x.forEach((_, j) => {
      const nudged = [...x];
      nudged[j] += 1e-6;
      const up = residuals(nudged, when);
      r.forEach((value, i) => (jacobian[i][j] = (up[i] - value) / 1e-6));
    });
    const jjt = r.map((_, i) => r.map((__, k) => jacobian[i].reduce((sum, v, j) => sum + v * jacobian[k][j], 0) + (i === k ? 1e-10 : 0)));
    const y = solveLinear(jjt, r);
    if (!y) throw new Error('singular conditions');
    x = x.map((v, j) => v - r.reduce((sum, _, i) => sum + jacobian[i][j] * y[i], 0));
  }
  const final = residuals(x, when);
  if (Math.max(...final.map(Math.abs)) > 1e-9) throw new Error(`conditions not met: ${final}`);
  const p = named(x);
  const { vertices } = walk(p);
  const convex = [p.A, p.B, p.C, p.D, p.E].every((a) => a > 1 && a < 179);
  const positive = [p.a, p.b, p.c, p.d, p.e].every((s) => s > 0.05);
  if (!convex || !positive) throw new Error(`not a convex pentagon: ${JSON.stringify(p)}`);
  return { vertices, p };
}

const sub = (a, b) => [a[0] - b[0], a[1] - b[1]];
const cross = (a, b) => a[0] * b[1] - a[1] * b[0];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1];
const norm = (a) => Math.hypot(a[0], a[1]);

function polygonArea(v) {
  let s = 0;
  v.forEach((a, i) => (s += cross(a, v[(i + 1) % v.length])));
  return s / 2;
}

/** Whether two convex polygons share interior, by separating axes. */
function overlap(a, b) {
  for (const [p, q] of [[a, b], [b, a]]) {
    for (let i = 0; i < p.length; i++) {
      const edge = sub(p[(i + 1) % p.length], p[i]);
      const length = norm(edge);
      const n = [-edge[1] / length, edge[0] / length];
      // Counterclockwise, so the interior of p lies on the side of n.
      const reach = Math.max(...q.map((v) => dot(sub(v, p[i]), n)));
      if (reach < EPS) return false;
    }
  }
  return true;
}

/** Sectors a convex tile fills around a point: null if it does not touch it. */
function sector(tile, point) {
  const n = tile.length;
  for (let i = 0; i < n; i++) {
    if (norm(sub(tile[i], point)) < EPS) {
      const out = sub(tile[(i + 1) % n], point);
      const back = sub(tile[(i + n - 1) % n], point);
      return [Math.atan2(out[1], out[0]), Math.atan2(back[1], back[0])];
    }
  }
  for (let i = 0; i < n; i++) {
    const p = tile[i];
    const q = tile[(i + 1) % n];
    const edge = sub(q, p);
    const length = norm(edge);
    const along = dot(sub(point, p), edge) / length;
    if (along > 0 && along < length && Math.abs(cross(edge, sub(point, p))) / length < EPS) {
      const angle = Math.atan2(edge[1], edge[0]);
      return [angle, angle + Math.PI];
    }
  }
  return null;
}

/** Whether a corner of one tile lies inside an edge of the other. */
function endsInside(tile, other) {
  return other.some((point) => {
    const s = sector(tile, point);
    return s && Math.abs(wrap(s[1] - s[0]) - Math.PI) < 1e-6;
  });
}

const TAU = 2 * Math.PI;
const wrap = (a) => ((a % TAU) + TAU) % TAU;

/** The free angular gaps around a point, as [start, size]. */
function gaps(tiles, point) {
  const sectors = [];
  for (const t of tiles) {
    if (Math.abs(t.centre[0] - point[0]) > t.radius + EPS || Math.abs(t.centre[1] - point[1]) > t.radius + EPS) continue;
    const s = sector(t.vertices, point);
    if (s) sectors.push([wrap(s[0]), wrap(s[1] - s[0])]);
  }
  if (!sectors.length) return [[0, TAU]];
  sectors.sort((a, b) => a[0] - b[0]);
  const found = [];
  for (let i = 0; i < sectors.length; i++) {
    const end = sectors[i][0] + sectors[i][1];
    const next = i + 1 < sectors.length ? sectors[i + 1][0] : sectors[0][0] + TAU;
    if (next - end > 1e-6) found.push([wrap(end), next - end]);
  }
  return found;
}

function makeTile(vertices, mirrored) {
  const centre = [0, 0];
  vertices.forEach((v) => {
    centre[0] += v[0] / vertices.length;
    centre[1] += v[1] / vertices.length;
  });
  const radius = Math.max(...vertices.map((v) => norm(sub(v, centre))));
  return { vertices, centre, radius, mirrored };
}

/** All copies of the pentagon as vertex lists, with their own edge starting at 0. */
function shapes(vertices) {
  const list = [];
  const n = vertices.length;
  for (const mirrored of [false, true]) {
    const base = mirrored ? [...vertices].reverse().map(([x, y]) => [-x, y]) : vertices;
    for (let i = 0; i < n; i++) {
      const from = base[i];
      const out = sub(base[(i + 1) % n], from);
      const back = sub(base[(i + n - 1) % n], from);
      const heading = Math.atan2(out[1], out[0]);
      const [c, s] = [Math.cos(-heading), Math.sin(-heading)];
      const local = base.map((v) => {
        const [x, y] = sub(v, from);
        return [c * x - s * y, s * x + c * y];
      });
      const corner = Math.acos(dot(out, back) / (norm(out) * norm(back)));
      list.push({ mirrored, local, corner, length: norm(out) });
    }
  }
  return list;
}

/** Offsets at which an edge can overhang a corner, made of a few side lengths. */
function slides(sides) {
  const values = new Set();
  const lengths = [...sides, 0];
  for (const a of lengths) for (const b of lengths) for (const c of lengths) {
    for (const v of [a + b - c, a - b]) {
      if (v > 1e-6) values.add(Math.round(v * 1e9) / 1e9);
    }
  }
  return [...values].sort((x, y) => x - y);
}

function place(shape, point, angle, slide) {
  const [c, s] = [Math.cos(angle), Math.sin(angle)];
  const vertices = shape.local.map(([x, y]) => [point[0] + c * (x - slide) - s * y, point[1] + s * (x - slide) + c * y]);
  return makeTile(vertices, shape.mirrored);
}

/** Copies that map a filled patch onto itself, as far as the patch can tell. */
function translations(tiles, radius) {
  const key = (v) => `${Math.round(v[0] * 1e5)},${Math.round(v[1] * 1e5)}`;
  const present = new Set(tiles.map((t) => `${key(t.centre)}|${t.mirrored}|${key(sub(t.vertices[0], t.centre))}`));
  const origin = tiles[0];
  const found = [];
  for (const t of tiles) {
    if (t === origin || t.mirrored !== origin.mirrored) continue;
    const shift = sub(t.vertices[0], origin.vertices[0]);
    if (norm(sub(sub(t.vertices[1], origin.vertices[1]), shift)) > 1e-6 || norm(shift) > radius * 0.8) continue;
    // The rim of a patch can still be wrong without the core being so.
    const reach = radius - norm(shift) - 2 * origin.radius - 1;
    const near = tiles.filter((u) => norm(u.centre) < reach);
    const fits = near.every((u) => present.has(`${key([u.centre[0] + shift[0], u.centre[1] + shift[1]])}|${u.mirrored}|${key(sub(u.vertices[0], u.centre))}`));
    if (fits && near.length >= 4) found.push(shift);
  }
  return found.sort((a, b) => norm(a) - norm(b));
}

/** Candidate lattices from the translations, smallest cell first, with short vectors. */
function bases(list) {
  const seen = new Map();
  const limited = list.slice(0, 40);
  for (let i = 0; i < limited.length; i++) {
    for (let j = i + 1; j < limited.length; j++) {
      let [t1, t2] = [limited[i], limited[j]];
      const size = Math.abs(cross(t1, t2));
      if (size < 1e-6) continue;
      // Lagrange reduction, which keeps the cell and shortens its sides.
      for (let k = 0; k < 20; k++) {
        if (norm(t1) > norm(t2)) [t1, t2] = [t2, t1];
        const m = Math.round(dot(t1, t2) / dot(t1, t1));
        if (m === 0) break;
        t2 = [t2[0] - m * t1[0], t2[1] - m * t1[1]];
      }
      const key = `${Math.round(size * 1e4)}|${Math.round(norm(t1) * 1e4)}|${Math.round(norm(t2) * 1e4)}`;
      if (!seen.has(key)) seen.set(key, { size, t1, t2 });
    }
  }
  return [...seen.values()].sort((p, q) => p.size - q.size);
}

/** Every angle that corners of the pentagon can fill, with or without a straight edge. */
function fillableAngles(corners) {
  const values = new Set([0]);
  const key = (a) => Math.round(a * 1e6);
  let frontier = [0];
  const found = [0];
  while (frontier.length) {
    const next = [];
    for (const a of frontier) {
      for (const c of corners) {
        const sum = a + c;
        if (sum < TAU + 1e-6 && !values.has(key(sum))) {
          values.add(key(sum));
          next.push(sum);
          found.push(sum);
        }
      }
    }
    frontier = next;
  }
  return found;
}

function search(vertices, seconds, target, edgeToEdge, patch = 1.5) {
  const all = shapes(vertices);
  const fillable = fillableAngles(all.slice(0, 5).map((s) => s.corner));
  const canFill = (size) => size > TAU - 1e-6 || fillable.some((a) => Math.abs(a - size) < 1e-5 || (!edgeToEdge && Math.abs(a + Math.PI - size) < 1e-5)) || size > Math.PI * 1.02;
  const unit = polygonArea(vertices);
  const sides = vertices.map((v, i) => norm(sub(vertices[(i + 1) % 5], v)));
  const overhangs = slides(sides);
  const smallest = Math.min(...all.map((s) => s.corner));
  const scale = Math.sqrt(unit);
  // Enough to see two periods in every direction, and no more: effort grows
  // quickly with the size of the patch.
  const radius = (3 + 1.3 * Math.sqrt(target)) * scale * patch;
  const start = makeTile(vertices, false);
  const centre = start.centre;
  const shifted = makeTile(vertices.map((v) => sub(v, centre)), false);
  const deadline = Date.now() + seconds * 1000;
  let best = null;
  let nodes = 0;
  let deepest = 0;
  let jumpTo = 0;
  let patches = 0;

  const tiles = [shifted];

  function choose() {
    let pick = null;
    const seen = new Set();
    for (const t of tiles) {
      for (const v of t.vertices) {
        if (norm(v) > radius) continue;
        const k = `${Math.round(v[0] * 1e6)},${Math.round(v[1] * 1e6)}`;
        if (seen.has(k)) continue;
        seen.add(k);
        for (const [from, size] of gaps(tiles, v)) {
          if (size < smallest - 1e-6 || !canFill(size)) return { dead: true };
          // Nearest first, so that a mistake shows before anything unrelated is
          // tried on top of it. Among equals, the tightest gap has the fewest options.
          // A corner with more than a straight angle free can take the most tiles.
          const score = norm(v) + size * 0.15 + (size > Math.PI + 1e-6 ? 1.5 : 0);
          if (!pick || score < pick.score) pick = { score, point: v, from, size };
        }
      }
    }
    return pick;
  }

  function descend() {
    if (Date.now() > deadline) return true;
    nodes++;
    const pick = choose();
    if (pick?.dead) return false;
    deepest = Math.max(deepest, tiles.length);
    if (!pick) {
      patches++;
      for (const lattice of bases(translations(tiles, radius))) {
        const count = Math.round(lattice.size / unit);
        if (best && count >= best.count) break;
        const candidate = { count, t1: lattice.t1, t2: lattice.t2, tiles: tiles.map((t) => ({ ...t })) };
        // A small patch can look periodic by chance, so this has to hold up too.
        if (!extract(candidate, unit).problem) {
          best = candidate;
          break;
        }
      }
      if (best && best.count <= target) return true;
      // The rim has many ways to fill up and none of them repairs the core.
      jumpTo = Math.ceil(tiles.length * 0.5);
      return false;
    }
    const { point, from, size } = pick;
    const options = [];
    for (const shape of all) {
      if (shape.corner <= size + 1e-6) options.push([shape, 0]);
      // A long edge passing the corner needs at least a straight angle of room.
      if (!edgeToEdge && size >= Math.PI - 1e-6) {
        for (const slide of overhangs) if (slide < shape.length - 1e-6) options.push([shape, slide]);
      }
    }
    for (const [shape, slide] of options) {
      const tile = place(shape, point, from, slide);
      const close = tiles.filter((t) => norm(sub(t.centre, tile.centre)) < t.radius + tile.radius + EPS);
      if (close.some((t) => overlap(t.vertices, tile.vertices))) continue;
      if (edgeToEdge && close.some((t) => endsInside(t.vertices, tile.vertices) || endsInside(tile.vertices, t.vertices))) continue;
      tiles.push(tile);
      const stop = descend();
      tiles.pop();
      if (stop) return true;
      if (jumpTo && tiles.length > jumpTo) return false;
      jumpTo = 0;
    }
    return false;
  }

  const finished = !descend();
  if (process.env.PENTAGON_DEBUG) console.log(`  ${edgeToEdge ? 'edge-to-edge' : 'loose'}: ${nodes} nodes, deepest ${deepest}, ${patches} full patches, radius ${(radius / scale).toFixed(1)}, ${finished ? 'exhausted' : 'stopped'}, ${overhangs.length} overhangs, sides ${sides.map((v) => v.toFixed(3))}`);
  return { best, nodes };
}

/**
 * Type 3 tiles by three-fold rotations about A and D. Side a turns onto b, and
 * sides c and e together turn onto d, about C and D.
 */
function threefold(vertices) {
  const [A, , , D] = vertices;
  const [c, s] = [Math.cos(TAU / 3), Math.sin(TAU / 3)];
  const turn = (v, o) => [o[0] + c * (v[0] - o[0]) - s * (v[1] - o[1]), o[1] + s * (v[0] - o[0]) + c * (v[1] - o[1])];
  const copies = [vertices, vertices.map((v) => turn(v, A)), vertices.map((v) => turn(turn(v, A), A))];
  // Two rotations in opposite senses make a translation.
  const inverse = (v, o) => turn(turn(v, o), o);
  const through = (v) => turn(inverse(v, D), A);
  const t1 = sub(through([0, 0]), [0, 0]);
  const t2 = [c * t1[0] - s * t1[1], s * t1[0] + c * t1[1]];
  const middle = [0, 0];
  copies.flat().forEach((v) => {
    middle[0] += v[0] / 15;
    middle[1] += v[1] / 15;
  });
  // Wrapped around the origin, as `extract` expects.
  const tiles = copies.map((tile) => makeTile(tile.map((v) => sub(v, middle)), false));
  return { count: 3, t1, t2, tiles };
}

/** One period of the tiling, proven without the search: no overlap, no gap. */
function extract(found, unit) {
  const { t1, t2, tiles, count } = found;
  const lattice = cross(t1, t2);
  const inside = tiles.filter((t) => {
    const u = cross(t.centre, t2) / lattice;
    const v = cross(t1, t.centre) / lattice;
    // Around the origin, where the patch reaches furthest in all directions.
    return u >= -0.5 - 1e-6 && u < 0.5 - 1e-6 && v >= -0.5 - 1e-6 && v < 0.5 - 1e-6;
  });
  if (inside.length !== count) return { problem: `period holds ${inside.length} tiles, expected ${count}` };
  const area = inside.reduce((sum, t) => sum + polygonArea(t.vertices), 0);
  if (Math.abs(area - Math.abs(lattice)) > 1e-6) return { problem: `area ${area} against lattice ${lattice}` };
  for (let i = -3; i <= 3; i++) {
    for (let j = -3; j <= 3; j++) {
      for (const a of inside) {
        for (const b of inside) {
          if (i === 0 && j === 0 && a === b) continue;
          const moved = b.vertices.map((v) => [v[0] + i * t1[0] + j * t2[0], v[1] + i * t1[1] + j * t2[1]]);
          if (overlap(a.vertices, moved)) return { problem: `overlap at ${i},${j}` };
        }
      }
    }
  }
  const k = 1 / Math.sqrt(unit);
  const fix = (v) => [Number((v[0] * k).toFixed(12)), Number((v[1] * k).toFixed(12))];
  return {
    polygons: inside.map((t) => ({ mirrored: t.mirrored, vertices: t.vertices.map(fix) })),
    t1: fix(t1),
    t2: fix(t2),
  };
}

const args = process.argv.slice(2);
const out = args.find((a) => !a.startsWith('--'));
const seconds = Number(args.find((a) => a.startsWith('--seconds='))?.slice(10) ?? 60);
const only = args.find((a) => a.startsWith('--types='))?.slice(8).split(',').map(Number);
// Smallest period the literature reports per type, where the search can stop early.
const EXPECTED = { 1: 2, 2: 4, 3: 3, 4: 4, 5: 6, 6: 4, 7: 8, 8: 8, 9: 8, 10: 6, 11: 8, 12: 8, 13: 8, 14: 6, 15: 12 };

// Bagina and Sugimoto: exactly these types have edge-to-edge tilings.
const EDGE_TO_EDGE = new Set([1, 2, 4, 5, 6, 7, 8, 9]);

const results = {};
for (const type of only ?? Object.keys(TYPES).map(Number)) {
  try {
    const { vertices, p } = solvePentagon(TYPES[type]);
    // Generic pentagons of an edge-to-edge type need not have such a tiling, so
    // that is only tried first, on half the time.
    let { best, nodes } = type === 3 ? { best: threefold(vertices), nodes: 0 } : EDGE_TO_EDGE.has(type) ? search(vertices, seconds / 2, EXPECTED[type], true, TYPES[type].patch) : { best: null, nodes: 0 };
    if (type !== 3 && (!best || best.count > EXPECTED[type])) {
      const loose = search(vertices, seconds, EXPECTED[type], false, TYPES[type].patch);
      nodes += loose.nodes;
      if (loose.best && (!best || loose.best.count < best.count)) best = loose.best;
    }
    if (!best) {
      console.log(`type ${type}: nothing found (${nodes} nodes)`);
      continue;
    }
    const period = extract(best, polygonArea(vertices));
    if (period.problem) {
      console.log(`type ${type}: FAILED CHECK ${period.problem}`);
      continue;
    }
    const f = (v) => Number(v.toFixed(3));
    const unit = Math.sqrt(polygonArea(vertices));
    results[type] = { type, pentagon: vertices.map((v) => [v[0] / unit, v[1] / unit]), angles: [p.A, p.B, p.C, p.D, p.E].map(f), sides: [p.a, p.b, p.c, p.d, p.e].map(f), ...period };
    console.log(`type ${type}: ${best.count} tiles per period (expected ${EXPECTED[type]}), ${period.polygons.filter((q) => q.mirrored).length} mirrored, ${nodes} nodes, angles ${results[type].angles}`);
  } catch (error) {
    console.log(`type ${type}: ${error.message}`);
  }
}
if (out) writeFileSync(out, JSON.stringify(results, null, 1));
