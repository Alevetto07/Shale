import type { ReactNode } from "react";

/** Visual language for the Shale window. Dark, greyscale, monospace. */
export const theme = {
  color: {
    bg: "#0d0d0d",
    panel: "#111111",
    ink: "#f2f2f2",
    muted: "#8a8a8a",
    dim: "#3a3a3a",
    line: "#2a2a2a",
  },
  font: {
    wordmark: '"Space Grotesk", system-ui, sans-serif',
    mono: '"IBM Plex Mono", ui-monospace, monospace',
  },
  box: {
    tl: "┌",
    tr: "┐",
    bl: "└",
    br: "┘",
    h: "─",
    v: "│",
  },
  stone: [
    "        /\\",
    "       /  \\",
    "      / /\\ \\",
    "     / /  \\ \\",
    "    / /    \\ \\",
    "   /_/      \\_\\",
  ].join("\n"),
} as const;

export function meter(fraction: number, width = 24): string {
  const clamped = Math.min(1, Math.max(0, fraction));
  const filled = Math.round(clamped * width);
  return "█".repeat(filled) + "░".repeat(width - filled);
}

export function track(value: number, max: number, width = 28): string {
  if (max <= 0) return "●" + theme.box.h.repeat(width - 1);
  const index = Math.round((value / max) * (width - 1));
  return theme.box.h.repeat(index) + "●" + theme.box.h.repeat(width - 1 - index);
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
