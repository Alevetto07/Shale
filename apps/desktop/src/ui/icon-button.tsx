import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import * as Tooltip from "@radix-ui/react-tooltip";
import { cn } from "./cn";

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  shortcut?: string;
  notify?: boolean;
  children: ReactNode;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, shortcut, notify, className, children, ...props },
  ref,
) {
  const hint = shortcut ? `${label} (${shortcut})` : label;
  return (
    <Tooltip.Root delayDuration={300}>
      <Tooltip.Trigger asChild>
        <button
          ref={ref}
          type="button"
          className={cn("ui-icon-btn", className)}
          aria-label={label}
          {...props}
        >
          {children}
          {notify ? <span className="ui-icon-btn__dot" aria-hidden /> : null}
        </button>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content className="ui-tooltip" sideOffset={6}>
          {hint}
          <Tooltip.Arrow className="ui-tooltip__arrow" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
});