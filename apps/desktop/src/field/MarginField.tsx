import { type RefObject, useEffect, useRef } from "react";
import { pointer, prefersStill, subscribe } from "./loop";

const TWINKLE = [".", "·", "+", "*", "✧"];

interface Star {
  x: number;
  y: number;
  glyph: number;
  phase: number;
}

interface Spark {
  x: number;
  y: number;
  born: number;
}

/**
 * Sparse stars in the window margin, plus a comet that follows the pointer
 * only outside the plate. Both share the start-screen clock and hold still
 * when that clock is not running.
 */
export function MarginField({
  live,
  comet,
  plateRef,
}: {
  live: boolean;
  comet: boolean;
  plateRef: RefObject<HTMLElement | null>;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const stars = useRef<Star[]>([]);
  const trail = useRef<Spark[]>([]);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;

    const margin = 14;
    const layout = () => {
      const ratio = 1;
      canvas.width = Math.floor(window.innerWidth * ratio);
      canvas.height = Math.floor(window.innerHeight * ratio);
      canvas.style.width = `${window.innerWidth}px`;
      canvas.style.height = `${window.innerHeight}px`;
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      const next: Star[] = [];
      const bands = [
        { x: 0, y: 0, w: window.innerWidth, h: margin },
        { x: 0, y: window.innerHeight - margin, w: window.innerWidth, h: margin },
        { x: 0, y: margin, w: margin, h: window.innerHeight - margin * 2 },
        { x: window.innerWidth - margin, y: margin, w: margin, h: window.innerHeight - margin * 2 },
      ];
      let n = 0;
      for (const band of bands) {
        const count = Math.max(2, Math.round((band.w * band.h) / 1400));
        for (let i = 0; i < count; i += 1) {
          n += 1;
          const hash = Math.sin(n * 91.7) * 43758.5453;
          const unit = hash - Math.floor(hash);
          const hash2 = Math.sin(n * 37.3 + 4.2) * 24634.6345;
          const unit2 = hash2 - Math.floor(hash2);
          next.push({
            x: band.x + unit * band.w,
            y: band.y + unit2 * band.h,
            glyph: n % TWINKLE.length,
            phase: unit * Math.PI * 2,
          });
        }
      }
      stars.current = next;
    };

    const overPlate = (x: number, y: number) => {
      const plate = plateRef.current;
      if (!plate) return false;
      const rect = plate.getBoundingClientRect();
      return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    };

    const paint = (time: number, animate: boolean) => {
      const width = window.innerWidth;
      const height = window.innerHeight;
      context.clearRect(0, 0, width, height);
      context.font = "12px 'Geist Mono', ui-monospace, monospace";
      context.textAlign = "center";
      context.textBaseline = "middle";
      for (const star of stars.current) {
        const twinkle = animate ? 0.55 + 0.45 * Math.sin(time * 0.0016 + star.phase) : 0.7;
        const glyph = animate ? TWINKLE[Math.floor((star.phase + time * 0.0004) % TWINKLE.length)] : TWINKLE[star.glyph];
        context.fillStyle = `rgba(233, 232, 242, ${(0.14 * twinkle).toFixed(3)})`;
        context.fillText(glyph, star.x, star.y);
      }

      if (comet && animate && !overPlate(pointer.x, pointer.y)) {
        const last = trail.current[trail.current.length - 1];
        if (!last || Math.hypot(last.x - pointer.x, last.y - pointer.y) > 6) {
          trail.current.push({ x: pointer.x, y: pointer.y, born: time });
          if (trail.current.length > 22) trail.current.shift();
        }
      }
      const kept: Spark[] = [];
      for (const spark of trail.current) {
        const age = animate ? (time - spark.born) / 520 : 0.2;
        if (age > 1 || overPlate(spark.x, spark.y)) continue;
        kept.push(spark);
        context.fillStyle = `rgba(233, 232, 242, ${(0.45 * (1 - age)).toFixed(3)})`;
        context.fillRect(spark.x - 1.2, spark.y - 1.2, 2.4, 2.4);
      }
      trail.current = kept;
    };

    layout();
    paint(0, false);
    const onResize = () => {
      layout();
      paint(0, false);
    };
    window.addEventListener("resize", onResize);
    if (!live || prefersStill()) {
      return () => window.removeEventListener("resize", onResize);
    }
    let last = 0;
    const unsubscribe = subscribe((time) => {
      if (time - last < 32) return;
      last = time;
      paint(time, true);
    });
    return () => {
      unsubscribe();
      window.removeEventListener("resize", onResize);
    };
  }, [live, comet, plateRef]);

  return <canvas ref={ref} className="margin-field" aria-hidden="true" />;
}
