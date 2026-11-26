/**
 * Settings that belong to one object rather than the whole plate.
 * An empty field follows the plate. The keys are the same ones the process
 * sheet writes, so a change here and a change there are the same override.
 */

type NumberField = {
  kind: "number";
  key: string;
  label: string;
  title: string;
};

type ChoiceField = {
  kind: "choice";
  key: string;
  label: string;
  title: string;
  options: { value: string; label: string }[];
};

export type ObjectField = NumberField | ChoiceField;

const PLATE = { value: "", label: "Plate" };

export const OBJECT_OVERRIDE_FIELDS: ObjectField[] = [
  { kind: "number", key: "wall_line_count", label: "Wall lines", title: "Wall line count for this object" },
  { kind: "number", key: "infill_sparse_density", label: "Infill density", title: "Sparse infill density for this object, in percent" },
  { kind: "number", key: "speed_print", label: "Print speed", title: "Print speed for this object, in millimetres per second" },
  { kind: "number", key: "top_layers", label: "Top layers", title: "Top shell layers for this object" },
  { kind: "number", key: "bottom_layers", label: "Bottom layers", title: "Bottom shell layers for this object" },
  {
    kind: "choice",
    key: "support_enable",
    label: "Support",
    title: "Support for this object",
    options: [PLATE, { value: "1", label: "On" }, { value: "0", label: "Off" }],
  },
  {
    kind: "choice",
    key: "adhesion_type",
    label: "Brim type",
    title: "Brim type for this object",
    options: [
      PLATE,
      { value: "brim", label: "Brim" },
      { value: "skirt", label: "Skirt" },
      { value: "raft", label: "Raft" },
      { value: "none", label: "None" },
    ],
  },
  { kind: "number", key: "brim_width", label: "Brim width", title: "Brim width for this object, in millimetres" },
  {
    kind: "choice",
    key: "z_seam_type",
    label: "Seam",
    title: "Seam position for this object",
    options: [
      PLATE,
      { value: "back", label: "Back" },
      { value: "shortest", label: "Shortest" },
      { value: "random", label: "Random" },
      { value: "sharpest_corner", label: "Sharpest corner" },
    ],
  },
];

export function ObjectOverrides(props: {
  settings: Record<string, string>;
  onChange: (key: string, value: string) => void;
}) {
  return (
    <div className="object-overrides">
      <p className="hint">For this object. Leave a field empty to follow the plate.</p>
      {OBJECT_OVERRIDE_FIELDS.map((field) =>
        field.kind === "number" ? (
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
        ) : (
          <label key={field.key}>
            <span>{field.label}</span>
            <select
              aria-label={field.title}
              value={props.settings[field.key] ?? ""}
              onChange={(event) => props.onChange(field.key, event.target.value)}
            >
              {field.options.map((option) => (
                <option key={option.value || "plate"} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>
        ),
      )}
    </div>
  );
}
