import type { HatPattern } from '../patterns/hatPatterns';
import { effectiveRows } from '../patterns/tableRows';

/** Named after their shade on the field, in the order of the table's states. */
const STATES = ['empty', 'dark', 'mid', 'light'];

function State({ state, capital }: { state: number; capital?: boolean }) {
  const name = STATES[state];
  return (
    <span className="state">
      <span className={`swatch swatch-${state}`} aria-hidden="true" />
      {capital ? name[0].toUpperCase() + name.slice(1) : name}
    </span>
  );
}

export function TableRulePanel(props: { pattern: HatPattern; onReturn: () => void }) {
  return (
    <>
      <p className="note notice">{props.pattern.name} runs on an extended ruleset, not on a B/S rule.</p>
      <p className="note">Cells have four states instead of two. Each generation, a cell follows the first line that applies, counting the tiles that touch it at an edge or corner:</p>
      <ol className="table-rows">
        {effectiveRows(props.pattern.table).map((row, index) => (
          <li key={index}>
            {row.own === null ? 'Any' : <State state={row.own} capital />} cell with{' '}
            {row.conditions.map((condition, c) => (
              <span key={condition.state}>
                {c > 0 && ' and '}
                {condition.count}+ <State state={condition.state} />
              </span>
            ))}{' '}
            neighbours {row.next === row.own ? 'stays' : 'becomes'} <State state={row.next} />
          </li>
        ))}
      </ol>
      <p className="note">Every other cell becomes <State state={0} />.</p>
      <p className="note">Like a B/S rule, this only counts neighbours and treats every cell alike. Unlike one, it was found by an evolutionary search that rewarded travel, so the glider is the goal of the rule and not a surprise.</p>
      <button className="return-rule" onMouseDown={(e) => e.preventDefault()} onClick={props.onReturn}>
        Return to basic ruleset
      </button>
    </>
  );
}
