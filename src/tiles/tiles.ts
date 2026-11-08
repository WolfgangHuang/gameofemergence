/**
 * Periodic tilings as seen by the renderer. A tiling is a set of polygons in one
 * period of a lattice, and each polygon belongs to one cell of the engine's grid.
 * Deforming a tile moves polygons around but leaves the grid and its neighbours
 * alone, which is why it works while the simulation is running.
 */

export type Vec = [number, number];
export type Family = 'quad' | 'triangle' | 'hexagon';

/** Numbers as understood by the engine's `world_set_topology`. */
export const TOPOLOGY = { moore: 0, vonNeumann: 1, hexagonal: 2, triangleEdges: 3, triangleVertices: 4 } as const;
export type Topology = keyof typeof TOPOLOGY;

export interface Polygon {
  /** Grid cell of this polygon, relative to the lattice point it hangs from. */
  cell: Vec;
  vertices: Vec[];
}

export interface Tiling {
  polygons: Polygon[];
  /** Lattice vectors in world units. */
  t1: Vec;
  t2: Vec;
  /** The grid displacement that goes with each lattice vector. */
  g1: Vec;
  g2: Vec;
  /** How many lattice periods away a polygon can reach into a given period. */
  reach: number;
  /** World coordinates per grid unit, as columns (a, b) and (c, d). */
  gridToWorld: [number, number, number, number];
  worldToGrid: [number, number, number, number];
}

const add = (a: Vec, b: Vec): Vec => [a[0] + b[0], a[1] + b[1]];
const sub = (a: Vec, b: Vec): Vec => [a[0] - b[0], a[1] - b[1]];
const scale = (a: Vec, k: number): Vec => [a[0] * k, a[1] * k];
const cross = (a: Vec, b: Vec) => a[0] * b[1] - a[1] * b[0];

export function area(vertices: Vec[]) {
  let sum = 0;
  vertices.forEach((v, i) => (sum += cross(v, vertices[(i + 1) % vertices.length])));
  return sum / 2;
}

function segmentsCross(a: Vec, b: Vec, c: Vec, d: Vec) {
  const side = (p: Vec, q: Vec, r: Vec) => cross(sub(q, p), sub(r, p));
  return side(a, b, c) * side(a, b, d) < 0 && side(c, d, a) * side(c, d, b) < 0;
}

/** Rejects self-intersecting and collapsed outlines. */
export function isValidOutline(vertices: Vec[]) {
  const n = vertices.length;
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      if (segmentsCross(vertices[i], vertices[(i + 1) % n], vertices[j], vertices[(j + 1) % n])) return false;
    }
  }
  const size = Math.abs(area(vertices));
  let longest = 0;
  vertices.forEach((v, i) => (longest = Math.max(longest, Math.hypot(...sub(v, vertices[(i + 1) % n])))));
  return size > 0.08 * longest * longest;
}

/** Any quadrilateral tiles the plane when every other copy is turned by 180 degrees. */
function quadTiling([a, b, c, d]: Vec[]): Pick<Tiling, 'polygons' | 't1' | 't2' | 'g1' | 'g2'> {
  const t1 = sub(sub(add(b, c), d), a);
  const t2 = sub(sub(add(c, d), b), a);
  const corners = [
    [a, b],
    [d, c],
  ];
  const vertex = (i: number, j: number) =>
    add(add(corners[j & 1][i & 1], scale(t1, i >> 1)), scale(t2, j >> 1));
  const polygons: Polygon[] = [];
  for (let j = 0; j < 2; j++) {
    for (let i = 0; i < 2; i++) {
      polygons.push({ cell: [i, j], vertices: [vertex(i, j), vertex(i + 1, j), vertex(i + 1, j + 1), vertex(i, j + 1)] });
    }
  }
  return { polygons, t1, t2, g1: [2, 0], g2: [0, 2] };
}

/** Two copies of any triangle make a parallelogram. */
function triangleTiling([a, b, c]: Vec[]): Pick<Tiling, 'polygons' | 't1' | 't2' | 'g1' | 'g2'> {
  const u = sub(b, a);
  const v = sub(c, a);
  return {
    polygons: [
      { cell: [0, 0], vertices: [a, b, c] },
      { cell: [1, 0], vertices: [b, add(b, v), c] },
    ],
    t1: u,
    t2: v,
    g1: [2, 0],
    g2: [1, 1],
  };
}

/** A centrally symmetric hexagon tiles by translation alone. */
function hexagonTiling(half: Vec[]): Pick<Tiling, 'polygons' | 't1' | 't2' | 'g1' | 'g2'> {
  const [p, q, r] = half;
  const vertices = [p, q, r, scale(p, -1), scale(q, -1), scale(r, -1)];
  return { polygons: [{ cell: [0, 0], vertices }], t1: add(p, q), t2: add(q, r), g1: [1, 0], g2: [0, 1] };
}

export function buildTiling(family: Family, outline: Vec[]): Tiling {
  const raw = family === 'quad' ? quadTiling(outline) : family === 'triangle' ? triangleTiling(outline) : hexagonTiling(outline);
  // Scaled to unit area, so that deforming a tile keeps the size of the picture.
  const k = 1 / Math.sqrt(Math.abs(area(raw.polygons[0].vertices)));
  const polygons = raw.polygons.map(({ cell, vertices }) => ({ cell, vertices: vertices.map((v) => scale(v, k)) }));
  const t1 = scale(raw.t1, k);
  const t2 = scale(raw.t2, k);

  const determinant = cross(raw.g1, raw.g2);
  const gx: Vec = scale(sub(scale(t1, raw.g2[1]), scale(t2, raw.g1[1])), 1 / determinant);
  const gy: Vec = scale(sub(scale(t2, raw.g1[0]), scale(t1, raw.g2[0])), 1 / determinant);
  const worldDeterminant = cross(gx, gy);

  let far = 0;
  const lattice = cross(t1, t2);
  for (const polygon of polygons) {
    for (const v of polygon.vertices) {
      far = Math.max(far, Math.abs(cross(v, t2) / lattice), Math.abs(cross(t1, v) / lattice));
    }
  }

  return {
    polygons,
    t1,
    t2,
    g1: raw.g1,
    g2: raw.g2,
    reach: Math.min(3, Math.ceil(far) + 1),
    gridToWorld: [gx[0], gx[1], gy[0], gy[1]],
    worldToGrid: [gy[1] / worldDeterminant, -gx[1] / worldDeterminant, -gy[0] / worldDeterminant, gx[0] / worldDeterminant],
  };
}

export function gridToWorld(tiling: Tiling, x: number, y: number): Vec {
  const [a, b, c, d] = tiling.gridToWorld;
  return [a * x + c * y, b * x + d * y];
}

export function worldToGrid(tiling: Tiling, x: number, y: number): Vec {
  const [a, b, c, d] = tiling.worldToGrid;
  return [a * x + c * y, b * x + d * y];
}

function contains(vertices: Vec[], x: number, y: number) {
  let inside = false;
  for (let i = 0, j = vertices.length - 1; i < vertices.length; j = i++) {
    const [xi, yi] = vertices[i];
    const [xj, yj] = vertices[j];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** Finds the grid cell under a world position. */
export function locate(tiling: Tiling, x: number, y: number): Vec {
  const { t1, t2, g1, g2, polygons, reach } = tiling;
  const lattice = cross(t1, t2);
  const p: Vec = [x, y];
  const u = Math.floor(cross(p, t2) / lattice);
  const v = Math.floor(cross(t1, p) / lattice);
  for (let dv = -reach; dv <= reach; dv++) {
    for (let du = -reach; du <= reach; du++) {
      const i = u + du;
      const j = v + dv;
      const local = sub(sub(p, scale(t1, i)), scale(t2, j));
      for (const polygon of polygons) {
        if (contains(polygon.vertices, local[0], local[1])) {
          return [polygon.cell[0] + i * g1[0] + j * g2[0], polygon.cell[1] + i * g1[1] + j * g2[1]];
        }
      }
    }
  }
  // A point exactly on an edge can fall through every test.
  const [gx, gy] = worldToGrid(tiling, x, y);
  return [Math.floor(gx), Math.floor(gy)];
}

/**
 * Grid position of the line between cell columns -1 and 0, and between rows -1
 * and 0. Where a cell's polygon lies relative to its grid position depends on the
 * family, so this goes by the polygons' centres.
 */
export function seamOffset(tiling: Tiling): Vec {
  let sum: Vec = [0, 0];
  for (const { cell, vertices } of tiling.polygons) {
    const centre = scale(vertices.reduce(add, [0, 0] as Vec), 1 / vertices.length);
    sum = add(sum, sub(worldToGrid(tiling, centre[0], centre[1]), cell));
  }
  return sub(scale(sum, 1 / tiling.polygons.length), [0.5, 0.5]);
}

/** The polygon of a grid cell in world coordinates. */
export function cellPolygon(tiling: Tiling, x: number, y: number): Vec[] {
  const { t1, t2, g1, g2, polygons } = tiling;
  const determinant = cross(g1, g2);
  for (const polygon of polygons) {
    const dx = x - polygon.cell[0];
    const dy = y - polygon.cell[1];
    const i = (dx * g2[1] - dy * g2[0]) / determinant;
    const j = (dy * g1[0] - dx * g1[1]) / determinant;
    if (Number.isInteger(i) && Number.isInteger(j)) {
      const shift = add(scale(t1, i), scale(t2, j));
      return polygon.vertices.map((v) => add(v, shift));
    }
  }
  throw new Error(`no polygon for cell ${x}, ${y}`);
}

export interface Preset {
  name: string;
  outline: Vec[];
}

export interface Neighbourhood {
  name: string;
  topology: Topology;
  count: number;
  /** Rules with sustained activity, the first being the default. */
  rules: string[];
}

export interface FamilyInfo {
  name: string;
  /** The outline's vertices that can be dragged; the rest follow by symmetry. */
  presets: Preset[];
  neighbourhoods: Neighbourhood[];
}

const hexagon = (scaleX = 1, scaleY = 1, shear = 0): Vec[] =>
  [0, 1, 2].map((k) => {
    const angle = (Math.PI / 3) * k - Math.PI / 2;
    const y = Math.sin(angle) * scaleY;
    return [Math.cos(angle) * scaleX + shear * y, y];
  });

export const FAMILIES: Record<Family, FamilyInfo> = {
  quad: {
    name: 'Quadrilateral',
    presets: [
      { name: 'Square', outline: [[0, 0], [1, 0], [1, 1], [0, 1]] },
      { name: 'Rectangle', outline: [[0, 0], [1.6, 0], [1.6, 0.8], [0, 0.8]] },
      { name: 'Parallelogram', outline: [[0, 0], [1, 0], [1.5, 0.85], [0.5, 0.85]] },
      { name: 'Rhombus', outline: [[0, 0], [0.75, 0.5], [0, 1], [-0.75, 0.5]] },
      { name: 'Trapezoid', outline: [[0, 0], [1.5, 0], [1.1, 0.9], [0.4, 0.9]] },
      { name: 'Kite', outline: [[0, 0], [0.7, 0.4], [0, 1.4], [-0.7, 0.4]] },
      { name: 'Dart', outline: [[0, 0], [0.8, 1], [0, 0.55], [-0.8, 1]] },
    ],
    neighbourhoods: [
      { name: 'Edges and corners', topology: 'moore', count: 8, rules: ['B3/S23', 'B36/S23', 'B2/S', 'B3678/S34678', 'B368/S245', 'B3/S12345', 'B1357/S1357', 'B4678/S35678'] },
      { name: 'Edges only', topology: 'vonNeumann', count: 4, rules: ['B24/S23', 'B23/S14', 'B23/S', 'B24/S'] },
    ],
  },
  triangle: {
    name: 'Triangle',
    presets: [
      { name: 'Equilateral', outline: [[0, 0], [1, 0], [0.5, Math.sqrt(3) / 2]] },
      { name: 'Right', outline: [[0, 0], [1, 0], [0, 1]] },
      { name: 'Isosceles', outline: [[0, 0], [1, 0], [0.5, 1.4]] },
      { name: 'Scalene', outline: [[0, 0], [1.3, 0], [0.3, 0.8]] },
    ],
    neighbourhoods: [
      { name: 'Edges and corners', topology: 'triangleVertices', count: 12, rules: ['B4/S345', 'B45/S257', 'B3/S267', 'B46/S456', 'B36/S2'] },
      { name: 'Edges only', topology: 'triangleEdges', count: 3, rules: ['B2/S12', 'B23/S12', 'B23/S2', 'B1/S'] },
    ],
  },
  hexagon: {
    name: 'Hexagon',
    presets: [
      { name: 'Regular', outline: hexagon() },
      { name: 'Wide', outline: hexagon(1.4, 0.8) },
      { name: 'Tall', outline: hexagon(0.75, 1.3) },
      { name: 'Skewed', outline: hexagon(1, 1, 0.45) },
      { name: 'Brick', outline: [[-0.75, -0.42], [0.75, -0.42], [0.75, 0]] },
    ],
    neighbourhoods: [{ name: 'Edges', topology: 'hexagonal', count: 6, rules: ['B24/S3', 'B2/S34', 'B25/S34', 'B24/S35', 'B35/S123'] }],
  },
};

export type HatTopology = 'hatVertices' | 'hatEdges';

/** Rules found to stay lively on the hat graph; hats have up to 7 neighbours. */
export const HAT_NEIGHBOURHOODS: { name: string; topology: HatTopology; count: number; rules: string[] }[] = [
  { name: 'Edges and corners', topology: 'hatVertices', count: 7, rules: ['B25/S3', 'B25/S36', 'B25/S35', 'B37/S123', 'B36/S234', 'B24/S46'] },
  { name: 'Edges only', topology: 'hatEdges', count: 6, rules: ['B23/S4', 'B2/S246', 'B26/S24', 'B2/S24', 'B2/S245', 'B24/S26'] },
];

export const MIN_TORUS_SIDE = 8;
export const MAX_TORUS_SIDE = 4096;

/** Odd sides would not let the tiles of most families fit together across the seam. */
export const isValidTorusSide = (n: number) => Number.isInteger(n) && n % 2 === 0 && n >= MIN_TORUS_SIDE && n <= MAX_TORUS_SIDE;

/** Points of the Tile(a, b) family as angles: a = cos, b = sin. */
export const HAT_SHAPES: { name: string; degrees: number }[] = [
  { name: 'Comet', degrees: 6 },
  { name: 'Turtle', degrees: 30 },
  { name: 'Spectre shape', degrees: 45 },
  { name: 'Hat', degrees: 60 },
  { name: 'Chevron', degrees: 84 },
];

/** Whether dragging one vertex needs the mirrored one to follow. */
export const fullOutline = (family: Family, outline: Vec[]): Vec[] =>
  family === 'hexagon' ? [...outline, ...outline.map((v) => scale(v, -1))] : outline;
