import type { Renderer } from './renderer';

const BATCH = 8192;
const BUCKET = 3;
export const FIELDS_PER_CELL = 5;

export type Vec = [number, number];

export function compile(gl: WebGL2RenderingContext, vertex: string, fragment: string) {
  const program = gl.createProgram()!;
  for (const [type, source] of [
    [gl.VERTEX_SHADER, vertex],
    [gl.FRAGMENT_SHADER, fragment],
  ] as const) {
    const shader = gl.createShader(type)!;
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) ?? 'shader');
    gl.attachShader(program, shader);
  }
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) ?? 'program');
  const uniforms = new Map<string, WebGLUniformLocation | null>();
  return {
    program,
    uniform(name: string) {
      if (!uniforms.has(name)) uniforms.set(name, gl.getUniformLocation(program, name));
      return uniforms.get(name)!;
    },
  };
}

/** What tilings without a lattice share on screen: their cells and finding them. */
export abstract class GraphScene {
  protected gl: WebGL2RenderingContext;
  protected instances: WebGLBuffer;

  /** A kind of orientation, then four parts of a position, per cell. */
  protected cells = new Int32Array(FIELDS_PER_CELL * 4096);
  count = 0;
  private uploaded = 0;
  private capacity = 0;

  /** Long distances: world position = stretch * lattice position, as complex numbers. */
  stretch: Vec = [1, 0];

  private buckets = new Map<string, number[]>();
  private indexed = 0;
  protected batches: { first: number; count: number; x: number; y: number; radius: number }[] = [];

  constructor(gl: WebGL2RenderingContext) {
    this.gl = gl;
    this.instances = gl.createBuffer()!;
  }

  abstract centerOf(id: number): Vec;
  protected abstract containsPoint(id: number, x: number, y: number): boolean;
  abstract draw(renderer: Renderer, center: Vec, scale: number, hover: { id: number; reach: number } | null): void;

  append(data: Int32Array) {
    const needed = this.count * FIELDS_PER_CELL + data.length;
    if (needed > this.cells.length) {
      const grown = new Int32Array(Math.max(needed, this.cells.length * 2));
      grown.set(this.cells.subarray(0, this.count * FIELDS_PER_CELL));
      this.cells = grown;
    }
    this.cells.set(data, this.count * FIELDS_PER_CELL);
    this.count += data.length / FIELDS_PER_CELL;
  }

  protected syncInstances() {
    if (this.uploaded === this.count) return;
    const { gl } = this;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.instances);
    if (this.count > this.capacity) {
      this.capacity = Math.max(this.count, this.capacity * 2, 4096);
      gl.bufferData(gl.ARRAY_BUFFER, this.capacity * FIELDS_PER_CELL * 4, gl.DYNAMIC_DRAW);
      this.uploaded = 0;
    }
    gl.bufferSubData(
      gl.ARRAY_BUFFER,
      this.uploaded * FIELDS_PER_CELL * 4,
      this.cells,
      this.uploaded * FIELDS_PER_CELL,
      (this.count - this.uploaded) * FIELDS_PER_CELL,
    );
    this.uploaded = this.count;
  }

  /** Everything has moved, so the index starts over. */
  protected resetIndex() {
    this.buckets.clear();
    this.indexed = 0;
    this.batches = [];
  }

  protected updateIndex() {
    for (let id = this.indexed; id < this.count; id++) {
      const [x, y] = this.centerOf(id);
      const key = `${Math.floor(x / BUCKET)},${Math.floor(y / BUCKET)}`;
      const list = this.buckets.get(key);
      if (list) list.push(id);
      else this.buckets.set(key, [id]);
    }
    this.indexed = this.count;
    // Batches let drawing skip whatever is off screen. Their extent is measured
    // from the cells, as generation can place neighbouring ids far apart.
    this.batches = [];
    for (let first = 0; first < this.count; first += BATCH) {
      const count = Math.min(BATCH, this.count - first);
      let [minX, minY, maxX, maxY] = [Infinity, Infinity, -Infinity, -Infinity];
      for (let id = first; id < first + count; id++) {
        const [x, y] = this.centerOf(id);
        minX = Math.min(minX, x);
        maxX = Math.max(maxX, x);
        minY = Math.min(minY, y);
        maxY = Math.max(maxY, y);
      }
      this.batches.push({ first, count, x: (minX + maxX) / 2, y: (minY + maxY) / 2, radius: Math.hypot(maxX - minX, maxY - minY) / 2 + 4 });
    }
  }

  /** The cell under a world position, if that part of the field is generated. */
  locate(x: number, y: number): number | null {
    this.updateIndex();
    const [bx, by] = [Math.floor(x / BUCKET), Math.floor(y / BUCKET)];
    for (let dy = -2; dy <= 2; dy++) {
      for (let dx = -2; dx <= 2; dx++) {
        for (const id of this.buckets.get(`${bx + dx},${by + dy}`) ?? []) {
          if (this.containsPoint(id, x, y)) return id;
        }
      }
    }
    return null;
  }

  /** Cells whose centres lie within a distance. */
  around(x: number, y: number, squared: number): number[] {
    this.updateIndex();
    const reach = Math.ceil(Math.sqrt(squared) / BUCKET) + 1;
    const [bx, by] = [Math.floor(x / BUCKET), Math.floor(y / BUCKET)];
    const found: number[] = [];
    for (let dy = -reach; dy <= reach; dy++) {
      for (let dx = -reach; dx <= reach; dx++) {
        for (const id of this.buckets.get(`${bx + dx},${by + dy}`) ?? []) {
          const [cx, cy] = this.centerOf(id);
          if ((cx - x) ** 2 + (cy - y) ** 2 <= squared) found.push(id);
        }
      }
    }
    return found;
  }

  /** Converts between world and lattice positions over long distances. */
  worldOf(lattice: Vec): Vec {
    const [sr, si] = this.stretch;
    return [sr * lattice[0] - si * lattice[1], sr * lattice[1] + si * lattice[0]];
  }

  latticeOf(world: Vec): Vec {
    const [sr, si] = this.stretch;
    const norm = sr * sr + si * si;
    return [(sr * world[0] + si * world[1]) / norm, (sr * world[1] - si * world[0]) / norm];
  }
}
