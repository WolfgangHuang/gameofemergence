import { parseRule, ruleName, type Rule } from '../patterns/rule';

const NAMES: Record<string, string> = {
  'B3/S23': 'Conway',
  'B36/S23': 'HighLife',
  'B2/S': 'Seeds',
  'B3678/S34678': 'Day & Night',
  'B368/S245': 'Morley',
  'B3/S12345': 'Maze',
  'B1357/S1357': 'Replicator',
  'B4678/S35678': 'Anneal',
};

function Counts(props: { label: string; bits: number; count: number; disabled?: number; onToggle: (count: number) => void }) {
  const counts = Array.from({ length: props.count + 1 }, (_, n) => n);
  return (
    <div className="counts" role="group" aria-label={props.label}>
      <span className="caption">{props.label}</span>
      <div className="dots">
        {counts.map((n) => {
          const on = ((props.bits >> n) & 1) === 1;
          return (
            <button
              key={n}
              className={on ? 'dot on' : 'dot'}
              aria-pressed={on}
              aria-label={`${props.label} with ${n} neighbours`}
              disabled={props.disabled === n}
              title={props.disabled === n ? 'Birth from nothing would fill the unbounded field at once' : undefined}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => props.onToggle(n)}
            >
              {n}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function RulePanel(props: { rule: Rule; count: number; presets: string[]; birthFromNothing: boolean; onChange: (rule: Rule) => void }) {
  const { rule, onChange } = props;
  const current = ruleName(rule);
  return (
    <>
      <Counts label="Born" bits={rule.birth} count={props.count} disabled={props.birthFromNothing ? undefined : 0} onToggle={(n) => onChange({ ...rule, birth: rule.birth ^ (1 << n) })} />
      <Counts label="Survives" bits={rule.survive} count={props.count} onToggle={(n) => onChange({ ...rule, survive: rule.survive ^ (1 << n) })} />
      <div className="chips">
        {props.presets.map((preset) => (
          <button key={preset} className={preset === current ? 'chip active' : 'chip'} title={preset} onMouseDown={(e) => e.preventDefault()} onClick={() => onChange(parseRule(preset))}>
            {NAMES[preset] ?? preset}
          </button>
        ))}
      </div>
    </>
  );
}
