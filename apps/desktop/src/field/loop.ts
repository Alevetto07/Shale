/** One animation clock for the start-screen field. It does not run while hidden. */

type Frame = (time: number) => void;

const listeners = new Set<Frame>();
let handle = 0;
let hidden = typeof document !== "undefined" && document.hidden;

const motion = () =>
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export function prefersStill(): boolean {
  return motion();
}

export const pointer = { x: -1e6, y: -1e6 };

let pointerBound = false;

function bindPointer() {
  if (pointerBound || typeof window === "undefined") return;
  pointerBound = true;
  window.addEventListener(
    "pointermove",
    (event) => {
      pointer.x = event.clientX;
      pointer.y = event.clientY;
    },
    { passive: true },
  );
  document.addEventListener("visibilitychange", () => {
    hidden = document.hidden;
    if (hidden) stop();
    else kick();
  });
}

function tick(time: number) {
  handle = requestAnimationFrame(tick);
  for (const listener of listeners) listener(time);
}

function kick() {
  if (handle || hidden || motion() || listeners.size === 0) return;
  handle = requestAnimationFrame(tick);
}

function stop() {
  if (!handle) return;
  cancelAnimationFrame(handle);
  handle = 0;
}

/** Subscribe to the shared loop. Returns an unsubscribe. No loop is started for reduced motion. */
export function subscribe(listener: Frame): () => void {
  bindPointer();
  if (motion()) return () => {};
  listeners.add(listener);
  kick();
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) stop();
  };
}
