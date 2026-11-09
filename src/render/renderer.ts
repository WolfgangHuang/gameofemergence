import type { FrameWindow, TorusSize } from '../sim/protocol';
import { seamOffset, type Tiling, type Vec } from '../tiles/tiles';
import { PALETTES, type Palette } from './palette';

const FULLSCREEN = /* glsl */ `#version 300 es
void main() {
  vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

/** Fades the trail texture and stamps the current frame on top of it. */
const GLOW = /* glsl */ `#version 300 es
precision highp float;
precision highp int;
uniform sampler2D u_frame;
uniform sampler2D u_previous;
uniform ivec2 u_shift;
uniform float u_decay;
out vec4 color;
void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  ivec2 q = p + u_shift;
  ivec2 size = textureSize(u_previous, 0);
  float previous = all(greaterThanEqual(q, ivec2(0))) && all(lessThan(q, size))
    ? texelFetch(u_previous, q, 0).r : 0.0;
  color = vec4(max(texelFetch(u_frame, p, 0).r, previous - u_decay), 0.0, 0.0, 1.0);
}`;

const LOOKUP_SIZE = 512;
const FIELD_OF_VIEW = 40;
const MAX_VERTICES = 60;
const MAX_POLYGONS = 12;

/**
 * Renders one lattice period into a lookup texture: for each texel, the grid
 * cell it belongs to and the distance to that cell's outline. This runs only when
 * the tile changes, so the display shader needs no geometry at all, whatever the
 * shape of the tile.
 */
const LOOKUP = /* glsl */ `#version 300 es
precision highp float;
precision highp int;
uniform vec2 u_t1;
uniform vec2 u_t2;
uniform ivec2 u_g1;
uniform ivec2 u_g2;
uniform vec2 u_vertices[${MAX_VERTICES}];
uniform int u_counts[${MAX_POLYGONS}];
uniform ivec2 u_cells[${MAX_POLYGONS}];
uniform int u_polygons;
uniform int u_reach;
out vec4 color;

float edgeDistance(vec2 p, vec2 a, vec2 b) {
  vec2 ab = b - a;
  return length(p - a - ab * clamp(dot(p - a, ab) / dot(ab, ab), 0.0, 1.0));
}

void main() {
  vec2 uv = gl_FragCoord.xy / float(${LOOKUP_SIZE});
  vec2 p = uv.x * u_t1 + uv.y * u_t2;
  for (int dv = -u_reach; dv <= u_reach; dv++) {
    for (int du = -u_reach; du <= u_reach; du++) {
      vec2 local = p - float(du) * u_t1 - float(dv) * u_t2;
      int first = 0;
      for (int polygon = 0; polygon < u_polygons; polygon++) {
        int n = u_counts[polygon];
        bool inside = false;
        float nearest = 1e9;
        for (int i = 0; i < n; i++) {
          vec2 a = u_vertices[first + i];
          vec2 b = u_vertices[first + (i + 1) % n];
          if ((a.y > local.y) != (b.y > local.y) &&
              local.x < (b.x - a.x) * (local.y - a.y) / (b.y - a.y) + a.x) inside = !inside;
          nearest = min(nearest, edgeDistance(local, a, b));
        }
        if (inside) {
          ivec2 cell = u_cells[polygon] + du * u_g1 + dv * u_g2 + ivec2(128);
          int d = int(clamp(nearest, 0.0, 1.0) * 65535.0);
          color = vec4(vec2(cell) / 255.0, float(d >> 8) / 255.0, float(d & 255) / 255.0);
          return;
        }
        first += n;
      }
    }
  }
  color = vec4(128.0 / 255.0, 128.0 / 255.0, 0.0, 0.0);
}`;

/** What a point of the field looks like, shared by the flat view and the donut. */
const FIELD = /* glsl */ `
precision highp float;
precision highp int;
uniform highp sampler2D u_lookup;
uniform sampler2D u_frame;
uniform sampler2D u_glow;
uniform ivec2 u_base;       // added to a cell's grid position to get it relative to the window
uniform ivec2 u_g1;
uniform ivec2 u_g2;
uniform int u_level;
uniform ivec2 u_hover;      // relative to the window
uniform float u_brush;      // squared brush radius in world units, negative when hidden
uniform mat2 u_gridToWorld;
uniform sampler2D u_ghost;
uniform ivec2 u_ghostAt;    // top left of the pattern preview, relative to the window
uniform bool u_hasGhost;
uniform ivec2 u_period;     // size of a torus in cells, zero on the plane
uniform mat2 u_worldToGrid;
uniform float u_seam;       // seam line opacity
uniform vec3 u_paper;
uniform vec3 u_line;
uniform vec3 u_ink;
uniform vec3 u_trail;
uniform vec3 u_ghostColour;
out vec4 color;

// Integer % is undefined for negative numbers in this version of GLSL.
int wrap(int value, int size) {
  int rest = value - size * int(floor(float(value) / float(size)));
  return rest < 0 ? rest + size : rest >= size ? rest - size : rest;
}

ivec2 wrapped(ivec2 value) {
  return u_period.x > 0 ? ivec2(wrap(value.x, u_period.x), wrap(value.y, u_period.y)) : value;
}

// lattice: position in lattice periods. fromSeam: grid position relative to a seam.
// scale: device pixels per world unit at this point.
vec3 fieldColor(vec2 lattice, vec2 fromSeam, float scale) {
  vec2 whole = floor(lattice);
  ivec2 texel = min(ivec2((lattice - whole) * float(${LOOKUP_SIZE})), ivec2(${LOOKUP_SIZE - 1}));
  vec4 entry = texelFetch(u_lookup, texel, 0);
  ivec2 local = ivec2(round(entry.rg * 255.0)) - ivec2(128);
  float dist = (round(entry.b * 255.0) * 256.0 + round(entry.a * 255.0)) / 65535.0 * scale;
  ivec2 cell = local + int(whole.x) * u_g1 + int(whole.y) * u_g2 + u_base;
  float grid = smoothstep(5.0, 13.0, scale);

  // On a torus the picture repeats, whichever copy of a cell is on screen.
  ivec2 t = wrapped(cell) >> u_level;
  ivec2 size = textureSize(u_frame, 0);
  bool inside = all(greaterThanEqual(t, ivec2(0))) && all(lessThan(t, size));
  float live = inside ? texelFetch(u_frame, t, 0).r : 0.0;
  float glow = inside ? texelFetch(u_glow, t, 0).r : 0.0;

  vec3 c = u_paper;
  c = mix(c, u_line, grid * (1.0 - smoothstep(0.0, 1.0, dist)));
  c = mix(c, u_trail, 0.5 * glow * glow);
  if (u_brush >= 0.0) {
    ivec2 apart = wrapped(cell - u_hover);
    // The nearest copy of the brush centre counts.
    if (u_period.x > 0) apart -= u_period * ivec2(greaterThan(2 * apart, u_period));
    vec2 d = u_gridToWorld * vec2(apart);
    if (dot(d, d) <= u_brush) c = mix(c, u_trail, 0.3);
  }
  // Coverage below one cell per texel still has to read as something being there.
  // Outlines stay faintly visible through live cells, which separates them and
  // hides the stair steps of slanted edges.
  c = mix(c, u_ink, sqrt(live) * (1.0 - 0.35 * grid * (1.0 - smoothstep(0.0, 1.0, dist))));
  if (u_hasGhost) {
    ivec2 g = wrapped(cell - u_ghostAt);
    if (all(greaterThanEqual(g, ivec2(0))) && all(lessThan(g, textureSize(u_ghost, 0)))) {
      c = mix(c, u_ghostColour, 0.85 * texelFetch(u_ghost, g, 0).r);
    }
  }
  if (u_seam > 0.0) {
    vec2 off = fromSeam - vec2(u_period) * round(fromSeam / vec2(u_period));
    // Grid units to device pixels, measured across each family of lines.
    vec2 perUnit = vec2(length(vec2(u_worldToGrid[0][0], u_worldToGrid[1][0])), length(vec2(u_worldToGrid[0][1], u_worldToGrid[1][1])));
    vec2 away = abs(off) * scale / perUnit;
    c = mix(c, u_ghostColour, u_seam * (1.0 - smoothstep(0.5, 1.5, min(away.x, away.y))));
  }
  return c;
}`;

const DISPLAY = /* glsl */ `#version 300 es
${FIELD}
uniform vec2 u_resolution;
uniform float u_scale;      // device pixels per world unit
uniform mat2 u_worldToLattice;
uniform vec2 u_latticeFraction;  // fractional lattice position of the screen centre
uniform vec2 u_seamCenter;  // grid position of the screen centre, seen from a seam

void main() {
  vec2 px = vec2(gl_FragCoord.x, u_resolution.y - gl_FragCoord.y) - u_resolution / 2.0;
  vec2 world = px / u_scale;
  color = vec4(fieldColor(u_latticeFraction + u_worldToLattice * world, u_seamCenter + u_worldToGrid * world, u_scale), 1.0);
}`;

const DONUT_U = 320;
const DONUT_V = 160;

/** A ring around the z axis, built from the vertex number alone. */
const DONUT_VERTEX = /* glsl */ `#version 300 es
precision highp float;
precision highp int;
uniform mat4 u_transform;
uniform float u_minor;
out vec2 v_uv;
out vec3 v_normal;
out vec3 v_position;
const float TAU = 6.283185307179586;
const ivec2 CORNERS[6] = ivec2[6](ivec2(0, 0), ivec2(1, 0), ivec2(1, 1), ivec2(0, 0), ivec2(1, 1), ivec2(0, 1));
void main() {
  int quad = gl_VertexID / 6;
  ivec2 at = ivec2(quad % ${DONUT_U}, quad / ${DONUT_U}) + CORNERS[gl_VertexID % 6];
  v_uv = vec2(at) / vec2(${DONUT_U}.0, ${DONUT_V}.0);
  float around = TAU * v_uv.x;
  float tube = TAU * v_uv.y;
  vec3 out_ = vec3(cos(around), sin(around), 0.0);
  v_normal = cos(tube) * out_ + vec3(0.0, 0.0, sin(tube));
  v_position = out_ + u_minor * v_normal;
  gl_Position = u_transform * vec4(v_position, 1.0);
}`;

const DONUT = /* glsl */ `#version 300 es
${FIELD}
in vec2 v_uv;
in vec3 v_normal;
in vec3 v_position;
uniform vec2 u_size;        // field size in cells
uniform bool u_swap;        // whether the field's height runs around the ring
uniform vec2 u_seamShift;   // grid position of the seam
uniform mat2 u_gridToLattice;
uniform vec3 u_eye;
uniform vec3 u_light;
uniform float u_rim;

void main() {
  // Seen from outside, the ring runs to the right and the tube upwards. The field's
  // x runs to the right and y downwards, and neither mapping may mirror it.
  vec2 fromSeam = u_swap ? vec2(v_uv.y * u_size.x, v_uv.x * u_size.y) : vec2(v_uv.x * u_size.x, -v_uv.y * u_size.y);
  vec2 grid = fromSeam + u_seamShift;
  vec2 world = u_gridToWorld * grid;
  float scale = 1.0 / max(max(length(dFdx(world)), length(dFdy(world))), 1e-6);
  vec3 c = fieldColor(u_gridToLattice * grid, fromSeam, scale);

  vec3 normal = normalize(v_normal);
  float facing = max(dot(normal, normalize(u_eye - v_position)), 0.0);
  float rim = pow(1.0 - facing, 3.0);
  color = vec4(c * (0.8 + 0.2 * max(dot(normal, u_light), 0.0)) + u_rim * rim, 1.0);
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

interface Layer {
  texture: WebGLTexture;
  width: number;
  height: number;
}

export interface View {
  /** Grid position of the screen centre. */
  centerX: number;
  centerY: number;
  /** Device pixels per world unit; a tile has an area of one. */
  scale: number;
  hover: { x: number; y: number; reach: number } | null;
  /** Top left cell of the pattern preview. */
  ghostAt: { x: number; y: number } | null;
  torus: TorusSize | null;
}

export interface DonutView {
  torus: TorusSize;
  /** Turn around the ring's axis, and tilt towards the viewer, in radians. */
  yaw: number;
  pitch: number;
  zoom: number;
}

type Mat4 = number[];

/** Column by column, as WebGL wants it. */
function multiply(a: Mat4, b: Mat4): Mat4 {
  const out = new Array<number>(16).fill(0);
  for (let column = 0; column < 4; column++) {
    for (let row = 0; row < 4; row++) {
      for (let k = 0; k < 4; k++) out[column * 4 + row] += a[k * 4 + row] * b[column * 4 + k];
    }
  }
  return out;
}

export class Renderer {
  readonly gl: WebGL2RenderingContext;
  readonly emptyVao: WebGLVertexArrayObject;
  palette: Palette = PALETTES.light;
  private display;
  private donut;
  private glow;
  private lookupProgram;
  private framebuffer: WebGLFramebuffer;
  private lookup: Layer;
  private frame: Layer;
  private trailA: Layer;
  private trailB: Layer;
  private ghost: Layer;
  private tiling: Tiling | null = null;
  private seam: Vec = [0, 0];
  private window: FrameWindow = { ox: 0, oy: 0, width: 1, height: 1, level: 0 };
  private hasFrame = false;
  private pendingDecay = 0;
  private fadeMs = 1400;
  private glowUntil = 0;

  constructor(private canvas: HTMLCanvasElement) {
    // Antialiasing is for the hat's polygons; full-screen triangles do not notice it.
    const gl = canvas.getContext('webgl2', { antialias: true, alpha: false, powerPreference: 'high-performance' });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    this.display = compile(gl, FULLSCREEN, DISPLAY);
    this.donut = compile(gl, DONUT_VERTEX, DONUT);
    this.glow = compile(gl, FULLSCREEN, GLOW);
    this.lookupProgram = compile(gl, FULLSCREEN, LOOKUP);
    this.framebuffer = gl.createFramebuffer()!;
    this.frame = this.createLayer();
    this.trailA = this.createLayer();
    this.trailB = this.createLayer();
    this.ghost = this.createLayer();
    this.lookup = this.createLayer();
    this.emptyVao = gl.createVertexArray()!;
    gl.bindVertexArray(this.emptyVao);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.bindTexture(gl.TEXTURE_2D, this.lookup.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, LOOKUP_SIZE, LOOKUP_SIZE, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  }

  private createLayer(): Layer {
    const { gl } = this;
    const texture = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, texture);
    for (const [key, value] of [
      [gl.TEXTURE_MIN_FILTER, gl.NEAREST],
      [gl.TEXTURE_MAG_FILTER, gl.NEAREST],
      [gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE],
      [gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE],
    ]) {
      gl.texParameteri(gl.TEXTURE_2D, key, value);
    }
    const layer = { texture, width: 0, height: 0 };
    this.resizeLayer(layer, 1, 1);
    return layer;
  }

  private resizeLayer(layer: Layer, width: number, height: number) {
    if (layer.width === width && layer.height === height) return false;
    const { gl } = this;
    gl.bindTexture(gl.TEXTURE_2D, layer.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, width, height, 0, gl.RED, gl.UNSIGNED_BYTE, null);
    layer.width = width;
    layer.height = height;
    return true;
  }

  /** Redraws the lookup texture for a new or deformed tile. */
  setTiling(tiling: Tiling) {
    const { gl, lookupProgram: program } = this;
    this.tiling = tiling;
    this.seam = seamOffset(tiling);
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.framebuffer);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.lookup.texture, 0);
    gl.viewport(0, 0, LOOKUP_SIZE, LOOKUP_SIZE);
    gl.useProgram(program.program);
    gl.uniform2f(program.uniform('u_t1'), tiling.t1[0], tiling.t1[1]);
    gl.uniform2f(program.uniform('u_t2'), tiling.t2[0], tiling.t2[1]);
    gl.uniform2i(program.uniform('u_g1'), tiling.g1[0], tiling.g1[1]);
    gl.uniform2i(program.uniform('u_g2'), tiling.g2[0], tiling.g2[1]);
    gl.uniform2fv(program.uniform('u_vertices'), tiling.polygons.flatMap((p) => p.vertices.flat()));
    gl.uniform1iv(program.uniform('u_counts'), tiling.polygons.map((p) => p.vertices.length));
    gl.uniform2iv(program.uniform('u_cells'), tiling.polygons.flatMap((p) => p.cell));
    gl.uniform1i(program.uniform('u_polygons'), tiling.polygons.length);
    gl.uniform1i(program.uniform('u_reach'), tiling.reach);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  }

  /** Uploads the pattern preview as a bitmap of its bounding box. */
  setGhost(shape: { cells: number[]; width: number; height: number }) {
    const { gl } = this;
    const data = new Uint8Array(shape.width * shape.height);
    for (let i = 0; i < shape.cells.length; i += 2) data[shape.cells[i + 1] * shape.width + shape.cells[i]] = 255;
    this.resizeLayer(this.ghost, shape.width, shape.height);
    gl.bindTexture(gl.TEXTURE_2D, this.ghost.texture);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, shape.width, shape.height, gl.RED, gl.UNSIGNED_BYTE, data);
  }

  setFade(ms: number) {
    this.fadeMs = ms;
  }

  /** Whether the picture still changes without any new frame arriving. */
  get animating() {
    return performance.now() < this.glowUntil;
  }

  upload(next: FrameWindow, data: Uint8Array<ArrayBuffer>) {
    const { gl } = this;
    this.resizeLayer(this.frame, next.width, next.height);
    gl.bindTexture(gl.TEXTURE_2D, this.frame.texture);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, next.width, next.height, gl.RED, gl.UNSIGNED_BYTE, data);

    const previous = this.window;
    const sameScale = this.hasFrame && previous.level === next.level;
    const shift: [number, number] = sameScale
      ? [(next.ox - previous.ox) >> next.level, (next.oy - previous.oy) >> next.level]
      : [0, 0];
    this.window = next;
    this.hasFrame = true;
    // A change of zoom level has no meaningful mapping for old texels.
    this.fadeTrail(shift, sameScale ? this.takeDecay() : 1);
    this.glowUntil = performance.now() + this.fadeMs;
  }

  /** Makes the next frame start without a trail, for when the field is replaced. */
  resetTrail() {
    this.hasFrame = false;
  }

  private takeDecay() {
    const decay = this.fadeMs > 0 ? this.pendingDecay : 1;
    this.pendingDecay = 0;
    return decay;
  }

  private fadeTrail(shift: [number, number], decay: number) {
    const { gl, glow } = this;
    this.resizeLayer(this.trailB, this.frame.width, this.frame.height);
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.framebuffer);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.trailB.texture, 0);
    gl.viewport(0, 0, this.trailB.width, this.trailB.height);
    gl.useProgram(glow.program);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.frame.texture);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.trailA.texture);
    gl.uniform1i(glow.uniform('u_frame'), 0);
    gl.uniform1i(glow.uniform('u_previous'), 1);
    gl.uniform2i(glow.uniform('u_shift'), shift[0], shift[1]);
    gl.uniform1f(glow.uniform('u_decay'), decay);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    [this.trailA, this.trailB] = [this.trailB, this.trailA];
  }

  /** Advances the afterglow. Shared by all ways of drawing the field. */
  advanceTrail(deltaMs: number) {
    if (this.hasFrame && this.animating) {
      this.pendingDecay += this.fadeMs > 0 ? deltaMs / this.fadeMs : 1;
      // Trails are stored in 8 bits, so smaller decrements would round away.
      if (this.pendingDecay >= 2 / 255) this.fadeTrail([0, 0], this.takeDecay());
    }
  }

  /** A program simply ignores the colours it does not use. */
  bindPalette(program: { uniform(name: string): WebGLUniformLocation | null }) {
    const { gl, palette } = this;
    gl.uniform3fv(program.uniform('u_paper'), palette.paper);
    gl.uniform3fv(program.uniform('u_line'), palette.line);
    gl.uniform3fv(program.uniform('u_ink'), palette.ink);
    gl.uniform3fv(program.uniform('u_trail'), palette.trail);
    gl.uniform3fv(program.uniform('u_ghostColour'), palette.ghost);
    gl.uniform1f(program.uniform('u_rim'), palette.rim);
  }

  clearToPaper() {
    const { gl, palette } = this;
    gl.clearColor(palette.paper[0], palette.paper[1], palette.paper[2], 1);
  }

  bindCellTextures(program: { uniform(name: string): WebGLUniformLocation | null }) {
    const { gl } = this;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.frame.texture);
    gl.uniform1i(program.uniform('u_frame'), 0);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.trailA.texture);
    gl.uniform1i(program.uniform('u_glow'), 1);
  }

  /** Everything both views need to look up the field. */
  private bindField(
    program: Renderer['display'],
    tiling: Tiling,
    options: { base: Vec; torus: TorusSize | null; seam: number; hover?: View['hover']; ghostAt?: View['ghostAt'] },
  ) {
    const { gl, window: w } = this;
    const textures = [
      ['u_lookup', this.lookup],
      ['u_frame', this.frame],
      ['u_glow', this.trailA],
      ['u_ghost', this.ghost],
    ] as const;
    textures.forEach(([name, layer], i) => {
      gl.activeTexture(gl.TEXTURE0 + i);
      gl.bindTexture(gl.TEXTURE_2D, layer.texture);
      gl.uniform1i(program.uniform(name), i);
    });
    this.bindPalette(program);
    const { hover, ghostAt, torus } = options;
    gl.uniform2i(program.uniform('u_base'), options.base[0], options.base[1]);
    gl.uniform2i(program.uniform('u_g1'), tiling.g1[0], tiling.g1[1]);
    gl.uniform2i(program.uniform('u_g2'), tiling.g2[0], tiling.g2[1]);
    gl.uniform1i(program.uniform('u_level'), w.level);
    gl.uniform2i(program.uniform('u_hover'), hover ? hover.x - w.ox : 0, hover ? hover.y - w.oy : 0);
    gl.uniform1f(program.uniform('u_brush'), hover ? hover.reach : -1);
    gl.uniformMatrix2fv(program.uniform('u_gridToWorld'), false, tiling.gridToWorld);
    gl.uniform1i(program.uniform('u_hasGhost'), ghostAt ? 1 : 0);
    gl.uniform2i(program.uniform('u_ghostAt'), ghostAt ? ghostAt.x - w.ox : 0, ghostAt ? ghostAt.y - w.oy : 0);
    gl.uniform2i(program.uniform('u_period'), torus?.width ?? 0, torus?.height ?? 0);
    gl.uniformMatrix2fv(program.uniform('u_worldToGrid'), false, tiling.worldToGrid);
    gl.uniform1f(program.uniform('u_seam'), options.seam);
  }

  draw(view: View, deltaMs: number) {
    const { gl, canvas, display, window: w, tiling } = this;
    if (!tiling) return;
    this.advanceTrail(deltaMs);

    // Exact integers go to the shader separately from the small fractional part,
    // so that precision does not suffer far from the origin.
    const { g1, g2, t1, t2, gridToWorld } = tiling;
    const gridDeterminant = g1[0] * g2[1] - g1[1] * g2[0];
    const latticeU = (view.centerX * g2[1] - view.centerY * g2[0]) / gridDeterminant;
    const latticeV = (view.centerY * g1[0] - view.centerX * g1[1]) / gridDeterminant;
    const wholeU = Math.floor(latticeU);
    const wholeV = Math.floor(latticeV);
    const worldDeterminant = t1[0] * t2[1] - t1[1] * t2[0];

    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.useProgram(display.program);
    const torus = view.torus;
    let seam = 0;
    if (torus) {
      const [a, b, c, d] = gridToWorld;
      const periodPixels = view.scale * Math.min(Math.hypot(a, b) * torus.width, Math.hypot(c, d) * torus.height);
      // Lines would only clutter a field that has shrunk to a few pixels.
      seam = 0.4 * Math.min(1, Math.max(0, (periodPixels - 12) / 36));
      gl.uniform2f(display.uniform('u_seamCenter'), modulo(view.centerX - this.seam[0], torus.width), modulo(view.centerY - this.seam[1], torus.height));
    }
    this.bindField(display, tiling, {
      base: [wholeU * g1[0] + wholeV * g2[0] - w.ox, wholeU * g1[1] + wholeV * g2[1] - w.oy],
      torus,
      seam,
      hover: view.hover,
      ghostAt: view.ghostAt,
    });
    gl.uniform2f(display.uniform('u_resolution'), canvas.width, canvas.height);
    gl.uniform1f(display.uniform('u_scale'), view.scale);
    gl.uniformMatrix2fv(display.uniform('u_worldToLattice'), false, [
      t2[1] / worldDeterminant,
      -t1[1] / worldDeterminant,
      -t2[0] / worldDeterminant,
      t1[0] / worldDeterminant,
    ]);
    gl.uniform2f(display.uniform('u_latticeFraction'), latticeU - wholeU, latticeV - wholeV);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  /** The torus as a ring in space, which needs the whole field in the frame texture. */
  drawDonut(view: DonutView, deltaMs: number) {
    const { gl, canvas, donut: program, window: w, tiling } = this;
    if (!tiling) return;
    this.advanceTrail(deltaMs);

    // The longer side runs around the ring, and the tube follows the proportions of
    // the field as far as a ring can: too thick closes the hole, too thin looks like wire.
    const { torus } = view;
    const [gxx, gxy, gyx, gyy] = tiling.gridToWorld;
    const across = Math.hypot(gxx, gxy) * torus.width;
    const down = Math.hypot(gyx, gyy) * torus.height;
    const swap = down > across;
    const minor = Math.min(0.45, Math.max(0.18, Math.min(across, down) / Math.max(across, down)));

    const aspect = canvas.width / canvas.height;
    const tangent = Math.tan((FIELD_OF_VIEW * Math.PI) / 360);
    const reach = 1 + minor;
    const distance = Math.max(((1.15 * reach) / (tangent * Math.min(1, aspect))) / view.zoom, reach + 0.12);
    const near = 0.02;
    const far = distance + 2 * reach;
    const perspective: Mat4 = [1 / (tangent * aspect), 0, 0, 0, 0, 1 / tangent, 0, 0, 0, 0, (far + near) / (near - far), -1, 0, 0, (2 * far * near) / (near - far), 0];
    const [sinY, cosY] = [Math.sin(view.yaw), Math.cos(view.yaw)];
    const [sinP, cosP] = [Math.sin(view.pitch), Math.cos(view.pitch)];
    const turn: Mat4 = [cosY, sinY, 0, 0, -sinY, cosY, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
    const tilt: Mat4 = [1, 0, 0, 0, 0, cosP, sinP, 0, 0, -sinP, cosP, 0, 0, 0, 0, 1];
    const rotation = multiply(tilt, turn);
    const away: Mat4 = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, -distance, 1];
    // A rotation's inverse is its transpose, which takes camera directions into the model.
    const intoModel = (x: number, y: number, z: number) => [0, 1, 2].map((i) => rotation[i * 4] * x + rotation[i * 4 + 1] * y + rotation[i * 4 + 2] * z);

    const { g1, g2 } = tiling;
    const determinant = g1[0] * g2[1] - g1[1] * g2[0];
    gl.bindVertexArray(this.emptyVao);
    gl.viewport(0, 0, canvas.width, canvas.height);
    this.clearToPaper();
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);
    gl.useProgram(program.program);
    this.bindField(program, tiling, { base: [-w.ox, -w.oy], torus, seam: 0.3 });
    gl.uniformMatrix4fv(program.uniform('u_transform'), false, multiply(perspective, multiply(away, rotation)));
    gl.uniform1f(program.uniform('u_minor'), minor);
    gl.uniform2f(program.uniform('u_size'), torus.width, torus.height);
    gl.uniform1i(program.uniform('u_swap'), swap ? 1 : 0);
    gl.uniform2f(program.uniform('u_seamShift'), this.seam[0], this.seam[1]);
    gl.uniformMatrix2fv(program.uniform('u_gridToLattice'), false, [g2[1] / determinant, -g1[1] / determinant, -g2[0] / determinant, g1[0] / determinant]);
    const [ex, ey, ez] = intoModel(0, 0, distance);
    gl.uniform3f(program.uniform('u_eye'), ex, ey, ez);
    const length = Math.hypot(-0.4, 0.7, 0.6);
    const [lx, ly, lz] = intoModel(-0.4 / length, 0.7 / length, 0.6 / length);
    gl.uniform3f(program.uniform('u_light'), lx, ly, lz);
    gl.drawArrays(gl.TRIANGLES, 0, DONUT_U * DONUT_V * 6);
    gl.disable(gl.DEPTH_TEST);
  }
}

const modulo = (value: number, size: number) => value - size * Math.floor(value / size);

/** Squared reach of a brush in world units, rounded up so that discs look round. */
export function brushReach(radius: number) {
  return radius * radius + radius * 0.8;
}
