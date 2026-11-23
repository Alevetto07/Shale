import { useState } from "react";
import { Slider } from ".";

type Mode = "off" | "auto" | "manual";

/** Reserved slice-settings slot — UI only until v0.2. */
export function NonPlanarPlaceholder() {
  const [mode, setMode] = useState<Mode>("off");
  const [maxAngle, setMaxAngle] = useState(45);
  const [minHeight, setMinHeight] = useState(0.2);
  const disabled = true;

  return (
    <div className="nonplanar-slot" data-slot="non-planar">
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

      <div className="nonplanar-slot__sliders" aria-disabled={disabled}>
        <Slider
          label="Max angle"
          min={0}
          max={90}
          step={1}
          value={[maxAngle]}
          unit="°"
          disabled={disabled}
          onValueChange={setMaxAngle}
          formatValue={(value) => String(Math.round(value))}
        />
        <Slider
          label="Min layer height"
          min={0.05}
          max={0.6}
          step={0.01}
          value={[minHeight]}
          unit="mm"
          disabled={disabled}
          onValueChange={setMinHeight}
        />
      </div>

      <p className="nonplanar-slot__note">Coming in v0.2 — no engine settings are written yet.</p>
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