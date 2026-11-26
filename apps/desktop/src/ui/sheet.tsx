import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { motion } from "motion/react";
import type { ReactNode } from "react";
import { springPanel } from "./springs";
import { useReducedMotion } from "./use-reduced-motion";

function keepPortaledMenus(event: { preventDefault: () => void; target: EventTarget | null }) {
  const target = event.target as HTMLElement | null;
  if (target?.closest(".ui-dropdown, .ui-tooltip, .ui-select__menu")) event.preventDefault();
}

/** Right-hand sheet. Enters on a critically damped spring; exit is immediate. */
export function Sheet({
  open,
  onOpenChange,
  title,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  children: ReactNode;
}) {
  const reduce = useReducedMotion();
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="ui-dialog-overlay" />
        <DialogPrimitive.Content
          className="ui-sheet"
          aria-describedby={undefined}
          onInteractOutside={keepPortaledMenus}
          onFocusOutside={keepPortaledMenus}
        >
          <motion.div
            className="ui-sheet__motion"
            initial={reduce ? { opacity: 0 } : { opacity: 0, x: 28 }}
            animate={{ opacity: 1, x: 0 }}
            transition={reduce ? { duration: 0 } : springPanel}
          >
            <header className="ui-sheet__header">
              <DialogPrimitive.Title className="ui-sheet__title">{title}</DialogPrimitive.Title>
              <DialogPrimitive.Description className="visually-hidden">{title}</DialogPrimitive.Description>
              <DialogPrimitive.Close className="ui-icon-btn" aria-label="Close">
                <X size={16} strokeWidth={1.5} />
              </DialogPrimitive.Close>
            </header>
            <div className="ui-sheet__body">{children}</div>
          </motion.div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
