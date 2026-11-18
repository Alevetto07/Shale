import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { DemoFrame } from "./DemoFrame";

const ToolpathScene3d = lazy(() =>
  import("./ToolpathScene3d").then((m) => ({ default: m.ToolpathScene3d })),
);

type Layer = {
  z: number;
  walls: number[][][];
  infill: number[][][];
};

type Payload = {
  source: string;
  layer_height_mm: number;
  layers: Layer[];
};

export function ToolpathDemo() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [data, setData] = useState<Payload | null>(null);
  const [layer, setLayer] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [show3d, setShow3d] = useState(false);
  const reduced =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  useEffect(() => {
    fetch("/demo/toolpath-cube.json")
      .then((r) => r.json())
      .then((json: Payload) => {
        setData(json);
        setLayer(json.layers.length - 1);
      })
      .catch(() => setData(null));
  }, []);

  useEffect(() => {
    if (!playing || reduced || !data) return;
    const id = window.setInterval(() => {
      setLayer((value) => (value + 1 >= data.layers.length ? 0 : value + 1));
    }, 120);
    return () => window.clearInterval(id);
  }, [playing, reduced, data]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !data) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    canvas.width = Math.floor(w * dpr);
    canvas.height = Math.floor(h * dpr);
    ctx.scale(dpr, dpr);

    const pad = 16;
    const span = 256;
    const scale = (Math.min(w, h) - pad * 2) / span;
    const ox = (w - span * scale) / 2;
    const oy = (h - span * scale) / 2;
    const map = (x: number, y: number) => [ox + x * scale, oy + (span - y) * scale];

    ctx.fillStyle = "#06080c";
    ctx.fillRect(0, 0, w, h);
    ctx.strokeStyle = "rgba(255,255,255,0.04)";
    for (let g = 0; g <= span; g += 20) {
      const a = map(g, 0);
      const b = map(g, span);
      ctx.beginPath();
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
      ctx.stroke();
      const c = map(0, g);
      const d = map(span, g);
      ctx.beginPath();
      ctx.moveTo(c[0], c[1]);
      ctx.lineTo(d[0], d[1]);
      ctx.stroke();
    }

    const current = data.layers[Math.min(layer, data.layers.length - 1)];
    if (!current) return;

    const drawPaths = (paths: number[][][], color: string, dash: number[] = []) => {
      ctx.strokeStyle = color;
      ctx.lineWidth = 1.25;
      ctx.setLineDash(dash);
      for (const path of paths) {
        ctx.beginPath();
        path.forEach(([x, y], i) => {
          const [px, py] = map(x, y);
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        });
        ctx.stroke();
      }
      ctx.setLineDash([]);
    };

    drawPaths(current.infill, "rgba(126, 184, 255, 0.35)", [3, 5]);
    drawPaths(current.walls, "rgba(245, 244, 240, 0.85)");
  }, [data, layer]);

  const readout = useMemo(() => {
    if (!data) return "Loading paths…";
    const z = data.layers[layer]?.z ?? 0;
    return `Layer ${layer + 1} / ${data.layers.length} · Z ${z.toFixed(2)} mm`;
  }, [data, layer]);

  return (
    <DemoFrame
      title="Toolpath preview"
      note="Synthetic cube paths (verification proportions)"
    >
      <canvas ref={canvasRef} className="toolpath-canvas" aria-label={readout} />
      <p className="demo-readout mono">{readout}</p>
      <div className="demo-controls">
        <input
          type="range"
          min={0}
          max={Math.max(0, (data?.layers.length ?? 1) - 1)}
          value={layer}
          onChange={(e) => setLayer(Number(e.target.value))}
          aria-label="Scrub layers"
        />
        <button
          type="button"
          className="demo-btn"
          onClick={() => setPlaying((p) => !p)}
          disabled={reduced || !data}
        >
          {playing ? "Pause" : "Play"}
        </button>
      </div>
      {show3d ? (
        <Suspense fallback={<p className="dim">Loading 3D…</p>}>
          <ToolpathScene3d layerIndex={layer} />
        </Suspense>
      ) : (
        <button type="button" className="demo-btn" onClick={() => setShow3d(true)}>
          Load 3D layer (three.js)
        </button>
      )}
      {data && <p className="demo-footnote dim">{data.source}</p>}
    </DemoFrame>
  );
}