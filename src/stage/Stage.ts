import { HatScene } from '../render/HatScene';
import { brushReach, Renderer } from '../render/renderer';
import type { Camera, Recipe } from '../share/link';
import type { Shape } from '../patterns/rle';
import type { FromWorker, Stats, ToWorker, TorusSize } from '../sim/protocol';
import { gridToWorld, locate, worldToGrid, type Tiling } from '../tiles/tiles';

export type Tool = 'brush' | 'eraser' | 'hand' | 'stamp';

const MIN_CELL = 1 / 64;
const MAX_CELL = 96;
const COORD_LIMIT = 1e9;
/** A window may hold this many texels per device pixel before it drops a level. */
const MAX_TEXELS_PER_PIXEL = 2;
const MAX_SOUP = 2048;
/** In lattice units. A random field beyond this would stall the generation. */
const MAX_HAT_SOUP = 250;
/** World units between stroke samples, about half a hat. */
const HAT_STROKE_STEP = 0.5;

/** Fraction of the screen that a torus fills after fitting it in. */
const TORUS_FIT = 0.9;

interface Point {
  x: number;
  y: number;
}

interface SavedCamera {
  x: number;
  y: number;
  size: number;
}

export interface FieldSnapshot {
  cells: Int32Array | null;
  generation: number;
  population: number;
  /** Set while the field still follows from a single random fill. */
  recipe: Recipe | null;
  camera: Camera;
}

interface FieldOrigin {
  /** Nothing has been done to this field since it was last empty. */
  pristine: boolean;
  recipe: Recipe | null;
}

export class Stage {
  tool: Tool = 'brush';
  brushRadius = 0;
  onStats: (stats: Stats & { rate: number }) => void = () => {};
  private stampShape: Shape | null = null;

  private renderer: Renderer;
  private worker: Worker;
  private dpr = 1;
  /** Grid position of the screen centre. It stays put when the tile deforms. */
  private centerX = 0;
  private centerY = 0;
  /** Device pixels per world unit. */
  private cellSize = 1;
  private tiling: Tiling | null = null;
  private topology = -1;
  private torus: TorusSize | null = null;
  /** The size the saved torus camera belongs to. */
  private lastTorus: TorusSize | null = null;
  private savedCamera: { plane?: SavedCamera; torus?: SavedCamera } = {};
  private origins: Record<'plane' | 'torus' | 'hat', FieldOrigin> = {
    plane: { pristine: true, recipe: null },
    torus: { pristine: true, recipe: null },
    hat: { pristine: true, recipe: null },
  };
  private nextExport = 0;
  private exports = new Map<number, (m: Extract<FromWorker, { t: 'cells' }>) => void>();
  private fitPending = false;

  /** Set while the hat tiling is showing; `hat` exists once the worker built it. */
  private hatMode = false;
  private hat: HatScene | null = null;
  private pendingMorph = 60;
  /** Lattice position of the screen centre, which stays put during a morph. */
  private hatCenter: [number, number] = [0, 0];
  private hatHover: number | null = null;
  private hatSeed: [number, number] | null = null;
  private lastHatPoint: Point | null = null;
  capped = false;
  onCapped: (capped: boolean) => void = () => {};

  private running = false;
  private frameInFlight = false;
  private frameDirty = true;
  private needsDraw = true;
  private spare: ArrayBuffer | null = null;
  private raf = 0;
  private lastTime = 0;

  private pointers = new Map<number, Point>();
  private dragMode: 'paint' | 'pan' | 'pinch' | null = null;
  private paintAlive = true;
  private lastCell: Point | null = null;
  private lastPan: Point = { x: 0, y: 0 };
  private pinch = { distance: 0, center: { x: 0, y: 0 } };
  private hoverCell: Point | null = null;

  private rate = 0;
  private rateGeneration = 0;
  private rateTime = 0;
  private statsTime = 0;

  private abort = new AbortController();

  constructor(private canvas: HTMLCanvasElement) {
    this.renderer = new Renderer(canvas);
    this.worker = new Worker(new URL('../sim/worker.ts', import.meta.url), { type: 'module' });
    this.worker.onmessage = (event: MessageEvent<FromWorker>) => this.receive(event.data);

    this.resize();
    this.cellSize = 14 * this.dpr;

    const { signal } = this.abort;
    window.addEventListener('resize', () => this.resize(), { signal });
    canvas.addEventListener('pointerdown', (e) => this.pointerDown(e), { signal });
    canvas.addEventListener('pointermove', (e) => this.pointerMove(e), { signal });
    canvas.addEventListener('pointerup', (e) => this.pointerUp(e), { signal });
    canvas.addEventListener('pointercancel', (e) => this.pointerUp(e), { signal });
    canvas.addEventListener('pointerleave', () => this.setHover(null), { signal });
    canvas.addEventListener('wheel', (e) => this.wheel(e), { signal, passive: false });
    canvas.addEventListener('contextmenu', (e) => e.preventDefault(), { signal });

    this.raf = requestAnimationFrame((t) => this.tick(t));
  }

  dispose() {
    cancelAnimationFrame(this.raf);
    this.abort.abort();
    this.worker.terminate();
  }

  // Commands

  setRunning(running: boolean) {
    this.running = running;
    this.send({ t: 'running', running });
    this.frameDirty = true;
  }

  setSpeed(gps: number | null) {
    this.send({ t: 'speed', gps });
  }

  setFade(ms: number) {
    this.renderer.setFade(ms);
  }

  step() {
    this.send({ t: 'step' });
    this.frameDirty = true;
  }

  clear() {
    this.origin.pristine = true;
    this.origin.recipe = null;
    this.send({ t: 'clear' });
    this.frameDirty = true;
  }

  /** Picks the hat tiling, the plane or a torus. Call `setTiling` afterwards for the latter two. */
  setSurface(hat: boolean, torus: TorusSize | null) {
    const next = hat ? null : torus;
    const sameField = hat === this.hatMode && next?.width === this.torus?.width && next?.height === this.torus?.height;
    if (sameField) return;
    const resized = next !== null && (next.width !== this.lastTorus?.width || next.height !== this.lastTorus?.height);

    if (!this.hatMode) this.savedCamera[this.torus ? 'torus' : 'plane'] = { x: this.centerX, y: this.centerY, size: this.cellSize };
    // A new size drops cells, so the random fill alone no longer explains the field.
    if (resized) this.origins.torus.recipe = null;
    if (next && resized) {
      this.fitPending = true;
    } else if (!hat) {
      const saved = this.savedCamera[next ? 'torus' : 'plane'];
      if (saved) {
        this.centerX = saved.x;
        this.centerY = saved.y;
        this.cellSize = saved.size;
      }
    }
    this.hatMode = hat;
    this.torus = next;
    if (next) this.lastTorus = next;
    // Each field has its own topology and rule, which the caller sends next.
    this.topology = -1;
    this.renderer.resetTrail();
    this.send({ t: 'surface', hat, torus: next });
    this.viewChanged();
  }

  setHatEdgesOnly(edgesOnly: boolean) {
    this.send({ t: 'hatEdgesOnly', edgesOnly });
    this.frameDirty = true;
  }

  setHatTable(table: number[]) {
    const rows = Uint32Array.from(table);
    this.send({ t: 'hatTable', rows }, [rows.buffer]);
    this.frameDirty = true;
  }

  /** What a click places while the stamp tool is active on the hat tiling. */
  setHatSeed(seed: [number, number] | null) {
    this.hatSeed = seed;
  }

  setMorph(degrees: number) {
    this.pendingMorph = degrees;
    this.hat?.setMorph(degrees);
    this.viewChanged();
  }

  setTiling(tiling: Tiling, topology: number) {
    this.tiling = tiling;
    this.renderer.setTiling(tiling);
    if (topology !== this.topology) {
      this.topology = topology;
      this.send({ t: 'topology', topology });
    }
    if (this.fitPending) this.fitTorus();
    this.viewChanged();
  }

  setRule(birth: number, survive: number) {
    this.send({ t: 'rule', birth, survive });
    this.frameDirty = true;
  }

  setTool(tool: Tool) {
    this.tool = tool;
    this.needsDraw = true;
  }

  setStamp(shape: Shape | null) {
    this.stampShape = shape;
    if (shape) this.renderer.setGhost(shape);
    this.needsDraw = true;
  }

  /** Fills the visible area, or a centred part of it when zoomed far out. */
  randomize(density = 0.3) {
    const seed = (Math.random() * 2 ** 32) >>> 0;
    if (this.hatMode) {
      if (!this.hat) return;
      const short = (value: number) => Number(value.toFixed(3));
      const radius = short(Math.min(this.hatViewRadius(), MAX_HAT_SOUP));
      this.applyRecipe({ kind: 'hat', x: short(this.hatCenter[0]), y: short(this.hatCenter[1]), radius, density, seed });
      return;
    }
    if (this.torus) {
      this.applyRecipe({ kind: 'grid', x: 0, y: 0, ...this.torus, density, seed });
      return;
    }
    const view = this.visibleCells();
    const width = Math.min(Math.ceil(view.width), MAX_SOUP);
    const height = Math.min(Math.ceil(view.height), MAX_SOUP);
    const x = Math.floor(view.x + (view.width - width) / 2);
    const y = Math.floor(view.y + (view.height - height) / 2);
    this.applyRecipe({ kind: 'grid', x, y, width, height, density, seed });
  }

  /** Performs a random fill. Same recipe on an empty field, same result. */
  applyRecipe(recipe: Recipe) {
    const origin = this.origin;
    if (recipe.kind === 'hat') {
      const { x, y, radius, density, seed } = recipe;
      this.send({ t: 'hatSoup', x, y, radius, density, seed });
    } else {
      const { x, y, width, height, density, seed } = recipe;
      this.send({ t: 'soup', x, y, width, height, density, seed });
    }
    origin.recipe = origin.pristine ? recipe : null;
    origin.pristine = false;
    this.frameDirty = true;
  }

  /** Adds live cells, given as flat x, y pairs. */
  loadCells(cells: Int32Array<ArrayBuffer>) {
    this.send({ t: 'paint', cells, alive: true }, [cells.buffer]);
    this.frameDirty = true;
  }

  setCamera(camera: Camera) {
    this.cellSize = clamp(camera.size * this.dpr, MIN_CELL, MAX_CELL);
    if (this.hatMode) this.hatCenter = [camera.x, camera.y];
    else {
      this.centerX = camera.x;
      this.centerY = camera.y;
    }
    this.fitPending = false;
    this.viewChanged();
  }

  /** What a link needs to know about the field being shown. */
  snapshot(maxCells: number): Promise<FieldSnapshot> {
    const id = this.nextExport++;
    const { recipe } = this.origin;
    const [x, y] = this.hatMode ? this.hatCenter : [this.centerX, this.centerY];
    const camera = { x, y, size: this.cellSize / this.dpr };
    return new Promise((resolve) => {
      this.exports.set(id, ({ cells, generation, population }) => resolve({ cells, generation, population, recipe, camera }));
      this.send({ t: 'exportCells', id, max: maxCells });
    });
  }

  private get origin() {
    return this.origins[this.hatMode ? 'hat' : this.torus ? 'torus' : 'plane'];
  }

  // Worker

  private send(message: ToWorker, transfer: Transferable[] = []) {
    if (message.t === 'paint' || message.t === 'hatPaint' || message.t === 'hatSeed') {
      this.origin.pristine = false;
      this.origin.recipe = null;
    }
    this.worker.postMessage(message, transfer);
  }

  private receive(m: FromWorker) {
    if (m.t === 'ready') return;
    if (m.t === 'cells') {
      this.exports.get(m.id)?.(m);
      this.exports.delete(m.id);
      return;
    }
    if (m.t === 'hatReady') {
      this.hat = new HatScene(this.renderer.gl, m.outline, m.ratio);
      this.hat.setMorph(this.pendingMorph);
      this.viewChanged();
      return;
    }
    if (m.t === 'hatCells') {
      this.hat?.append(m.cells);
      if (m.capped !== this.capped) {
        this.capped = m.capped;
        this.onCapped(m.capped);
      }
      this.needsDraw = true;
      return;
    }
    this.frameInFlight = false;
    if (m.t === 'frame') {
      this.renderer.upload(m, m.data);
      this.spare = m.data.buffer;
      this.needsDraw = true;
    } else {
      this.spare = m.spare;
    }
    this.report(m);
  }

  private report(stats: Stats) {
    const now = performance.now();
    if (now - this.rateTime >= 500) {
      const seconds = (now - this.rateTime) / 1000;
      this.rate = this.running ? Math.max(0, stats.generation - this.rateGeneration) / seconds : 0;
      this.rateGeneration = stats.generation;
      this.rateTime = now;
    }
    if (now - this.statsTime >= 100 || !this.running) {
      this.statsTime = now;
      this.onStats({ ...stats, rate: this.rate });
    }
  }

  private requestFrame() {
    if (this.frameInFlight || !(this.frameDirty || this.running)) return;
    if (this.hatMode) {
      const spare = this.spare;
      this.spare = null;
      this.frameInFlight = true;
      this.frameDirty = false;
      const radius = this.hat ? this.hatViewRadius() * 1.15 + 8 : 0;
      this.send({ t: 'hatFrame', x: this.hatCenter[0], y: this.hatCenter[1], radius, spare }, spare ? [spare] : []);
      return;
    }
    if (!this.tiling) return;
    const view = this.visibleCells();
    // Overscan keeps the picture filled while panning ahead of the next frame.
    const margin = Math.max(view.width, view.height) / 8;
    const fullWidth = view.width + 2 * margin;
    const fullHeight = view.height + 2 * margin;
    // A slanted tile needs a bounding box larger than the screen, so the level
    // follows from the texel count rather than from the zoom alone.
    const budget = MAX_TEXELS_PER_PIXEL * this.canvas.width * this.canvas.height;
    let level = 0;
    while ((fullWidth * fullHeight) / 4 ** level > budget) level++;
    const texel = 2 ** level;
    let ox = Math.floor((view.x - margin) / texel) * texel;
    let oy = Math.floor((view.y - margin) / texel) * texel;
    let width = Math.ceil(fullWidth / texel) + 1;
    let height = Math.ceil(fullHeight / texel) + 1;
    // One period of a torus is enough, the renderer repeats it.
    if (this.torus) {
      if (width * texel >= this.torus.width) {
        ox = 0;
        width = Math.ceil(this.torus.width / texel);
      }
      if (height * texel >= this.torus.height) {
        oy = 0;
        height = Math.ceil(this.torus.height / texel);
      }
    }
    const spare = this.spare;
    this.spare = null;
    this.frameInFlight = true;
    this.frameDirty = false;
    this.send({ t: 'frame', ox, oy, width, height, level, spare }, spare ? [spare] : []);
  }

  private tick(time: number) {
    this.raf = requestAnimationFrame((t) => this.tick(t));
    const delta = time - this.lastTime;
    this.lastTime = time;
    this.requestFrame();
    if (this.hatMode) {
      if (this.hat && (this.needsDraw || this.renderer.animating)) {
        this.needsDraw = false;
        this.renderer.advanceTrail(delta);
        const hover = this.hatHover === null || this.tool === 'hand' ? null : { id: this.hatHover, reach: this.tool === 'stamp' ? 0 : brushReach(this.brushRadius) };
        this.hat.draw(this.renderer, this.hatCenter, this.cellSize, hover);
      }
      return;
    }
    if (this.needsDraw || this.renderer.animating) {
      this.needsDraw = false;
      this.renderer.draw(
        {
          centerX: this.centerX,
          centerY: this.centerY,
          scale: this.cellSize,
          hover: this.hoverCell && (this.tool === 'brush' || this.tool === 'eraser') ? { ...this.hoverCell, reach: brushReach(this.brushRadius) } : null,
          ghostAt: this.hoverCell && this.tool === 'stamp' && this.stampShape ? this.stampOrigin(this.hoverCell, this.stampShape) : null,
          torus: this.torus,
        },
        delta,
      );
    }
  }

  // Camera

  private resize() {
    this.dpr = window.devicePixelRatio || 1;
    const { canvas } = this;
    canvas.width = Math.round(canvas.clientWidth * this.dpr);
    canvas.height = Math.round(canvas.clientHeight * this.dpr);
    this.viewChanged();
  }

  private viewChanged() {
    this.frameDirty = true;
    this.needsDraw = true;
  }

  /** Centres the torus and shows all of it. */
  private fitTorus() {
    this.fitPending = false;
    const { torus, tiling, canvas } = this;
    if (!torus || !tiling) return;
    const across = gridToWorld(tiling, torus.width, 0);
    const down = gridToWorld(tiling, 0, torus.height);
    const worldWidth = Math.abs(across[0]) + Math.abs(down[0]);
    const worldHeight = Math.abs(across[1]) + Math.abs(down[1]);
    this.centerX = torus.width / 2;
    this.centerY = torus.height / 2;
    this.cellSize = clamp(TORUS_FIT * Math.min(canvas.width / worldWidth, canvas.height / worldHeight), MIN_CELL, MAX_CELL);
  }

  /** Bounding box in grid cells of what is on screen. */
  private visibleCells() {
    const { canvas, cellSize, tiling } = this;
    const halfWidth = canvas.width / cellSize / 2;
    const halfHeight = canvas.height / cellSize / 2;
    if (!tiling) return { x: this.centerX - halfWidth, y: this.centerY - halfHeight, width: 2 * halfWidth, height: 2 * halfHeight };
    let extentX = 0;
    let extentY = 0;
    for (const [wx, wy] of [
      [halfWidth, halfHeight],
      [halfWidth, -halfHeight],
    ]) {
      const [gx, gy] = worldToGrid(tiling, wx, wy);
      extentX = Math.max(extentX, Math.abs(gx));
      extentY = Math.max(extentY, Math.abs(gy));
    }
    return { x: this.centerX - extentX, y: this.centerY - extentY, width: 2 * extentX, height: 2 * extentY };
  }

  private device(e: { clientX: number; clientY: number }): Point {
    const rect = this.canvas.getBoundingClientRect();
    return { x: (e.clientX - rect.left) * this.dpr, y: (e.clientY - rect.top) * this.dpr };
  }

  /** How far the screen reaches from its centre, in lattice units. */
  private hatViewRadius() {
    const hat = this.hat!;
    const world = Math.hypot(this.canvas.width, this.canvas.height) / 2 / this.cellSize;
    return world / Math.hypot(hat.stretch[0], hat.stretch[1]);
  }

  /** World position under a device pixel in the hat tiling. */
  private hatWorld(p: Point): [number, number] {
    const hat = this.hat!;
    const [cx, cy] = hat.worldOf(this.hatCenter);
    return [cx + (p.x - this.canvas.width / 2) / this.cellSize, cy + (p.y - this.canvas.height / 2) / this.cellSize];
  }

  private paintHatAt(p: Point) {
    const hat = this.hat!;
    const [x, y] = this.hatWorld(p);
    const id = hat.locate(x, y);
    if (id === null) return;
    const [cx, cy] = hat.centerOf(id);
    const reach = brushReach(this.brushRadius);
    const cells = Uint32Array.from(reach > 0 ? hat.around(cx, cy, reach) : [id]);
    this.send({ t: 'hatPaint', cells, alive: this.paintAlive }, [cells.buffer]);
    this.frameDirty = true;
  }

  private paintHatStroke(from: Point, to: Point) {
    const distance = Math.hypot(to.x - from.x, to.y - from.y) / this.cellSize;
    const steps = Math.max(1, Math.ceil(distance / HAT_STROKE_STEP));
    for (let i = 1; i <= steps; i++) {
      this.paintHatAt({ x: from.x + ((to.x - from.x) * i) / steps, y: from.y + ((to.y - from.y) * i) / steps });
    }
  }

  private cellAt(p: Point): Point {
    const tiling = this.tiling!;
    const [cx, cy] = gridToWorld(tiling, this.centerX, this.centerY);
    const [x, y] = locate(
      tiling,
      cx + (p.x - this.canvas.width / 2) / this.cellSize,
      cy + (p.y - this.canvas.height / 2) / this.cellSize,
    );
    return { x, y };
  }

  private panBy(dx: number, dy: number) {
    if (this.hatMode) {
      if (!this.hat) return;
      const [lx, ly] = this.hat.latticeOf([dx / this.cellSize, dy / this.cellSize]);
      this.hatCenter = [this.hatCenter[0] - lx, this.hatCenter[1] - ly];
      this.viewChanged();
      return;
    }
    const [gx, gy] = worldToGrid(this.tiling!, dx / this.cellSize, dy / this.cellSize);
    this.centerX = clamp(this.centerX - gx, -COORD_LIMIT, COORD_LIMIT);
    this.centerY = clamp(this.centerY - gy, -COORD_LIMIT, COORD_LIMIT);
    this.viewChanged();
  }

  private zoomAt(p: Point, factor: number) {
    const size = clamp(this.cellSize * factor, MIN_CELL, MAX_CELL);
    if (this.hatMode) {
      if (!this.hat) return;
      const fromX = p.x - this.canvas.width / 2;
      const fromY = p.y - this.canvas.height / 2;
      const [lx, ly] = this.hat.latticeOf([fromX / this.cellSize - fromX / size, fromY / this.cellSize - fromY / size]);
      this.hatCenter = [this.hatCenter[0] + lx, this.hatCenter[1] + ly];
      this.cellSize = size;
      this.viewChanged();
      return;
    }
    const fromCenterX = p.x - this.canvas.width / 2;
    const fromCenterY = p.y - this.canvas.height / 2;
    const [gx, gy] = worldToGrid(
      this.tiling!,
      fromCenterX / this.cellSize - fromCenterX / size,
      fromCenterY / this.cellSize - fromCenterY / size,
    );
    this.centerX += gx;
    this.centerY += gy;
    this.cellSize = size;
    this.viewChanged();
  }

  private wheel(e: WheelEvent) {
    e.preventDefault();
    const lines = e.deltaMode === WheelEvent.DOM_DELTA_LINE ? 16 : 1;
    this.zoomAt(this.device(e), Math.exp(-e.deltaY * lines * (e.ctrlKey ? 0.01 : 0.0018)));
  }

  // Pointer input

  private pointerDown(e: PointerEvent) {
    this.canvas.setPointerCapture(e.pointerId);
    const p = this.device(e);
    this.pointers.set(e.pointerId, p);

    if (this.pointers.size === 2) {
      this.dragMode = 'pinch';
      this.pinch = this.measurePinch();
      this.setHover(null);
      return;
    }
    const wantsPan = this.tool === 'hand' || e.button === 1 || e.button === 2 || e.shiftKey;
    if (wantsPan) {
      this.dragMode = 'pan';
      this.lastPan = p;
      return;
    }
    if (this.hatMode) {
      if (!this.hat) return;
      if (this.tool === 'stamp') {
        const [x, y] = this.hatWorld(p);
        const cell = this.hat.locate(x, y);
        if (cell !== null && this.hatSeed) {
          this.send({ t: 'hatSeed', cell, first: this.hatSeed[0], second: this.hatSeed[1] });
          this.frameDirty = true;
        }
        return;
      }
      this.dragMode = 'paint';
      this.paintAlive = this.tool === 'brush';
      this.lastHatPoint = p;
      this.paintHatAt(p);
      return;
    }
    if (this.tool === 'stamp') {
      if (this.stampShape) this.placeStamp(this.cellAt(p), this.stampShape);
      return;
    }
    this.dragMode = 'paint';
    this.paintAlive = this.tool === 'brush';
    this.lastCell = this.cellAt(p);
    this.paintLine(this.lastCell, this.lastCell);
  }

  private pointerMove(e: PointerEvent) {
    const p = this.device(e);
    if (this.pointers.has(e.pointerId)) this.pointers.set(e.pointerId, p);
    if (this.hatMode) {
      this.hatPointerMove(p, e.pointerType === 'mouse');
      return;
    }
    const cell = this.cellAt(p);

    switch (this.dragMode) {
      case 'pinch': {
        if (this.pointers.size < 2) break;
        const next = this.measurePinch();
        this.panBy(next.center.x - this.pinch.center.x, next.center.y - this.pinch.center.y);
        this.zoomAt(next.center, next.distance / this.pinch.distance);
        this.pinch = next;
        break;
      }
      case 'pan':
        this.panBy(p.x - this.lastPan.x, p.y - this.lastPan.y);
        this.lastPan = p;
        break;
      case 'paint':
        if (this.lastCell && (cell.x !== this.lastCell.x || cell.y !== this.lastCell.y)) {
          this.paintLine(this.lastCell, cell);
          this.lastCell = cell;
        }
        break;
    }
    if (e.pointerType === 'mouse') this.setHover(cell);
  }

  private hatPointerMove(p: Point, isMouse: boolean) {
    if (!this.hat) return;
    switch (this.dragMode) {
      case 'pinch': {
        if (this.pointers.size < 2) break;
        const next = this.measurePinch();
        this.panBy(next.center.x - this.pinch.center.x, next.center.y - this.pinch.center.y);
        this.zoomAt(next.center, next.distance / this.pinch.distance);
        this.pinch = next;
        break;
      }
      case 'pan':
        this.panBy(p.x - this.lastPan.x, p.y - this.lastPan.y);
        this.lastPan = p;
        break;
      case 'paint':
        if (this.lastHatPoint) this.paintHatStroke(this.lastHatPoint, p);
        this.lastHatPoint = p;
        break;
    }
    if (isMouse) {
      const [x, y] = this.hatWorld(p);
      const id = this.hat.locate(x, y);
      if (id !== this.hatHover) {
        this.hatHover = id;
        this.needsDraw = true;
      }
    }
  }

  private pointerUp(e: PointerEvent) {
    this.pointers.delete(e.pointerId);
    if (this.pointers.size === 0) this.dragMode = null;
    this.lastCell = null;
  }

  private measurePinch() {
    const [a, b] = [...this.pointers.values()];
    return {
      distance: Math.max(1, Math.hypot(a.x - b.x, a.y - b.y)),
      center: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 },
    };
  }

  private setHover(cell: Point | null) {
    if (cell === null && this.hatHover !== null) {
      this.hatHover = null;
      this.needsDraw = true;
    }
    const old = this.hoverCell;
    if (old?.x === cell?.x && old?.y === cell?.y) return;
    this.hoverCell = cell;
    this.needsDraw = true;
  }

  private stampOrigin(cell: Point, shape: Shape): Point {
    return { x: cell.x - (shape.width >> 1), y: cell.y - (shape.height >> 1) };
  }

  private placeStamp(cell: Point, shape: Shape) {
    const origin = this.stampOrigin(cell, shape);
    const buffer = Int32Array.from(shape.cells, (v, i) => v + (i % 2 === 0 ? origin.x : origin.y));
    this.send({ t: 'paint', cells: buffer, alive: true }, [buffer.buffer]);
    this.frameDirty = true;
  }

  /** Stamps the brush disc along a Bresenham line. */
  private paintLine(from: Point, to: Point) {
    const tiling = this.tiling!;
    const reach = brushReach(this.brushRadius);
    // The disc is round in world space, whatever the grid does to it.
    const distance = Math.sqrt(reach);
    const [ax, ay] = worldToGrid(tiling, distance, distance);
    const [bx, by] = worldToGrid(tiling, distance, -distance);
    const boundX = Math.ceil(Math.max(Math.abs(ax), Math.abs(bx)));
    const boundY = Math.ceil(Math.max(Math.abs(ay), Math.abs(by)));
    const stamp: number[] = [];
    for (let dy = -boundY; dy <= boundY; dy++) {
      for (let dx = -boundX; dx <= boundX; dx++) {
        const [wx, wy] = gridToWorld(tiling, dx, dy);
        if (wx * wx + wy * wy <= reach) stamp.push(dx, dy);
      }
    }

    const cells: number[] = [];
    let { x, y } = from;
    const stepX = Math.sign(to.x - x);
    const stepY = Math.sign(to.y - y);
    const distX = Math.abs(to.x - x);
    const distY = -Math.abs(to.y - y);
    let error = distX + distY;
    for (;;) {
      for (let i = 0; i < stamp.length; i += 2) cells.push(x + stamp[i], y + stamp[i + 1]);
      if (x === to.x && y === to.y) break;
      const doubled = 2 * error;
      if (doubled >= distY) {
        error += distY;
        x += stepX;
      }
      if (doubled <= distX) {
        error += distX;
        y += stepY;
      }
    }
    const buffer = Int32Array.from(cells);
    this.send({ t: 'paint', cells: buffer, alive: this.paintAlive }, [buffer.buffer]);
    this.frameDirty = true;
  }
}

function clamp(value: number, low: number, high: number) {
  return Math.min(high, Math.max(low, value));
}
