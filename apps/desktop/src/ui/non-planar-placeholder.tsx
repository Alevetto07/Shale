import { useState } from "react";
import { NumericControl } from "./numeric-control";

type Mode = "off" | "auto" | "manual";

/** Reserved slice-settings slot — UI only until v0.2. */
export function NonPlanarPlaceholder() {
  const [mode, setMode] = useState<Mode>("off");
  const [maxAngle, setMaxAngle] = useState(45);
  const [minHeight, setMinHeight] = useState(0.2);
  const disabled = true;

  return (
    <div className="nonplanar-slot" data-slot="non-planar" data-disabled={disabled ? "1" : "0"}>
      <p className="nonplanar-slot__title">Non-planar</p>
      <p className="nonplanar-slot__lead">Curved-layer slicing is not available in v0.1. These controls preview the layout for v0.2.</p>

      <div className="nonplanar-slot__modes" role="group" aria-label="Non-planar mode">
        <ModePill active={mode === "off"} onSelect={() => setMode("off")} disabled={disabled}>
          Off
        </ModePill>
        <ModePill active={mode === "auto"} onSelect={() => setMode("auto")} disabled={disabled}>
          Auto
        </ModePill>
        <ModePill active={mode === "manual"} onSelect={() => setMode("manual")} disabled={disabled}>
          Manual
        </ModePill>
      </div>

      <div className="nonplanar-slot__settings settings-body">
        <div className="menu-row setting-row setting-row--range">
          <span className="setting-name">Max angle</span>
          <NonPlanarSlider
            label="Max angle"
            value={maxAngle}
            min={0}
            max={90}
            step={1}
            digits={0}
            unit="°"
            disabled={disabled}
            onChange={setMaxAngle}
          />
        </div>
        <div className="menu-row setting-row setting-row--range">
          <span className="setting-name">Min layer height</span>
          <NonPlanarSlider
            label="Min layer height"
            value={minHeight}
            min={0.05}
            max={0.6}
            step={0.01}
            digits={2}
            unit="mm"
            disabled={disabled}
            onChange={setMinHeight}
          />
        </div>
      </div>

      <p className="nonplanar-slot__note">Coming in v0.2 — no engine settings are written yet.</p>
    </div>
  );
}

function NonPlanarSlider(props: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  digits: number;
  unit?: string;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <div className="nonplanar-slot__slider" aria-disabled={props.disabled ? "true" : undefined}>
      <NumericControl {...props} />
    </div>
  );
}

function ModePill({
  children,
  active,
  onSelect,
  disabled,
}: {
  children: string;
  active: boolean;
  onSelect: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className="nonplanar-slot__mode"
      data-active={active ? "1" : "0"}
      disabled={disabled}
      onClick={onSelect}
    >
      {children}
    </button>
  );
}