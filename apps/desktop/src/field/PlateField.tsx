import { useEffect, useLayoutEffect, useRef } from "react";
import fieldUrl from "./andromeda.jpg";
import { pointer, prefersStill, subscribe } from "./loop";
import { createField, type FieldPainter } from "./paint";

/**
 * ASCII Andromeda. Glyphs take their ramp step and tint from the NASA image.
 * The field drifts slowly. Near the pointer a glyph steps up the ramp,
 * brightens, and eases aside, then springs back. A short comet of star
 * glyphs follows the pointer on the start screen only.
 *
 * The painter is bound once, on mount. `live` and `comet` are read through refs
 * inside the shared clock, so opening a panel never rebuilds the painter — and
 * the canvas node is re-bound if React ever replaces it, which leaves a fresh
 * node blank until the next still frame.
 */
export function PlateField({ live, comet }: { live: boolean; comet: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const liveRef = useRef(live);
  const cometRef = useRef(comet);
  liveRef.current = live;
  cometRef.current = comet;
  const fieldRef = useRef<FieldPainter | null>(null);
  const boundRef = useRef<HTMLCanvasElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const paintStillRef = useRef<() => void>(() => {});

  useEffect(() => {
    const image = new Image();
    image.src = fieldUrl;
    imageRef.current = image;
    let cancelled = false;
    let unsubscribe = () => {};

    const bind = (node: HTMLCanvasElement) => {
      boundRef.current = node;
      const field = createField(node, image);
      fieldRef.current = field;
      paintStillRef.current = () => field.paint(0, -1e6, -1e6, false);
      paintStillRef.current();
    };

    const start = () => {
      const node = ref.current;
      if (cancelled || !node) return;
      bind(node);
      if (prefersStill()) return;
      let bounds = node.getBoundingClientRect();
      const onResize = () => {
        if (ref.current) bounds = ref.current.getBoundingClientRect();
      };
      window.addEventListener("resize", onResize);
      unsubscribe = subscribe((time) => {
        const live = ref.current;
        if (!live) return;
        if (live !== boundRef.current) bind(live);
        if (!liveRef.current) return;
        fieldRef.current?.paint(time, pointer.x - bounds.left, pointer.y - bounds.top, cometRef.current);
      });
      const stop = unsubscribe;
      unsubscribe = () => {
        stop();
        window.removeEventListener("resize", onResize);
      };
    };

    const ready = () => {
      const decoded = typeof image.decode === "function" ? image.decode().catch(() => {}) : Promise.resolve();
      void decoded.then(() => {
        if (cancelled) return;
        if (document.fonts?.ready) {
          void document.fonts.ready.then(() => {
            if (!cancelled) start();
          });
        } else {
          start();
        }
      });
    };

    if (image.complete && image.naturalWidth > 0) {
      ready();
    } else {
      image.onload = () => {
        if (!cancelled) ready();
      };
    }

    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, []);

  // A panel switch can leave a freshly mounted canvas untouched; repaint a still
  // frame whenever the bitmap turns out to be empty.
  useLayoutEffect(() => {
    const node = ref.current;
    const field = fieldRef.current;
    if (!node || !field || node.width < 2 || node.height < 2) return;
    if (node !== boundRef.current) {
      const image = imageRef.current;
      if (!image) return;
      boundRef.current = node;
      const rebound = createField(node, image);
      fieldRef.current = rebound;
      paintStillRef.current = () => rebound.paint(0, -1e6, -1e6, false);
      paintStillRef.current();
      return;
    }
    const context = node.getContext("2d");
    if (!context) return;
    const probe = context.getImageData(node.width >> 1, node.height >> 1, 1, 1).data;
    if (probe[3] === 0) paintStillRef.current();
  });

  return <canvas ref={ref} className="plate-field" aria-hidden="true" />;
}
