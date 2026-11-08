import { useRef, useState } from 'react';
import { area, buildTiling, FAMILIES, fullOutline, HAT_NEIGHBOURHOODS, HAT_SHAPES, isValidOutline, type Family, type Vec } from '../tiles/tiles';

export interface TileState {
  family: Family | 'hat';
  /** Name of the preset, or null once a vertex has been dragged. */
  preset: string | null;
  outline: Vec[];
  neighbourhood: number;
  /** Position in the Tile(a, b) family, in degrees. */
  morph: number;
}

const HAT_LABEL = 'Einstein';

const VIEW_RADIUS = 2.4;

function centre(points: Vec[]): Vec {
  const sum = points.reduce((total, p) => [total[0] + p[0], total[1] + p[1]], [0, 0]);
  return [sum[0] / points.length, sum[1] / points.length];
}

const pathOf = (vertices: Vec[]) => `M${vertices.map(([x, y]) => `${x.toFixed(3)} ${y.toFixed(3)}`).join('L')}Z`;

/** Shows the tile among its neighbours, with handles on its corners. */
function TileEditor(props: { family: Family; state: TileState; onDrag: (outline: Vec[]) => void }) {
  const { family } = props;
  const { outline } = props.state;
  const full = fullOutline(family, outline);
  const svgRef = useRef<SVGSVGElement>(null);
  const [dragging, setDragging] = useState<number | null>(null);
  // Fixed while dragging, so that the handle does not run away under the pointer.
  const [view, setView] = useState<{ key: string; at: Vec } | null>(null);
  const key = `${family}/${props.state.preset}`;
  const at = view && (dragging !== null || props.state.preset === null) ? view.at : centre(full);
  if (dragging === null && view?.key !== key && props.state.preset !== null) setView({ key, at });

  const tiling = buildTiling(family, outline);
  // The tiling is scaled to unit area; the editor shows it at the outline's own size.
  const k = Math.sqrt(Math.abs(area(full)));
  const copies: string[] = [];
  for (let j = -3; j <= 3; j++) {
    for (let i = -3; i <= 3; i++) {
      for (const polygon of tiling.polygons) {
        copies.push(
          pathOf(
            polygon.vertices.map(([x, y]): Vec => [
              (x + i * tiling.t1[0] + j * tiling.t2[0]) * k,
              (y + i * tiling.t1[1] + j * tiling.t2[1]) * k,
            ]),
          ),
        );
      }
    }
  }

  const move = (event: React.PointerEvent, index: number) => {
    const matrix = svgRef.current!.getScreenCTM()!.inverse();
    const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix);
    const next = outline.map((v, i): Vec => (i === index ? [point.x, point.y] : v));
    if (isValidOutline(fullOutline(family, next))) props.onDrag(next);
  };

  return (
    <svg
      ref={svgRef}
      className="tile-editor"
      viewBox={`${at[0] - VIEW_RADIUS} ${at[1] - VIEW_RADIUS * 0.7} ${2 * VIEW_RADIUS} ${1.4 * VIEW_RADIUS}`}
      aria-label="Drag the corners to deform the tile"
    >
      <path className="tile-copies" d={copies.join('')} />
      <path className="tile-main" d={pathOf(full)} />
      {full.map(([x, y], index) =>
        index < outline.length ? (
          <circle
            key={index}
            className={dragging === index ? 'tile-handle dragging' : 'tile-handle'}
            cx={x}
            cy={y}
            r={0.11}
            onPointerDown={(e) => {
              e.currentTarget.setPointerCapture(e.pointerId);
              setView({ key, at });
              setDragging(index);
            }}
            onPointerMove={(e) => dragging === index && move(e, index)}
            onPointerUp={() => setDragging(null)}
            onPointerCancel={() => setDragging(null)}
          />
        ) : (
          <circle key={index} className="tile-follower" cx={x} cy={y} r={0.05} />
        ),
      )}
    </svg>
  );
}

function Chips(props: { items: string[]; active: string | null; onPick: (item: string) => void }) {
  return (
    <div className="chips">
      {props.items.map((item) => (
        <button key={item} className={item === props.active ? 'chip active' : 'chip'} onMouseDown={(e) => e.preventDefault()} onClick={() => props.onPick(item)}>
          {item}
        </button>
      ))}
    </div>
  );
}

function HatPanel({ state, onChange }: { state: TileState; onChange: (state: TileState) => void }) {
  const shape = HAT_SHAPES.find((s) => s.degrees === state.morph);
  const label = (n: (typeof HAT_NEIGHBOURHOODS)[number]) => `${n.name} (${n.count})`;
  return (
    <>
      <label className="slider">
        <span className="caption">
          Family
          <span className="value">{shape?.name ?? `${state.morph}\u00b0`}</span>
        </span>
        <input type="range" min={6} max={84} value={state.morph} onChange={(e) => onChange({ ...state, morph: Number(e.target.value) })} />
      </label>
      <Chips items={HAT_SHAPES.map((s) => s.name)} active={shape?.name ?? null} onPick={(name) => onChange({ ...state, morph: HAT_SHAPES.find((s) => s.name === name)!.degrees })} />
      <div>
        <span className="caption">Neighbours</span>
        <Chips
          items={HAT_NEIGHBOURHOODS.map(label)}
          active={label(HAT_NEIGHBOURHOODS[state.neighbourhood])}
          onPick={(picked) => onChange({ ...state, neighbourhood: HAT_NEIGHBOURHOODS.findIndex((n) => label(n) === picked) })}
        />
      </div>
      <p className="note">
        One tile, no repeating pattern. These shapes share the hat's tiling, so mirrored tiles remain, also in the Spectre shape, whose own tiling avoids them. Tiling by{' '}
        Smith, Myers, Kaplan and Goodman-Strauss, built after Kaplan's hatviz (<a href="/THIRD_PARTY_NOTICES.txt">notices</a>).
      </p>
    </>
  );
}

export function TilePanel({ state, onChange }: { state: TileState; onChange: (state: TileState) => void }) {
  const families = Object.keys(FAMILIES) as Family[];
  const familyChips = (
    <Chips
      items={[...families.map((f) => FAMILIES[f].name), HAT_LABEL]}
      active={state.family === 'hat' ? HAT_LABEL : FAMILIES[state.family].name}
      onPick={(name) => {
        if (name === HAT_LABEL) {
          if (state.family !== 'hat') onChange({ ...state, family: 'hat', preset: 'Hat', neighbourhood: 0 });
          return;
        }
        const family = families.find((f) => FAMILIES[f].name === name)!;
        if (family === state.family) return;
        const preset = FAMILIES[family].presets[0];
        onChange({ ...state, family, preset: preset.name, outline: preset.outline, neighbourhood: 0 });
      }}
    />
  );
  if (state.family === 'hat') {
    return (
      <>
        {familyChips}
        <HatPanel state={state} onChange={onChange} />
      </>
    );
  }
  const family = state.family;
  const info = FAMILIES[family];
  return (
    <>
      {familyChips}
      <TileEditor family={family} state={state} onDrag={(outline) => onChange({ ...state, preset: null, outline })} />
      <Chips
        items={info.presets.map((p) => p.name)}
        active={state.preset}
        onPick={(name) => {
          const preset = info.presets.find((p) => p.name === name)!;
          onChange({ ...state, preset: preset.name, outline: preset.outline });
        }}
      />
      {info.neighbourhoods.length > 1 && (
        <div>
          <span className="caption">Neighbours</span>
          <Chips
            items={info.neighbourhoods.map((n) => `${n.name} (${n.count})`)}
            active={`${info.neighbourhoods[state.neighbourhood].name} (${info.neighbourhoods[state.neighbourhood].count})`}
            onPick={(label) =>
              onChange({ ...state, neighbourhood: info.neighbourhoods.findIndex((n) => `${n.name} (${n.count})` === label) })
            }
          />
        </div>
      )}
    </>
  );
}
