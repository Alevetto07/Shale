/**
 * Marketing-site Andromeda field — inspired by the desktop start screen ASCII
 * painter, using the same NASA M31 reference image shipped with the app.
 */

import { useEffect, useRef } from "react";

const RAMP = " .:-=+*#%@";
const CELL = 11;
const BG = [6, 7, 11] as const;

function prefersReducedMotion(): boolean {
  if (typeof window === "undefined") return true;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function AndromedaField() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const reduced = prefersReducedMotion();
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) return;

    const image = new Image();
    image.src = "/media/andromeda.jpg";
    let cancelled = false;
    let raf = 0;
    let cols = 0;
    let rows = 0;
    let baseGlyph: Uint8Array = new Uint8Array(0);
    let baseR: Uint8Array = new Uint8Array(0);
    let baseG: Uint8Array = new Uint8Array(0);
    let baseB: Uint8Array = new Uint8Array(0);

    const paintStill = () => {
      if (!image.complete || image.naturalWidth < 1) return;
      const rect = canvas.getBoundingClientRect();
      const w = Math.max(1, Math.floor(rect.width));
      const h = Math.max(1, Math.floor(rect.height));
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }
      cols = Math.max(1, Math.ceil(w / CELL));
      rows = Math.max(1, Math.ceil(h / CELL));
      const count = cols * rows;

      const sample = document.createElement("canvas");
      const iw = image.width;
      const ih = image.height;
      sample.width = iw;
      sample.height = ih;
      const source = sample.getContext("2d", { willReadFrequently: true });
      if (!source) return;
      source.drawImage(image, 0, 0);
      const pixels = source.getImageData(0, 0, iw, ih).data;

      const fit = Math.max(w / iw, h / ih) * 1.05;
      const drawW = iw * fit;
      const drawH = ih * fit;
      const originX = (w - drawW) * 0.5;
      const originY = (h - drawH) * 0.5;

      baseGlyph = new Uint8Array(count);
      baseR = new Uint8Array(count);
      baseG = new Uint8Array(count);
      baseB = new Uint8Array(count);

      context.fillStyle = `rgb(${BG[0]}, ${BG[1]}, ${BG[2]})`;
      context.fillRect(0, 0, w, h);
      context.font = `${CELL}px "Geist Mono", ui-monospace, monospace`;
      context.textBaseline = "top";

      for (let row = 0; row < rows; row++) {
        for (let col = 0; col < cols; col++) {
          const x = col * CELL + originX;
          const y = row * CELL + originY;
          const cx = Math.floor(((col + 0.5) * CELL - originX) / fit);
          const cy = Math.floor(((row + 0.5) * CELL - originY) / fit);
          const i = row * cols + col;
          if (cx < 0 || cy < 0 || cx >= iw || cy >= ih) {
            baseGlyph[i] = 0;
            continue;
          }
          const p = (cy * iw + cx) * 4;
          const r = pixels[p];
          const g = pixels[p + 1];
          const b = pixels[p + 2];
          const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
          const step = Math.min(
            RAMP.length - 1,
            Math.floor((lum / 255) * RAMP.length),
          );
          baseGlyph[i] = step;
          baseR[i] = r;
          baseG[i] = g;
          baseB[i] = b;
        }
      }

      for (let row = 0; row < rows; row++) {
        for (let col = 0; col < cols; col++) {
          const i = row * cols + col;
          const step = baseGlyph[i];
          if (step === 0) continue;
          const ch = RAMP[step];
          const px = col * CELL;
          const py = row * CELL;
          const r = baseR[i];
          const g = baseG[i];
          const b = baseB[i];
          context.fillStyle = `rgba(${r}, ${g}, ${b}, ${0.35 + step * 0.08})`;
          context.fillText(ch, px, py);
        }
      }
    };

    const paintAnimated = (time: number) => {
      paintStill();
      const w = canvas.width;
      const h = canvas.height;
      const driftX = Math.sin(time * 0.00012) * 6;
      const driftY = Math.cos(time * 0.00009) * 4;
      context.font = `${CELL}px "Geist Mono", ui-monospace, monospace`;
      context.textBaseline = "top";

      for (let row = 0; row < rows; row++) {
        for (let col = 0; col < cols; col++) {
          const i = row * cols + col;
          const step = baseGlyph[i];
          if (step < 3) continue;
          const twinkle =
            0.85 +
            0.15 * Math.sin(time * 0.002 + col * 0.4 + row * 0.31 + step);
          const ch = RAMP[Math.min(RAMP.length - 1, step + (twinkle > 0.97 ? 1 : 0))];
          const px = col * CELL + driftX * (step / RAMP.length);
          const py = row * CELL + driftY * (step / RAMP.length);
          context.fillStyle = `rgba(${baseR[i]}, ${baseG[i]}, ${baseB[i]}, ${twinkle * 0.5})`;
          context.fillText(ch, px, py);
        }
      }
      raf = requestAnimationFrame(paintAnimated);
    };

    const start = () => {
      if (cancelled) return;
      paintStill();
      if (!reduced) raf = requestAnimationFrame(paintAnimated);
    };

    const onResize = () => {
      if (reduced) paintStill();
    };

    image.onload = () => start();
    if (image.complete) start();
    window.addEventListener("resize", onResize);

    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", onResize);
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      className="andromeda-canvas"
      aria-hidden="true"
    />
  );
}