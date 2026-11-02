import { useEffect, useId, useRef, useState } from "react";

export function AsciiCheck({
  checked,
  onChange,
  children,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  children: string;
}) {
  return (
    <button
      type="button"
      className="ascii-check"
      role="checkbox"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    >
      <span aria-hidden="true">{checked ? "[x]" : "[ ]"}</span>
      <span>{children}</span>
    </button>
  );
}

export function AsciiStepper({
  value,
  min,
  max,
  step,
  digits = 2,
  onChange,
  label,
}: {
  value: number;
  min?: number;
  max?: number;
  step: number;
  digits?: number;
  onChange: (value: number) => void;
  label: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const clamp = (next: number) => {
    if (!Number.isFinite(next)) return value;
    let clamped = next;
    if (min != null) clamped = Math.max(min, clamped);
    if (max != null) clamped = Math.min(max, clamped);
    const factor = 10 ** digits;
    return Math.round(clamped * factor) / factor;
  };
  return (
    <span className="ascii-step">
      <button type="button" aria-label={`Decrease ${label}`} onClick={() => { setDraft(null); onChange(clamp(value - step)); }}>
        {"<"}
      </button>
      <input
        aria-label={label}
        inputMode="decimal"
        value={draft ?? (Number.isFinite(value) ? value.toFixed(digits) : "")}
        onFocus={() => setDraft(Number.isFinite(value) ? value.toFixed(digits) : "")}
        onBlur={() => setDraft(null)}
        onChange={(event) => {
          setDraft(event.target.value);
          const next = Number(event.target.value);
          if (Number.isFinite(next)) onChange(clamp(next));
        }}
      />
      <button type="button" aria-label={`Increase ${label}`} onClick={() => onChange(clamp(value + step))}>
        {">"}
      </button>
    </span>
  );
}

export function AsciiField({
  value,
  onChange,
  placeholder,
  label,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  label: string;
}) {
  return (
    <span className="ascii-field">
      <span aria-hidden="true">[</span>
      <input aria-label={label} value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} />
      <span aria-hidden="true">]</span>
    </span>
  );
}

export function AsciiSelect({
  value,
  options,
  onChange,
  label,
}: {
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
  label: string;
}) {
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const cursorRef = useRef(0);
  const listId = useId();
  const current = options.find((option) => option.value === value);
  const shown = current?.label || "none";
  cursorRef.current = cursor;

  useEffect(() => {
    if (!open) return;
    setCursor(Math.max(0, options.findIndex((option) => option.value === value)));
  }, [open, options, value]);

  useEffect(() => {
    if (!open) return;
    function onPointer(event: MouseEvent) {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
        return;
      }
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setCursor((index) => Math.min(options.length - 1, index + 1));
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setCursor((index) => Math.max(0, index - 1));
      } else if (event.key === "Enter") {
        event.preventDefault();
        const option = options[cursorRef.current];
        if (option) onChange(option.value);
        setOpen(false);
      }
    }
    window.addEventListener("mousedown", onPointer);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onPointer);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, options, onChange]);

  return (
    <div className="ascii-select" ref={root}>
      <button
        type="button"
        className="ascii-select-closed"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((next) => !next)}
      >
        [ {shown} v]
      </button>
      {open && (
        <div className="ascii-menu" id={listId} role="listbox" aria-label={label}>
          <div className="ascii-menu-edge" aria-hidden="true">
            ┌{"─".repeat(22)}
          </div>
          {options.length === 0 && <div className="ascii-option">  none</div>}
          {options.map((option, index) => (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={option.value === value}
              className={index === cursor ? "ascii-option cursor" : "ascii-option"}
              onMouseEnter={() => setCursor(index)}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
            >
              {option.value === value ? "> " : "  "}
              {option.label}
            </button>
          ))}
          <div className="ascii-menu-edge" aria-hidden="true">
            └{"─".repeat(22)}
          </div>
        </div>
      )}
    </div>
  );
}
