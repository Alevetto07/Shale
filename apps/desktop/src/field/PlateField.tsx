import { useEffect, useRef } from "react";
import fieldUrl from "./hubble-deep-field.jpg";
import { pointer, prefersStill, subscribe } from "./loop";

const RAMP = " .:-=+*#%@";
const CELL = 9;

/**
 * ASCII plate of the 1995 Hubble Deep Field. Each cell takes its ramp step
 * and its tint from the source pixel. The field drifts slowly. Near the
 * pointer the glyph steps up the ramp, brightens, and eases aside, then
 * springs back. Reduced motion and a paused plate paint one still frame.
 */
export function PlateField({ live }: { live: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;

    let cancelled = false;
    let unsubscribe = () => {};
    const image = new Image();
    image.src = fieldUrl;

    image.onload = () => {
      if (cancelled) return;
      const sample = document.createElement("canvas");
      sample.width = image.width;
      sample.height = image.height;
      const source = sample.getContext("2d", { willReadFrequently: true });
      if (!source) return;
      source.drawImage(image, 0, 0);
      const pixels = source.getImageData(0, 0, image.width, image.height);
      const data = pixels.data;
      const iw = image.width;
      const ih = image.height;

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
        if (canvas.width !== nextW || canvas.height !== nextH) {
          canvas.width = nextW;
          canvas.height = nextH;
        }
        if (nextW !== width || nextH !== height) {
          width = nextW;
          height = nextH;
          cols = Math.max(1, Math.ceil(width / CELL));
          rows = Math.max(1, Math.ceil(height / CELL));
          offsetX = new Float32Array(cols * rows);
          offsetY = new Float32Array(cols * rows);
        }
      };

      const paint = (time: number, animate: boolean) => {
        resize();
        context.fillStyle = "#06070b";
        context.fillRect(0, 0, width, height);
        context.font = "11px 'Geist Mono', ui-monospace, monospace";
        context.textAlign = "center";
        context.textBaseline = "middle";

        const fit = Math.min(width / iw, height / ih);
        const drawW = iw * fit;
        const drawH = ih * fit;
        const originX = (width - drawW) / 2;
        const shift = animate ? (time * 0.006) % drawH : 0;
        const bounds = canvas.getBoundingClientRect();
        const px = pointer.x - bounds.left;
        const py = pointer.y - bounds.top;

        for (let row = 0; row < rows; row += 1) {
          for (let col = 0; col < cols; col += 1) {
            const index = row * cols + col;
            let cx = col * CELL + CELL / 2;
            let cy = row * CELL + CELL / 2;
            const localX = cx - originX;
            const localY = ((cy - (height - drawH) / 2 + shift) % drawH + drawH) % drawH;
            if (localX < 0 || localX >= drawW) {
              offsetX[index] = 0;
              offsetY[index] = 0;
              continue;
            }
            const sx = Math.min(iw - 1, Math.floor((localX / drawW) * iw));
            const sy = Math.min(ih - 1, Math.floor((localY / drawH) * ih));
            const pixel = (sy * iw + sx) * 4;
            let red = data[pixel];
            let green = data[pixel + 1];
            let blue = data[pixel + 2];
            const luma = Math.pow((0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255, 0.45);
            const dx = cx - px;
            const dy = cy - py;
            const dist = Math.hypot(dx, dy);
            const influence = animate && dist < 120 ? (1 - dist / 120) ** 2 : 0;
            const desiredX = dist > 0 ? (dx / dist) * influence * 14 : 0;
            const desiredY = dist > 0 ? (dy / dist) * influence * 14 : 0;
            if (animate) {
              offsetX[index] += (desiredX - offsetX[index]) * 0.16;
              offsetY[index] += (desiredY - offsetY[index]) * 0.16;
            } else {
              offsetX[index] = 0;
              offsetY[index] = 0;
            }
            let step = Math.round(luma * (RAMP.length - 1));
            step = Math.min(RAMP.length - 1, step + Math.round(influence * 5));
            if (step <= 0 || cy > height - 78) continue;
            const gain = 1 + influence * 1.35;
            red = Math.min(255, red * gain);
            green = Math.min(255, green * gain);
            blue = Math.min(255, blue * gain);
            cx += offsetX[index];
            cy += offsetY[index];
            if (red + green + blue < 24) {
              context.fillStyle = `rgba(233, 232, 242, ${(0.22 + influence * 0.5).toFixed(3)})`;
            } else {
              context.fillStyle = `rgb(${red | 0}, ${green | 0}, ${blue | 0})`;
            }
            context.fillText(RAMP[step], cx, cy);
          }
        }
      };

      paint(0, false);
      if (!live || prefersStill()) return;
      let last = 0;
      unsubscribe = subscribe((time) => {
        if (time - last < 32) return;
        last = time;
        paint(time, true);
      });
    };

    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [live]);

  return <canvas ref={ref} className="plate-field" aria-hidden="true" />;
}
