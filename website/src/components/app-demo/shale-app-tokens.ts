/** Mirrors apps/desktop theme.tsx box + colours for marketing chrome. */
export const shaleBox = {
  tl: "┌",
  tr: "┐",
  bl: "└",
  br: "┘",
  h: "─",
  v: "│",
} as const;

export const WORKFLOW_STEPS = [
  { id: "printer", label: "Pick printer" },
  { id: "import", label: "Import model" },
  { id: "settings", label: "Settings" },
  { id: "slice", label: "Slice" },
  { id: "preview", label: "Preview layers" },
  { id: "send", label: "Send" },
  { id: "cli", label: "CLI / MCP" },
] as const;

export type WorkflowStepId = (typeof WORKFLOW_STEPS)[number]["id"];

export function meter(fraction: number, width = 22): string {
  const f = Math.min(1, Math.max(0, fraction));
  const n = Math.round(f * width);
  return "█".repeat(n) + "░".repeat(width - n);
}