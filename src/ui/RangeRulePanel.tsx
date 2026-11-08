import { rangeRuleName, type RangeRule } from '../patterns/rule';

const span = ([low, high]: [number, number]) => (low === high ? String(low) : `${low} to ${high}`);

export function RangeRulePanel(props: { range: RangeRule; onReturn: () => void }) {
  const { radius, birth, survive } = props.range;
  return (
    <>
      <p className="note notice">{rangeRuleName(props.range)} counts a larger neighbourhood, so it is not a B/S rule of this panel.</p>
      <p className="note">Neighbours are all tiles within {radius} rings, over edges and corners, without the tile itself. A dead tile comes alive with {span(birth)} live neighbours, and a live tile stays alive with {span(survive)}.</p>
      <button className="return-rule" onMouseDown={(e) => e.preventDefault()} onClick={props.onReturn}>
        Return to basic ruleset
      </button>
    </>
  );
}
