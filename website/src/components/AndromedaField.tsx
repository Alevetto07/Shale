/**
 * Hero Andromeda — luminance-mapped ASCII of NASA M31, same ramp and gamma as the app.
 * The full galaxy is letterboxed inside the canvas (contain), never cover-cropped.
 */

import { useEffect, useRef } from "react";

const RAMP = " .:-=+*#%@";
const CELL = 8;
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
    let pixels: Uint8ClampedArray | null = null;
    let iw = 0;
    let ih = 0;
    let baseGlyph = new Uint8Array(0);
    let baseR = new Uint8Array(0);
    let baseG = new Uint8Array(0);
    let baseB = new Uint8Array(0);
    let twinkleOn = new Uint8Array(0);
    let twinkleRate = new Float32Array(0);
    let twinklePhase = new Float32Array(0);
    let twinkleAmp = new Uint8Array(0);
    let twinkleList: number[] = [];
    let lastTwinkleTick = 0;
    let fitted = { originX: 0, originY: 0, drawW: 1, drawH: 1 };
    let width = 0;
    let height = 0;

    const layout = () => {
      if (!image.complete || image.naturalWidth < 1 || !pixels) return false;
      const rect = canvas.getBoundingClientRect();
      const w = Math.max(1, Math.floor(rect.width));
      const h = Math.max(1, Math.floor(rect.height));
      if (w === width && h === height && cols > 0) return false;
      width = w;
      height = h;
      canvas.width = w;
      canvas.height = h;
      cols = Math.max(1, Math.ceil(w / CELL));
      rows = Math.max(1, Math.ceil(h / CELL));
      const count = cols * rows;

      const fit = Math.min(w / iw, h / ih) * 0.96;
      const drawW = iw * fit;
      const drawH = ih * fit;
      fitted = {
        drawW,
        drawH,
        originX: (w - drawW) / 2,
        originY: (h - drawH) / 2,
      };

      baseGlyph = new Uint8Array(count);
      baseR = new Uint8Array(count);
      baseG = new Uint8Array(count);
      baseB = new Uint8Array(count);
      twinkleOn = new Uint8Array(count);
      twinkleRate = new Float32Array(count);
      twinklePhase = new Float32Array(count);
      twinkleAmp = new Uint8Array(count);
      twinkleList = [];

      for (let row = 0; row < rows; row++) {
        for (let col = 0; col < cols; col++) {
          const hash = ((col * 73856093) ^ (row * 19349663)) >>> 0;
          if (hash % 89 !== 0) continue;
          const index = row * cols + col;
          twinkleList.push(index);
          const wobble = (hash >>> 8) % 1000;
          twinkleRate[index] = 0.0009 + (wobble / 1000) * 0.0022;
          twinklePhase[index] = ((hash >>> 3) % 628) / 100;
          twinkleAmp[index] = wobble % 3 === 0 ? 2 : 1;
        }
      }

      return true;
    };

    const sampleField = () => {
      const { originX, originY, drawW, drawH } = fitted;
      for (let row = 0; row < rows; row++) {
        for (let col = 0; col < cols; col++) {
          const index = row * cols + col;
          const cx = col * CELL + CELL / 2;
          const cy = row * CELL + CELL / 2;
          if (
            cx < originX ||
            cx > originX + drawW ||
            cy < originY ||
            cy > originY + drawH
          ) {
            baseGlyph[index] = 0;
            continue;
          }
          const localX = cx - originX;
          const localY = cy - originY;
          const sx = Math.min(
            iw - 1,
            Math.max(0, Math.floor((localX / drawW) * iw)),
          );
          const sy = Math.min(
            ih - 1,
            Math.max(0, Math.floor((localY / drawH) * ih)),
          );
          const pixel = (sy * iw + sx) * 4;
          const red = pixels![pixel];
          const green = pixels![pixel + 1];
          const blue = pixels![pixel + 2];
          const luma = Math.pow(
            (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255,
            0.45,
          );
          const step = Math.round(luma * (RAMP.length - 1));
          baseGlyph[index] = step;
          baseR[index] = red;
          baseG[index] = green;
          baseB[index] = blue;
        }
      }
    };

    const paint = (time: number) => {
      if (!pixels) return;
      const resized = layout();
      if (resized) sampleField();

      const driftX = reduced ? 0 : Math.sin(time * 0.0001) * 5;
      const driftY = reduced ? 0 : Math.cos(time * 0.000085) * 4;

      if (!reduced && twinkleList.length && time - lastTwinkleTick >= 125) {
        lastTwinkleTick = time;
        for (const index of twinkleList) {
          if (baseGlyph[index] < 3) continue;
          const on =
            Math.sin(time * twinkleRate[index] + twinklePhase[index]) > 0.74
              ? 1
              : 0;
          twinkleOn[index] = on;
        }
      }

      context.fillStyle = `rgb(${BG[0]}, ${BG[1]}, ${BG[2]})`;
      context.fillRect(0, 0, width, height);
      context.font = `${CELL}px "Geist Mono", ui-monospace, monospace`;
      context.textBaseline = "top";

      for (let row = 0; row < rows; row++) {
        for (let col = 0; col < cols; col++) {
          const index = row * cols + col;
          let step = baseGlyph[index];
          const lit = twinkleOn[index] === 1;
          if (step <= 0 && !lit) continue;
          if (lit && step > 0) {
            step = Math.min(RAMP.length - 1, step + twinkleAmp[index]);
          }
          if (step <= 0) continue;

          const gain = lit ? 1.4 : 1.15;
          let red = Math.min(255, baseR[index] * gain);
          let green = Math.min(255, baseG[index] * gain);
          let blue = Math.min(255, baseB[index] * gain);
          if (red + green + blue < 32) {
            const ink = 0.38 + (lit ? 0.32 : 0);
            red = 233 * ink;
            green = 232 * ink;
            blue = 242 * ink;
          }

          const t = step / (RAMP.length - 1);
          const alpha =
            step === 1 ? 0.22 + t * 0.15 : 0.45 + t * 0.55;
          const ch = RAMP[step];
          const px = col * CELL + driftX * t;
          const py = row * CELL + driftY * t;
          context.fillStyle = `rgba(${red | 0}, ${green | 0}, ${blue | 0}, ${alpha})`;
          context.fillText(ch, px, py);
        }
      }
    };

    const loop = (time: number) => {
      if (cancelled) return;
      paint(time);
      raf = requestAnimationFrame(loop);
    };

    const start = () => {
      if (cancelled) return;
      iw = image.width;
      ih = image.height;
      const sample = document.createElement("canvas");
      sample.width = iw;
      sample.height = ih;
      const source = sample.getContext("2d", { willReadFrequently: true });
      if (!source) return;
      source.drawImage(image, 0, 0);
      pixels = source.getImageData(0, 0, iw, ih).data;

      layout();
      sampleField();
      paint(0);
      if (!reduced) raf = requestAnimationFrame(loop);
    };

    const onResize = () => {
      paint(reduced ? 0 : performance.now());
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