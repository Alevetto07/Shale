import { FEATURE_LABELS, featureColor } from "../../lib/gcode-types";

const ORDER = ["wall-outer", "wall-inner", "skin", "infill", "support"] as const;

export function FeatureLegend({ showTravel }: { showTravel: boolean }) {
  const items = showTravel ? [...ORDER, "travel" as const] : ORDER;
  return (
    <ul className="gcode-legend" aria-label="Toolpath features">
      {items.map((k) => (
        <li key={k}>
          <span className="gcode-legend__swatch" style={{ background: featureColor(k) }} />
          {FEATURE_LABELS[k] ?? k}
        </li>
      ))}
    </ul>
  );
}