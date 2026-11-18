import { useEffect, useRef, useState } from "react";
import { useDemo } from "../../lib/demo-context";

export function HeroLayerCanvas() {
  const { data, layer, setLayer } = useDemo();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [animLayer, setAnimLayer] = useState(0);

  useEffect(() => {
    if (!data) return;
    const id = window.setInterval(() => {
      setAnimLayer((p) => (p >= data.layers.length - 1 ? 0 : p + 1));
    }, 110);
    return () => window.clearInterval(id);
  }, [data]);

  useEffect(() => {
    const onScroll = () => {
      if (!data) return;
      const t = Math.min(1, Math.max(0, window.scrollY / Math.min(900, window.innerHeight * 1.2)));
      const idx = Math.floor(t * (data.layers.length - 1));
      setAnimLayer(idx);
      setLayer(idx);
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener("scroll", onScroll);
  }, [data, setLayer]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !data) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const w = rect.width;
    const h = rect.height;
    const b = data.meta.bounds as {
      minX: number;
      maxX: number;
      minY: number;
      maxY: number;
    };
    const pw = b.maxX - b.minX;
    const ph = b.maxY - b.minY;
    const pad = 0.12;
    const scale = (Math.min(w, h) * (1 - pad * 2)) / Math.max(pw, ph, 1);
    const cx = (b.minX + b.maxX) / 2;
    const cy = (b.minY + b.maxY) / 2;

    ctx.fillStyle = "#05070a";
    ctx.fillRect(0, 0, w, h);

  const pathCol =
      getComputedStyle(document.documentElement).getPropertyValue("--path").trim() || "#9eb8ff";

    ctx.strokeStyle = "rgba(255,255,255,0.06)";
    ctx.lineWidth = 1;
    const gridStep = 14;
    for (let gx = 0; gx < w; gx += gridStep) {
      ctx.beginPath();
      ctx.moveTo(gx, 0);
      ctx.lineTo(gx, h);
      ctx.stroke();
    }
    for (let gy = 0; gy < h; gy += gridStep) {
      ctx.beginPath();
      ctx.moveTo(0, gy);
      ctx.lineTo(w, gy);
      ctx.stroke();
    }

    const layIdx = Math.min(animLayer, data.layers.length - 1);
    const layers = data.layers.slice(0, layIdx + 1);

    const toScreen = (x: number, y: number) => {
      const sx = w / 2 + (x - cx) * scale;
      const sy = h * 0.58 - (y - cy) * scale * 0.82 - (layIdx / data.layers.length) * 12;
      return [sx, sy] as const;
    };

    for (let li = 0; li < layers.length; li++) {
      const lay = layers[li];
      const layerFade = 0.35 + (li / layers.length) * 0.65;
      for (const seg of lay.segments) {
        ctx.strokeStyle =
          seg.type === "wall" ? `rgba(240,238,234,${0.4 * layerFade})` : pathCol;
        ctx.globalAlpha = seg.type === "infill" ? 0.45 * layerFade : 0.9 * layerFade;
        ctx.lineWidth = seg.type === "wall" ? 2.2 : 1.1;
        ctx.beginPath();
        seg.points.forEach(([x, y], i) => {
          const [sx, sy] = toScreen(x, y);
          if (i === 0) ctx.moveTo(sx, sy);
          else ctx.lineTo(sx, sy);
        });
        ctx.stroke();
      }
    }
    ctx.globalAlpha = 1;

    const top = layers[layers.length - 1];
    const p0 = top?.segments[0]?.points[0];
    if (p0) {
      const [nx, ny] = toScreen(p0[0], p0[1]);
      ctx.fillStyle = pathCol;
      ctx.shadowColor = pathCol;
      ctx.shadowBlur = 16;
      ctx.beginPath();
      ctx.arc(nx, ny - 4, 5, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;
    }
  }, [data, animLayer]);

  const active = data ? animLayer : layer;

  return (
    <div className="hero-print" data-synthetic aria-label="Illustrative 2D layer preview of examples/t-wing.stl">
      <canvas ref={canvasRef} className="hero-layer-canvas" />
      <div className="hero-print__chrome">
        <span className="hero-print__tag">Illustrative · t-wing.stl</span>
        <span className="hero-print__layer">
          Layer {active + 1}
          {data ? ` / ${data.layers.length}` : ""}
        </span>
      </div>
    </div>
  );
}