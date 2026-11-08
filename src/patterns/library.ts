import data from './patterns.json';
import { parseRle, type Shape } from './rle';

export interface Pattern {
  id: string;
  name: string;
  category: string;
  note: string;
  shape: Shape;
}

interface Expectation {
  kind: string;
  period?: number;
  dx?: number;
  dy?: number;
  delta?: number;
  population?: number;
}

function gcd(a: number, b: number): number {
  return b === 0 ? a : gcd(b, a % b);
}

function speed(cells: number, period: number) {
  const divisor = gcd(cells, period);
  const top = cells / divisor;
  return `${top === 1 ? '' : top}c/${period / divisor}`;
}

function noteFor(expect: Expectation) {
  switch (expect.kind) {
    case 'oscillator':
      return `period ${expect.period}`;
    case 'spaceship':
      return `${speed(Math.max(Math.abs(expect.dx!), Math.abs(expect.dy!)), expect.period!)}, period ${expect.period}`;
    case 'growth':
      return `+${expect.delta} cells every ${expect.period}`;
    case 'methuselah':
      return expect.population ? `settles at ${expect.population} cells` : 'dies out';
    default:
      return '';
  }
}

export const PATTERNS: Pattern[] = data.map(({ id, name, category, rle, expect }) => ({
  id,
  name,
  category,
  note: noteFor(expect),
  shape: parseRle(rle),
}));

export const CATEGORIES = [...new Set(PATTERNS.map((p) => p.category))];
