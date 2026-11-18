import { useMemo } from "react";
import { useDemo } from "../../lib/demo-context";

export function SliceSettingsDemo() {
  const { layerHeight, infill, walls, setLayerHeight, setInfill, setWalls, estimates } = useDemo();
  const hatchGap = Math.max(4, 24 - infill / 3);

  const timeLabel = useMemo(() => {
    const m = Math.floor(estimates.time_s / 60);
    const s = Math.round(estimates.time_s % 60);
    return m > 0 ? `~${m}m ${s}s` : `~${s}s`;
  }, [estimates.time_s]);

  return (
    <section className="demo-panel demo-panel--slice" id="slice-settings">
      <header className="demo-panel__head">
        <div>
          <p className="demo-panel__eyebrow">Slicing</p>
          <h2 className="demo-panel__title">Slice settings</h2>
          <p className="demo-panel__note">Sliders update the hatch and the 3D toolpath density.</p>
        </div>
      </header>
      <div className="slice-layout">
        <div className="slice-sliders">
          <label>
            Layer height
            <input
              type="range"
              min={0.12}
              max={0.32}
              step={0.02}
              value={layerHeight}
              onChange={(e) => setLayerHeight(Number(e.target.value))}
            />
            <span className="mono">{layerHeight.toFixed(2)} mm</span>
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
            <rect
              x="18"
              y="18"
              width="64"
              height="64"
              fill="url(#hatch)"
              opacity={infill / 50}
              stroke="var(--path)"
              strokeWidth={walls * 0.35}
            />
          </svg>
        </div>
      </div>
      <dl className="slice-stats slice-stats--prominent">
        <div>
          <dt>Layers</dt>
          <dd className="mono">{estimates.layers}</dd>
        </div>
        <div>
          <dt>Filament</dt>
          <dd className="mono">{estimates.filament_m.toFixed(2)} m</dd>
        </div>
        <div>
          <dt>Est. slice</dt>
          <dd className="mono">{timeLabel}</dd>
        </div>
      </dl>
      <p className="demo-footnote dim">Scaled from committed cube-slice.json metadata — illustrative, not live CuraEngine.</p>
    </section>
  );
}