import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { viewMotion } from "./motion";
import { useReducedMotion } from "./use-reduced-motion";

/** Home ↔ workspace: cross-fade plus a 10px translate, 220ms. Tab bodies do not use this. */
export function ViewSwap({ view, children }: { view: string; children: ReactNode }) {
  const reduce = useReducedMotion();
  const motionProps = viewMotion(reduce);
  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.div
        key={view}
        className="app-surface"
        initial={motionProps.initial}
        animate={motionProps.animate}
        exit={motionProps.exit}
        transition={motionProps.transition}
      >
        {children}
      </motion.div>
    </AnimatePresence>
  );
}
