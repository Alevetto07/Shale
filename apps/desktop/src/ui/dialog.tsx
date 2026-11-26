import * as DialogPrimitive from "@radix-ui/react-dialog";
import { motion } from "motion/react";
import type { ReactNode } from "react";
import { cn } from "./cn";
import { springPanel } from "./springs";
import { useReducedMotion } from "./use-reduced-motion";

function keepPortaledMenus(event: { preventDefault: () => void; target: EventTarget | null }) {
  const target = event.target as HTMLElement | null;
  if (target?.closest(".ui-dropdown, .ui-tooltip, .ui-select__menu")) event.preventDefault();
}

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  kicker,
  children,
  className,
  dismissOnOutside = true,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  kicker?: string;
  children: ReactNode;
  className?: string;
  /** Wizard stays up if the pointer lands on the scrim. Escape still closes. */
  dismissOnOutside?: boolean;
}) {
  const reduce = useReducedMotion();
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="ui-dialog-overlay" />
        <DialogPrimitive.Content
          className={cn("ui-dialog", className)}
          onInteractOutside={(event) => {
            keepPortaledMenus(event);
            if (!dismissOnOutside) event.preventDefault();
          }}
          onFocusOutside={(event) => {
            keepPortaledMenus(event);
          }}
        >
          <motion.div
            className="ui-dialog__motion"
            initial={reduce ? false : { y: 8 }}
            animate={{ y: 0 }}
            transition={reduce ? { duration: 0 } : springPanel}
          >
            {kicker ? <p className="ui-dialog__kicker">{kicker}</p> : null}
            <DialogPrimitive.Title className="ui-dialog__title">{title}</DialogPrimitive.Title>
            {description ? (
              <DialogPrimitive.Description className="ui-dialog__desc">{description}</DialogPrimitive.Description>
            ) : (
              <DialogPrimitive.Description className="visually-hidden">{title}</DialogPrimitive.Description>
            )}
            {children}
          </motion.div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
