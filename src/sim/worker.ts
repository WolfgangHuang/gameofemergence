import wasmUrl from './engine.wasm?url';
import type { FromWorker, StatusKind, ToWorker } from './protocol';

interface Exports {
  memory: WebAssembly.Memory;
  world_new(): number;
  world_set_rule(w: number, birth: number, survive: number): number;
  world_set_topology(w: number, topology: number): void;
  world_set_torus(w: number, width: number, height: number): number;
  world_step(w: number, n: number): void;
  world_clear(w: number): void;
  world_scratch(w: number, n: number): number;
  world_paint(w: number, n: number, alive: number): void;
  world_soup(w: number, x: number, y: number, width: number, height: number, density: number, seed: number): void;
  world_frame(w: number, ox: number, oy: number, width: number, height: number, level: number): number;
  world_generation(w: number): number;
  world_population(w: number): number;
  world_status_kind(w: number): number;
  world_status_period(w: number): number;
  world_status_detail(w: number): number;
  world_status_since(w: number): number;

  hat_new(): number;
  hat_set_rule(w: number, birth: number, survive: number): number;
  hat_set_edges_only(w: number, edgesOnly: number): void;
  hat_set_table(w: number, n: number): void;
  hat_seed_pair(w: number, cell: number, first: number, second: number): void;
  hat_step(w: number, n: number): void;
  hat_clear(w: number): void;
  hat_scratch(w: number, n: number): number;
  hat_paint(w: number, n: number, alive: number): void;
  hat_soup(w: number, x: number, y: number, radius: number, density: number, seed: number): void;
  hat_load(w: number, x: number, y: number, radius: number): number;
  hat_cell_count(w: number): number;
  hat_cells(w: number, from: number): number;
  hat_outline(w: number): number;
  hat_states(w: number): number;
  hat_ratio_re(w: number): number;
  hat_ratio_im(w: number): number;
  hat_capped(w: number): number;
  hat_generation(w: number): number;
  hat_population(w: number): number;
  hat_status_kind(w: number): number;
  hat_status_period(w: number): number;
  hat_status_detail(w: number): number;
  hat_status_since(w: number): number;
}

const post = (message: FromWorker, transfer: Transferable[] = []) => self.postMessage(message, { transfer });

const SLICE_MS = 10;
const STATUS_KINDS: StatusKind[] = ['active', 'empty', 'still', 'oscillating', 'drifting', 'growing'];
/** Beyond this view radius, generating every visible hat would take too long. */
const MAX_LOAD_RADIUS = 700;
/** Cell states travel as a texture of this width, in order of their ids. */
const CELL_TEXTURE_WIDTH = 2048;
const FIELDS_PER_CELL = 5;

let e: Exports;
/** The unbounded plane, and the torus once it was asked for. */
let plane = 0;
let torus = 0;
let torusSize = '';
/** The grid field being shown, whichever of the two that is. */
let world = 0;
let hat = 0;
let hatActive = false;
let sentCells = 0;
let running = false;
let gps: number | null = 30;
let scheduled = false;
let due = 0;
let lastTick = 0;
/** Bumped on every state change, so that identical frames need not be rebuilt. */
let version = 0;
let lastFrameKey = '';

const yieldChannel = new MessageChannel();
yieldChannel.port1.onmessage = () => pump();

function schedule(immediately: boolean) {
  if (scheduled) return;
  scheduled = true;
  if (immediately) yieldChannel.port2.postMessage(0);
  else setTimeout(pump, 4);
}

function stepOnce() {
  if (hatActive) e.hat_step(hat, 1);
  else e.world_step(world, 1);
  version++;
}

function pump() {
  scheduled = false;
  if (!running) return;
  const start = performance.now();
  if (gps === null) {
    do {
      stepOnce();
    } while (performance.now() - start < SLICE_MS);
    schedule(true);
    return;
  }
  // A short backlog is kept, a long one dropped: a slow machine should run
  // slower rather than stall while catching up.
  due = Math.min(due + ((start - lastTick) / 1000) * gps, gps / 4 + 1);
  lastTick = start;
  while (due >= 1 && performance.now() - start < SLICE_MS) {
    stepOnce();
    due--;
  }
  schedule(false);
}

function stats() {
  if (hatActive) {
    return {
      generation: e.hat_generation(hat),
      population: e.hat_population(hat),
      status: STATUS_KINDS[e.hat_status_kind(hat)],
      period: e.hat_status_period(hat),
      detail: e.hat_status_detail(hat),
      since: e.hat_status_since(hat),
    };
  }
  return {
    generation: e.world_generation(world),
    population: e.world_population(world),
    status: STATUS_KINDS[e.world_status_kind(world)],
    period: e.world_status_period(world),
    detail: e.world_status_detail(world),
    since: e.world_status_since(world),
  };
}

function sendNewCells() {
  const count = e.hat_cell_count(hat);
  if (count === sentCells) return;
  const ptr = e.hat_cells(hat, sentCells);
  const cells = new Int32Array(e.memory.buffer, ptr, (count - sentCells) * FIELDS_PER_CELL).slice();
  sentCells = count;
  post({ t: 'hatCells', cells, capped: e.hat_capped(hat) !== 0 }, [cells.buffer]);
}

function hatFrame(m: { x: number; y: number; radius: number; spare: ArrayBuffer | null }) {
  if (m.radius <= MAX_LOAD_RADIUS) e.hat_load(hat, m.x, m.y, m.radius);
  sendNewCells();
  const count = e.hat_cell_count(hat);
  const key = `hat:${version}:${count}`;
  if (key === lastFrameKey) {
    post({ t: 'unchanged', spare: m.spare, ...stats() }, m.spare ? [m.spare] : []);
    return;
  }
  lastFrameKey = key;
  const width = CELL_TEXTURE_WIDTH;
  const height = Math.max(1, Math.ceil(count / width));
  const size = width * height;
  const data = m.spare?.byteLength === size ? new Uint8Array(m.spare) : new Uint8Array(size);
  data.set(new Uint8Array(e.memory.buffer, e.hat_states(hat), count));
  data.fill(0, count);
  post({ t: 'frame', data, ox: 0, oy: 0, width, height, level: 0, ...stats() }, [data.buffer]);
}

function handle(m: ToWorker) {
  switch (m.t) {
    case 'running':
      running = m.running;
      if (running) {
        due = 0;
        lastTick = performance.now();
        schedule(false);
      }
      break;
    case 'speed':
      gps = m.gps;
      break;
    case 'step':
      stepOnce();
      break;
    case 'surface':
      hatActive = m.hat;
      version++;
      if (m.torus) {
        const size = `${m.torus.width}x${m.torus.height}`;
        if (!torus) torus = e.world_new();
        if (size !== torusSize && e.world_set_torus(torus, m.torus.width, m.torus.height)) torusSize = size;
      }
      if (!m.hat) world = m.torus ? torus : plane;
      if (hatActive && !hat) {
        hat = e.hat_new();
        const outline = new Int32Array(e.memory.buffer, e.hat_outline(hat), 14 * 4).slice();
        post({ t: 'hatReady', outline, ratio: [e.hat_ratio_re(hat), e.hat_ratio_im(hat)] }, [outline.buffer]);
      }
      break;
    case 'hatEdgesOnly':
      if (hat) e.hat_set_edges_only(hat, m.edgesOnly ? 1 : 0);
      version++;
      break;
    case 'hatTable': {
      const n = m.rows.length;
      const ptr = e.hat_scratch(hat, n);
      new Uint32Array(e.memory.buffer, ptr, n).set(m.rows);
      e.hat_set_table(hat, n);
      version++;
      break;
    }
    case 'hatSeed':
      e.hat_seed_pair(hat, m.cell, m.first, m.second);
      version++;
      break;
    case 'paint': {
      const n = m.cells.length;
      const ptr = e.world_scratch(world, n);
      new Int32Array(e.memory.buffer, ptr, n).set(m.cells);
      e.world_paint(world, n / 2, m.alive ? 1 : 0);
      version++;
      break;
    }
    case 'hatPaint': {
      const n = m.cells.length;
      const ptr = e.hat_scratch(hat, n);
      new Uint32Array(e.memory.buffer, ptr, n).set(m.cells);
      e.hat_paint(hat, n, m.alive ? 1 : 0);
      version++;
      break;
    }
    case 'clear':
      if (hatActive) e.hat_clear(hat);
      else e.world_clear(world);
      version++;
      break;
    case 'topology':
      e.world_set_topology(world, m.topology);
      version++;
      break;
    case 'rule':
      if (hatActive) e.hat_set_rule(hat, m.birth, m.survive);
      else e.world_set_rule(world, m.birth, m.survive);
      version++;
      break;
    case 'soup':
      e.world_soup(world, m.x, m.y, m.width, m.height, m.density, m.seed);
      version++;
      break;
    case 'hatSoup':
      e.hat_soup(hat, m.x, m.y, m.radius, m.density, m.seed);
      version++;
      break;
    case 'hatFrame':
      if (hat) hatFrame(m);
      else post({ t: 'unchanged', spare: m.spare, ...stats() }, m.spare ? [m.spare] : []);
      break;
    case 'frame': {
      const key = `${version}:${m.ox}:${m.oy}:${m.width}:${m.height}:${m.level}`;
      if (key === lastFrameKey) {
        post({ t: 'unchanged', spare: m.spare, ...stats() }, m.spare ? [m.spare] : []);
        break;
      }
      lastFrameKey = key;
      const size = m.width * m.height;
      const ptr = e.world_frame(world, m.ox, m.oy, m.width, m.height, m.level);
      const data = m.spare?.byteLength === size ? new Uint8Array(m.spare) : new Uint8Array(size);
      data.set(new Uint8Array(e.memory.buffer, ptr, size));
      const { ox, oy, width, height, level } = m;
      post({ t: 'frame', data, ox, oy, width, height, level, ...stats() }, [data.buffer]);
      break;
    }
  }
}

const pending: ToWorker[] = [];
self.onmessage = (event: MessageEvent<ToWorker>) => {
  if (e) handle(event.data);
  else pending.push(event.data);
};

WebAssembly.instantiateStreaming(fetch(wasmUrl)).then(({ instance }) => {
  e = instance.exports as unknown as Exports;
  plane = e.world_new();
  world = plane;
  pending.splice(0).forEach(handle);
  post({ t: 'ready' });
});
