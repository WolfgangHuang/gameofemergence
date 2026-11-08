import type { Renderer } from './renderer';

const HR3 = Math.sqrt(3) / 2;
const SQRT3 = Math.sqrt(3);
const BATCH = 8192;
const BUCKET = 3;
export const FIELDS_PER_CELL = 5;

const VERTEX = /* glsl */ `#version 300 es
precision highp float;
precision highp int;
layout(location = 0) in int a_vertex;
layout(location = 1) in ivec4 a_split;
layout(location = 2) in int a_orientation;
uniform vec2 u_local[14];
uniform vec2 u_centroid;
uniform mat2 u_orientations[12];
uniform ivec4 u_reference;   // split position subtracted before going to floats
uniform vec2 u_parts;        // a-part and b-part factors, including the tile's scale
uniform vec2 u_offset;       // reference position minus camera, in world units
uniform vec2 u_viewport;     // device pixels
uniform float u_scale;
uniform int u_firstId;
uniform sampler2D u_frame;
uniform sampler2D u_glow;
uniform int u_hover;
uniform vec2 u_hoverAt;      // relative to the camera
uniform float u_brush;
flat out float v_live;
flat out float v_glow;
flat out float v_hovered;

vec2 hex(ivec2 v) {
  return vec2(float(v.x) + 0.5 * float(v.y), ${HR3} * float(v.y));
}

void main() {
  ivec4 d = a_split - u_reference;
  vec2 anchor = u_parts.x * hex(d.xy) + u_parts.y * hex(d.zw) + u_offset;
  int rotation = a_orientation & 7;
  int flip = (a_orientation >> 3) & 1;
  mat2 orientation = u_orientations[rotation + 6 * flip];
  vec2 world = anchor + orientation * u_local[a_vertex];
  vec2 clip = world * u_scale / (u_viewport / 2.0);
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);

  int id = gl_InstanceID + u_firstId;
  ivec2 texel = ivec2(id & 2047, id >> 11);
  ivec2 size = textureSize(u_frame, 0);
  bool known = all(lessThan(texel, size));
  v_live = known ? texelFetch(u_frame, texel, 0).r : 0.0;
  v_glow = known ? texelFetch(u_glow, texel, 0).r : 0.0;
  vec2 centre = anchor + orientation * u_centroid - u_hoverAt;
  v_hovered = u_brush >= 0.0 && (id == u_hover || dot(centre, centre) <= u_brush) ? 1.0 : 0.0;
}`;

const FILL = /* glsl */ `#version 300 es
precision highp float;
flat in float v_live;
flat in float v_glow;
flat in float v_hovered;
out vec4 color;
const vec3 PAPER = vec3(1.0);
const vec3 INK = vec3(0.098, 0.149, 0.212);
const vec3 TRAIL = vec3(0.490, 0.706, 0.941);
void main() {
  vec3 c = mix(PAPER, TRAIL, 0.5 * v_glow * v_glow);
  c = mix(c, TRAIL, 0.3 * v_hovered);
  color = vec4(mix(c, INK, v_live), 1.0);
}`;

const LINE = /* glsl */ `#version 300 es
precision highp float;
flat in float v_live;
uniform float u_grid;
out vec4 color;
const vec3 LINE = vec3(0.835, 0.878, 0.925);
void main() {
  // Fainter over live cells, where a full line would break them apart.
  color = vec4(LINE, u_grid * (v_live > 0.5 ? 0.35 : 1.0));
}`;

function compile(gl: WebGL2RenderingContext, vertex: string, fragment: string) {
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

type Vec = [number, number];

const hex = (q: number, r: number): Vec => [q + 0.5 * r, HR3 * r];

/** Triangulates a simple polygon, tolerating straight-through vertices. */
export function earClip(points: Vec[]): number[] {
  let signed = 0;
  points.forEach((p, i) => {
    const q = points[(i + 1) % points.length];
    signed += p[0] * q[1] - q[0] * p[1];
  });
  const sign = Math.sign(signed) || 1;
  const cross = (a: Vec, b: Vec, c: Vec) => sign * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]));
  const remaining = points.map((_, i) => i);
  const triangles: number[] = [];
  let guard = points.length * points.length;
  while (remaining.length > 3 && guard-- > 0) {
    for (let i = 0; i < remaining.length; i++) {
      const n = remaining.length;
      const [ia, ib, ic] = [remaining[(i + n - 1) % n], remaining[i], remaining[(i + 1) % n]];
      const [a, b, c] = [points[ia], points[ib], points[ic]];
      const turn = cross(a, b, c);
      if (turn < -1e-9) continue;
      const blocked = remaining.some(
        (j) => j !== ia && j !== ib && j !== ic && cross(a, b, points[j]) > 1e-9 && cross(b, c, points[j]) > 1e-9 && cross(c, a, points[j]) > 1e-9,
      );
      if (blocked) continue;
      if (turn > 1e-9) triangles.push(ia, ib, ic);
      remaining.splice(i, 1);
      break;
    }
  }
  if (remaining.length === 3) triangles.push(...remaining);
  return triangles;
}

/** The hat tiling on screen: positions, drawing and finding cells. */
export class HatScene {
  private gl: WebGL2RenderingContext;
  private fill;
  private line;
  private vao: WebGLVertexArrayObject;
  private fillVertices: WebGLBuffer;
  private lineVertices: WebGLBuffer;
  private instances: WebGLBuffer;
  private fillCount = 0;
  private lineCount = 0;

  /** Rotation + 8 * flip, then p.q, p.r, q.q, q.r per cell. */
  private cells = new Int32Array(FIELDS_PER_CELL * 4096);
  count = 0;
  private uploaded = 0;
  private capacity = 0;

  private partA: Vec[] = [];
  private partB: Vec[] = [];
  private ratio: Vec;
  private morphDegrees = 60;
  a = 1;
  b = SQRT3;
  /** World units per lattice unit of the hat's own shape. */
  size = 1;
  /** Long distances: world position = stretch * lattice position, as complex numbers. */
  stretch: Vec = [1, 0];
  private local: Vec[] = [];
  private centroid: Vec = [0, 0];

  private buckets = new Map<string, number[]>();
  private indexed = 0;
  private batches: { first: number; count: number; x: number; y: number; radius: number }[] = [];

  constructor(gl: WebGL2RenderingContext, outline: Int32Array, ratio: Vec) {
    this.gl = gl;
    this.ratio = ratio;
    for (let i = 0; i < 14; i++) {
      this.partA.push(hex(outline[i * 4], outline[i * 4 + 1]));
      this.partB.push(hex(outline[i * 4 + 2], outline[i * 4 + 3]));
    }
    this.fill = compile(gl, VERTEX, FILL);
    this.line = compile(gl, VERTEX, LINE);
    this.vao = gl.createVertexArray()!;
    this.fillVertices = gl.createBuffer()!;
    this.lineVertices = gl.createBuffer()!;
    this.instances = gl.createBuffer()!;
    this.setMorph(60);
  }

  /** The family's angle: a = cos, b = sin, so the hat is 60 and the turtle 30. */
  setMorph(degrees: number) {
    this.morphDegrees = degrees;
    const angle = (degrees * Math.PI) / 180;
    const [a, b] = [Math.cos(angle), Math.sin(angle)];
    // Lengths are a and b, while b-edges have length sqrt(3) in the outline.
    const raw = this.partA.map((v, i): Vec => [a * v[0] + (b / SQRT3) * this.partB[i][0], a * v[1] + (b / SQRT3) * this.partB[i][1]]);
    let area = 0;
    let cx = 0;
    let cy = 0;
    raw.forEach((p, i) => {
      const q = raw[(i + 1) % raw.length];
      const cross = p[0] * q[1] - q[0] * p[1];
      area += cross / 2;
      cx += ((p[0] + q[0]) * cross) / 6;
      cy += ((p[1] + q[1]) * cross) / 6;
    });
    const size = 1 / Math.sqrt(Math.abs(area));
    this.a = a;
    this.b = b;
    this.size = size;
    this.local = raw.map(([x, y]): Vec => [x * size, y * size]);
    this.centroid = [(cx / area) * size, (cy / area) * size];

    // (a + b / sqrt(3) * k) / (1 + k), for k = q / p over long distances.
    const [kr, ki] = this.ratio;
    const nr = a + (b / SQRT3) * kr;
    const ni = (b / SQRT3) * ki;
    const dr = 1 + kr;
    const di = ki;
    const norm = dr * dr + di * di;
    this.stretch = [(size * (nr * dr + ni * di)) / norm, (size * (ni * dr - nr * di)) / norm];

    this.updateMesh();
    // Everything has moved, so the index starts over.
    this.buckets.clear();
    this.indexed = 0;
    this.batches = [];
  }

  get morph() {
    return this.morphDegrees;
  }

  private updateMesh() {
    const { gl } = this;
    const triangles = earClip(this.local);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.fillVertices);
    gl.bufferData(gl.ARRAY_BUFFER, Int32Array.from(triangles), gl.DYNAMIC_DRAW);
    this.fillCount = triangles.length;
    const lines: number[] = [];
    for (let i = 0; i < 14; i++) lines.push(i, (i + 1) % 14);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.lineVertices);
    gl.bufferData(gl.ARRAY_BUFFER, Int32Array.from(lines), gl.STATIC_DRAW);
    this.lineCount = lines.length;
  }

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

  private syncInstances() {
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

  /** World position of the cell's anchor, without any camera. */
  private anchor(id: number): Vec {
    const c = this.cells;
    const at = id * FIELDS_PER_CELL;
    const [px, py] = hex(c[at + 1], c[at + 2]);
    const [qx, qy] = hex(c[at + 3], c[at + 4]);
    const toB = (this.b / SQRT3) * this.size;
    const toA = this.a * this.size;
    return [toA * px + toB * qx, toA * py + toB * qy];
  }

  private orient(id: number, [x, y]: Vec): Vec {
    const code = this.cells[id * FIELDS_PER_CELL];
    const my = code & 8 ? -y : y;
    const angle = ((code & 7) * Math.PI) / 3;
    const [c, s] = [Math.cos(angle), Math.sin(angle)];
    return [c * x - s * my, s * x + c * my];
  }

  centerOf(id: number): Vec {
    const [ax, ay] = this.anchor(id);
    const [cx, cy] = this.orient(id, this.centroid);
    return [ax + cx, ay + cy];
  }

  private updateIndex() {
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

  private containsPoint(id: number, x: number, y: number) {
    const [ax, ay] = this.anchor(id);
    let inside = false;
    let [px, py] = this.orient(id, this.local[13]);
    px += ax;
    py += ay;
    for (const vertex of this.local) {
      let [qx, qy] = this.orient(id, vertex);
      qx += ax;
      qy += ay;
      if (qy > y !== py > y && x < ((px - qx) * (y - qy)) / (py - qy) + qx) inside = !inside;
      px = qx;
      py = qy;
    }
    return inside;
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

  draw(renderer: Renderer, center: Vec, scale: number, hover: { id: number; reach: number } | null) {
    const { gl } = this;
    this.syncInstances();
    this.updateIndex();

    // The reference is the split position nearest the camera, so that what the
    // shader turns into floats is small, however far out the camera is.
    const [kr, ki] = this.ratio;
    const dr = 1 + kr;
    const norm = dr * dr + ki * ki;
    const toP: Vec = [(center[0] * dr + center[1] * ki) / norm, (center[1] * dr - center[0] * ki) / norm];
    const toQ: Vec = [center[0] - toP[0], center[1] - toP[1]];
    const round = ([x, y]: Vec): [number, number] => {
      const r = Math.round(y / HR3);
      return [Math.round(x - 0.5 * r), r];
    };
    const [pq, pr] = round(toP);
    const [qq, qr] = round(toQ);
    const [pa, pb] = hex(pq, pr);
    const [qa, qb] = hex(qq, qr);
    const toA = this.a * this.size;
    const toB = (this.b / SQRT3) * this.size;
    const cameraWorld = this.worldOf(center);
    const offset: Vec = [toA * pa + toB * qa - cameraWorld[0], toA * pb + toB * qb - cameraWorld[1]];

    const canvas = gl.canvas as HTMLCanvasElement;
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(1, 1, 1, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    const orientations: number[] = [];
    for (const flip of [1, -1]) {
      for (let k = 0; k < 6; k++) {
        const angle = (k * Math.PI) / 3;
        const [c, s] = [Math.cos(angle), Math.sin(angle)];
        // Columns of rotation * diag(1, flip).
        orientations.push(c, s, -s * flip, c * flip);
      }
    }

    const halfW = canvas.width / scale / 2;
    const halfH = canvas.height / scale / 2;
    const visible = this.batches.filter(
      (batch) => Math.abs(batch.x - cameraWorld[0]) <= halfW + batch.radius && Math.abs(batch.y - cameraWorld[1]) <= halfH + batch.radius,
    );
    const grid = Math.min(1, Math.max(0, (scale - 5) / 8));

    gl.bindVertexArray(this.vao);
    gl.enableVertexAttribArray(0);
    gl.enableVertexAttribArray(1);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribDivisor(0, 0);
    gl.vertexAttribDivisor(1, 1);
    gl.vertexAttribDivisor(2, 1);

    const pass = (program: typeof this.fill, buffer: WebGLBuffer, mode: number, count: number) => {
      gl.useProgram(program.program);
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.vertexAttribIPointer(0, 1, gl.INT, 0, 0);
      gl.uniform2fv(program.uniform('u_local'), this.local.flat());
      gl.uniform2f(program.uniform('u_centroid'), this.centroid[0], this.centroid[1]);
      gl.uniformMatrix2fv(program.uniform('u_orientations'), false, orientations);
      gl.uniform4i(program.uniform('u_reference'), pq, pr, qq, qr);
      gl.uniform2f(program.uniform('u_parts'), toA, toB);
      gl.uniform2f(program.uniform('u_offset'), offset[0], offset[1]);
      gl.uniform2f(program.uniform('u_viewport'), canvas.width, canvas.height);
      gl.uniform1f(program.uniform('u_scale'), scale);
      gl.uniform1i(program.uniform('u_hover'), hover ? hover.id : -1);
      gl.uniform1f(program.uniform('u_brush'), hover ? hover.reach : -1);
      if (hover) {
        const [hx, hy] = this.centerOf(hover.id);
        gl.uniform2f(program.uniform('u_hoverAt'), hx - cameraWorld[0], hy - cameraWorld[1]);
      }
      renderer.bindCellTextures(program);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.instances);
      for (const batch of visible) {
        const start = batch.first * FIELDS_PER_CELL * 4;
        gl.vertexAttribIPointer(2, 1, gl.INT, FIELDS_PER_CELL * 4, start);
        gl.vertexAttribIPointer(1, 4, gl.INT, FIELDS_PER_CELL * 4, start + 4);
        gl.uniform1i(program.uniform('u_firstId'), batch.first);
        gl.drawArraysInstanced(mode, 0, count, batch.count);
      }
    };

    pass(this.fill, this.fillVertices, gl.TRIANGLES, this.fillCount);
    if (grid > 0) {
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.useProgram(this.line.program);
      gl.uniform1f(this.line.uniform('u_grid'), grid * grid * (3 - 2 * grid));
      pass(this.line, this.lineVertices, gl.LINES, this.lineCount);
      gl.disable(gl.BLEND);
    }
    gl.bindVertexArray(renderer.emptyVao);
  }
}
