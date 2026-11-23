import { useReducedMotion as useMotionReduced } from "motion/react";

/** True when the user prefers reduced motion (OS or Motion). */
export function useReducedMotion(): boolean {
  return useMotionReduced() ?? false;
}