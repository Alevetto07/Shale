import * as SliderPrimitive from "@radix-ui/react-slider";
import {
  forwardRef,
  useCallback,
  useId,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type KeyboardEvent,
} from "react";
import { cn } from "./cn";

export interface SliderProps extends Omit<ComponentPropsWithoutRef<typeof SliderPrimitive.Root>, "onValueChange"> {
  label: string;
  formatValue?: (value: number) => string;
  unit?: string;
  onValueChange?: (value: number) => void;
  allowInput?: boolean;
  inputDigits?: number;
}

export const Slider = forwardRef<HTMLSpanElement, SliderProps>(function Slider(
  {
    className,
    label,
    value,
    defaultValue,
    min = 0,
    max = 100,
    step = 1,
    formatValue,
    unit = "",
    onValueChange,
    allowInput = true,
    inputDigits = 2,
    disabled,
    ...props
  },
  ref,
) {
  const [dragging, setDragging] = useState(false);
  const [focused, setFocused] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const bubbleId = useId();

  const current = Array.isArray(value) ? value[0] : Array.isArray(defaultValue) ? defaultValue[0] : min;
  const shown = formatValue ? formatValue(current) : `${current.toFixed(inputDigits)}${unit ? ` ${unit}` : ""}`;

  const emit = useCallback(
    (next: number) => {
      const clamped = Math.min(max, Math.max(min, next));
      onValueChange?.(clamped);
    },
    [max, min, onValueChange],
  );

  const onKey = (event: KeyboardEvent) => {
    const fine = event.shiftKey ? step / 10 : step;
    if (event.key === "ArrowRight" || event.key === "ArrowUp") {
      event.preventDefault();
      emit(current + fine);
    }
    if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
      event.preventDefault();
      emit(current - fine);
    }
  };

  return (
    <div className={cn("ui-slider", className)} data-dragging={dragging || focused ? "1" : "0"}>
      <SliderPrimitive.Root
        ref={ref}
        className="ui-slider__root"
        value={value}
        defaultValue={defaultValue}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        onValueChange={(vals) => emit(vals[0] ?? min)}
        onPointerDown={() => setDragging(true)}
        onPointerUp={() => setDragging(false)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        onKeyDown={onKey}
        {...props}
      >
        <SliderPrimitive.Track className="ui-slider__track">
          <SliderPrimitive.Range className="ui-slider__range" />
        </SliderPrimitive.Track>
        <SliderPrimitive.Thumb
          className="ui-slider__thumb"
          aria-label={label}
          aria-describedby={bubbleId}
        />
      </SliderPrimitive.Root>
      {(dragging || focused) && (
        <span id={bubbleId} className="ui-slider__bubble" role="status">
          {shown}
        </span>
      )}
      {allowInput ? (
        <input
          ref={inputRef}
          className="ui-slider__input"
          aria-label={`${label} exact value`}
          inputMode="decimal"
          disabled={disabled}
          value={draft ?? (Number.isFinite(current) ? current.toFixed(inputDigits) : "")}
          onFocus={() => setDraft(Number.isFinite(current) ? current.toFixed(inputDigits) : "")}
          onBlur={() => setDraft(null)}
          onChange={(event) => {
            setDraft(event.target.value);
            const next = Number(event.target.value);
            if (Number.isFinite(next)) emit(next);
          }}
          onKeyDown={onKey}
        />
      ) : null}
    </div>
  );
});