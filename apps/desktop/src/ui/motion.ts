import { springPanel, springTab } from "./springs";

/** Ease already defined as `--ease-out` in tokens.css. Do not invent a second curve. */
export const EASE_OUT = [0.22, 1, 0.36, 1] as const;

export const SIDEBAR_EXPANDED = 268;
export const SIDEBAR_RAIL = 52;

export { springPanel, springTab };

export function sidebarSpan(collapsed: boolean): number {
  return collapsed ? SIDEBAR_RAIL : SIDEBAR_EXPANDED;
}

/**
 * The main column's layout width commits once (so the WebGL canvas resizes
 * once). This offset keeps that column visually pinned to the sidebar edge
 * while the clip animates, and it is applied as a transform.
 */
export function shellShift(animatedWidth: number, layoutSpan: number): number {
  return animatedWidth - layoutSpan;
}

export function viewMotion(reduce: boolean): {
  initial: { opacity: number; y: number };
  animate: { opacity: number; y: number };
  exit: { opacity: number; y: number };
  transition: { duration: number; ease: readonly [number, number, number, number] };
} {
  if (reduce) {
    return {
      initial: { opacity: 1, y: 0 },
      animate: { opacity: 1, y: 0 },
      exit: { opacity: 1, y: 0 },
      transition: { duration: 0, ease: EASE_OUT },
    };
  }
  return {
    initial: { opacity: 0, y: 10 },
    animate: { opacity: 1, y: 0 },
    exit: { opacity: 0, y: -8 },
    transition: { duration: 0.22, ease: EASE_OUT },
  };
}

export function isPaletteShortcut(event: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}): boolean {
  return event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey) && !event.altKey;
}
