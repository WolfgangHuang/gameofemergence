/**
 * The state of a field as a URL fragment. There is no server, so a link has to
 * carry everything itself. Fragments never leave the browser, and whatever comes
 * in through one is untrusted, so everything is checked on the way in.
 *
 * Example: #v=1&p=Square&r=B3/S23&c=0,0,14&a=10,20,bo-2bo-3o
 *
 * Cells of the hat tiling go by their place in it, five numbers each, in `h`, as
 * their ids depend on the order in which the field was generated:
 * #v=1&f=hat&m=60&r=R2B5-7S3-8&c=0,0,14&h=1,2,3,4,5,...
 */
import { parseRle, RleError, toRle } from '../patterns/rle.ts';
import { parseRangeRule, parseRule, rangeRuleName, ruleName, type RangeRule, type Rule } from '../patterns/rule.ts';
import { HAT_PATTERNS } from '../patterns/hatPatterns.ts';
import type { TorusSize } from '../sim/protocol.ts';
import { FAMILIES, fullOutline, HAT_NEIGHBOURHOODS, isValidOutline, isValidTorusSide, neighbourhoodsOf, PENROSE_NEIGHBOURHOODS, type Family, type Vec } from '../tiles/tiles.ts';

const VERSION = '1';
/** Browsers take far more, but nothing sensible can be done with such a link. */
export const MAX_LINK_LENGTH = 32_000;
/** Above this, some messengers and mail programs start to cut links off. */
export const LONG_LINK_LENGTH = 2_000;
/** A field with more cells is not even tried. */
export const MAX_SHARED_CELLS = 500_000;
const MAX_DECOMPRESSED = 16 * 1024 * 1024;
const COMPRESS_FROM = 200;
const COORD_LIMIT = 1e9;
const MAX_SOUP_SIDE = 4096;
const MAX_HAT_SOUP_RADIUS = 250;
export const HAT_KEY_FIELDS = 5;
/** Far more than fits into a link of ours, so whatever exceeds this is not one. */
const MAX_HAT_LINK_CELLS = 10_000;

/** How a random field came about. It reproduces the field exactly, in far fewer characters. */
export type Recipe =
  | { kind: 'grid'; x: number; y: number; width: number; height: number; density: number; seed: number }
  | { kind: 'hat'; x: number; y: number; radius: number; density: number; seed: number };

export interface Camera {
  x: number;
  y: number;
  /** CSS pixels per world unit, as device pixels differ between screens. */
  size: number;
}

export interface SharedTile {
  family: Family | 'hat' | 'penrose';
  preset: string | null;
  outline: Vec[];
  neighbourhood: number;
  morph: number;
}

export interface Settings {
  tile: SharedTile;
  rule: Rule;
  /** Id of a hat pattern whose table replaces the rule. */
  table: string | null;
  /** A rule on a larger neighbourhood of the hat tiling, which replaces the rule. */
  range?: RangeRule | null;
  torus: TorusSize | null;
  camera: Camera;
}

export interface CodedCells {
  compressed: boolean;
  x: number;
  y: number;
  data: string;
}

export interface ParsedLink {
  /** Absent settings stay at their defaults. */
  tile: SharedTile | null;
  rule: Rule | null;
  table: string | null;
  range: RangeRule | null;
  torus: TorusSize | null;
  camera: Camera | null;
  recipe: Recipe | null;
  cells: CodedCells | null;
  /** Places in the hat tiling, five numbers each. */
  hatCells: Int32Array<ArrayBuffer> | null;
  /** What had to be ignored, for showing to the user. */
  problems: string[];
}

export type LinkContent =
  /** Reproduces the field as it is now. */
  | 'exact'
  /** Reproduces the random field this run started from. */
  | 'start'
  /** Settings only, because the field is empty. */
  | 'empty'
  /** Settings only, because there are too many cells. */
  | 'too-many'
  /** Settings only, because cells of a four-state table have no form in a link yet. */
  | 'hat-cells';

const round = (value: number, digits: number) => Number(value.toFixed(digits));

function settingsParts({ tile, rule, table, range, torus, camera }: Settings) {
  const parts = [`v=${VERSION}`];
  if (tile.family !== 'quad') parts.push(`f=${tile.family}`);
  if (tile.family === 'hat') parts.push(`m=${tile.morph}`);
  else if (tile.family === 'penrose') {
    // One shape, nothing more to say.
  } else if (tile.preset !== null) parts.push(`p=${encodeURIComponent(tile.preset)}`);
  else parts.push(`o=${tile.outline.flat().map((v) => round(v, 4)).join(',')}`);
  if (tile.neighbourhood !== 0) parts.push(`n=${tile.neighbourhood}`);
  parts.push(table ? `k=${encodeURIComponent(table)}` : `r=${range ? rangeRuleName(range) : ruleName(rule)}`);
  if (torus) parts.push(`t=${torus.width}x${torus.height}`);
  parts.push(`c=${round(camera.x, 2)},${round(camera.y, 2)},${round(camera.size, 3)}`);
  return parts;
}

function recipePart(recipe: Recipe) {
  const place = recipe.kind === 'grid' ? [recipe.x, recipe.y, recipe.width, recipe.height] : [recipe.x, recipe.y, recipe.radius];
  return `s=${[...place, recipe.density, recipe.seed].join(',')}`;
}

function toBase64Url(bytes: Uint8Array) {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
}

function fromBase64Url(text: string) {
  const binary = atob(text.replaceAll('-', '+').replaceAll('_', '/'));
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}

async function deflate(text: string) {
  const stream = new Blob([text]).stream().pipeThrough(new CompressionStream('deflate-raw'));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

/** Stops early, as a few bytes can unpack into gigabytes. */
async function inflate(bytes: Uint8Array<ArrayBuffer>) {
  const reader = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('deflate-raw')).getReader();
  const decoder = new TextDecoder();
  let text = '';
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return text + decoder.decode();
    total += value.length;
    if (total > MAX_DECOMPRESSED) {
      await reader.cancel();
      throw new RleError('too large');
    }
    text += decoder.decode(value, { stream: true });
  }
}

/** Plain or compressed, whichever is shorter. Null if the cells are too far apart. */
async function cellsPart(cells: ArrayLike<number>) {
  const rle = toRle(cells);
  if (!rle) return null;
  // `$` and `!` are safe in a fragment, but link detectors tend to stop at them.
  const plain = `a=${rle.x},${rle.y},${rle.body.replaceAll('$', '-')}`;
  if (rle.body.length < COMPRESS_FROM) return plain;
  const packed = `z=${rle.x},${rle.y},${toBase64Url(await deflate(rle.body))}`;
  return packed.length < plain.length ? packed : plain;
}

export async function buildLink(input: {
  settings: Settings;
  recipe: Recipe | null;
  generation: number;
  population: number;
  /** Flat x, y pairs, or null where they are not available. */
  cells: ArrayLike<number> | null;
  /** Places in the hat tiling, five numbers each, or null where they are not available. */
  hatCells?: ArrayLike<number> | null;
}): Promise<{ hash: string; content: LinkContent }> {
  const { settings, recipe, generation, population, cells, hatCells } = input;
  const base = settingsParts(settings);
  const hash = (...extra: string[]) => `#${[...base, ...extra].join('&')}`;

  if (recipe && generation === 0) return { hash: hash(recipePart(recipe)), content: 'exact' };
  if (population === 0) return { hash: hash(), content: 'empty' };
  if (cells && cells.length > 0) {
    const part = await cellsPart(cells);
    if (part && hash(part).length <= MAX_LINK_LENGTH) return { hash: hash(part), content: 'exact' };
  }
  if (hatCells && hatCells.length > 0) {
    const part = `h=${Array.from(hatCells).join(',')}`;
    if (hash(part).length <= MAX_LINK_LENGTH) return { hash: hash(part), content: 'exact' };
    if (!recipe) return { hash: hash(), content: 'too-many' };
  }
  if (recipe) return { hash: hash(recipePart(recipe)), content: 'start' };
  return { hash: hash(), content: settings.tile.family === 'hat' ? 'hat-cells' : 'too-many' };
}

function numbers(text: string, count: number) {
  const values = text.split(',').map(Number);
  return values.length === count && values.every(Number.isFinite) && !text.split(',').includes('') ? values : null;
}

function parseTile(params: Map<string, string>, problems: string[]): SharedTile | null {
  const name = params.get('f') ?? 'quad';
  if (name === 'hat') {
    const morph = Number(params.get('m') ?? 60);
    const neighbourhood = Number(params.get('n') ?? 0);
    const validN = Number.isInteger(neighbourhood) && neighbourhood >= 0 && neighbourhood < HAT_NEIGHBOURHOODS.length;
    if (!validN) problems.push('neighbourhood');
    return {
      family: 'hat',
      preset: 'Hat',
      outline: FAMILIES.quad.presets[0].outline,
      neighbourhood: validN ? neighbourhood : 0,
      morph: Number.isFinite(morph) ? Math.min(84, Math.max(6, Math.round(morph))) : 60,
    };
  }
  if (name === 'penrose') {
    const neighbourhood = Number(params.get('n') ?? 0);
    const validN = Number.isInteger(neighbourhood) && neighbourhood >= 0 && neighbourhood < PENROSE_NEIGHBOURHOODS.length;
    if (!validN) problems.push('neighbourhood');
    return { family: 'penrose', preset: 'Rhombs', outline: FAMILIES.quad.presets[0].outline, neighbourhood: validN ? neighbourhood : 0, morph: 60 };
  }
  if (!Object.hasOwn(FAMILIES, name)) {
    problems.push('tile');
    return null;
  }
  const family = name as Family;
  const info = FAMILIES[family];
  // Pentagons have neighbourhoods per type, so this needs the preset first.
  const preset = info.presets.find((p) => p.name === params.get('p'));
  if (params.has('p') && !preset) problems.push('tile preset');
  const chosen = preset ?? info.presets[0];
  const neighbourhood = Number(params.get('n') ?? 0);
  const validN = Number.isInteger(neighbourhood) && neighbourhood >= 0 && neighbourhood < neighbourhoodsOf(family, chosen.name).length;
  if (!validN) problems.push('neighbourhood');
  const tile = { family, morph: 60, neighbourhood: validN ? neighbourhood : 0 };

  const outline = params.get('o');
  if (outline !== undefined) {
    const corners = info.presets[0].outline.length;
    const values = numbers(outline, corners * 2);
    const vertices = values && values.map((_, i) => (i % 2 === 0 ? ([values[i], values[i + 1]] as Vec) : null)).filter((v) => v !== null);
    if (vertices && !info.fixed && isValidOutline(fullOutline(family, vertices))) return { ...tile, preset: null, outline: vertices };
    problems.push('tile shape');
  }
  return { ...tile, preset: chosen.name, outline: chosen.outline };
}

function neighbourCount(tile: SharedTile) {
  if (tile.family === 'penrose') return PENROSE_NEIGHBOURHOODS[tile.neighbourhood].count;
  return tile.family === 'hat' ? HAT_NEIGHBOURHOODS[tile.neighbourhood].count : neighbourhoodsOf(tile.family, tile.preset)[tile.neighbourhood].count;
}

function parseRecipe(text: string, hat: boolean, problems: string[]): Recipe | null {
  if (hat) {
    const values = numbers(text, 5);
    if (values) {
      const [x, y, radius, density, seed] = values;
      if (Math.abs(x) <= COORD_LIMIT && Math.abs(y) <= COORD_LIMIT && radius > 0 && radius <= MAX_HAT_SOUP_RADIUS && density >= 0 && density <= 1 && Number.isInteger(seed) && seed >= 0 && seed < 2 ** 32) {
        return { kind: 'hat', x, y, radius, density, seed };
      }
    }
  } else {
    const values = numbers(text, 6);
    if (values) {
      const [x, y, width, height, density, seed] = values;
      const sideOk = (n: number) => Number.isInteger(n) && n > 0 && n <= MAX_SOUP_SIDE;
      if (Number.isInteger(x) && Number.isInteger(y) && Math.abs(x) <= COORD_LIMIT && Math.abs(y) <= COORD_LIMIT && sideOk(width) && sideOk(height) && density >= 0 && density <= 1 && Number.isInteger(seed) && seed >= 0 && seed < 2 ** 32) {
        return { kind: 'grid', x, y, width, height, density, seed };
      }
    }
  }
  problems.push('random field');
  return null;
}

/** Keeps a list to what can be a place in the tiling. The engine checks that a hat is there. */
/** Orientations that exist: rotation + 8 * flip on the hat, turn + 16 * thick on rhombs. */
const validCode = (penrose: boolean, v: number) => (penrose ? (v >= 0 && v < 10) || (v >= 16 && v < 26) : (v >= 0 && v < 6) || (v >= 8 && v < 14));

function parseHatCells(text: string, problems: string[], penrose: boolean) {
  const parts = text.split(',');
  const values = parts.map(Number);
  const valid =
    text !== '' &&
    parts.length % HAT_KEY_FIELDS === 0 &&
    parts.length <= MAX_HAT_LINK_CELLS * HAT_KEY_FIELDS &&
    !parts.includes('') &&
    values.every((v, i) => Number.isInteger(v) && (i % HAT_KEY_FIELDS === 0 ? validCode(penrose, v) : Math.abs(v) <= COORD_LIMIT));
  if (!valid) {
    problems.push('cells');
    return null;
  }
  return Int32Array.from(values);
}

function parseCoded(text: string, compressed: boolean, problems: string[]): CodedCells | null {
  const [x, y, ...rest] = text.split(',');
  const data = rest.join(',');
  const place = numbers(`${x},${y}`, 2);
  if (!place || !place.every((v) => Number.isInteger(v) && Math.abs(v) <= COORD_LIMIT) || data === '' || data.length > MAX_LINK_LENGTH * 2) {
    problems.push('cells');
    return null;
  }
  return { compressed, x: place[0], y: place[1], data };
}

/** Returns null if this fragment is not one of our links at all. */
export function parseLink(hash: string): ParsedLink | null {
  const params = new Map<string, string>();
  for (const part of hash.replace(/^#/, '').split('&')) {
    const at = part.indexOf('=');
    if (at <= 0) continue;
    try {
      params.set(part.slice(0, at), decodeURIComponent(part.slice(at + 1)));
    } catch {
      params.set(part.slice(0, at), '');
    }
  }
  if (!params.has('v')) return null;

  const problems: string[] = [];
  const parsed: ParsedLink = { tile: null, rule: null, table: null, range: null, torus: null, camera: null, recipe: null, cells: null, hatCells: null, problems };
  if (params.get('v') !== VERSION) {
    problems.push('version');
    return parsed;
  }

  parsed.tile = parseTile(params, problems);
  const tile: SharedTile = parsed.tile ?? { family: 'quad', preset: 'Square', outline: FAMILIES.quad.presets[0].outline, neighbourhood: 0, morph: 60 };
  const isHat = tile.family === 'hat';
  const isGraph = isHat || tile.family === 'penrose';

  const torus = params.get('t');
  if (torus !== undefined) {
    const match = /^(\d+)x(\d+)$/.exec(torus);
    const size = match && { width: Number(match[1]), height: Number(match[2]) };
    if (size && isValidTorusSide(size.width) && isValidTorusSide(size.height) && !isGraph) parsed.torus = size;
    else problems.push('torus size');
  }

  const rule = params.get('r');
  if (rule !== undefined) {
    if (/^B[0-9a-f]*\/S[0-9a-f]*$/.test(rule)) {
      const { birth, survive } = parseRule(rule);
      const all = (1 << (neighbourCount(tile) + 1)) - 1;
      // Birth from nothing needs a finite field.
      const allowed = parsed.torus ? all : all & ~1;
      parsed.rule = { birth: birth & allowed, survive: survive & all };
    } else {
      // A larger neighbourhood counts over edges and corners, and only the hat has it.
      const range = isHat && tile.neighbourhood === 0 ? parseRangeRule(rule) : null;
      if (range) parsed.range = range;
      else problems.push('rule');
    }
  }

  const table = params.get('k');
  if (table !== undefined) {
    if (isHat && tile.neighbourhood === 0 && HAT_PATTERNS.some((p) => p.id === table)) parsed.table = table;
    else problems.push('pattern');
  }

  const camera = params.get('c');
  if (camera !== undefined) {
    const values = numbers(camera, 3);
    if (values && Math.abs(values[0]) <= COORD_LIMIT && Math.abs(values[1]) <= COORD_LIMIT && values[2] > 0) parsed.camera = { x: values[0], y: values[1], size: values[2] };
    else problems.push('view');
  }

  const recipe = params.get('s');
  if (recipe !== undefined) parsed.recipe = parseRecipe(recipe, isGraph, problems);
  for (const [key, compressed] of [['a', false], ['z', true]] as const) {
    const cells = params.get(key);
    if (cells === undefined) continue;
    if (isGraph) problems.push('cells');
    else parsed.cells = parseCoded(cells, compressed, problems);
  }
  const hatCells = params.get('h');
  if (hatCells !== undefined) {
    if (isGraph) parsed.hatCells = parseHatCells(hatCells, problems, !isHat);
    else problems.push('cells');
  }
  return parsed;
}

/**
 * The fragment in what somebody wants to open: a link, a bare fragment or a find
 * file of the search tool. Null if it holds none of ours.
 */
export function linkFromImport(text: string): string | null {
  let candidate = text.trim();
  if (candidate.startsWith('{')) {
    try {
      const file: unknown = JSON.parse(candidate);
      const link = typeof file === 'object' && file !== null ? (file as { link?: unknown }).link : null;
      if (typeof link !== 'string') return null;
      candidate = link;
    } catch {
      return null;
    }
  }
  const at = candidate.indexOf('#');
  if (at < 0) return null;
  const hash = candidate.slice(at);
  return hash.length <= MAX_LINK_LENGTH * 2 && parseLink(hash) ? hash : null;
}

/** Flat x, y pairs in field coordinates. Throws `RleError` on anything malformed. */
export async function decodeCells(coded: CodedCells) {
  let body: string;
  if (coded.compressed) {
    try {
      body = await inflate(fromBase64Url(coded.data));
    } catch (error) {
      throw error instanceof RleError ? error : new RleError('damaged');
    }
  } else {
    body = coded.data.replaceAll('-', '$');
  }
  const { cells } = parseRle(body);
  return Int32Array.from(cells, (v, i) => v + (i % 2 === 0 ? coded.x : coded.y));
}
