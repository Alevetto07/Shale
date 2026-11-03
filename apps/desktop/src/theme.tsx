import type { ReactNode } from "react";

/** Deep Field. Dark only. One accent, used sparingly. */
export const theme = {
  color: {
    bg: "#06070b",
    panel: "#0b0d14",
    ink: "#e9e8f2",
    muted: "rgba(233, 232, 242, 0.72)",
    dim: "rgba(233, 232, 242, 0.58)",
    line: "rgba(233, 232, 242, 0.12)",
    accent: "#b9b3ff",
  },
  font: {
    wordmark: '"Space Grotesk", system-ui, sans-serif',
    mono: '"Geist Mono", ui-monospace, monospace',
    prose: '"Inter", system-ui, sans-serif',
  },
  box: {
    tl: "┌",
    tr: "┐",
    bl: "└",
    br: "┘",
    h: "─",
    v: "│",
  },
} as const;

export function meter(fraction: number, width = 24): string {
  const clamped = Math.min(1, Math.max(0, fraction));
  const filled = Math.round(clamped * width);
  return "█".repeat(filled) + "░".repeat(width - filled);
}

/** Layer track. `marks` are instruction positions in the same units as `value`. */
export function track(value: number, max: number, width = 28, marks: number[] = []): string {
  const cells: string[] = Array.from({ length: width }, () => theme.box.h);
  const at = (n: number) => {
    if (max <= 0) return 0;
    return Math.min(width - 1, Math.max(0, Math.round((n / max) * (width - 1))));
  };
  for (const mark of marks) {
    const index = at(mark);
    if (cells[index] === theme.box.h) cells[index] = "+";
  }
  cells[at(value)] = "●";
  return cells.join("");
}

export function Frame({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="frame">
      <div className="frame-rule" aria-hidden="true">
        <span>{theme.box.tl}</span>
        <span className="frame-title"> {title} </span>
        <span className="rule">{theme.box.h.repeat(48)}</span>
        <span>{theme.box.tr}</span>
      </div>
      <div className="frame-body">
        <span className="frame-side" aria-hidden="true">
          {theme.box.v}
        </span>
        <div className="frame-content">{children}</div>
        <span className="frame-side" aria-hidden="true">
          {theme.box.v}
        </span>
      </div>
      <div className="frame-rule" aria-hidden="true">
        <span>{theme.box.bl}</span>
        <span className="rule">{theme.box.h.repeat(48)}</span>
        <span>{theme.box.br}</span>
      </div>
    </section>
  );
}
