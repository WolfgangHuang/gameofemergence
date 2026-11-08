import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { Pattern } from './patterns/library';
import { flip, rotate, type Shape } from './patterns/rle';
import type { Stats, TorusSize } from './sim/protocol';
import { Stage, type Tool } from './stage/Stage';
import { BrushIcon, ChevronIcon, ClearIcon, EraserIcon, HandIcon, PauseIcon, PlayIcon, SeedIcon, StampIcon, StepIcon } from './ui/icons';
import type { HatPattern } from './patterns/hatPatterns';
import { HatPatternPanel, PatternPanel } from './ui/PatternPanel';
import { parseRule, ruleName, RulePanel, type Rule } from './ui/RulePanel';
import { TilePanel, type TileState } from './ui/TilePanel';
import { DEFAULT_TORUS, TorusPanel } from './ui/TorusPanel';
import { buildTiling, FAMILIES, HAT_NEIGHBOURHOODS, HAT_SHAPES, TOPOLOGY } from './tiles/tiles';

const IDLE_MS = 2500;
const SPEED_MAX = 30;
const numberFormat = new Intl.NumberFormat('en-US');
const format = (n: number) => numberFormat.format(n);

type StageStats = Stats & { rate: number };
type Section = 'tile' | 'field' | 'rule' | 'patterns';

const INITIAL_TILE: TileState = { family: 'quad', preset: 'Square', outline: FAMILIES.quad.presets[0].outline, neighbourhood: 0, morph: 60 };

/** Slider positions map to 1–1000 generations per second; the last one is unlimited. */
function speedFromSlider(value: number) {
  return value >= SPEED_MAX ? null : Math.round(10 ** ((value / (SPEED_MAX - 1)) * 3));
}

function describeStatus({ status, period, detail, generation, population }: StageStats): { label: string; detail?: string; settled: boolean } {
  switch (status) {
    case 'empty':
      return { label: generation > 0 ? 'Extinct' : 'Empty', settled: generation > 0 };
    case 'still':
      return { label: 'Still', detail: 'nothing changes any more', settled: true };
    case 'oscillating':
      return { label: 'Oscillating', detail: `repeats every ${period} generations`, settled: true };
    case 'drifting':
      return { label: 'Drifting', detail: `${detail} moving ${detail === 1 ? 'object' : 'objects'} that will never meet again, period ${period}`, settled: true };
    case 'growing':
      return { label: 'Growing steadily', detail: `+${detail} cells every ${period} generations`, settled: false };
    default:
      return { label: population === 0 ? 'Empty' : generation === 0 ? 'Ready' : 'Evolving', settled: false };
  }
}

function IconButton(props: { label: string; keyHint: string; active?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      className={props.active ? 'icon active' : 'icon'}
      aria-label={props.label}
      aria-pressed={props.active}
      title={`${props.label} (${props.keyHint})`}
      // Keeps focus away from the button, so that Space stays play/pause after a click.
      onMouseDown={(e) => e.preventDefault()}
      onClick={props.onClick}
    >
      {props.children}
    </button>
  );
}

function Slider(props: { label: string; value: number; min: number; max: number; readout: string; onChange: (value: number) => void }) {
  return (
    <label className="slider">
      <span className="caption">
        {props.label}
        <span className="value">{props.readout}</span>
      </span>
      <input type="range" min={props.min} max={props.max} value={props.value} onChange={(e) => props.onChange(Number(e.target.value))} />
    </label>
  );
}

function PanelSection(props: { title: string; readout?: string; open: boolean; onToggle: () => void; children: ReactNode }) {
  return (
    <section className="section">
      <button className="section-head" aria-expanded={props.open} onMouseDown={(e) => e.preventDefault()} onClick={props.onToggle}>
        <span className="caption">{props.title}</span>
        <span className="section-end">
          {props.readout && <span className="value">{props.readout}</span>}
          <ChevronIcon open={props.open} />
        </span>
      </button>
      {props.open && <div className="section-body">{props.children}</div>}
    </section>
  );
}

export function App() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const stageRef = useRef<Stage | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [tool, setTool] = useState<Tool>('brush');
  const [brushSize, setBrushSize] = useState(1);
  const [speed, setSpeed] = useState(15);
  const [fade, setFade] = useState(14);
  const [density, setDensity] = useState(30);
  const [tile, setTile] = useState<TileState>(INITIAL_TILE);
  const isHat = tile.family === 'hat';
  /** Both survive a visit to the hat tiling, which cannot be a torus. */
  const [torusOn, setTorusOn] = useState(false);
  const [torusSize, setTorusSize] = useState<TorusSize>(DEFAULT_TORUS);
  const torus = useMemo(() => (torusOn && !isHat ? torusSize : null), [torusOn, isHat, torusSize]);
  const onTorus = torus !== null;
  const neighbourhood = tile.family === 'hat' ? HAT_NEIGHBOURHOODS[tile.neighbourhood] : FAMILIES[tile.family].neighbourhoods[tile.neighbourhood];
  const [rule, setRule] = useState<Rule>(parseRule(neighbourhood.rules[0]));
  /** A pattern's own priority table, which replaces the rule while it is set. */
  const [table, setTable] = useState<number[] | null>(null);
  const [stopWhenSettled, setStopWhenSettled] = useState(false);
  const [patternId, setPatternId] = useState<string | null>(null);
  const [stamp, setStamp] = useState<Shape | null>(null);
  const [open, setOpen] = useState<Section | null>('tile');
  const [stats, setStats] = useState<StageStats>({ generation: 0, population: 0, status: 'active', period: 0, detail: 0, since: 0, rate: 0 });
  const [idle, setIdle] = useState(false);

  const gps = speedFromSlider(speed);
  const fadeSeconds = fade / 10;
  const described = describeStatus(stats);
  const conway = ruleName(rule) === 'B3/S23';
  const patternsFit = neighbourhood.topology === 'moore';
  const tiling = useMemo(() => (tile.family === 'hat' ? null : buildTiling(tile.family, tile.outline)), [tile.family, tile.outline]);
  const [capped, setCapped] = useState(false);
  const toggleSection = (section: Section) => setOpen((current) => (current === section ? null : section));

  useEffect(() => {
    try {
      const stage = new Stage(canvasRef.current!);
      stage.onStats = setStats;
      stageRef.current = stage;
      return () => {
        stage.dispose();
        stageRef.current = null;
      };
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error));
    }
  }, []);

  useEffect(() => stageRef.current?.setRunning(running), [running]);
  useEffect(() => stageRef.current?.setSpeed(gps), [gps]);
  useEffect(() => stageRef.current?.setFade(fadeSeconds * 1000), [fadeSeconds]);
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    stage.setSurface(isHat, torus);
    if (tiling) stage.setTiling(tiling, TOPOLOGY[neighbourhood.topology as keyof typeof TOPOLOGY]);
    else stage.setHatEdgesOnly(neighbourhood.topology === 'hatEdges');
    stage.onCapped = setCapped;
  }, [tiling, neighbourhood.topology, isHat, torus]);
  useEffect(() => stageRef.current?.setMorph(tile.morph), [tile.morph]);
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    if (table && isHat) stage.setHatTable(table);
    else stage.setRule(rule.birth, rule.survive);
  }, [rule, table, isHat, onTorus]);
  // Birth from nothing needs a finite field.
  useEffect(() => {
    if (!onTorus && rule.birth & 1) setRule((current) => ({ ...current, birth: current.birth & ~1 }));
  }, [onTorus, rule.birth]);
  // Patterns belong to one kind of field.
  useEffect(() => {
    setTool((current) => (current === 'stamp' ? 'brush' : current));
    setPatternId(null);
    setTable(null);
  }, [isHat]);
  // Each neighbourhood starts from a rule known to stay lively on it.
  useEffect(() => {
    setRule(parseRule(neighbourhood.rules[0]));
    // Hat patterns need all neighbours, and a rule from the panel ends their table.
    if (neighbourhood.topology !== 'hatVertices') setTable(null);
    if (neighbourhood.topology !== 'moore' && neighbourhood.topology !== 'hatVertices') {
      setTool((current) => (current === 'stamp' ? 'brush' : current));
      setPatternId(null);
    }
  }, [neighbourhood]);
  useEffect(() => stageRef.current?.setTool(tool), [tool]);
  useEffect(() => stageRef.current?.setStamp(stamp), [stamp]);
  useEffect(() => {
    if (stageRef.current) stageRef.current.brushRadius = brushSize - 1;
  }, [brushSize]);

  // Stops once on reaching a settled state, so that Play can still resume afterwards.
  const wasSettled = useRef(false);
  useEffect(() => {
    if (described.settled && !wasSettled.current && stopWhenSettled) setRunning(false);
    wasSettled.current = described.settled;
  }, [described.settled, stopWhenSettled]);

  const densityRef = useRef(density);
  densityRef.current = density;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || (e.target instanceof HTMLButtonElement && e.key === ' ')) return;
      if (e.target instanceof HTMLInputElement && e.target.type === 'number') return;
      const stage = stageRef.current;
      switch (e.key.toLowerCase()) {
        case ' ':
          e.preventDefault();
          setRunning((r) => !r);
          break;
        case 'n':
          stage?.step();
          break;
        case 'b':
          setTool('brush');
          break;
        case 'e':
          setTool('eraser');
          break;
        case 'h':
          setTool('hand');
          break;
        case 'p':
          setOpen('patterns');
          break;
        case 's':
          stage?.randomize(densityRef.current / 100);
          break;
        case 'c':
          stage?.clear();
          break;
        case 'r':
          setStamp((shape) => shape && rotate(shape));
          break;
        case 'f':
          setStamp((shape) => shape && flip(shape));
          break;
        case 'escape':
          setTool((current) => (current === 'stamp' ? 'brush' : current));
          setPatternId(null);
          break;
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // While running, the controls recede until the pointer moves again.
  useEffect(() => {
    if (!running) {
      setIdle(false);
      return;
    }
    let timer = setTimeout(() => setIdle(true), IDLE_MS);
    const wake = () => {
      setIdle(false);
      clearTimeout(timer);
      timer = setTimeout(() => setIdle(true), IDLE_MS);
    };
    window.addEventListener('pointermove', wake);
    window.addEventListener('keydown', wake);
    return () => {
      clearTimeout(timer);
      window.removeEventListener('pointermove', wake);
      window.removeEventListener('keydown', wake);
    };
  }, [running]);

  if (failure) return <p className="failure">This site needs WebGL2, which is not available here. ({failure})</p>;

  const empty = stats.population === 0 && stats.generation === 0;
  const pickTool = (next: Tool) => {
    setTool(next);
    setPatternId(null);
  };
  const pickHatPattern = (pattern: HatPattern) => {
    setTile((current) => ({ ...current, neighbourhood: 0 }));
    setTable(pattern.table);
    setPatternId(pattern.id);
    stageRef.current?.setHatSeed(pattern.seed);
    setTool('stamp');
  };
  const changeRule = (next: Rule) => {
    setRule(next);
    setTable(null);
    if (isHat) {
      setPatternId(null);
      setTool((current) => (current === 'stamp' ? 'brush' : current));
    }
  };
  const pickPattern = (pattern: Pattern) => {
    setPatternId(pattern.id);
    setStamp(pattern.shape);
    setTool('stamp');
  };

  return (
    <>
      <canvas ref={canvasRef} className={tool === 'hand' ? 'field hand' : 'field'} />

      <div className={idle ? 'chrome idle' : 'chrome'}>
        <header className="brand">Game of Emergence</header>

        <aside className="panel tools" aria-label="Tools">
          <IconButton label="Brush" keyHint="B" active={tool === 'brush'} onClick={() => pickTool('brush')}>
            <BrushIcon />
          </IconButton>
          <IconButton label="Eraser" keyHint="E" active={tool === 'eraser'} onClick={() => pickTool('eraser')}>
            <EraserIcon />
          </IconButton>
          <IconButton label="Move" keyHint="H" active={tool === 'hand'} onClick={() => pickTool('hand')}>
            <HandIcon />
          </IconButton>
          <IconButton label="Patterns" keyHint="P" active={tool === 'stamp'} onClick={() => toggleSection('patterns')}>
            <StampIcon />
          </IconButton>
          <span className="rule" />
          <IconButton label="Random field" keyHint="S" onClick={() => stageRef.current?.randomize(density / 100)}>
            <SeedIcon />
          </IconButton>
          <IconButton label="Clear" keyHint="C" onClick={() => stageRef.current?.clear()}>
            <ClearIcon />
          </IconButton>
        </aside>

        <aside className="panel settings" aria-label="Settings">
          <PanelSection title="Tile" readout={isHat ? (HAT_SHAPES.find((s) => s.degrees === tile.morph)?.name ?? `${tile.morph}\u00b0`) : (tile.preset ?? 'Custom')} open={open === 'tile'} onToggle={() => toggleSection('tile')}>
            <TilePanel state={tile} onChange={setTile} />
            {isHat && capped && <p className="note warn">The cell limit of this browser tab is reached, so the field ends here.</p>}
          </PanelSection>
          <PanelSection title="Rule" readout={table ? 'Pattern table' : ruleName(rule)} open={open === 'rule'} onToggle={() => toggleSection('rule')}>
            {table && <p className="note warn">A four-state table set by the pattern is active. Choosing a rule below ends it.</p>}
            <RulePanel rule={rule} count={neighbourhood.count} presets={neighbourhood.rules} birthFromNothing={onTorus} onChange={changeRule} />
          </PanelSection>
          <PanelSection title="Patterns" open={open === 'patterns'} onToggle={() => toggleSection('patterns')}>
            {isHat ? (
              <HatPatternPanel selected={patternId} onSelect={pickHatPattern} />
            ) : patternsFit ? (
              <PatternPanel selected={patternId} conway={conway} onSelect={pickPattern} />
            ) : (
              <p className="note">The library is made for quadrilaterals with eight neighbours. No patterns are collected for this grid yet.</p>
            )}
          </PanelSection>
          <PanelSection title="Field" readout={torus ? `${torus.width} \u00d7 ${torus.height}` : isHat ? undefined : 'Unbounded'} open={open === 'field'} onToggle={() => toggleSection('field')}>
            {isHat ? <p className="note">This tiling never repeats, so its edges cannot be joined into a torus.</p> : <TorusPanel on={torusOn} size={torusSize} onToggle={setTorusOn} onResize={setTorusSize} />}
            <Slider label="Brush" min={1} max={12} value={brushSize} readout={String(brushSize)} onChange={setBrushSize} />
            <Slider label="Afterglow" min={0} max={40} value={fade} readout={fade === 0 ? 'off' : `${fadeSeconds.toFixed(1)} s`} onChange={setFade} />
            <Slider label="Random density" min={5} max={95} value={density} readout={`${density} %`} onChange={setDensity} />
            <label className="check">
              <input type="checkbox" checked={stopWhenSettled} onChange={(e) => setStopWhenSettled(e.target.checked)} />
              <span>Pause when settled</span>
            </label>
          </PanelSection>
        </aside>

        <div className={described.settled ? 'status settled' : 'status'} role="status">
          <span className="status-dot" />
          <span className="status-label">{described.label}</span>
          {described.detail && <span className="status-detail">{described.detail}</span>}
          {described.settled && stats.since > 0 && <span className="status-detail">recognised at generation {format(stats.since)}</span>}
        </div>

        <footer className="panel transport" aria-label="Simulation">
          <button
            className="play"
            aria-label={running ? 'Pause' : 'Play'}
            title={`${running ? 'Pause' : 'Play'} (Space)`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setRunning(!running)}
          >
            {running ? <PauseIcon /> : <PlayIcon />}
          </button>
          <IconButton label="Single step" keyHint="N" onClick={() => stageRef.current?.step()}>
            <StepIcon />
          </IconButton>
          <Slider label="Speed" min={0} max={SPEED_MAX} value={speed} readout={gps === null ? 'max' : `${gps} /s`} onChange={setSpeed} />
          <dl className="readout">
            <div>
              <dt>Generation</dt>
              <dd>{format(stats.generation)}</dd>
            </div>
            <div>
              <dt>Population</dt>
              <dd>{format(stats.population)}</dd>
            </div>
            <div>
              <dt>Rate</dt>
              <dd>{running ? `${format(Math.round(stats.rate))} /s` : '–'}</dd>
            </div>
          </dl>
        </footer>

        <p className={empty && tool !== 'stamp' ? 'hint' : 'hint gone'}>Draw on the field, press S for a random start, or pick a pattern.</p>
      </div>
    </>
  );
}
