import { toast } from "sonner";

export interface UndoToastOptions {
  /** Stable id when the same action may fire twice in quick succession. */
  id?: string;
  description?: string;
}

/** Transient confirmation with an undo affordance wired to caller state. */
export function toastUndo(message: string, onUndo: () => void, options?: UndoToastOptions) {
  toast(message, {
    id: options?.id,
    description: options?.description,
    action: {
      label: "Undo",
      onClick: (event) => {
        event.preventDefault();
        onUndo();
        toast.dismiss(options?.id);
      },
    },
  });
}