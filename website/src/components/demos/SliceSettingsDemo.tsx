import { useMemo, useState } from "react";
import { DemoFrame } from "./DemoFrame";

/** Baseline: 10 mm cube, A1 0.4 mm — docs/verification.md (54 layers, 2.19 m @ 0.2 mm / ~15% infill). */
const BASE = { layers: 54, filament_m: 2.1943, seconds: 48 };

function estimate(layer: number, infill: number, walls: number) {
  const layerScale = 0.2 / layer;
  const infillScale = infill / 15;
  const wallScale = 1 + (walls - 3) * 0.08;
  const layers = Math.ceil(10 / layer);
  const filament = BASE.filament_m * layerScale * infillScale * wallScale;
  const seconds = BASE.seconds * layerScale * infillScale * wallScale;
  return { layers, filament, seconds };
}

export function SliceSettingsDemo() {
  const [layer, setLayer] = useState(0.2);
  const [infill, setInfill] = useState(20);
  const [walls, setWalls] = useState(3);

  const stats = useMemo(() => estimate(layer, infill, walls), [layer, infill, walls]);
  const hatchGap = Math.max(4, 24 - infill / 3);

  return (
    <DemoFrame title="Slice settings" note="Estimates scale from measured A1 cube baseline">
      <div className="slice-layout">
        <div className="slice-sliders">
          <label>
            Layer height
            <input
              type="range"
              min={0.12}
              max={0.32}
              step={0.02}
              value={layer}
              onChange={(e) => setLayer(Number(e.target.value))}
            />
            <span className="mono">{layer.toFixed(2)} mm</span>
          </label>
          <label>
            Infill
            <input
              type="range"
              min={5}
              max={40}
              step={1}
              value={infill}
              onChange={(e) => setInfill(Number(e.target.value))}
            />
            <span className="mono">{infill}%</span>
          </label>
          <label>
            Walls
            <input
              type="range"
              min={2}
              max={5}
              step={1}
              value={walls}
              onChange={(e) => setWalls(Number(e.target.value))}
            />
            <span className="mono">{walls}</span>
          </label>
        </div>
        <div className="slice-preview" style={{ "--hatch": `${hatchGap}px` } as React.CSSProperties}>
          <svg viewBox="0 0 100 100" className="slice-svg" aria-hidden>
            <rect x="15" y="15" width="70" height="70" fill="none" stroke="var(--path)" strokeWidth="1.2" />
            <defs>
              <pattern id="hatch" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(35)">
                <line x1="0" y1="0" x2="0" y2="8" stroke="var(--path)" strokeWidth="0.6" opacity="0.5" />
              </pattern>
            </defs>
            <rect x="18" y="18" width="64" height="64" fill="url(#hatch)" opacity={infill / 50} />
          </svg>
        </div>
      </div>
      <dl className="slice-stats">
        <div>
          <dt>Layers</dt>
          <dd className="mono">{stats.layers}</dd>
        </div>
        <div>
          <dt>Filament</dt>
          <dd className="mono">{stats.filament.toFixed(2)} m</dd>
        </div>
        <div>
          <dt>Slice time</dt>
          <dd className="mono">~{stats.seconds.toFixed(0)} s</dd>
        </div>
      </dl>
      <p className="demo-footnote dim">Scaled from verification.md cube slice — not a live CuraEngine run in the browser.</p>
    </DemoFrame>
  );
}