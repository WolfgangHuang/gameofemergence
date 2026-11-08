export interface Rule {
  birth: number;
  survive: number;
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
