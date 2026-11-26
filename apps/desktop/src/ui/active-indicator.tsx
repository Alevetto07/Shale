import { motion } from "motion/react";
import { springTab } from "./motion";
import { useReducedMotion } from "./use-reduced-motion";

export function ActiveIndicator({ layoutId, tone }: { layoutId: string; tone: "pane" | "settings" }) {
  const reduce = useReducedMotion();
  if (reduce) {
    return <span className="ui-active-indicator" data-tone={tone} data-reduced="1" />;
  }
  return (
    <motion.span
      className="ui-active-indicator"
      data-tone={tone}
      layoutId={layoutId}
      transition={springTab}
    />
  );
}
