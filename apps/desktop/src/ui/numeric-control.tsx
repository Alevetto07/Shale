import { Slider } from "./slider";
import { Stepper } from "./stepper";

/**
 * A ranged value is the filled slider. Values without both ends stay a stepper
 * (transforms, an unbounded layer index).
 */
export function NumericControl({
  value,
  min,
  max,
  step,
  digits = 2,
  onChange,
  label,
  unit,
}: {
  value: number;
  min?: number;
  max?: number;
  step: number;
  digits?: number;
  onChange: (value: number) => void;
  label: string;
  unit?: string;
}) {
  if (min != null && max != null) {
    return (
      <Slider
        label={label}
        value={[Number.isFinite(value) ? value : min]}
        min={min}
        max={max}
        step={step}
        unit={unit}
        inputDigits={digits}
        onValueChange={onChange}
      />
    );
  }
  return (
    <Stepper
      label={label}
      value={value}
      min={min}
      max={max}
      step={step}
      digits={digits}
      onChange={onChange}
    />
  );
}
