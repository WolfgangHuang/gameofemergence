import { compile, FIELDS_PER_CELL, GraphScene, type Vec } from './GraphScene';
import { CELL_FILL, CELL_LINE } from './HatScene';
import type { Renderer } from './renderer';

/** Makes the average rhomb one unit of area, like the other tiles. */
const SIZE = 1 / Math.sqrt((((1 + Math.sqrt(5)) / 2) * Math.sin((2 * Math.PI) / 5) + Math.sin(Math.PI / 5)) / ((3 + Math.sqrt(5)) / 2));
const THICK = 16;

/** The powers of z = e^(2 pi i / 5) that make up a position, see `penrose.rs`. */
const BASIS: Vec[] = [0, 1, 2, 3].map((i) => [Math.cos((2 * Math.PI * i) / 5), Math.sin((2 * Math.PI * i) / 5)]);

/** A rhomb's corners when turned by nothing: thin ones at spread 1, thick at spread 3. */
const shape = (spread: number): Vec[] => {
  const [c, s] = [Math.cos((spread * Math.PI) / 5), Math.sin((spread * Math.PI) / 5)];
  return [[0, 0], [SIZE, 0], [SIZE * (1 + c), SIZE * s], [SIZE * c, SIZE * s]];
};
const THIN = shape(1);
const THICK_SHAPE = shape(3);

const VERTEX = /* glsl */ `#version 300 es
precision highp float;
precision highp int;
layout(location = 0) in int a_vertex;
layout(location = 1) in ivec4 a_split;
layout(location = 2) in int a_orientation;
uniform vec2 u_local[8];
uniform vec2 u_basis[4];
uniform mat2 u_turns[10];
uniform ivec4 u_reference;
uniform vec2 u_offset;
uniform vec2 u_viewport;
uniform float u_scale;
uniform int u_firstId;
uniform sampler2D u_frame;
uniform sampler2D u_glow;
uniform int u_hover;
uniform vec2 u_hoverAt;
uniform float u_brush;
flat out float v_live;
flat out float v_glow;
flat out float v_hovered;

void main() {
  ivec4 d = a_split - u_reference;
  vec2 anchor = u_offset + ${SIZE} * (float(d.x) * u_basis[0] + float(d.y) * u_basis[1] + float(d.z) * u_basis[2] + float(d.w) * u_basis[3]);
  int kind = (a_orientation & ${THICK}) != 0 ? 4 : 0;
  mat2 turn = u_turns[a_orientation & 15];
  vec2 world = anchor + turn * u_local[kind + a_vertex];
  vec2 clip = world * u_scale / (u_viewport / 2.0);
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);

  int id = gl_InstanceID + u_firstId;
  ivec2 texel = ivec2(id & 2047, id >> 11);
  ivec2 size = textureSize(u_frame, 0);
  bool known = all(lessThan(texel, size));
  v_live = known ? texelFetch(u_frame, texel, 0).r : 0.0;
  v_glow = known ? texelFetch(u_glow, texel, 0).r : 0.0;
  vec2 centre = anchor + turn * (0.5 * u_local[kind + 2]) - u_hoverAt;
  v_hovered = u_brush >= 0.0 && (id == u_hover || dot(centre, centre) <= u_brush) ? 1.0 : 0.0;
}`;

/** Penrose's rhombs on screen. */
export class PenroseScene extends GraphScene {
  private fill;
  private line;
  private vao: WebGLVertexArrayObject;
  private fillVertices: WebGLBuffer;
  private lineVertices: WebGLBuffer;

  constructor(gl: WebGL2RenderingContext) {
    super(gl);
    this.stretch = [SIZE, 0];
    this.fill = compile(gl, VERTEX, CELL_FILL);
    this.line = compile(gl, VERTEX, CELL_LINE);
    this.vao = gl.createVertexArray()!;
    this.fillVertices = gl.createBuffer()!;
    this.lineVertices = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.fillVertices);
    gl.bufferData(gl.ARRAY_BUFFER, Int32Array.from([0, 1, 2, 0, 2, 3]), gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.lineVertices);
    gl.bufferData(gl.ARRAY_BUFFER, Int32Array.from([0, 1, 1, 2, 2, 3, 3, 0]), gl.STATIC_DRAW);
  }

  private anchor(id: number): Vec {
    const at = id * FIELDS_PER_CELL;
    let [x, y] = [0, 0];
    for (let i = 0; i < 4; i++) {
      x += this.cells[at + 1 + i] * BASIS[i][0];
      y += this.cells[at + 1 + i] * BASIS[i][1];
    }
    return [SIZE * x, SIZE * y];
  }

  private corners(id: number): Vec[] {
    const code = this.cells[id * FIELDS_PER_CELL];
    const angle = ((code & 15) * Math.PI) / 5;
    const [c, s] = [Math.cos(angle), Math.sin(angle)];
    const [ax, ay] = this.anchor(id);
    return (code & THICK ? THICK_SHAPE : THIN).map(([x, y]): Vec => [ax + c * x - s * y, ay + s * x + c * y]);
  }

  override centerOf(id: number): Vec {
    const corners = this.corners(id);
    return [(corners[0][0] + corners[2][0]) / 2, (corners[0][1] + corners[2][1]) / 2];
  }

  protected containsPoint(id: number, x: number, y: number) {
    const corners = this.corners(id);
    let inside = false;
    for (let a = 0, b = 3; a < 4; b = a++) {
      const [xa, ya] = corners[a];
      const [xb, yb] = corners[b];
      if (ya > y !== yb > y && x < ((xb - xa) * (y - ya)) / (yb - ya) + xa) inside = !inside;
    }
    return inside;
  }

  /**
   * Positions go to the shader relative to a cell near the camera. Its parts have
   * to be close to those of the cells on screen, not just its position, or floats
   * would have to cancel large numbers. Vertices of one tiling guarantee that.
   */
  private reference(center: Vec): number {
    let [best, distance] = [0, Infinity];
    for (let reach = 2; reach <= 256 && distance === Infinity; reach *= 4) {
      for (const id of this.around(center[0], center[1], reach * reach)) {
        const [x, y] = this.centerOf(id);
        const d = (x - center[0]) ** 2 + (y - center[1]) ** 2;
        if (d < distance) [best, distance] = [id, d];
      }
    }
    return best;
  }

  draw(renderer: Renderer, center: Vec, scale: number, hover: { id: number; reach: number } | null) {
    const { gl } = this;
    this.syncInstances();
    this.updateIndex();

    const cameraWorld = this.worldOf(center);
    const reference = this.count ? this.reference(cameraWorld) : 0;
    const base = reference * FIELDS_PER_CELL;
    const parts = this.count ? [this.cells[base + 1], this.cells[base + 2], this.cells[base + 3], this.cells[base + 4]] : [0, 0, 0, 0];
    const anchor = this.count ? this.anchor(reference) : ([0, 0] as Vec);
    const offset: Vec = [anchor[0] - cameraWorld[0], anchor[1] - cameraWorld[1]];

    const canvas = gl.canvas as HTMLCanvasElement;
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, canvas.width, canvas.height);
    renderer.clearToPaper();
    gl.clear(gl.COLOR_BUFFER_BIT);

    const turns: number[] = [];
    for (let k = 0; k < 10; k++) {
      const [c, s] = [Math.cos((k * Math.PI) / 5), Math.sin((k * Math.PI) / 5)];
      turns.push(c, s, -s, c);
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
      gl.uniform2fv(program.uniform('u_local'), [...THIN, ...THICK_SHAPE].flat());
      gl.uniform2fv(program.uniform('u_basis'), BASIS.flat());
      gl.uniformMatrix2fv(program.uniform('u_turns'), false, turns);
      gl.uniform4i(program.uniform('u_reference'), parts[0], parts[1], parts[2], parts[3]);
      gl.uniform2f(program.uniform('u_offset'), offset[0], offset[1]);
      gl.uniform2f(program.uniform('u_viewport'), canvas.width, canvas.height);
      gl.uniform1f(program.uniform('u_scale'), scale);
      gl.uniform1i(program.uniform('u_hover'), hover ? hover.id : -1);
      gl.uniform1f(program.uniform('u_brush'), hover ? hover.reach : -1);
      if (hover) {
        const [hx, hy] = this.centerOf(hover.id);
        gl.uniform2f(program.uniform('u_hoverAt'), hx - cameraWorld[0], hy - cameraWorld[1]);
      }
      renderer.bindPalette(program);
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

    pass(this.fill, this.fillVertices, gl.TRIANGLES, 6);
    if (grid > 0) {
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.useProgram(this.line.program);
      gl.uniform1f(this.line.uniform('u_grid'), grid * grid * (3 - 2 * grid));
      pass(this.line, this.lineVertices, gl.LINES, 8);
      gl.disable(gl.BLEND);
    }
    gl.bindVertexArray(renderer.emptyVao);
  }
}
