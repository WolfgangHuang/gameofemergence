import { useEffect, useState } from 'react';
import type { TorusSize } from '../sim/protocol';
import { MAX_TORUS_SIDE, MIN_TORUS_SIDE } from '../tiles/tiles';

const SIZES = [64, 128, 256, 512, 1024, 2048, 4096];
export const DEFAULT_TORUS: TorusSize = { width: 128, height: 128 };
const MIN_SIDE = MIN_TORUS_SIDE;
const MAX_SIDE = MAX_TORUS_SIDE;

function validSide(value: number, fallback: number) {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(MAX_SIDE, Math.max(MIN_SIDE, Math.round(value / 2) * 2));
}

function SideInput(props: { label: string; value: number; onCommit: (value: number) => void }) {
  const [text, setText] = useState(String(props.value));
  useEffect(() => setText(String(props.value)), [props.value]);
  const commit = () => {
    const value = validSide(Number.parseFloat(text), props.value);
    setText(String(value));
    if (value !== props.value) props.onCommit(value);
  };
  return (
    <label className="side">
      <span className="caption">{props.label}</span>
      <input
        type="number"
        min={MIN_SIDE}
        max={MAX_SIDE}
        step={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') e.currentTarget.blur();
        }}
      />
    </label>
  );
}

function Chip(props: { label: string; active: boolean; onPick: () => void }) {
  return (
    <button className={props.active ? 'chip active' : 'chip'} onMouseDown={(e) => e.preventDefault()} onClick={props.onPick}>
      {props.label}
    </button>
  );
}

export function TorusPanel(props: { on: boolean; size: TorusSize; onToggle: (on: boolean) => void; onResize: (size: TorusSize) => void }) {
  const { on, size, onToggle, onResize } = props;
  return (
    <>
      <div>
        <span className="caption">Edges</span>
        <div className="chips">
          <Chip label="Unbounded" active={!on} onPick={() => onToggle(false)} />
          <Chip label="Torus" active={on} onPick={() => onToggle(true)} />
        </div>
      </div>
      {on && (
        <>
          <div className="chips">
            {SIZES.map((side) => (
              <Chip key={side} label={String(side)} active={size.width === side && size.height === side} onPick={() => onResize({ width: side, height: side })} />
            ))}
          </div>
          <div className="sides">
            <SideInput label="Width" value={size.width} onCommit={(width) => onResize({ ...size, width })} />
            <SideInput label="Height" value={size.height} onCommit={(height) => onResize({ ...size, height })} />
          </div>
          <p className="note">Opposite edges touch, so whatever leaves comes back. Each field keeps its own cells. Shrinking drops what no longer fits. Birth with 0 neighbours is allowed here.</p>
        </>
      )}
    </>
  );
}
