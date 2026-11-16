import type { CSSProperties } from "react";
import { UI_STYLES, type UiStyleId } from "./ui-styles";

export function UiStylePicker({
  value,
  onChange,
  compact,
}: {
  value: UiStyleId;
  onChange: (id: UiStyleId) => void;
  /** Tighter layout for the start-screen settings panel. */
  compact?: boolean;
}) {
  return (
    <div className={compact ? "ui-style-picker ui-style-picker--compact" : "ui-style-picker"}>
      <span className="ui-style-picker-label">Interface style</span>
      <ul className="ui-style-picker-grid" role="listbox" aria-label="Interface style">
        {UI_STYLES.map((style) => {
          const selected = style.id === value;
          return (
            <li key={style.id} role="presentation">
              <button
                type="button"
                role="option"
                aria-selected={selected}
                className={selected ? "ui-style-swatch selected" : "ui-style-swatch"}
                title={style.label}
                onClick={() => onChange(style.id)}
              >
                <span
                  className="ui-style-swatch-chip"
                  aria-hidden="true"
                  style={
                    {
                      "--swatch-bg": style.swatch.bg,
                      "--swatch-panel": style.swatch.panel,
                      "--swatch-accent": style.swatch.accent,
                      "--swatch-ink": style.swatch.ink,
                    } as CSSProperties
                  }
                >
                  <span className="ui-style-swatch-bar" />
                  <span className="ui-style-swatch-dot" />
                </span>
                <span className="ui-style-swatch-name">{style.label}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}