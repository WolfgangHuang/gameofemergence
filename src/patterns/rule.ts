export interface Rule {
  birth: number;
  survive: number;
}

/**
 * A rule on all cells within `radius` rings, in the style of Larger than Life but
 * without the cell itself: born with `birth[0]` to `birth[1]` live cells among them,
 * surviving with `survive[0]` to `survive[1]`.
 */
export interface RangeRule {
  radius: number;
  birth: [number, number];
  survive: [number, number];
}

export const MAX_RANGE_RADIUS = 3;
/** The width of the engine's count mask. */
const MAX_RANGE_COUNT = 127;

export function rangeRuleName({ radius, birth, survive }: RangeRule) {
  return `R${radius}B${birth[0]}-${birth[1]}S${survive[0]}-${survive[1]}`;
}

/** Null for anything but a rule the engine can run. */
export function parseRangeRule(text: string): RangeRule | null {
  const match = /^R(\d+)B(\d+)-(\d+)S(\d+)-(\d+)$/.exec(text);
  if (!match) return null;
  const [radius, bl, bh, sl, sh] = match.slice(1).map(Number);
  const valid = radius >= 1 && radius <= MAX_RANGE_RADIUS && bl >= 1 && bl <= bh && bh <= MAX_RANGE_COUNT && sl <= sh && sh <= MAX_RANGE_COUNT;
  return valid ? { radius, birth: [bl, bh], survive: [sl, sh] } : null;
}

/** Counts above nine are written as hexadecimal digits. */
export function parseRule(text: string): Rule {
  const [, born, survives] = /^B(\w*)\/S(\w*)$/.exec(text)!;
  const bits = (digits: string) => [...digits].reduce((mask, digit) => mask | (1 << parseInt(digit, 16)), 0);
  return { birth: bits(born), survive: bits(survives) };
}

export function ruleName({ birth, survive }: Rule) {
  const digits = (bits: number) => {
    let text = '';
    for (let n = 0; n < 16; n++) if ((bits >> n) & 1) text += n.toString(16);
    return text;
  };
  return `B${digits(birth)}/S${digits(survive)}`;
}
