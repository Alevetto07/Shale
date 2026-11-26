import * as Tooltip from "@radix-ui/react-tooltip";
import type { ReactNode } from "react";

/** Mount once near the app root so icon buttons can show shortcuts. */
export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <Tooltip.Provider delayDuration={300} skipDelayDuration={0}>
      {children}
    </Tooltip.Provider>
  );
}