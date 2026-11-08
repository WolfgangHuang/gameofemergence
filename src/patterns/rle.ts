import { parseRule, ruleName, type Rule } from './rule.ts';

/** Cells as flat x, y pairs, normalised so that the bounding box starts at 0, 0. */
export interface Shape {
  cells: number[];
  width: number;
  height: number;
}

/** Text from outside can claim anything, so what it unpacks into is limited. */
export const MAX_RLE_CELLS = 2_000_000;
const MAX_RLE_EXTENT = 1 << 24;
const LINE_LENGTH = 70;

export class RleError extends Error {}

/** Parses the body of an RLE pattern, as used by Golly and LifeWiki. */
export function parseRle(rle: string): Shape {
  const cells: number[] = [];
  let x = 0;
  let y = 0;
  let count = '';
  for (const char of rle) {
    if (char >= '0' && char <= '9') {
      count += char;
      continue;
    }
    if (char === ' ' || char === '\n' || char === '\r' || char === '\t') continue;
    if (char === '!') break;
    const run = count === '' ? 1 : Number(count);
    count = '';
    if (char === 'o') {
      if (cells.length / 2 + run > MAX_RLE_CELLS) throw new RleError('too many cells');
      if (x + run > MAX_RLE_EXTENT) throw new RleError('too large');
      for (let i = 0; i < run; i++) cells.push(x + i, y);
      x += run;
    } else if (char === 'b') {
      x += run;
    } else if (char === '$') {
      y += run;
      x = 0;
      if (y > MAX_RLE_EXTENT) throw new RleError('too large');
    } else {
      throw new RleError(`unexpected "${char}"`);
    }
  }
  return normalise(cells);
}

/** The body of an RLE pattern without the closing `!`, and where its bounding box starts. */
export function toRle(cells: ArrayLike<number>) {
  const count = cells.length / 2;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (let i = 0; i < count; i++) {
    minX = Math.min(minX, cells[2 * i]);
    maxX = Math.max(maxX, cells[2 * i]);
    minY = Math.min(minY, cells[2 * i + 1]);
    maxY = Math.max(maxY, cells[2 * i + 1]);
  }
  if (count === 0 || maxX - minX >= MAX_RLE_EXTENT || maxY - minY >= MAX_RLE_EXTENT) return null;

  const order = Uint32Array.from({ length: count }, (_, i) => i).sort((a, b) => cells[2 * a + 1] - cells[2 * b + 1] || cells[2 * a] - cells[2 * b]);
  const token = (run: number, char: string) => (run === 1 ? char : `${run}${char}`);
  const parts: string[] = [];
  let row = minY;
  let column = minX;
  for (let i = 0; i < count; ) {
    const start = order[i];
    const y = cells[2 * start + 1];
    const x = cells[2 * start];
    if (y !== row) {
      parts.push(token(y - row, '$'));
      row = y;
      column = minX;
    }
    if (x !== column) parts.push(token(x - column, 'b'));
    let run = 1;
    while (i + run < count && cells[2 * order[i + run] + 1] === y && cells[2 * order[i + run]] === x + run) run++;
    parts.push(token(run, 'o'));
    column = x + run;
    i += run;
  }
  return { x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1, body: parts.join('') };
}

function parseHeaderRule(text: string): Rule {
  // A suffix like `:T100,100` describes a bounded grid, which is not part of the rule.
  const name = text.split(':')[0];
  const standard = /^B([0-8]*)\/S([0-8]*)$/i.exec(name);
  const old = /^([0-8]*)\/([0-8]*)$/.exec(name);
  if (!standard && !old) throw new RleError(`the rule ${name} is not supported`);
  const [born, survives] = standard ? [standard[1], standard[2]] : [old![2], old![1]];
  return parseRule(`B${born}/S${survives}`);
}

/** Parses a complete pattern file, with comments and header being optional. */
export function parseRleFile(text: string): { shape: Shape; rule: Rule | null } {
  let rule: Rule | null = null;
  const body: string[] = [];
  for (const raw of text.split('\n')) {
    const line = raw.trim();
    if (line === '' || line.startsWith('#')) continue;
    if (/^x\s*=/.test(line)) {
      const named = /rule\s*=\s*([^\s,]+(?:,\d+)?)/.exec(line);
      if (named) rule = parseHeaderRule(named[1]);
      continue;
    }
    body.push(line);
  }
  const shape = parseRle(body.join(''));
  if (shape.cells.length === 0) throw new RleError('no live cells');
  return { shape, rule };
}

/** A complete pattern file. The cells of a torus keep its size in the rule, as Golly writes it. */
export function formatRleFile(cells: ArrayLike<number>, rule: Rule, torus: { width: number; height: number } | null) {
  const rle = toRle(cells);
  if (!rle) return null;
  const suffix = torus ? `:T${torus.width},${torus.height}` : '';
  const lines = [`x = ${rle.width}, y = ${rle.height}, rule = ${ruleName(rule)}${suffix}`];
  let line = '';
  for (const token of rle.body.match(/\d*[bo$]/g) ?? []) {
    if (line.length + token.length > LINE_LENGTH) {
      lines.push(line);
      line = '';
    }
    line += token;
  }
  lines.push(`${line}!`);
  return lines.join('\n');
}

function normalise(cells: number[]): Shape {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (let i = 0; i < cells.length; i += 2) {
    minX = Math.min(minX, cells[i]);
    maxX = Math.max(maxX, cells[i]);
    minY = Math.min(minY, cells[i + 1]);
    maxY = Math.max(maxY, cells[i + 1]);
  }
  return {
    cells: cells.map((v, i) => v - (i % 2 === 0 ? minX : minY)),
    width: maxX - minX + 1,
    height: maxY - minY + 1,
  };
}

export function rotate(shape: Shape): Shape {
  const cells: number[] = [];
  for (let i = 0; i < shape.cells.length; i += 2) cells.push(-shape.cells[i + 1], shape.cells[i]);
  return normalise(cells);
}

export function flip(shape: Shape): Shape {
  const cells: number[] = [];
  for (let i = 0; i < shape.cells.length; i += 2) cells.push(-shape.cells[i], shape.cells[i + 1]);
  return normalise(cells);
}
