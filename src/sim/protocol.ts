export interface TorusSize {
  width: number;
  height: number;
}

/** A window of the field, rasterised at one texel per `2^level` cells per axis. */
export interface FrameWindow {
  ox: number;
  oy: number;
  width: number;
  height: number;
  level: number;
}

export type ToWorker =
  | { t: 'running'; running: boolean }
  /** Generations per second, `null` meaning as fast as possible. */
  | { t: 'speed'; gps: number | null }
  | { t: 'step' }
  /** Flat list of x, y pairs. */
  | { t: 'paint'; cells: Int32Array; alive: boolean }
  | { t: 'clear' }
  /**
   * Which field the commands below apply to. Each keeps its cells while inactive.
   * `torus` is null on the plane. A new size keeps the cells that still fit.
   */
  | { t: 'surface'; hat: boolean; torus: TorusSize | null }
  | { t: 'hatEdgesOnly'; edgesOnly: boolean }
  /** A priority table in the layout of `set_table_from_scratch`. */
  | { t: 'hatTable'; rows: Uint32Array }
  /** Starts a pattern on a cell and one of its edge neighbours. */
  | { t: 'hatSeed'; cell: number; first: number; second: number }
  /** Cell ids of the hat tiling. */
  | { t: 'hatPaint'; cells: Uint32Array; alive: boolean }
  /** Places in the hat tiling, five numbers each, which unlike ids hold in any session. */
  | { t: 'hatPaintKeys'; cells: Int32Array; alive: boolean }
  /** All cells within `radius` rings count as neighbours. */
  | { t: 'hatRange'; radius: number; birth: [number, number]; survive: [number, number] }
  /** Positions in lattice units. */
  | { t: 'hatSoup'; x: number; y: number; radius: number; density: number; seed: number }
  | { t: 'hatFrame'; x: number; y: number; radius: number; spare: ArrayBuffer | null }
  /** See `TOPOLOGY` in tiles.ts. */
  | { t: 'topology'; topology: number }
  /** One bit per neighbour count. */
  | { t: 'rule'; birth: number; survive: number }
  | { t: 'soup'; x: number; y: number; width: number; height: number; density: number; seed: number }
  | ({ t: 'frame'; spare: ArrayBuffer | null } & FrameWindow)
  /** Asks for all live cells of the grid field, unless there are more than `max`. */
  | { t: 'exportCells'; id: number; max: number };

export type StatusKind = 'active' | 'empty' | 'still' | 'oscillating' | 'drifting' | 'growing';

export interface Stats {
  generation: number;
  population: number;
  status: StatusKind;
  /** Period of the oscillation, drift or growth. */
  period: number;
  /** Moving objects when drifting, population gain per period when growing. */
  detail: number;
  /** Generation at which the status was recognised. */
  since: number;
}

export type FromWorker =
  | { t: 'ready' }
  | { t: 'hatReady'; outline: Int32Array; ratio: [number, number] }
  /** Newly generated cells, five numbers each. Always sent before their first frame. */
  | { t: 'hatCells'; cells: Int32Array; capped: boolean }
  | ({ t: 'frame'; data: Uint8Array<ArrayBuffer> } & FrameWindow & Stats)
  /** Flat x, y pairs, or null on the hat tiling and above the requested maximum. There, `hatCells` has five numbers per cell. */
  | { t: 'cells'; id: number; cells: Int32Array | null; hatCells: Int32Array | null; generation: number; population: number }
  /** How many cells of a list the tiling does not have. */
  | { t: 'hatMissing'; count: number }
  /** Nothing changed since the last frame for this window. Returns the spare buffer. */
  | ({ t: 'unchanged'; spare: ArrayBuffer | null } & Stats);
