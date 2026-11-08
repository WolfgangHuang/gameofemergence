import { CATEGORIES, PATTERNS, type Pattern } from '../patterns/library';
import type { HatPattern } from '../patterns/hatPatterns';
import { HAT_PATTERNS } from '../patterns/hatPatterns';
import type { Shape } from '../patterns/rle';

function Thumbnail({ shape }: { shape: Shape }) {
  let path = '';
  for (let i = 0; i < shape.cells.length; i += 2) path += `M${shape.cells[i]} ${shape.cells[i + 1]}h1v1h-1z`;
  return (
    <svg className="thumbnail" viewBox={`-1 -1 ${shape.width + 2} ${shape.height + 2}`} aria-hidden="true">
      <path d={path} />
    </svg>
  );
}

export function HatPatternPanel(props: { selected: string | null; onSelect: (pattern: HatPattern) => void }) {
  return (
    <>
      <p className="note">Click to pick up, click the field to place. Most starting points launch one or two gliders, along one of six directions. Esc ends. Picking one switches to its own rule.</p>
      <div className="pattern-group">
        <span className="caption">Spaceships</span>
        {HAT_PATTERNS.map((pattern) => (
          <button
            key={pattern.id}
            className={props.selected === pattern.id ? 'pattern active' : 'pattern'}
            aria-pressed={props.selected === pattern.id}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => props.onSelect(pattern)}
          >
            <span className="pattern-text">
              <span>{pattern.name}</span>
              <small>{pattern.note}</small>
            </span>
          </button>
        ))}
      </div>
    </>
  );
}

export function PatternPanel(props: { selected: string | null; conway: boolean; onSelect: (pattern: Pattern) => void }) {
  return (
    <>
      <p className="note">Click to pick up, click the field to place. R rotates, F flips, Esc ends.</p>
      {!props.conway && <p className="note warn">These only behave as described under Conway's rule, B3/S23.</p>}
      {CATEGORIES.map((category) => (
        <div key={category} className="pattern-group">
          <span className="caption">{category}</span>
          {PATTERNS.filter((p) => p.category === category).map((pattern) => (
            <button
              key={pattern.id}
              className={props.selected === pattern.id ? 'pattern active' : 'pattern'}
              aria-pressed={props.selected === pattern.id}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => props.onSelect(pattern)}
            >
              <Thumbnail shape={pattern.shape} />
              <span className="pattern-text">
                <span>{pattern.name}</span>
                {pattern.note && <small>{pattern.note}</small>}
              </span>
            </button>
          ))}
        </div>
      ))}
    </>
  );
}
