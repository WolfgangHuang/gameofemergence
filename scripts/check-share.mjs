// Checks links and RLE without a browser: everything has to survive the round
// trip, and whatever comes in from outside must be rejected rather than obeyed.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { formatRleFile, parseRle, parseRleFile, RleError, toRle } from '../src/patterns/rle.ts';
import { buildLink, decodeCells, linkFromImport, MAX_LINK_LENGTH, parseLink } from '../src/share/link.ts';
import { FAMILIES } from '../src/tiles/tiles.ts';

const root = resolve(import.meta.dirname, '..');
const patterns = JSON.parse(readFileSync(resolve(root, 'src/patterns/patterns.json'), 'utf8'));
let failures = 0;
const report = (ok, message) => {
  if (!ok) failures++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${message}`);
};
// Independent of the order of keys.
const canonical = (v) => (Array.isArray(v) ? v.map(canonical) : v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, canonical(v[k])])) : v);
const same = (a, b) => JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
const sorted = (cells) => {
  const pairs = [];
  for (let i = 0; i < cells.length; i += 2) pairs.push([cells[i], cells[i + 1]]);
  return pairs.sort((p, q) => p[1] - q[1] || p[0] - q[0]);
};
const throwsRle = (f) => {
  try {
    f();
    return false;
  } catch (error) {
    return error instanceof RleError;
  }
};

// RLE: the library's patterns survive writing and reading.
let wrong = 0;
for (const { rle } of patterns) {
  const { cells } = parseRle(rle);
  const written = toRle(cells);
  if (!same(sorted(parseRle(written.body).cells), sorted(cells))) wrong++;
  const file = parseRleFile(formatRleFile(cells, { birth: 8, survive: 12 }, null));
  if (!same(sorted(file.shape.cells), sorted(cells)) || file.rule.birth !== 8 || file.rule.survive !== 12) wrong++;
}
report(wrong === 0, `RLE round trip of ${patterns.length} patterns`);

const golly = `#N Glider
#O Someone with a b and an o in the name
#C another comment
x = 3, y = 3, rule = B36/S23
bo$2bo
$3o!`;
const parsed = parseRleFile(golly);
report(same(sorted(parsed.shape.cells), sorted([1, 0, 2, 1, 0, 2, 1, 2, 2, 2])) && parsed.rule.birth === 72, 'RLE file with comments, header and line breaks');
report(parseRleFile('x = 3, y = 1\n3o!').rule === null, 'RLE without a rule leaves the rule alone');
report(parseRleFile('x = 3, y = 1, rule = 23/3\n3o!').rule.birth === 8, 'RLE with the old S/B rule form');
report(parseRleFile('x = 3, y = 1, rule = B3/S23:T64,64\n3o!').rule.survive === 12, 'RLE with a bounded grid suffix');
report(toRle([5, 5, 7, 5, 5, 9]).body === 'obo4$o', 'RLE writes gaps and rows');

for (const [name, text] of [
  ['several states', 'x = 2, y = 1, rule = Generations\nA.B!'],
  ['a huge run', '99999999999o!'],
  ['a huge gap', '999999999$o!'],
  ['too many cells', '3000000o!'],
  ['no cells', 'x = 1, y = 1\n3b!'],
  ['a non-totalistic rule', 'x = 1, y = 1, rule = B2a/S12\no!'],
]) {
  report(throwsRle(() => parseRleFile(text)), `RLE refuses ${name}`);
}

// Links.
const quad = FAMILIES.quad.presets[0];
const settings = {
  tile: { family: 'quad', preset: 'Square', outline: quad.outline, neighbourhood: 0, morph: 60 },
  rule: { birth: 8, survive: 12 },
  table: null,
  torus: null,
  camera: { x: 12.345, y: -7, size: 14 },
};
const glider = [101, 50, 102, 51, 100, 52, 101, 52, 102, 52];
{
  const { hash, content } = await buildLink({ settings, recipe: null, generation: 7, population: 5, cells: glider });
  const link = parseLink(hash);
  report(content === 'exact' && link.problems.length === 0, `link with a glider: ${hash}`);
  report(same(link.rule, settings.rule) && link.tile.preset === 'Square' && link.camera.x === 12.35, 'link keeps rule, tile and view');
  report(same(sorted(await decodeCells(link.cells)), sorted(glider)), 'link keeps where the cells are');
  report(hash.length < 100, 'a glider makes a short link');
}
for (const family of ['quad', 'triangle', 'hexagon', 'pentagon']) {
  for (const preset of FAMILIES[family].presets) {
    for (let neighbourhood = 0; neighbourhood < (preset.neighbourhoods ?? FAMILIES[family].neighbourhoods).length; neighbourhood++) {
      const tile = { family, preset: preset.name, outline: preset.outline, neighbourhood, morph: 60 };
      const { hash } = await buildLink({ settings: { ...settings, tile }, recipe: null, generation: 0, population: 0, cells: [] });
      const back = parseLink(hash);
      if (!same(back.tile, tile) || back.problems.length) report(false, `tile ${family}/${preset.name} did not survive: ${hash}`);
    }
  }
}
console.log('ok   checked all presets in a link');
{
  const outline = [[0, 0], [1.25, 0.1], [1, 1.5], [0.2, 1]];
  const tile = { family: 'quad', preset: null, outline, neighbourhood: 1, morph: 60 };
  const { hash } = await buildLink({ settings: { ...settings, tile }, recipe: null, generation: 0, population: 0, cells: [] });
  report(same(parseLink(hash).tile.outline, outline) && parseLink(hash).tile.neighbourhood === 1, 'a deformed tile survives a link');
}
{
  // Penrose has one shape, and its own codes for what a cell's orientation is.
  const penrose = { family: 'penrose', preset: 'Rhombs', outline: quad.outline, neighbourhood: 1, morph: 60 };
  const cells = [16, 3, -1, 0, 2, 9, 1, 1, 0, 0];
  const { hash } = await buildLink({ settings: { ...settings, tile: penrose }, recipe: null, generation: 5, population: 2, cells: null, hatCells: cells });
  const back = parseLink(hash);
  report(same(back.tile, penrose) && back.problems.length === 0, `a Penrose link keeps its tile: ${hash}`);
  report(same(Array.from(back.hatCells), cells), 'a Penrose link keeps its cells');
  report(parseLink('#v=1&f=penrose&h=10,0,0,0,0').problems.includes('cells'), 'a rhomb orientation that does not exist is refused');
  report(parseLink('#v=1&f=hat&m=60&h=16,0,0,0,0').problems.includes('cells'), 'the hat does not take the codes of rhombs');
  report(parseLink('#v=1&f=penrose&n=2').problems.includes('neighbourhood'), 'Penrose has two neighbourhoods');
  report(parseLink('#v=1&f=penrose&t=64x64').problems.includes('torus size'), 'Penrose cannot be a torus');
}
{
  const recipe = { kind: 'grid', x: -30, y: 4, width: 200, height: 100, density: 0.3, seed: 4000000000 };
  const fresh = await buildLink({ settings, recipe, generation: 0, population: 6000, cells: null });
  report(fresh.content === 'exact' && same(parseLink(fresh.hash).recipe, recipe), 'a fresh random field goes by its recipe');
  const ran = await buildLink({ settings, recipe, generation: 50, population: 40, cells: [1, 1, 2, 2] });
  report(ran.content === 'exact' && parseLink(ran.hash).recipe === null && parseLink(ran.hash).cells, 'after running, cells are preferred if they fit');
  const big = await buildLink({ settings, recipe, generation: 50, population: 1e7, cells: null });
  report(big.content === 'start' && same(parseLink(big.hash).recipe, recipe), 'too many cells fall back to the recipe');
  const none = await buildLink({ settings, recipe: null, generation: 50, population: 1e7, cells: null });
  report(none.content === 'too-many' && parseLink(none.hash).cells === null, 'too many cells without a recipe keep the settings');
}
{
  const size = 128;
  const cells = [];
  let s = 12345;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      s = (s * 1664525 + 1013904223) >>> 0;
      if (s / 2 ** 32 < 0.3) cells.push(x, y);
    }
  }
  const torus = { width: size, height: size };
  const { hash, content } = await buildLink({ settings: { ...settings, torus }, recipe: null, generation: 5, population: cells.length / 2, cells });
  const link = parseLink(hash);
  report(content === 'exact' && link.cells.compressed && same(link.torus, torus), `random 128x128 is compressed, ${hash.length} characters`);
  report(same(sorted(await decodeCells(link.cells)), sorted(cells)), 'compressed cells survive');
  report(hash.length < 4000, 'that stays below 4,000 characters');

  const huge = [];
  for (let y = 0; y < 1024; y++) for (let x = 0; x < 1024; x++) { s = (s * 1664525 + 1013904223) >>> 0; if (s / 2 ** 32 < 0.3) huge.push(x, y); }
  const tooLong = await buildLink({ settings, recipe: null, generation: 5, population: huge.length / 2, cells: huge });
  report(tooLong.content === 'too-many' && tooLong.hash.length <= MAX_LINK_LENGTH, 'random 1024x1024 does not go into a link');
}
{
  const hat = { family: 'hat', preset: 'Hat', outline: quad.outline, neighbourhood: 0, morph: 45 };
  const recipe = { kind: 'hat', x: 1.5, y: -2.25, radius: 40.5, density: 0.3, seed: 77 };
  const { hash, content } = await buildLink({ settings: { ...settings, tile: hat, table: 'hat-glider-a' }, recipe, generation: 0, population: 900, cells: null });
  const link = parseLink(hash);
  report(content === 'exact' && link.tile.morph === 45 && link.table === 'hat-glider-a' && same(link.recipe, recipe), 'hat link with morph, table and recipe');
  const painted = await buildLink({ settings: { ...settings, tile: hat }, recipe: null, generation: 0, population: 3, cells: null });
  report(painted.content === 'hat-cells', 'hat cells that are not available are reported as not shared');

  const keys = [1, 10, -20, 30, -40, 8, 11, -21, 31, -41];
  const kept = await buildLink({ settings: { ...settings, tile: hat }, recipe: null, generation: 9, population: 2, cells: null, hatCells: keys });
  report(kept.content === 'exact' && same(Array.from(parseLink(kept.hash).hatCells), keys) && parseLink(kept.hash).problems.length === 0, `hat cells go by their place: ${kept.hash}`);

  const range = { radius: 2, birth: [5, 7], survive: [3, 8] };
  const ranged = await buildLink({ settings: { ...settings, tile: { ...hat, neighbourhood: 0 }, range }, recipe: null, generation: 0, population: 2, cells: null, hatCells: keys });
  const back = parseLink(ranged.hash);
  report(same(back.range, range) && back.rule === null && back.problems.length === 0, `a rule on rings survives: ${ranged.hash}`);

  const many = Array.from({ length: 5000 * 5 }, (_, i) => (i % 5 === 0 ? 0 : 100000 + i));
  const toomany = await buildLink({ settings: { ...settings, tile: hat }, recipe: null, generation: 9, population: 5000, cells: null, hatCells: many });
  report(toomany.content === 'too-many' && toomany.hash.length <= MAX_LINK_LENGTH, 'too many hat cells keep the settings');

  report(linkFromImport(`{"format":"game-of-emergence-find","link":${JSON.stringify(ranged.hash)}}`) === ranged.hash, 'a find file opens');
  report(linkFromImport(`http://localhost:5173/${ranged.hash}\n`) === ranged.hash, 'a whole link opens');
  for (const text of ['', 'nothing', '{"link":3}', '{broken', '#foo=1']) report(linkFromImport(text) === null, `refuses to open ${JSON.stringify(text)}`);
}

// Anything can arrive in a fragment.
report(parseLink('') === null && parseLink('#section') === null && parseLink('#foo=1') === null, 'unrelated fragments are left alone');
report(parseLink('#v=99&r=B3/S23').problems.includes('version') && parseLink('#v=99&r=B3/S23').rule === null, 'a newer link version is ignored');
const hostile = [
  ['f=__proto__', 'tile'],
  ['f=constructor', 'tile'],
  ['p=Nonsense', 'tile preset'],
  ['o=1,2,3', 'tile shape'],
  ['o=0,0,1,1,0,1,1,0', 'tile shape'],
  ['o=NaN,0,1,0,1,1,0,1', 'tile shape'],
  ['n=7', 'neighbourhood'],
  ['n=-1', 'neighbourhood'],
  ['t=7x64', 'torus size'],
  ['t=64x99999', 'torus size'],
  ['r=<script>', 'rule'],
  ['k=hat-glider-a', 'pattern'],
  ['c=1,2', 'view'],
  ['c=1,2,-3', 'view'],
  ['c=1e30,2,3', 'view'],
  ['s=0,0,99999,99999,0.3,1', 'random field'],
  ['s=0,0,10,10,5,1', 'random field'],
  ['s=0,0,10,10,0.3,-1', 'random field'],
  ['a=x,y,bo', 'cells'],
  ['a=0,0,', 'cells'],
  ['h=1,2,3,4,5', 'cells'],
  ['f=hat&h=1,2,3,4', 'cells'],
  ['f=hat&h=6,0,0,0,0', 'cells'],
  ['f=hat&h=14,0,0,0,0', 'cells'],
  ['f=hat&h=-1,0,0,0,0', 'cells'],
  ['f=hat&h=1,2,3,4,x', 'cells'],
  ['f=hat&h=1,2,3,4,1e30', 'cells'],
  ['f=hat&h=1.5,2,3,4,5', 'cells'],
  ['f=hat&h=', 'cells'],
  ['r=R2B5-7S3-8', 'rule'],
  ['f=hat&n=1&r=R2B5-7S3-8', 'rule'],
  ['f=hat&r=R9B5-7S3-8', 'rule'],
  ['f=hat&r=R2B0-7S3-8', 'rule'],
  ['f=hat&r=R2B7-5S3-8', 'rule'],
  ['f=hat&r=R2B5-7S3-999', 'rule'],
];
for (const [part, problem] of hostile) report(parseLink(`#v=1&${part}`).problems.includes(problem), `rejects ${part}`);
report(parseLink('#v=1&r=B012345678/S').rule.birth === 0b111111110, 'no birth from nothing on the plane');
report(parseLink('#v=1&t=64x64&r=B012345678/S').rule.birth === 0b111111111, 'birth from nothing on a torus');
report(parseLink('#v=1&r=Bf/Sf').rule.birth === 0, 'counts beyond the neighbours are dropped');
report(parseLink('#v=1&f=hat&t=64x64').torus === null, 'no torus on the hat');
report(parseLink('#v=1&f=hat&m=1000').tile.morph === 84, 'morph stays in range');

const rejects = async (coded) => {
  try {
    await decodeCells(coded);
    return false;
  } catch (error) {
    return error instanceof RleError;
  }
};
report(await rejects({ compressed: false, x: 0, y: 0, data: '99999999999o' }), 'cells claiming too much are refused');
report(await rejects({ compressed: true, x: 0, y: 0, data: 'not-really-deflate' }), 'damaged compressed cells are refused');
{
  // Unpacks to 40 MB of `b` from next to nothing.
  const stream = new Blob(['b'.repeat(40 * 1024 * 1024)]).stream().pipeThrough(new CompressionStream('deflate-raw'));
  const bytes = new Uint8Array(await new Response(stream).arrayBuffer());
  const data = btoa(String.fromCharCode(...bytes)).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
  report(data.length < 70000 && (await rejects({ compressed: true, x: 0, y: 0, data })), `a zip bomb of ${data.length} characters is stopped`);
}

console.log(failures ? `${failures} checks failed` : 'all share checks passed');
process.exitCode = failures ? 1 : 0;
