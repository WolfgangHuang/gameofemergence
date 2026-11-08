/** Cells as flat x, y pairs, normalised so that the bounding box starts at 0, 0. */
export interface Shape {
  cells: number[];
  width: number;
  height: number;
}

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
    const run = count === '' ? 1 : Number(count);
    count = '';
    if (char === 'o') {
      for (let i = 0; i < run; i++) cells.push(x + i, y);
      x += run;
    } else if (char === 'b') {
      x += run;
    } else if (char === '$') {
      y += run;
      x = 0;
    } else if (char === '!') {
      break;
    }
  }
  return normalise(cells);
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
