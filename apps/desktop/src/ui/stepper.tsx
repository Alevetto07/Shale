import { ChevronLeft, ChevronRight } from "lucide-react";
import { useState } from "react";

export function Stepper({
  value,
  min,
  max,
  step,
  digits = 2,
  onChange,
  label,
  disabled,
}: {
  value: number;
  min?: number;
  max?: number;
  step: number;
  digits?: number;
  onChange: (value: number) => void;
  label: string;
  disabled?: boolean;
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
    <span className="ui-stepper" data-ui-stepper="">
      <button
        type="button"
        disabled={disabled}
        aria-label={`Decrease ${label}`}
        onClick={() => {
          setDraft(null);
          onChange(clamp(value - step));
        }}
      >
        <ChevronLeft size={16} strokeWidth={2} aria-hidden />
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
        disabled={disabled}
      />
      <button type="button" disabled={disabled} aria-label={`Increase ${label}`} onClick={() => onChange(clamp(value + step))}>
        <ChevronRight size={16} strokeWidth={2} aria-hidden />
      </button>
    </span>
  );
}
