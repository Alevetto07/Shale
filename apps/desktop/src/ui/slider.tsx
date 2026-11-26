import * as SliderPrimitive from "@radix-ui/react-slider";
import {
  forwardRef,
  useCallback,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type KeyboardEvent,
} from "react";
import NumberFlow from "@number-flow/react";
import { cn } from "./cn";
import { EASE_OUT } from "./motion";
import { formatSliderLabel, sliderFlowParts, sliderValueLabelPercent, sliderValueOnFill } from "./slider-placement";

const FLOW_EASE = `cubic-bezier(${EASE_OUT.join(", ")})`;

export interface SliderProps extends Omit<ComponentPropsWithoutRef<typeof SliderPrimitive.Root>, "onValueChange"> {
  label: string;
  formatValue?: (value: number) => string;
  unit?: string;
  onValueChange?: (value: number) => void;
  allowInput?: boolean;
  inputDigits?: number;
  /** Absolute positions along the same scale as `value`, drawn as ticks. */
  marks?: number[];
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
    marks,
    disabled,
    ...props
  },
  ref,
) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const current = Array.isArray(value) ? value[0] : Array.isArray(defaultValue) ? defaultValue[0] : min;
  const span = Number(max) - Number(min) || 1;
  const thumbPercent = ((current - Number(min)) / span) * 100;
  const labelPercent = sliderValueLabelPercent(thumbPercent);
  const onFill = sliderValueOnFill(thumbPercent, labelPercent);
  const displayLabel = formatSliderLabel(current, inputDigits, unit, formatValue);
  const flow = sliderFlowParts(displayLabel);

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
    <div
      className={cn("ui-slider", className)}
      data-editing={editing ? "1" : "0"}
      data-disabled={disabled ? "1" : "0"}
    >
      <div className="ui-slider__shell">
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
          onKeyDown={onKey}
          {...props}
        >
          <SliderPrimitive.Track className="ui-slider__track">
            <SliderPrimitive.Range className="ui-slider__range" />
            {marks?.map((mark, index) => {
              const markSpan = Number(max) - Number(min) || 1;
              const pct = ((mark - Number(min)) / markSpan) * 100;
              return (
                <span
                  key={`${mark}-${index}`}
                  className="ui-slider__mark"
                  style={{ left: `${Math.min(100, Math.max(0, pct))}%` }}
                  aria-hidden
                />
              );
            })}
          </SliderPrimitive.Track>
          <SliderPrimitive.Thumb className="ui-slider__thumb" aria-label={label} />
        </SliderPrimitive.Root>
        {allowInput && !editing ? (
          <button
            type="button"
            className="ui-slider__value"
            style={{ left: `${labelPercent}%` }}
            data-on-fill={onFill ? "1" : "0"}
            aria-label={`${label} exact value`}
            disabled={disabled}
            data-value={displayLabel}
            onClick={() => {
              setEditing(true);
              setDraft(Number.isFinite(current) ? current.toFixed(inputDigits) : "");
              queueMicrotask(() => inputRef.current?.focus());
            }}
          >
            <SliderDigits label={displayLabel} flow={flow} />
          </button>
        ) : allowInput === false ? (
          <span
            className="ui-slider__value ui-slider__value--readout"
            style={{ left: `${labelPercent}%` }}
            data-on-fill={onFill ? "1" : "0"}
            aria-hidden
            data-value={displayLabel}
          >
            <SliderDigits label={displayLabel} flow={flow} />
          </span>
        ) : null}
        {allowInput && editing ? (
          <input
            ref={inputRef}
            className="ui-slider__value-input"
            style={{ left: `${labelPercent}%` }}
            aria-label={`${label} exact value`}
            inputMode="decimal"
            disabled={disabled}
            value={draft ?? ""}
            onChange={(event) => {
              setDraft(event.target.value);
              const next = Number(event.target.value);
              if (Number.isFinite(next)) emit(next);
            }}
            onBlur={() => {
              setEditing(false);
              setDraft(null);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === "Escape") {
                event.preventDefault();
                setEditing(false);
                setDraft(null);
              }
              onKey(event);
            }}
          />
        ) : null}
      </div>
    </div>
  );
});

function SliderDigits({
  label,
  flow,
}: {
  label: string;
  flow: ReturnType<typeof sliderFlowParts>;
}) {
  if (!flow) return <span className="ui-slider__digits">{label}</span>;
  return (
    <NumberFlow
      className="ui-slider__digits"
      value={flow.value}
      prefix={flow.prefix}
      suffix={flow.suffix}
      locales="en-GB"
      isolate
      respectMotionPreference
      format={{
        minimumFractionDigits: flow.digits,
        maximumFractionDigits: flow.digits,
        useGrouping: false,
      }}
      transformTiming={{ duration: 180, easing: FLOW_EASE }}
      spinTiming={{ duration: 180, easing: FLOW_EASE }}
      opacityTiming={{ duration: 120, easing: "ease-out" }}
    />
  );
}