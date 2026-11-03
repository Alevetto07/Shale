import { useEffect, useRef } from "react";
import fieldUrl from "./andromeda.jpg";
import { pointer, prefersStill, subscribe } from "./loop";
import { createField } from "./paint";

/**
 * ASCII Andromeda. Glyphs take their ramp step and tint from the NASA image.
 * The field drifts slowly. Near the pointer a glyph steps up the ramp,
 * brightens, and eases aside, then springs back. A short comet of star
 * glyphs follows the pointer on the start screen only.
 */
export function PlateField({ live, comet }: { live: boolean; comet: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const liveRef = useRef(live);
  const cometRef = useRef(comet);
  liveRef.current = live;
  cometRef.current = comet;

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    let cancelled = false;
    let unsubscribe = () => {};
    const image = new Image();
    image.src = fieldUrl;
    image.onload = () => {
      if (cancelled) return;
      const field = createField(canvas, image);
      const paintStill = () => field.paint(0, -1e6, -1e6, false);
      const start = () => {
        paintStill();
        if (!liveRef.current || prefersStill()) return;
        let bounds = canvas.getBoundingClientRect();
        const onResize = () => {
          bounds = canvas.getBoundingClientRect();
        };
        window.addEventListener("resize", onResize);
        unsubscribe = subscribe((time) => {
          if (!liveRef.current) return;
          field.paint(time, pointer.x - bounds.left, pointer.y - bounds.top, cometRef.current);
        });
        const stop = unsubscribe;
        unsubscribe = () => {
          stop();
          window.removeEventListener("resize", onResize);
        };
      };
      if (document.fonts?.ready) {
        void document.fonts.ready.then(() => {
          if (!cancelled) start();
        });
      } else {
        start();
      }
    };
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [live]);

  return <canvas ref={ref} className="plate-field" aria-hidden="true" />;
}
