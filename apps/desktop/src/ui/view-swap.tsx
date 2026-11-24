import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { useReducedMotion } from "./use-reduced-motion";

/**
 * Home, the workspace and Device fade in. Prepare and Preview share one
 * surface so the plate is not recreated. A plain div, not a motion component:
 * motion's press tracking re-rendered the plate on every camera drag.
 */
export function ViewSwap({ view, children }: { view: string; children: ReactNode }) {
  const reduce = useReducedMotion();
  return (
    <div key={view} className={reduce ? "app-surface" : "app-surface app-surface--enter"}>
      {children}
    </div>
  );
}

/**
 * Sidebar cross-fade between Prepare and Preview. The plate stays mounted, so
 * the toolpath is not rebuilt just because the headings changed.
 */
export function PaneFade({ pane, children }: { pane: string; children: ReactNode }) {
  const reduce = useReducedMotion();
  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.div
        key={pane}
        className="pane-fade"
        initial={reduce ? { opacity: 1, y: 0 } : { opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        exit={reduce ? { opacity: 1, y: 0 } : { opacity: 0, y: -6 }}
        transition={reduce ? { duration: 0 } : { duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
      >
        {children}
      </motion.div>
    </AnimatePresence>
  );
}
