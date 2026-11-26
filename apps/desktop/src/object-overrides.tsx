/**
 * A few settings that belong to one object rather than the whole plate.
 * An empty field follows the plate. The keys are the same ones the process
 * sheet writes, so a change here and a change there are the same override.
 */

export const OBJECT_OVERRIDE_FIELDS = [
  { key: "wall_line_count", label: "Wall lines", title: "Wall line count for this object" },
  { key: "infill_sparse_density", label: "Infill density", title: "Sparse infill density for this object, in percent" },
  { key: "speed_print", label: "Print speed", title: "Print speed for this object, in millimetres per second" },
] as const;

export function ObjectOverrides(props: {
  settings: Record<string, string>;
  onChange: (key: string, value: string) => void;
}) {
  return (
    <div className="object-overrides">
      <p className="hint">For this object. Leave a field empty to follow the plate.</p>
      {OBJECT_OVERRIDE_FIELDS.map((field) => (
        <label key={field.key}>
          <span>{field.label}</span>
          <input
            aria-label={field.title}
            inputMode="decimal"
            value={props.settings[field.key] ?? ""}
            placeholder="Plate"
            onChange={(event) => {
              const next = event.target.value.trim();
              if (next !== "" && !/^\d+(\.\d+)?$/.test(next)) return;
              props.onChange(field.key, next);
            }}
          />
        </label>
      ))}
    </div>
  );
}
