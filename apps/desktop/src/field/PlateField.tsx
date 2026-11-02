import { useEffect, useRef } from "react";
import { pointer, prefersStill, subscribe } from "./loop";

const RAMP = " .:-=+*#%@";
const CELL = 16;

/**
 * Drifting star field for the empty plate. Glyphs near the pointer climb the
 * ramp and ease aside, then spring back. A still frame is painted when the
 * plate is paused or the user prefers reduced motion.
 */
export function PlateField({ live }: { live: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;

    let width = 0;
    let height = 0;
    let cols = 0;
    let rows = 0;
    let offsetX = new Float32Array(0);
    let offsetY = new Float32Array(0);

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const nextW = Math.max(1, Math.floor(rect.width));
      const nextH = Math.max(1, Math.floor(rect.height));
      const ratio = 1;
      canvas.width = Math.floor(nextW * ratio);
      canvas.height = Math.floor(nextH * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      if (nextW !== width || nextH !== height) {
        width = nextW;
        height = nextH;
        cols = Math.max(1, Math.ceil(width / CELL));
        rows = Math.max(1, Math.ceil(height / CELL));
        offsetX = new Float32Array(cols * rows);
        offsetY = new Float32Array(cols * rows);
      }
    };

    const baseIndex = (col: number, row: number) => {
      const n = Math.sin(col * 127.1 + row * 311.7) * 43758.5453;
      const hash = n - Math.floor(n);
      if (hash > 0.16) return 0;
      return 1 + Math.floor(hash * 18);
    };

    const paint = (time: number, animate: boolean) => {
      resize();
      context.clearRect(0, 0, width, height);
      context.font = "13px 'Geist Mono', ui-monospace, monospace";
      context.textAlign = "center";
      context.textBaseline = "middle";
      const drift = animate ? (time * 0.008) % (rows * CELL) : 0;
      const bounds = canvas.getBoundingClientRect();
      const px = pointer.x - bounds.left;
      const py = pointer.y - bounds.top;
      const caption = { x: width * 0.5, y: height - 36, hw: 220, hh: 28 };

      for (let row = 0; row < rows; row += 1) {
        for (let col = 0; col < cols; col += 1) {
          const index = row * cols + col;
          const seed = baseIndex(col, Math.floor(row + drift / CELL));
          let cx = col * CELL + CELL / 2;
          let cy = ((row * CELL + drift) % (rows * CELL)) + CELL / 2;
          const dx = cx - px;
          const dy = cy - py;
          const dist = Math.hypot(dx, dy);
          const influence = dist < 120 ? (1 - dist / 120) ** 2 : 0;
          const desiredX = dist > 0 ? (dx / dist) * influence * 16 : 0;
          const desiredY = dist > 0 ? (dy / dist) * influence * 16 : 0;
          if (animate) {
            offsetX[index] += (desiredX - offsetX[index]) * 0.16;
            offsetY[index] += (desiredY - offsetY[index]) * 0.16;
          } else {
            offsetX[index] = 0;
            offsetY[index] = 0;
          }
          const ramp = Math.min(RAMP.length - 1, Math.round(seed + influence * (RAMP.length - 1)));
          if (ramp <= 0) continue;
          cx += offsetX[index];
          cy += offsetY[index];
          if (Math.abs(cx - caption.x) < caption.hw && Math.abs(cy - caption.y) < caption.hh) continue;
          const alpha = 0.16 + (ramp / (RAMP.length - 1)) * 0.78;
          context.fillStyle = `rgba(233, 232, 242, ${alpha.toFixed(3)})`;
          context.fillText(RAMP[ramp] === " " ? "" : RAMP[ramp], cx, cy);
        }
      }
    };

    paint(0, false);
    if (!live || prefersStill()) return;
    let last = 0;
    return subscribe((time) => {
      if (time - last < 32) return;
      last = time;
      paint(time, true);
    });
  }, [live]);

  return <canvas ref={ref} className="plate-field" aria-hidden="true" />;
}
